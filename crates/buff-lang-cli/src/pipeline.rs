//! Shared compiler pipeline used by both `buff build` and `buff run`.
//!
//! The pipeline is split into two phases so callers can decide what to do
//! with the intermediate Rust source:
//!
//! - [`compile_to_rust`] — read a `.buff` file, lex, parse, and codegen into a
//!   Rust source string, writing it to `<file>.rs` alongside the input.
//! - [`compile_rust_to_exe`] — invoke `rustc --edition 2021` on a `.rs` file to
//!   produce a native executable. Takes a [`BuildMode`] (T56) to switch
//!   between fast-debug and release-with-LTO rustc flag sets.
//!
//! All fallible operations return [`anyhow::Result`] with rich, user-facing
//! context. No panics, no `unwrap`/`expect`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use buff_lang_codegen_buffhtml::{self as buffhtml_codegen, CodegenResult, SpanMap};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::{SourceFile, SourceId};
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

/// Compile-time optimization profile (T56).
///
/// Selects which set of rustc flags [`compile_rust_to_exe`] passes to the
/// backend. `Debug` (the default) preserves the v0.1 behavior — a single
/// `-O` flag for fast compilation. `Release` enables LTO + maximum
/// optimization via [`rustc_release_flags`] for production-ready binaries.
///
/// This enum intentionally mirrors the user-facing `--release` CLI flag: the
/// CLI translates `release: bool` into `BuildMode` via
/// [`BuildMode::from_release_flag`], keeping the pipeline decoupled from
/// clap-level concerns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BuildMode {
    /// Fast-debug compilation (v0.1 behavior): `rustc -O`. No LTO.
    /// Use this during development for tight edit-compile-run loops.
    #[default]
    Debug,
    /// Release-grade compilation: `opt-level=3` + `lto=fat` +
    /// `codegen-units=1`. Slower compile, smaller+faster binary.
    Release,
}

impl BuildMode {
    /// Translate the CLI `--release` boolean into a [`BuildMode`].
    ///
    /// `true` → [`BuildMode::Release`], `false` → [`BuildMode::Debug`].
    /// This is the single source of truth for the flag→mode mapping — every
    /// caller (`buff build`, `buff run`) goes through here so the behavior
    /// stays consistent across subcommands.
    pub fn from_release_flag(release: bool) -> Self {
        if release {
            BuildMode::Release
        } else {
            BuildMode::Debug
        }
    }

    /// Returns `true` when this mode is [`BuildMode::Release`].
    pub fn is_release(self) -> bool {
        matches!(self, BuildMode::Release)
    }
}

/// Output of the [`compile_to_rust`] phase: the generated Rust source plus the
/// path it was written to.
#[derive(Debug, Clone)]
pub struct CompileOutput {
    /// The generated Rust source code (already formatted via `prettyplease`).
    pub rust_source: String,
    /// Path of the `.rs` file that was written (alongside the input `.buff`).
    pub rust_file_path: PathBuf,
}

/// Run the front-end of the compiler: read → lex → parse → codegen → write.
///
/// Writes the generated Rust source to `file.with_extension("rs")` (i.e. the
/// `.rs` file sits next to the `.buff` source). The type-checking pass is
/// already integrated inside codegen (T12) and is non-fatal in v1.0.
///
/// # Errors
///
/// Returns an error if the file cannot be read, lexing fails, parsing fails,
/// codegen fails, or the `.rs` file cannot be written. Every error message
/// includes the source filename and (where possible) the line/column of the
/// offending span via [`SourceFile::lookup`].
pub fn compile_to_rust(file: &Path) -> Result<CompileOutput> {
    // 1. Read source.
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read source file `{}`", file.display()))?;

    // Build a SourceFile so we can map byte offsets to 1-based line/col for
    // diagnostic messages. SourceId(0) is fine — we only lex a single file.
    let source_id = SourceId(0);
    let source_file = SourceFile::new(file.to_path_buf(), source.clone());

    // 2. Lex. LexerError wraps the buff_lang_error::LexError via `inner`.
    let tokens = tokenize(&source, source_id)
        .map_err(|e| format_diagnostic_error("lex", &e.inner.diagnostic, &source_file, file))?;

    // 3. Parse.
    let decls = parse(&tokens, source_id)
        .map_err(|e| format_diagnostic_error("parse", &e.diagnostic, &source_file, file))?;

    // 4. Codegen (type inference is integrated inside RustCodegen).
    let rust_source = generate_rust(&decls)
        .map_err(|e| format_diagnostic_error("codegen", &e.diagnostic, &source_file, file))?;

    // 5. Write the .rs file alongside the .buff source.
    let rust_file_path = file.with_extension("rs");
    std::fs::write(&rust_file_path, &rust_source)
        .with_context(|| format!("failed to write `{}`", rust_file_path.display()))?;

    Ok(CompileOutput {
        rust_source,
        rust_file_path,
    })
}

