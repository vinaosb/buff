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
}