// ---------------------------------------------------------------------------
// T133: `.buffhtml` SFC pipeline (decision record rsx-syntax-feasibility.md).
// ---------------------------------------------------------------------------

/// File extension used by `.buffhtml` Single-File Components.
pub const BUFFHTML_EXT: &str = "buffhtml";

/// Output of the [`compile_buffhtml_to_rust`] phase (T133).
///
/// Mirrors [`CompileOutput`] but additionally carries the post-format
/// [`SpanMap`] built by `buff_lang_codegen_buffhtml::generate`. The
/// [`SpanMap`] lets the CLI's [`error_mapper`] reverse-map rustc
/// diagnostics (line:col in the generated `.rs`) back to the originating
/// `.buffhtml` byte span.
#[derive(Debug, Clone)]
pub struct BuffHtmlCompileOutput {
    /// The generated Rust source (after the script-block pass-through
    /// transformation — see [`inline_script_block`]).
    pub rust_source: String,
    /// Path of the `.rs` file written alongside the input `.buffhtml`.
    pub rust_file_path: PathBuf,
    /// Reverse-mapping table from generated `.rs` positions to `.buffhtml`
    /// spans. Consumed by [`error_mapper::translate_buffhtml_rustc_errors`].
    pub span_map: SpanMap,
}

/// Dispatch helper: detect the source file extension and call the matching
/// compile path. Returns the result boxed into [`CompileOutput`] so callers
/// that don't need span-mapping (e.g. the existing `compile_rust_to_exe`
/// invocation) can stay generic. The `.buffhtml` path additionally
/// round-trips the [`SpanMap`] via [`compile_buffhtml_to_rust`] internally;
/// callers that need the span_map should call [`compile_buffhtml_to_rust`]
/// directly.
///
/// # Errors
///
/// Propagates file-read / lex / parse / codegen errors. An unknown
/// extension is an error (only `.buff` and `.buffhtml` are recognised).
pub fn compile_to_rust_for_ext(file: &Path) -> Result<CompileOutput> {
    match file.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext == BUFFHTML_EXT => {
            let out = compile_buffhtml_to_rust(file)?;
            Ok(CompileOutput {
                rust_source: out.rust_source,
                rust_file_path: out.rust_file_path,
            })
        }
        _ => compile_to_rust(file),
    }
}

/// Run the front-end of the compiler on a `.buffhtml` file: read → parse →
/// codegen → script-block pass-through → write the `.rs` file.
///
/// This is the T133 sibling of [`compile_to_rust`]. The pipeline is:
///
/// 1. Read the `.buffhtml` source.
/// 2. Parse via [`buff_lang_buffhtml_parser::parse`] → [`RsxTemplateFile`].
/// 3. Derive the component name from the file stem (e.g. `counter.buffhtml`
///    → `Counter`). Falls back to
///    [`buffhtml_codegen::DEFAULT_COMPONENT_NAME`] if the stem is empty or
///    contains no alphabetic chars.
/// 4. Codegen via [`buffhtml_codegen::generate`] → [`CodegenResult`] (the
///    codegen emits `<script>` block contents as a `const
///    __BUFF_SCRIPT_SOURCE: &str = ...;` placeholder).
/// 5. Post-process via [`inline_script_block`] to splice the script source
///    verbatim into the component fn body. **T133 floor:** the script is
///    treated as raw Rust source (e.g. `let mut count = use_signal(|| 0);`).
///    Full Buff-syntax transpilation (`component`/`state`/`fn` shorthand)
///    is T134+.
/// 6. Write the `.rs` file alongside the input (replacing the `.buffhtml`
///    extension with `.rs`).
///
/// # Errors
///
/// Returns an error if the file cannot be read, parsing fails, codegen
/// fails, the script-block pass-through transformation fails, or the `.rs`
/// file cannot be written. Every error message includes the source
/// filename and (where possible) the line/column of the offending span.
pub fn compile_buffhtml_to_rust(file: &Path) -> Result<BuffHtmlCompileOutput> {
    // 1. Read source.
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read source file `{}`", file.display()))?;

    // Build a SourceFile so parse-error messages can carry line/col context.
    let source_id = SourceId(0);
    let source_file = SourceFile::new(file.to_path_buf(), source.clone());

    // 2. Parse via buff-lang-buffhtml-parser.
    let template = buff_lang_buffhtml_parser::parse(&source, source_id)
        .map_err(|e| format_buffhtml_parse_error(&e, &source_file, file))?;

    // 3. Derive component name from file stem (Counter.buffhtml -> Counter).
    let component_name = derive_component_name(file);

    // 4. Codegen via buff-lang-codegen-buffhtml.
    let codegen_out: CodegenResult = buffhtml_codegen::generate(&template, &component_name)
        .map_err(|e| anyhow::anyhow!("buffhtml codegen error in `{}`: {e}", file.display()))?;

    // 5. Post-process: convert __BUFF_SCRIPT_SOURCE const declaration into
    //    actual fn-body statements (T133 Rust-in-script-block pass-through).
    let final_rust = inline_script_block(codegen_out.rust_source).with_context(|| {
        format!(
            "buffhtml script-block post-process failed for `{}`",
            file.display()
        )
    })?;

    // 6. Write the .rs file alongside the .buffhtml source.
    let rust_file_path = file.with_extension("rs");
    std::fs::write(&rust_file_path, &final_rust)
        .with_context(|| format!("failed to write `{}`", rust_file_path.display()))?;

    Ok(BuffHtmlCompileOutput {
        rust_source: final_rust,
        rust_file_path,
        span_map: codegen_out.span_map,
    })
}

/// Derive the Rust component function name from a `.buffhtml` file path.
///
/// `counter.buffhtml` → `Counter`, `todo_list.buffhtml` → `TodoList`. The
/// result is sanitised: PascalCased, non-alphanumeric chars stripped. If
/// the stem is empty or contains no alphanumeric chars, falls back to
/// [`buffhtml_codegen::DEFAULT_COMPONENT_NAME`].
fn derive_component_name(file: &Path) -> String {
    let stem = match file.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return buffhtml_codegen::DEFAULT_COMPONENT_NAME.to_string(),
    };
    let pascal: String = stem
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    let mut head = String::new();
                    head.push(first.to_ascii_uppercase());
                    head + &chars.as_str().to_ascii_lowercase()
                }
                None => String::new(),
            }
        })
        .collect();
    if pascal.is_empty()
        || !pascal
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
    {
        buffhtml_codegen::DEFAULT_COMPONENT_NAME.to_string()
    } else {
        pascal
    }
}

/// T133 floor: splice the `<script lang="buff">` block contents verbatim
/// into the generated component fn body as Rust statements.
///
/// `buff-lang-codegen-buffhtml::generate` emits the script source as a
/// placeholder top-level constant:
///
/// ```text
/// #[doc = "buffhtml script block — extracted by buff-lang-cli"]
/// const __BUFF_SCRIPT_SOURCE: &str = "<raw script source>";
/// ```
///
/// so the generated `syn::File` is always syntactically valid Rust. This
/// function:
///
/// 1. Parses the generated `.rs` source via [`syn::parse_file`].
/// 2. Locates the `__BUFF_SCRIPT_SOURCE` const item and extracts its string
///    value (uses syn's `LitStr::value()` so any quoting form — `"..."`,
///    `r#"..."#`, escapes — is handled correctly).
/// 3. Removes the const item from the file.
/// 4. Parses the script source as a Rust block (`{ <script> }`) and
///    prepends the resulting statements to the component fn's body
///    (in front of the existing `rsx!{}` expression statement).
/// 5. Re-emits the file via `prettyplease::unparse`.
///
/// When no `__BUFF_SCRIPT_SOURCE` const is present (no `<script>` block),
/// the input is returned unchanged.
///
/// # T133 limitation — Rust-in-script-block pass-through
///
/// The script block contents are spliced as-is into the component fn body
/// — they must already be valid Rust (e.g. `let mut count =
/// use_signal(|| 0);`). Full Buff-syntax script transpilation (the
/// `component Name = fn(...) -> Element:` shorthand from decision record
/// §3 example 1) is **deferred to T134+**. The examples in `examples/*.buffhtml`
/// use Rust-compatible script syntax matching the T121b/T130 counter
/// pattern.
fn inline_script_block(rust_source: String) -> Result<String> {
    let mut file: syn::File = syn::parse_str(&rust_source)
        .with_context(|| "failed to parse codegen-buffhtml output as a syn::File")?;

    // 1. Locate + extract __BUFF_SCRIPT_SOURCE const value (if present).
    let mut extracted: Option<String> = None;
    let mut keep = Vec::with_capacity(file.items.len());
    for item in file.items.drain(..) {
        if let syn::Item::Const(ref c) = item {
            if c.ident == "__BUFF_SCRIPT_SOURCE" {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit_str),
                    ..
                }) = &*c.expr
                {
                    extracted = Some(lit_str.value());
                    continue; // drop this item from `keep`
                }
            }
        }
        keep.push(item);
    }
    file.items = keep;

    let script_source = match extracted {
        Some(s) => s,
        None => return Ok(rust_source), // no script block — pass-through unchanged
    };

    // 2. Parse the script source as a block of Rust statements.
    let wrapped = format!("{{ {script_source} }}");
    let script_block: syn::Block = syn::parse_str(&wrapped)
        .with_context(|| "failed to parse <script lang=\"buff\"> contents as Rust statements (T133 ships Rust-in-script-block pass-through; full Buff-syntax transpilation is T134+)")?;

    // 3. Inject the script statements into the #[component] fn body,
    //    prepending them before the existing `rsx!{}` expr stmt.
    let mut injected = false;
    for item in &mut file.items {
        if let syn::Item::Fn(fn_item) = item {
            let is_component = fn_item.attrs.iter().any(|a| a.path().is_ident("component"));
            if is_component {
                let mut new_stmts = script_block.stmts.clone();
                new_stmts.append(&mut fn_item.block.stmts);
                fn_item.block.stmts = new_stmts;
                injected = true;
                break;
            }
        }
    }
    if !injected {
        bail!("buffhtml script block present but no #[component] fn found to splice into");
    }

    Ok(prettyplease::unparse(&file))
}

/// Format a `BuffHtmlParseError` as a user-facing anyhow error with
/// line/column context (mirrors [`format_diagnostic_error`] for `.buff`
/// errors, but consumes the buffhtml-parser-specific error type).
fn format_buffhtml_parse_error(
    e: &buff_lang_buffhtml_parser::BuffHtmlParseError,
    source_file: &SourceFile,
    file: &Path,
) -> anyhow::Error {
    let span = e.span();
    let phase = match e {
        buff_lang_buffhtml_parser::BuffHtmlParseError::Lex { .. } => "lex",
        buff_lang_buffhtml_parser::BuffHtmlParseError::Parse { .. } => "parse",
    };
    let msg_core = match e {
        buff_lang_buffhtml_parser::BuffHtmlParseError::Lex { message, .. } => message.clone(),
        buff_lang_buffhtml_parser::BuffHtmlParseError::Parse { message, .. } => message.clone(),
    };
    let (line_col_note, source_line) = match source_file.lookup(span.start) {
        Some((line, col)) => (
            format!(" at {}:{}:{}\n  --> {}", file.display(), line, col, line),
            extract_source_line(source_file, line),
        ),
        None => (format!(" in {}", file.display()), String::new()),
    };
    let mut msg = format!("buffhtml {phase} error: {msg_core}{line_col_note}");
    if !source_line.is_empty() {
        msg.push_str(&format!("\n      | {source_line}"));
    }
    anyhow::anyhow!(msg)
}

/// Compile a generated Rust source file into a native executable via `rustc`.
///
/// Invokes `rustc --edition 2021 <opt-flags> <rust_file> -o <output>`, where
/// `<opt-flags>` depends on `mode`:
///
/// - [`BuildMode::Debug`] (default, v0.1 behavior): just `-O`
///   (equivalent to `-C opt-level=2`). Fast compilation, no LTO.
/// - [`BuildMode::Release`] (T56): [`rustc_release_flags()`] — `opt-level=3`
///   + `lto=fat` + `codegen-units=1`. Slower compilation, faster runtime.
///
/// The `output` path is passed verbatim to rustc — callers should pre-append
/// the platform executable extension (see [`with_exe_extension`]) if they
/// want a conventional name (e.g. `ola.exe` on Windows).
///
/// `buff_file` is the original `.buff` source path. When `rustc` emits
/// diagnostics referencing the intermediate `.rs` file, they are translated to
/// reference the `.buff` file instead via
/// [`error_mapper::translate_rustc_errors`].
///
/// # Errors
///
/// - Fails if `rustc` cannot be invoked (not installed / not in `PATH`).
/// - Fails if `rustc` exits with a non-zero status. Translated `rustc`
///   diagnostics are forwarded to the caller's stderr before bailing.
pub fn compile_rust_to_exe(
    rust_file: &Path,
    output: &Path,
    buff_file: &Path,
    mode: BuildMode,
) -> Result<PathBuf> {
    let mut cmd = Command::new("rustc");
    cmd.arg("--edition").arg("2021");

    // Select the optimization/LTO flag set based on the build mode.
    // Debug keeps the v0.1 `-O` exactly — byte-identical behavior with the
    // pre-T56 pipeline. Release swaps in the LTO + opt-level=3 + single
    // codegen-unit block.
    match mode {
        BuildMode::Debug => {
            cmd.arg("-O");
        }
        BuildMode::Release => {
            for flag in rustc_release_flags() {
                cmd.arg(flag);
            }
        }
    }

    cmd.arg(rust_file).arg("-o").arg(output);

    let result = cmd
        .output()
        .context("failed to invoke `rustc` — is it installed and on your PATH?")?;

    // Forward rustc's stderr (diagnostics / warnings), translating `.rs`
    // references to `.buff` so the user sees their original source location.
    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let translated = crate::error_mapper::translate_rustc_errors(&stderr, buff_file, rust_file);
        eprint!("{translated}");
    }

    if !result.status.success() {
        bail!("rustc exited with status {}", result.status);
    }

    // rustc writes exactly the path passed to `-o` (it does NOT append a
    // platform extension on its own on this toolchain), so the on-disk
    // artifact is precisely `output`.
    Ok(output.to_path_buf())
}

/// T133: Compile a `.buffhtml`-generated `.rs` into a native executable.
///
/// This is the span-aware sibling of [`compile_rust_to_exe`]: it passes
/// the post-format [`SpanMap`] (produced by
/// [`buff_lang_codegen_buffhtml::generate`]) to
/// [`crate::error_mapper::translate_buffhtml_rustc_errors`] so that
/// rustc's `--> foo.rs:LINE:COL` diagnostic references are reverse-mapped
/// to the originating `.buffhtml` line:col (with filename translation
/// always applied as a baseline).
///
/// Behaviour matches [`compile_rust_to_exe`] exactly otherwise — same
/// rustc invocation, same [`BuildMode`] flag selection, same exit-status
/// handling.
pub fn compile_buffhtml_rust_to_exe(
    rust_file: &Path,
    output: &Path,
    buffhtml_file: &Path,
    mode: BuildMode,
    span_map: &SpanMap,
    buffhtml_source: &str,
) -> Result<PathBuf> {
    let mut cmd = Command::new("rustc");
    cmd.arg("--edition").arg("2021");
    match mode {
        BuildMode::Debug => {
            cmd.arg("-O");
        }
        BuildMode::Release => {
            for flag in rustc_release_flags() {
                cmd.arg(flag);
            }
        }
    }
    cmd.arg(rust_file).arg("-o").arg(output);

    let result = cmd
        .output()
        .context("failed to invoke `rustc` — is it installed and on your PATH?")?;

    // Translate rustc stderr: filename (.rs -> .buffhtml) + line:col
    // reverse-mapping via the SpanMap side-table.
    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let translated = crate::error_mapper::translate_buffhtml_rustc_errors(
            &stderr,
            buffhtml_file,
            rust_file,
            span_map,
            buffhtml_source,
        );
        eprint!("{translated}");
    }

    if !result.status.success() {
        bail!("rustc exited with status {}", result.status);
    }
    Ok(output.to_path_buf())
}

/// The rustc CLI flags that implement release-grade optimization (T56).
///
/// Returns the argument sequence passed verbatim to `rustc` when
/// [`compile_rust_to_exe`] is called with [`BuildMode::Release`]:
///
/// - `-C opt-level=3` — maximum LLVM optimization (overrides the `-O`
///   default of `opt-level=2`).
/// - `-C lto=fat` — full link-time optimization across the entire crate
///   graph (the `fat` flavor gives LLVM the most inlining room; `thin` is
///   faster but less aggressive).
/// - `-C codegen-units=1` — force a single codegen unit so LLVM sees the
///   whole program at once (required for LTO to deliver its full benefit).
///
/// These are the rustc-level equivalent of Cargo's
/// `[profile.release]` block emitted by [`release_profile_toml`]. The
/// split exists because the v0.1 Buff pipeline invokes `rustc` directly on
/// a single `.rs` file (no Cargo project) — so the functional path goes
/// through these flags, while the TOML block is the documented contract
/// for any future Cargo-driven backend.
pub fn rustc_release_flags() -> Vec<&'static str> {
    vec![
        "-C",
        "opt-level=3",
        "-C",
        "lto=fat",
        "-C",
        "codegen-units=1",
    ]
}

/// Returns the Cargo `[profile.release]` TOML block that mirrors
/// [`rustc_release_flags`] (T56).
///
/// The block contains the three release knobs Cargo exposes for LTO +
/// maximum optimization:
///
/// ```toml
/// [profile.release]
/// lto = true
/// opt-level = 3
/// codegen-units = 1
/// ```
///
/// Although the current pipeline drives `rustc` directly (and therefore
/// uses [`rustc_release_flags`] functionally), this string is:
///
/// 1. The T56 QA assertion target — `release_profile_toml().contains("lto = true")`.
/// 2. The ready-to-inject block for `buff new` / `buff init` the day they
///    scaffold a `Cargo.toml` (multi-crate / FFI programs will need one).
/// 3. Documentation of the contract between the rustc-level flags and the
///    equivalent Cargo profile, so the two never silently drift.
///
/// Determinism: this is a pure fixed-string function — same output on every
/// call, no environment dependence, no side effects.
pub fn release_profile_toml() -> String {
    "[profile.release]\nlto = true\nopt-level = 3\ncodegen-units = 1\n".to_string()
}

/// Return `path` with the platform's executable extension applied.
///
/// - On Unix, `EXE_EXTENSION` is `""`, so the path is returned unchanged.
/// - On Windows, `EXE_EXTENSION` is `"exe"`: if `path` already ends with
///   `.exe` it is returned as-is, otherwise `.exe` is appended.
///
/// Callers should pass the result of this helper as the `-o` argument to
/// [`compile_rust_to_exe`] so that the produced executable has a conventional
/// name on the host platform.
pub fn with_exe_extension(path: &Path) -> PathBuf {
    let ext = std::env::consts::EXE_EXTENSION;
    if ext.is_empty() {
        return path.to_path_buf();
    }
    match path.extension() {
        Some(e) if e == ext => path.to_path_buf(),
        _ => {
            let mut p = path.to_path_buf();
            p.set_extension(ext);
            p
        }
    }
}

/// Format a phase-specific error (lex / parse / codegen) as a user-facing
/// anyhow error with line/column context when available.
///
/// (T35: made `pub` so [`crate::test_runner`] can reuse the same error
/// formatting without duplicating the logic.)
pub fn format_diagnostic_error(
    phase: &str,
    diagnostic: &buff_lang_error::Diagnostic,
    source_file: &SourceFile,
    file: &Path,
) -> anyhow::Error {
    let (line_col_note, source_line) = match source_file.lookup(diagnostic.span.start) {
        Some((line, col)) => (
            format!(" at {}:{}:{}\n  --> {}", file.display(), line, col, line),
            extract_source_line(source_file, line),
        ),
        None => (format!(" in {}", file.display()), String::new()),
    };

    let mut msg = format!("{phase} error: {}{line_col_note}", diagnostic.message);
    if !source_line.is_empty() {
        msg.push_str(&format!("\n      | {source_line}"));
    }
    for note in &diagnostic.notes {
        msg.push_str(&format!("\n  note: {note}"));
    }
    anyhow::anyhow!(msg)
}

/// Extract the 1-indexed `line_no` line of `source_file.content` (without the
/// trailing newline). Returns an empty string if the line number is out of
/// range.
fn extract_source_line(source_file: &SourceFile, line_no: usize) -> String {
    source_file
        .content
        .lines()
        .nth(line_no.saturating_sub(1))
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn with_exe_extension_unix_passthrough() {
        // On Unix, EXE_EXTENSION is "" so we always pass through.
        if std::env::consts::EXE_EXTENSION.is_empty() {
            assert_eq!(
                with_exe_extension(&PathBuf::from("/tmp/ola")),
                PathBuf::from("/tmp/ola")
            );
        }
    }

    #[test]
    fn with_exe_extension_appends_when_missing() {
        let p = with_exe_extension(&PathBuf::from("ola"));
        let ext = std::env::consts::EXE_EXTENSION;
        if ext.is_empty() {
            assert_eq!(p, PathBuf::from("ola"));
        } else {
            let mut expected = PathBuf::from("ola");
            expected.set_extension(ext);
            assert_eq!(p, expected);
        }
    }

    #[test]
    fn with_exe_extension_idempotent_when_already_present() {
        let ext = std::env::consts::EXE_EXTENSION;
        if ext.is_empty() {
            return;
        }
        let mut input = PathBuf::from("ola");
        input.set_extension(ext);
        let p = with_exe_extension(&input);
        assert_eq!(p, input, "should not double-append the extension");
    }

    #[test]
    fn extract_source_line_handles_utf8_and_multibyte() {
        let sf = SourceFile::new(
            PathBuf::from("test"),
            "first\nOlá, Buff!\nthird".to_string(),
        );
        assert_eq!(extract_source_line(&sf, 1), "first");
        assert_eq!(extract_source_line(&sf, 2), "Olá, Buff!");
        assert_eq!(extract_source_line(&sf, 3), "third");
        assert_eq!(extract_source_line(&sf, 99), "");
    }

    // -------------------------------------------------------------------------
    // T56: BuildMode + release helpers
    // -------------------------------------------------------------------------

    #[test]
    fn build_mode_from_release_flag_maps_bool() {
        assert_eq!(BuildMode::from_release_flag(false), BuildMode::Debug);
        assert_eq!(BuildMode::from_release_flag(true), BuildMode::Release);
    }

    #[test]
    fn build_mode_default_is_debug() {
        assert_eq!(BuildMode::default(), BuildMode::Debug);
    }

    #[test]
    fn build_mode_is_release_predicate() {
        assert!(!BuildMode::Debug.is_release());
        assert!(BuildMode::Release.is_release());
    }

    #[test]
    fn rustc_release_flags_contain_opt_level_3_and_lto() {
        let flags = rustc_release_flags();
        // Join to make the assertion robust against the interleaved `-C`
        // separator form (`["-C","opt-level=3","-C","lto=fat",...]`).
        let joined = flags.join(" ");
        assert!(
            joined.contains("opt-level=3"),
            "expected opt-level=3 in release flags, got: {joined}"
        );
        assert!(
            joined.contains("lto=fat"),
            "expected lto=fat in release flags, got: {joined}"
        );
        assert!(
            joined.contains("codegen-units=1"),
            "expected codegen-units=1 in release flags, got: {joined}"
        );
    }

    #[test]
    fn release_profile_toml_contains_lto_true_qa() {
        // T56 QA target — MUST contain `lto = true` (Cargo-profile form).
        let toml = release_profile_toml();
        assert!(
            toml.contains("lto = true"),
            "expected `lto = true` in release profile, got: {toml:?}"
        );
    }

    #[test]
    fn release_profile_toml_is_well_formed_profile_block() {
        let toml = release_profile_toml();
        assert!(
            toml.contains("[profile.release]"),
            "expected [profile.release] header, got: {toml:?}"
        );
        assert!(
            toml.contains("opt-level = 3"),
            "expected `opt-level = 3`, got: {toml:?}"
        );
        assert!(
            toml.contains("codegen-units = 1"),
            "expected `codegen-units = 1`, got: {toml:?}"
        );
    }

    #[test]
    fn release_profile_toml_is_deterministic() {
        // Pure fixed-string helper — same output every call.
        assert_eq!(release_profile_toml(), release_profile_toml());
    }

    // -------------------------------------------------------------------------
    // T133: .buffhtml pipeline (compile_buffhtml_to_rust + inline_script_block).
    // -------------------------------------------------------------------------

    /// Helper: write a fixture `.buffhtml` file in a unique-per-test temp dir.
    fn write_buffhtml_fixture(name: &str, contents: &str) -> PathBuf {
        // Use thread ID + process ID + name to avoid parallel-test collisions
        // (multiple pipeline tests share the std::process::id() namespace).
        let thread_id_str = format!("{:?}", std::thread::current().id());
        let thread_id_sanitised: String = thread_id_str
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let dir = std::env::temp_dir().join(format!(
            "buff-lang-cli-pipeline-tests-{}-{}",
            std::process::id(),
            thread_id_sanitised,
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap_or_else(|e| panic!("write {path:?}: {e}"));
        path
    }

    /// Helper: clean up a fixture path + its generated `.rs` sibling.
    /// Best-effort, never propagates errors. Does NOT remove the parent dir
    /// (it may be shared with parallel tests).
    fn cleanup_buffhtml_fixture(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("rs"));
    }

    #[test]
    fn derive_component_name_pascalcases_stem() {
        assert_eq!(
            derive_component_name(&PathBuf::from("counter.buffhtml")),
            "Counter"
        );
        assert_eq!(
            derive_component_name(&PathBuf::from("todo_list.buffhtml")),
            "TodoList"
        );
        assert_eq!(
            derive_component_name(&PathBuf::from("my-app/Counter.buffhtml")),
            "Counter"
        );
    }

    #[test]
    fn derive_component_name_handles_dotfile_stem() {
        // On Windows, `Path::new(".buffhtml").file_stem()` may return
        // "buffhtml" (treated as extension-only). Our function must still
        // produce a valid Rust ident — either the codegen default or a
        // sanitised form. Both are acceptable; we only assert non-empty
        // + valid first char.
        let name = derive_component_name(&PathBuf::from(".buffhtml"));
        assert!(!name.is_empty(), "name should be non-empty");
        assert!(
            name.chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_')
                .unwrap_or(false),
            "name should start with valid ident char, got {name:?}"
        );
    }

    #[test]
    fn inline_script_block_passes_through_when_no_const() {
        // No __BUFF_SCRIPT_SOURCE → unchanged.
        let src = "use dioxus::prelude::*;\nfn Foo() -> Element { rsx! {} }\n";
        let out = inline_script_block(src.to_string()).expect("inline ok");
        assert_eq!(out, src, "no script block → pass-through unchanged");
    }

    #[test]
    fn inline_script_block_splices_const_into_fn_body() {
        // Minimal source with __BUFF_SCRIPT_SOURCE const + a #[component] fn.
        // Use syn/prettyplease formatting on the way in so we know it's valid.
        let raw = "use dioxus::prelude::*;\n\
#[doc = \"buffhtml script block\"]\n\
const __BUFF_SCRIPT_SOURCE: &str = \"let mut count = 1;\";\n\
#[component]\n\
fn Counter() -> Element {\n    rsx! { \"hi\" }\n}\n";
        let out = inline_script_block(raw.to_string()).expect("inline ok");
        assert!(
            !out.contains("__BUFF_SCRIPT_SOURCE"),
            "const should be removed; got:\n{out}"
        );
        assert!(
            out.contains("let mut count = 1"),
            "script statement should be inside fn body; got:\n{out}"
        );
        assert!(
            out.contains("fn Counter()"),
            "component fn should remain; got:\n{out}"
        );
    }

    #[test]
    fn inline_script_block_errors_when_no_component_fn() {
        // Script present, but no #[component] fn → bail.
        let raw = "const __BUFF_SCRIPT_SOURCE: &str = \"let x = 1;\";\n";
        let result = inline_script_block(raw.to_string());
        assert!(result.is_err(), "expected error when no #[component] fn");
    }

    #[test]
    fn compile_buffhtml_to_rust_end_to_end_no_rustc() {
        // Vertical-slice test: parse + codegen + script-block splice + .rs write.
        // Does NOT invoke rustc — that's covered by `buff build` integration tests.
        let src = "<script lang=\"buff\">\n    let mut count = use_signal(|| 0);\n</script>\n\n<div>{count}</div>\n";
        let path = write_buffhtml_fixture("counter_test.buffhtml", src);

        let out = compile_buffhtml_to_rust(&path).expect("buffhtml compile ok");

        // .rs file written alongside.
        assert!(out.rust_file_path.exists(), "rs file should exist");
        assert_eq!(
            out.rust_file_path.extension().and_then(|e| e.to_str()),
            Some("rs"),
            "rs extension"
        );

        // Component name derived from stem (CounterTest).
        assert!(
            out.rust_source.contains("fn CounterTest()"),
            "expected CounterTest component; got:\n{}",
            out.rust_source
        );

        // Script statement spliced into fn body, NOT the const placeholder.
        assert!(
            !out.rust_source.contains("__BUFF_SCRIPT_SOURCE"),
            "const placeholder should be removed; got:\n{}",
            out.rust_source
        );
        assert!(
            out.rust_source.contains("let mut count = use_signal"),
            "script stmt should be inside fn body; got:\n{}",
            out.rust_source
        );

        cleanup_buffhtml_fixture(&path);
    }

    #[test]
    fn compile_to_rust_for_ext_dispatches_on_extension() {
        // Dispatcher picks buffhtml path when extension matches.
        let src = "<div>hello {1 + 2}</div>\n";
        let path = write_buffhtml_fixture("dispatch_test.buffhtml", src);

        let out = compile_to_rust_for_ext(&path).expect("dispatch ok");
        assert!(
            out.rust_source.contains("rsx!"),
            "expected rsx! macro; got:\n{}",
            out.rust_source
        );
        assert!(out.rust_file_path.exists());

        cleanup_buffhtml_fixture(&path);
    }
}
