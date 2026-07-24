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

use crate::compile_speed;

/// Compile-time optimization profile (T56 release / T60 minimal / T55 fast).
///
/// Selects which set of rustc flags [`compile_rust_to_exe`] passes to the
/// backend. `Debug` (the default) preserves the v0.1 behavior — a single
/// `-O` flag for fast compilation. `Release` enables LTO + maximum
/// optimization via [`rustc_release_flags`] for production-ready binaries.
/// `Minimal` (T60) optimizes for binary size via [`rustc_minimal_flags`]
/// (`opt-level=z`, `panic=abort`, `strip=symbols`, `lto=true`,
/// `codegen-units=1`) — used when the size budget matters more than
/// runtime speed (Lambda layers, embedded wasm shells, distribution
/// images). `Fast` (T55) disables optimization entirely
/// (`opt-level=0`, no LTO) for the fastest possible inner-loop compile —
/// the dev "I just want to see if it runs" mode.
///
/// This enum intentionally mirrors the user-facing `--release` /
/// `--minimal` / `--fast` CLI flags: the CLI translates the booleans into
/// [`BuildMode`] via [`BuildMode::from_flags_v2`] (or
/// [`BuildMode::from_flags`] for subcommands that pre-date T55), keeping
/// the pipeline decoupled from clap-level concerns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BuildMode {
    /// Fastest-possible compilation (T55 `--fast`): `opt-level=0`, no LTO.
    /// Skips ALL LLVM optimization so the edit-compile-run loop is as
    /// tight as possible. The binary is slower at runtime, but for "does
    /// it compile + run?" inner-loop feedback this is the right default.
    /// Distinct from [`BuildMode::Debug`] (which keeps `-O` =
    /// `opt-level=2`) — `Fast` is strictly faster to compile.
    Fast,
    /// Fast-debug compilation (v0.1 behavior): `rustc -O`. No LTO.
    /// Use this during development for tight edit-compile-run loops.
    #[default]
    Debug,
    /// Release-grade compilation: `opt-level=3` + `lto=fat` +
    /// `codegen-units=1`. Slower compile, smaller+faster binary.
    Release,
    /// Size-minimized compilation (T60): `opt-level=z` + `panic=abort` +
    /// `strip=symbols` + `lto=true` + `codegen-units=1`. Slowest compile,
    /// smallest binary. Use when binary size is the primary constraint
    /// (e.g. <5 MB target for console apps). Functional inverse of
    /// [`BuildMode::Release`] — Release trades size for speed, Minimal
    /// trades speed for size.
    Minimal,
}

impl BuildMode {
    /// Translate the CLI `--release` boolean into a [`BuildMode`].
    ///
    /// `true` → [`BuildMode::Release`], `false` → [`BuildMode::Debug`].
    /// This is the single source of truth for the flag→mode mapping — every
    /// caller (`buff build`, `buff run`) goes through here so the behavior
    /// stays consistent across subcommands.
    ///
    /// T60 note: subcommands that also accept `--minimal` should use
    /// [`BuildMode::from_flags`] instead — `Minimal` takes precedence over
    /// `Release` when both are set (mirrors `--release` precedence in
    /// cargo: a more-specific profile wins).
    pub fn from_release_flag(release: bool) -> Self {
        if release {
            BuildMode::Release
        } else {
            BuildMode::Debug
        }
    }

    /// Translate the CLI `--release` + `--minimal` booleans into a
    /// [`BuildMode`] (T60).
    ///
    /// Precedence (mirrors cargo's `--profile` semantics — more specific
    /// wins):
    ///
    /// - `minimal=true` → [`BuildMode::Minimal`] (regardless of `release`).
    /// - `minimal=false, release=true` → [`BuildMode::Release`].
    /// - `minimal=false, release=false` → [`BuildMode::Debug`] (default).
    ///
    /// The T60 acceptance ("console template builds <5 MB with `--minimal`")
    /// exercises the `minimal=true` arm. The `release=true` arm is the
    /// T56 contract preserved verbatim.
    pub fn from_flags(release: bool, minimal: bool) -> Self {
        if minimal {
            BuildMode::Minimal
        } else if release {
            BuildMode::Release
        } else {
            BuildMode::Debug
        }
    }

    /// Translate the CLI `--release` + `--minimal` + `--fast` booleans into a
    /// [`BuildMode`] (T55).
    ///
    /// Precedence (mirrors cargo's `--profile` semantics — more specific
    /// wins; the size-vs-speed-vs-compile-speed axes are mutually
    /// exclusive):
    ///
    /// - `minimal=true` → [`BuildMode::Minimal`] (regardless of others).
    /// - `minimal=false, release=true` → [`BuildMode::Release`].
    /// - `minimal=false, release=false, fast=true` → [`BuildMode::Fast`].
    /// - all `false` → [`BuildMode::Debug`] (default).
    ///
    /// `--fast` is strictly a dev-inner-loop flag (skip ALL optimisation
    /// for the fastest compile); `--release` (runtime speed) and
    /// `--minimal` (binary size) both override it when set together
    /// because a user who passes `--release --fast` clearly wants the
    /// optimised binary (the `--fast` was likely a leftover alias).
    pub fn from_flags_v2(release: bool, minimal: bool, fast: bool) -> Self {
        if minimal {
            BuildMode::Minimal
        } else if release {
            BuildMode::Release
        } else if fast {
            BuildMode::Fast
        } else {
            BuildMode::Debug
        }
    }

    /// Returns `true` when this mode is [`BuildMode::Release`].
    pub fn is_release(self) -> bool {
        matches!(self, BuildMode::Release)
    }

    /// Returns `true` when this mode is [`BuildMode::Minimal`] (T60).
    pub fn is_minimal(self) -> bool {
        matches!(self, BuildMode::Minimal)
    }

    /// Returns `true` when this mode is [`BuildMode::Fast`] (T55).
    pub fn is_fast(self) -> bool {
        matches!(self, BuildMode::Fast)
    }
}

/// User-facing debug-info selection for `buff build` / `buff run`.
///
/// Mirrors the `--debuginfo <line-tables-only|full|none>` CLI flag.
/// The pipeline passes the corresponding `-C debuginfo=N` flag to rustc.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DebugInfoChoice {
    /// `-C debuginfo=1` — line numbers only. Fast to compile, enough for
    /// backtraces and basic debugging. Default for dev builds.
    #[default]
    LineTablesOnly,
    /// `-C debuginfo=2` — full debug info (DWARF, etc.). Use when you
    /// need to step through code in gdb/lldb.
    Full,
    /// `-C debuginfo=0` — no debug info. Smallest binary, fastest
    /// compile. Use for production when you don't need backtraces.
    None,
}

/// Parse a `--debuginfo` CLI flag value into a [`DebugInfoChoice`].
///
/// Valid values: `line-tables-only`, `full`, `none`. Case-insensitive.
/// Returns an error for unknown values.
pub fn debuginfo_from_str(s: &str) -> Result<DebugInfoChoice> {
    match s.to_ascii_lowercase().as_str() {
        "line-tables-only" => Ok(DebugInfoChoice::LineTablesOnly),
        "full" => Ok(DebugInfoChoice::Full),
        "none" => Ok(DebugInfoChoice::None),
        other => bail!(
            "unknown debuginfo `{other}` (valid: line-tables-only, full, none)"
        ),
    }
}

impl DebugInfoChoice {
    /// Return the rustc `-C debuginfo=N` value for this choice.
    /// The caller should pass `-C` as a separate arg before this value.
    pub fn to_rustc_arg(&self) -> &'static str {
        match self {
            DebugInfoChoice::LineTablesOnly => "debuginfo=1",
            DebugInfoChoice::Full => "debuginfo=2",
            DebugInfoChoice::None => "debuginfo=0",
        }
    }
}

/// User-facing codegen backend selection for `buff build` / `buff run` (T4).
///
/// Mirrors the `--backend <llvm|cranelift>` CLI flag. The pipeline sets
/// the `CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift` env var on the
/// spawned rustc process when [`BackendChoice::Cranelift`] is selected
/// AND the build mode is [`BuildMode::Debug`]. Release builds always
/// use LLVM (no exception — Cranelift output is for dev inner-loop
/// speed only, never for shipped binaries).
///
/// When `Cranelift` is requested but not available (probe via
/// [`cranelift_available`] fails), the pipeline falls back to LLVM with
/// an `eprintln!` warning. Correctness is NEVER affected by the backend
/// choice — Cranelift output is behaviorally identical to LLVM output,
/// just (sometimes) faster to produce and slower to run.
///
/// # Stability
///
/// This enum is additive — future backends (`Cranelift`, others) are
/// appended as new variants. The `llvm` default never changes so the
/// absence of `--backend` always means "rustc's default backend".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackendChoice {
    /// LLVM backend (rustc default). The only backend used for release
    /// builds. Default for dev builds too — Cranelift is opt-in.
    #[default]
    Llvm,
    /// Cranelift backend (dev only). Faster compile, slower runtime.
    /// Requires nightly rustc + the `rustc-codegen-cranelift-preview`
    /// component. Falls back to LLVM with a warning when unavailable.
    Cranelift,
}

/// Parse a `--backend` CLI flag value into a [`BackendChoice`] (T4).
///
/// Valid values: `llvm`, `cranelift`. Case-insensitive.
/// Returns an error for unknown values.
pub fn backend_from_str(s: &str) -> Result<BackendChoice> {
    match s.to_ascii_lowercase().as_str() {
        "llvm" => Ok(BackendChoice::Llvm),
        "cranelift" => Ok(BackendChoice::Cranelift),
        other => bail!(
            "unknown backend `{other}` (valid: llvm, cranelift)"
        ),
    }
}

/// Probe whether the Cranelift codegen backend is available (T4).
///
/// Runs `rustc +nightly -C codegen-backend=cranelift --version` (silent
/// — output is discarded). Returns `true` when the probe succeeds (exit
/// 0), `false` on any failure (missing nightly, missing component,
/// rustc not on PATH, etc.).
///
/// This is the single source of truth for "is Cranelift usable on this
/// host?" — both the CLI pipeline and `buff-eval` consult it before
/// setting `CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift` on the spawned
/// rustc process. The probe is cheap (sub-second) and runs at most once
/// per `compile_rust_to_exe_with_speed` call.
///
/// # Why `+nightly`
///
/// `rustc-codegen-cranelift-preview` is currently nightly-only on
/// stable rustup channels. The probe therefore uses `+nightly` to
/// exercise the actual toolchain that would be used. A future stable
/// promotion would simplify this to bare `rustc`.
pub fn cranelift_available() -> bool {
    let probe = Command::new("rustc")
        .arg("+nightly")
        .arg("-C")
        .arg("codegen-backend=cranelift")
        .arg("--version")
        .output();
    match probe {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

/// User-facing linker selection for `buff build` / `buff run`.
///
/// Mirrors the `--linker <auto|mold|lld|system>` CLI flag. The pipeline
/// resolves this to a concrete [`compile_speed::FastLinker`] via
/// [`resolve_linker`] before passing flags to rustc.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LinkerChoice {
    /// Auto-detect: probe PATH for mold (Linux) → rust-lld → system default.
    #[default]
    Auto,
    /// Explicitly use the `mold` linker. Errors if mold is not on PATH.
    Mold,
    /// Explicitly use the `lld` linker (rust-lld or bare lld). Errors if
    /// neither is on PATH.
    Lld,
    /// Use rustc's default system linker (no `-C link-arg=-fuse-ld` flag).
    System,
}

/// Parse a `--linker` CLI flag value into a [`LinkerChoice`].
///
/// Valid values: `auto`, `mold`, `lld`, `system`. Case-insensitive.
/// Returns an error for unknown values.
pub fn linker_from_str(s: &str) -> Result<LinkerChoice> {
    match s.to_ascii_lowercase().as_str() {
        "auto" => Ok(LinkerChoice::Auto),
        "mold" => Ok(LinkerChoice::Mold),
        "lld" => Ok(LinkerChoice::Lld),
        "system" => Ok(LinkerChoice::System),
        other => bail!(
            "unknown linker `{other}` (valid: auto, mold, lld, system)"
        ),
    }
}

/// Resolve a [`LinkerChoice`] to a concrete [`compile_speed::FastLinker`],
/// probing PATH as needed.
///
/// - [`LinkerChoice::Auto`] → [`compile_speed::FastLinker::detect()`]
///   (mold → rust-lld → lld → None).
/// - [`LinkerChoice::Mold`] → [`compile_speed::FastLinker::Mold`] if mold
///   is on PATH, else an error.
/// - [`LinkerChoice::Lld`] → [`compile_speed::FastLinker::Lld`] if rust-lld
///   or lld is on PATH, else an error.
/// - [`LinkerChoice::System`] → [`compile_speed::FastLinker::None`].
///
/// # Errors
///
/// Returns an error when an explicit linker (`Mold` or `Lld`) is requested
/// but not found on PATH. [`LinkerChoice::Auto`] and [`LinkerChoice::System`]
/// never error.
pub fn resolve_linker(choice: LinkerChoice) -> Result<compile_speed::FastLinker> {
    match choice {
        LinkerChoice::Auto => Ok(compile_speed::FastLinker::detect()),
        LinkerChoice::Mold => {
            if compile_speed::on_path("mold") {
                Ok(compile_speed::FastLinker::Mold)
            } else {
                bail!(
                    "`--linker=mold` requested but `mold` was not found on PATH. \
                     Install mold (https://github.com/rui314/mold) or use \
                     `--linker=auto` / `--linker=system`."
                );
            }
        }
        LinkerChoice::Lld => {
            if compile_speed::on_path("rust-lld") || compile_speed::on_path("lld") {
                Ok(compile_speed::FastLinker::Lld)
            } else {
                bail!(
                    "`--linker=lld` requested but neither `rust-lld` nor `lld` \
                     was found on PATH. Use `--linker=auto` / `--linker=system`."
                );
            }
        }
        LinkerChoice::System => Ok(compile_speed::FastLinker::None),
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
/// **T55 caching**: this entry point checks the generated-Rust cache
/// ([`compile_speed::read_cache`]) keyed on a SHA-256 hash of the source
/// bytes BEFORE running codegen. On a cache hit, the entire lex → parse →
/// codegen pass is skipped — the cached `.rs` content is written alongside
/// the source and returned directly. This saves 30-50% on repeat builds
/// (the codegen pass is the bulk of front-end time). Equivalent to
/// [`compile_to_rust_with_cache`] with `use_cache = true`.
///
/// # Errors
///
/// Returns an error if the file cannot be read, lexing fails, parsing fails,
/// codegen fails, or the `.rs` file cannot be written. Every error message
/// includes the source filename and (where possible) the line/column of the
/// offending span via [`SourceFile::lookup`].
pub fn compile_to_rust(file: &Path) -> Result<CompileOutput> {
    compile_to_rust_with_cache(file, true)
}

/// Cache-controllable variant of [`compile_to_rust`] (T55).
///
/// When `use_cache` is `true` (the default for [`compile_to_rust`]):
/// 1. Read the source, compute [`compile_speed::source_cache_key`].
/// 2. Probe [`compile_speed::read_cache`] for a cached `.rs`.
/// 3. On hit → write the cached content to `file.rs` and return (codegen
///    skipped entirely).
/// 4. On miss → run the normal lex → parse → codegen, then
///    [`compile_speed::write_cache`] the result for next time.
///
/// When `use_cache` is `false` (`buff build --no-cache`), the cache is
/// bypassed completely — always runs the full front-end. Used for
/// debugging cache-corruption suspicion and for forcing a codegen refresh
/// after a compiler upgrade (the cache key is source-only, so a new
/// compiler version would serve stale output).
///
/// # Cache-write failure is non-fatal
///
/// If [`compile_speed::write_cache`] fails (disk full, permissions), the
/// error is logged to stderr but the build proceeds — the `.rs` file is
/// already written alongside the source, so rustc can still compile it.
/// A cache that can't be written to simply can't accelerate the NEXT build.
pub fn compile_to_rust_with_cache(file: &Path, use_cache: bool) -> Result<CompileOutput> {
    // 1. Read source.
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read source file `{}`", file.display()))?;

    // T55: probe the generated-Rust cache BEFORE running codegen. A hit
    // skips the entire lex → parse → syn/quote/prettyplease pass.
    let cache_key = compile_speed::source_cache_key(&source);
    if use_cache {
        if let Some(cached) = compile_speed::read_cache(&cache_key) {
            let rust_file_path = file.with_extension("rs");
            std::fs::write(&rust_file_path, &cached)
                .with_context(|| format!("failed to write `{}`", rust_file_path.display()))?;
            return Ok(CompileOutput {
                rust_source: cached,
                rust_file_path,
            });
        }
    }

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

    // T55: populate the cache for next time. Non-fatal on failure (the .rs
    // is already written; we just can't accelerate the next build).
    if use_cache {
        if let Err(e) = compile_speed::write_cache(&cache_key, &rust_source) {
            eprintln!("note: buff-cache write failed ({e}); build proceeds uncached");
        }
    }

    Ok(CompileOutput {
        rust_source,
        rust_file_path,
    })
}

/// T7: Incremental variant of [`compile_to_rust`] that consults a
/// [`salsa`]-backed [`crate::incremental::BuffDatabase`] before running
/// the front-end.
///
/// When called multiple times within a single CLI session (e.g. `buff
/// watch`, `buff repl`, the LSP server), the database memoizes the
/// lex + parse + typecheck passes keyed on the source file's path +
/// content. Unchanged inputs return the cached [`ParseOutcome`] /
/// [`TypeCheckOutcome`] without re-running tokenize + parse. This is
/// the primary incremental win for long-running sessions.
///
/// For one-shot invocations (`buff run file.buff`) the salsa layer
/// runs tokenize + parse once internally (populating the DB) and then
/// falls through to [`compile_to_rust_with_cache`] which re-runs them
/// to materialize the `Vec<Decl>` for codegen. Salsa's memoization
/// guarantees the source bytes are hot in the OS file cache for that
/// re-materialization, so the overhead is one extra tokenize + parse
/// pass — negligible relative to the rustc backend. The T55 `.rs`
/// byte-cache (keyed on a SHA-256 of the source) short-circuits
/// codegen entirely when the source is unchanged across invocations.
///
/// # Correctness
///
/// Salsa is purely a memoization cache. The underlying lex + parse +
/// codegen pipeline runs identically with or without it; the generated
/// Rust source is byte-identical. On a parse failure the regular
/// diagnostic path ([`format_diagnostic_error`]) is invoked via the
/// fallback to [`compile_to_rust_with_cache`].
///
/// # Errors
///
/// Propagates file-read / lex / parse / codegen errors identically to
/// [`compile_to_rust_with_cache`].
pub fn compile_to_rust_incremental(
    file: &Path,
    db: &mut crate::incremental::BuffDatabase,
) -> Result<CompileOutput> {
    // 1. Read source ONCE. Used both for the salsa input registration
    //    AND (via the fallthrough to compile_to_rust_with_cache) for
    //    the actual codegen pass.
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read source file `{}`", file.display()))?;

    // 2. Register the file as a salsa input. Salsa tracks (path,
    //    source) for change detection — a subsequent call with the
    //    same pair is a memoized no-op.
    let src_file = crate::incremental::SourceFile::new(db, file.to_path_buf(), source);

    // 3. Warm the parse + typecheck caches. On a cache hit (unchanged
    //    source since the last call on this DB) these return
    //    immediately without re-running tokenize + parse. On a miss
    //    they execute the full front-end inline.
    //
    //    We ignore the outcomes here — the actual diagnostic surface
    //    + codegen happens via the fallthrough below. The salsa layer
    //    is purely for change detection + memoization.
    let _parse_outcome = crate::incremental::parse_file(db, src_file);
    let _tc_outcome = crate::incremental::typecheck_file(db, src_file);

    // 4. Fall through to the regular pipeline for codegen. Salsa has
    //    already populated its memoization tables; the T55 `.rs`
    //    byte-cache short-circuits the codegen pass when the source
    //    hash matches.
    compile_to_rust_with_cache(file, true)
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
/// - [`BuildMode::Fast`] (T55): [`rustc_fast_flags()`] — `opt-level=0`, no
///   LTO. Fastest compile, slowest runtime. The dev inner-loop default
///   behind `buff build --fast`.
/// - [`BuildMode::Debug`] (default, v0.1 behavior): just `-O`
///   (equivalent to `-C opt-level=2`). Fast compilation, no LTO.
/// - [`BuildMode::Release`] (T56): [`rustc_release_flags()`] — `opt-level=3`
///   + `lto=fat` + `codegen-units=1`. Slower compilation, faster runtime.
///
/// **Linker selection**: uses [`LinkerChoice::default()`] (Auto) which
/// probes PATH for mold (Linux) → rust-lld → system default. Callers that
/// need explicit control should use [`compile_rust_to_exe_with_speed`]
/// with a [`LinkerChoice`] argument.
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
/// Equivalent to [`compile_rust_to_exe_with_speed`] with `use_sccache =
/// false`, `linker = LinkerChoice::default()`,
/// `debuginfo = DebugInfoChoice::default()`, and
/// `backend = BackendChoice::default()` (sccache is opt-in via
/// `buff build --sccache`).
///
/// # Errors
///
/// - Fails if `rustc` cannot be invoked (not installed / not in `PATH`).
/// - Fails if `rustc` exits with a non-zero status. Translated `rustc`
///   diagnostics are forwarded to the caller's stderr before bailing.
/// - Fails if an explicit linker is requested but not found on PATH
///   (only when called via [`compile_rust_to_exe_with_speed`] with
///   [`LinkerChoice::Mold`] or [`LinkerChoice::Lld`]).
pub fn compile_rust_to_exe(
    rust_file: &Path,
    output: &Path,
    buff_file: &Path,
    mode: BuildMode,
) -> Result<PathBuf> {
    compile_rust_to_exe_with_speed(rust_file, output, buff_file, mode, false, LinkerChoice::default(), DebugInfoChoice::default(), BackendChoice::default(), None)
}

/// sccache-aware variant of [`compile_rust_to_exe`] (T55).
///
/// Identical to [`compile_rust_to_exe`] except:
///
/// - The rustc invocation is wrapped in `sccache` when `use_sccache` is
///   `true` AND sccache is available on `PATH` (see
///   [`compile_speed::rustc_command`]). When sccache is requested but
///   missing, the build falls back to bare `rustc` with a stderr note
///   rather than failing.
/// - The linker is selected via the `linker` parameter (see [`LinkerChoice`]
///   and [`resolve_linker`]) instead of the hardcoded auto-detect. Pass
///   [`LinkerChoice::default()`] for the same behaviour as
///   [`compile_rust_to_exe`].
/// - The debug-info level is selected via `debuginfo` (T3). Pass
///   [`DebugInfoChoice::default()`] for the same behaviour as
///   [`compile_rust_to_exe`].
/// - The codegen backend is selected via `backend` (T4). Pass
///   [`BackendChoice::default()`] for the same behaviour as
///   [`compile_rust_to_exe`]. `Cranelift` is honoured ONLY for
///   [`BuildMode::Debug`]; release/minimal/fast builds always use LLVM
///   (the env-var gate is the safety rail — a `--release --backend=cranelift`
///   invocation silently uses LLVM rather than risking a non-LLVM
///   shipped binary).
/// - The cross-compilation target is selected via `target` (T112). When
///   `Some(triple)`, passes `--target <triple>` to rustc. Before invoking
///   rustc, probes `rustup target list --installed` to verify the target
///   is installed; if not, returns a clear error with the install command.
///   When `None` (default), no `--target` flag is passed (native compilation).
pub fn compile_rust_to_exe_with_speed(
    rust_file: &Path,
    output: &Path,
    buff_file: &Path,
    mode: BuildMode,
    use_sccache: bool,
    linker: LinkerChoice,
    debuginfo: DebugInfoChoice,
    backend: BackendChoice,
    target: Option<&str>,
) -> Result<PathBuf> {
    let sccache_on = use_sccache && compile_speed::sccache_available();
    if use_sccache && !sccache_on {
        eprintln!(
            "note: --sccache requested but `sccache` not found on PATH; \
             falling back to bare rustc"
        );
    }
    let mut cmd = compile_speed::rustc_command(use_sccache);
    cmd.arg("--edition").arg("2021");

    // Select the optimization/LTO flag set based on the build mode.
    // Debug keeps the v0.1 `-O` exactly — byte-identical behavior with the
    // pre-T56 pipeline. Release swaps in the LTO + opt-level=3 + single
    // codegen-unit block. Minimal (T60) adds size-first knobs:
    // opt-level=z + panic=abort + strip=symbols. Fast (T55) disables all
    // optimisation (opt-level=0) for the fastest possible compile.
    match mode {
        BuildMode::Fast => {
            for flag in rustc_fast_flags() {
                cmd.arg(flag);
            }
        }
        BuildMode::Debug => {
            cmd.arg("-O");
        }
        BuildMode::Release => {
            for flag in rustc_release_flags() {
                cmd.arg(flag);
            }
        }
        BuildMode::Minimal => {
            for flag in rustc_minimal_flags() {
                cmd.arg(flag);
            }
        }
    }

    // T2: resolve the linker choice (auto-detect or explicit) and pass
    // the -fuse-ld flag. No-op when the resolved linker is None (system
    // default). Errors when an explicit linker is requested but missing.
    let resolved = resolve_linker(linker)?;
    if resolved.is_fast() {
        for flag in resolved.rustc_flags() {
            cmd.arg(flag);
        }
        eprintln!("note: using fast linker `{}`", resolved.name());
    }

    // T4: Cranelift dev backend. ONLY honoured for Debug builds —
    // release/minimal/fast always use LLVM (the env-var gate is the
    // safety rail that prevents a non-LLVM shipped binary even when the
    // user passes `--release --backend=cranelift`). When Cranelift is
    // requested but unavailable (probe fails), fall back to LLVM with
    // a warning — correctness is never affected, only compile speed.
    //
    // The env var is set on the child `Command` (not via
    // `std::env::set_var`) so it is scoped to the rustc subprocess and
    // does not leak into the parent `buff` process or any subsequent
    // `Command::new` call in the same session.
    if matches!(backend, BackendChoice::Cranelift) && matches!(mode, BuildMode::Debug) {
        if cranelift_available() {
            cmd.env("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "cranelift");
            eprintln!(
                "note: using Cranelift dev backend (T4) — faster compile, \
                 slower runtime; release builds always use LLVM"
            );
        } else {
            eprintln!(
                "warning: --backend=cranelift requested but Cranelift is not \
                 available (install via `rustup component add \
                 rustc-codegen-cranelift-preview` on nightly); falling back \
                 to LLVM"
            );
        }
    }

    // T3: debug-info control. For Debug/Fast builds, default to
    // LineTablesOnly (-C debuginfo=1). For Release/Minimal, keep the
    // existing behavior (don't force debuginfo unless explicitly set).
    // The user's explicit --debuginfo flag always wins.
    cmd.arg("-C");
    cmd.arg(debuginfo.to_rustc_arg());

    // T112: cross-compilation target. When set, verify the target is
    // installed via `rustup target list --installed` before passing
    // `--target <triple>` to rustc. If not installed, surface a clear
    // error with the install command.
    if let Some(triple) = target {
        if !target_is_installed(triple) {
            bail!(
                "Target `{triple}` is not installed.\n\
                 Run: rustup target add {triple}\n\n\
                 Common targets:\n\
                   x86_64-unknown-linux-gnu   (Linux x86_64)\n\
                   aarch64-apple-darwin        (Apple Silicon macOS)\n\
                   x86_64-pc-windows-msvc     (Windows x86_64)\n\
                   wasm32-unknown-unknown      (WebAssembly)"
            );
        }
        cmd.arg("--target");
        cmd.arg(triple);
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
        BuildMode::Fast => {
            for flag in rustc_fast_flags() {
                cmd.arg(flag);
            }
        }
        BuildMode::Debug => {
            cmd.arg("-O");
        }
        BuildMode::Release => {
            for flag in rustc_release_flags() {
                cmd.arg(flag);
            }
        }
        BuildMode::Minimal => {
            for flag in rustc_minimal_flags() {
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

/// The rustc CLI flags that implement the no-optimization "fast" profile
/// (T55).
///
/// Returns the argument sequence passed verbatim to `rustc` when
/// [`compile_rust_to_exe`] is called with [`BuildMode::Fast`]:
///
/// - `-C opt-level=0` — disable ALL LLVM optimization. The single biggest
///   compile-time knob: LLVM's optimisation passes are the bulk of rustc's
///   wall-clock time on small programs. `opt-level=0` skips them entirely.
/// - `-C debuginfo=0` — omit debug symbols (they slow the linker). The
///   `--fast` mode is for "does it compile + run?" feedback, not
///   debugging; users who want debug info should use the default
///   [`BuildMode::Debug`] (which keeps the v0.1 `-O` + default debuginfo).
///
/// No LTO (LTO is a pure compile-time cost with no benefit when there's no
/// optimisation to do). This is strictly faster to compile than
/// [`BuildMode::Debug`] (which runs `opt-level=2`).
///
/// These are the rustc-level equivalent of a Cargo `[profile.dev]` block
/// with `opt-level = 0`. The split exists because the v0.1 Buff pipeline
/// invokes `rustc` directly on a single `.rs` file (no Cargo project) —
/// so the functional path goes through these flags, while a Cargo-driven
/// backend would use the profile block.
pub fn rustc_fast_flags() -> Vec<&'static str> {
    vec!["-C", "opt-level=0", "-C", "debuginfo=0"]
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

/// The rustc CLI flags that implement size-minimized compilation (T60).
///
/// Returns the argument sequence passed verbatim to `rustc` when
/// [`compile_rust_to_exe`] is called with [`BuildMode::Minimal`]:
///
/// - `-C opt-level=z` — LLVM optimize for SIZE (vs `opt-level=3` for
///   speed in [`BuildMode::Release`]). The single setting that
///   distinguishes `--minimal` from `--release`.
/// - `-C panic=abort` — replace the unwind payload (landing pads +
///   libunwind linkage) with a single abort shim. Biggest single
///   size win on small programs (typically -15..-25%).
/// - `-C strip=symbols` — pass `--strip-all` to the linker. Drops
///   symbol tables + debug info from the final binary.
/// - `-C lto=true` — whole-program Link-Time Optimization. Lets LLVM
///   eliminate dead code across crate boundaries that per-crate
///   `opt-level` cannot see. (`true` is the thin-lto default flavor;
///   [`rustc_release_flags`] uses `lto=fat` for max inlining — Minimal
///   prefers the smaller `true` since size is the primary target.)
/// - `-C codegen-units=1` — force a single codegen unit so LLVM sees
///   the whole program at once (required for LTO to deliver its full
///   benefit).
///
/// These are the rustc-level equivalent of Cargo's
/// `[profile.minimal]` block emitted by [`minimal_profile_toml`] (and
/// declared in the workspace-root `Cargo.toml`). The split exists
/// because the v0.1 Buff pipeline invokes `rustc` directly on a single
/// `.rs` file (no Cargo project) — so the functional path goes through
/// these flags, while the TOML block is the contract for any
/// Cargo-driven backend.
///
/// T60 acceptance: console-template Buff app builds <5 MB with these
/// flags + no extern crates. See `examples/minimal_console.buff`.
pub fn rustc_minimal_flags() -> Vec<&'static str> {
    vec![
        "-C",
        "opt-level=z",
        "-C",
        "panic=abort",
        "-C",
        "strip=symbols",
        "-C",
        "lto=true",
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

/// Returns the Cargo `[profile.minimal]` TOML block that mirrors
/// [`rustc_minimal_flags`] (T60).
///
/// The block contains the size-minimization knobs Cargo exposes for
/// binary-size-optimized builds:
///
/// ```toml
/// [profile.minimal]
/// inherits = "release"
/// panic = "abort"
/// strip = true
/// opt-level = "z"
/// lto = true
/// codegen-units = 1
/// ```
///
/// `inherits = "release"` is the key Cargo-only knob — it layers the
/// size-first settings on top of the release-grade baseline (so users
/// get the `-C opt-level=3` groundwork + Release LTO head start before
/// the size-first overrides kick in). The rustc-level equivalent
/// ([`rustc_minimal_flags`]) cannot express `inherits` (rustc has no
/// profile inheritance), so it specifies every flag directly.
///
/// Although the current single-file pipeline drives `rustc` directly
/// (and therefore uses [`rustc_minimal_flags`] functionally), this
/// string is:
///
/// 1. The T60 QA assertion target — `minimal_profile_toml().contains("opt-level = \"z\"")`.
/// 2. Already declared in the workspace-root `Cargo.toml` so
///    `cargo build --profile minimal` works in any cargo-driven path
///    (project / workspace builds via [`crate::project_pipeline`]).
/// 3. Documentation of the contract between the rustc-level flags and
///    the equivalent Cargo profile, so the two never silently drift.
///
/// Determinism: this is a pure fixed-string function — same output on
/// every call, no environment dependence, no side effects.
pub fn minimal_profile_toml() -> String {
    "[profile.minimal]\ninherits = \"release\"\npanic = \"abort\"\nstrip = true\nopt-level = \"z\"\nlto = true\ncodegen-units = 1\n".to_string()
}

/// Probe whether a rustc target triple is installed (T112).
///
/// Runs `rustup target list --installed` and checks if `<triple>` appears
/// in the output. Returns `true` when the target is listed, `false` on
/// any failure (rustup not on PATH, probe error, target not found).
///
/// The probe is cheap (sub-second) and runs at most once per
/// `compile_rust_to_exe_with_speed` call when `--target` is set.
fn target_is_installed(triple: &str) -> bool {
    let probe = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    match probe {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().any(|line| line.trim() == triple)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// T62: Profile-Guided Optimization (PGO) helpers.
// ---------------------------------------------------------------------------

/// Default directory where PGO profiling data is stored (T62).
///
/// Phase 1 (`buff build --pgo`) passes this to rustc via
/// `-C profile-generate=<PGO_DATA_DIR>` so the instrumented binary
/// writes its `*.profraw` counter files here on every run. Phase 3
/// (`buff build --pgo --use`) merges them into
/// `<PGO_DATA_DIR>/merged.profdata` via `llvm-profdata` and feeds
/// that back to rustc via `-C profile-use=<PGO_DATA_DIR>/merged.profdata`.
pub const PGO_DATA_DIR: &str = "./target/pgo-data";

/// Filename for the merged profile data consumed by Phase 3 (T62).
///
/// `llvm-profdata merge *.profdata -o <PGO_DATA_DIR>/<PGO_MERGED_PROFILE>`
/// produces this file; rustc then consumes it via
/// `-C profile-use=<PGO_DATA_DIR>/<PGO_MERGED_PROFILE>`.
pub const PGO_MERGED_PROFILE: &str = "merged.profdata";

/// The rustc CLI flags for Phase 1 of PGO — the instrumented build (T62).
///
/// Returns the argument sequence passed verbatim to `rustc` when
/// `buff build --pgo` runs Phase 1:
///
/// - `-C profile-generate=<dir>` — emit edge-profiling counters into
///   `<dir>/` on every execution of the resulting binary. The counters
///   land as `*.profraw` files (one per process run).
/// - `-C opt-level=3` + `-C lto=fat` + `-C codegen-units=1` — the same
///   release-grade baseline as [`rustc_release_flags`], so the
///   instrumented binary's runtime characteristics match the final
///   profile-guided build (the profile is only useful if the workload
///   exercises representative code paths at representative speeds).
///
/// The `<dir>` defaults to [`PGO_DATA_DIR`] but is parameterised so
/// callers (notably `commands::pgo`) can override it for tests or
/// custom layouts.
pub fn rustc_pgo_instrument_flags(profile_dir: &str) -> Vec<String> {
    vec![
        "-C".to_string(),
        format!("profile-generate={profile_dir}"),
        "-C".to_string(),
        "opt-level=3".to_string(),
        "-C".to_string(),
        "lto=fat".to_string(),
        "-C".to_string(),
        "codegen-units=1".to_string(),
    ]
}

/// The rustc CLI flags for Phase 3 of PGO — the profile-guided rebuild (T62).
///
/// Returns the argument sequence passed verbatim to `rustc` when
/// `buff build --pgo --use` runs Phase 3:
///
/// - `-C profile-use=<merged.profdata>` — feed the merged profile data
///   (produced by `llvm-profdata merge` over the Phase 1 `.profraw`
///   files) back to LLVM so it can drive inlining + block-layout
///   decisions. Typically yields 10%+ speedup vs `--release` on
///   compute-heavy code.
/// - `-C opt-level=3` + `-C lto=fat` + `-C codegen-units=1` — the same
///   release-grade baseline as [`rustc_release_flags`] (must match
///   Phase 1's [`rustc_pgo_instrument_flags`] so the profile maps onto
///   the same inlining decisions).
///
/// The `<merged_path>` is `<profile_dir>/<PGO_MERGED_PROFILE>` by
/// convention (see [`pgo_merged_profile_path`]).
pub fn rustc_pgo_use_flags(merged_profile_path: &str) -> Vec<String> {
    vec![
        "-C".to_string(),
        format!("profile-use={merged_profile_path}"),
        "-C".to_string(),
        "opt-level=3".to_string(),
        "-C".to_string(),
        "lto=fat".to_string(),
        "-C".to_string(),
        "codegen-units=1".to_string(),
    ]
}

/// Compute the conventional merged-profile path from a profile-data dir (T62).
///
/// `<dir>` defaults to [`PGO_DATA_DIR`] when `None`. Returns
/// `<dir>/<PGO_MERGED_PROFILE>` so callers don't have to repeat the
/// path-join incantation. Used by `commands::pgo` to pass to both
/// `llvm-profdata merge -o <path>` (Phase 3 setup) and
/// [`rustc_pgo_use_flags`] (Phase 3 rustc invocation).
pub fn pgo_merged_profile_path(profile_dir: Option<&str>) -> String {
    let dir = profile_dir.unwrap_or(PGO_DATA_DIR);
    format!("{dir}/{PGO_MERGED_PROFILE}")
}

/// Returns the Cargo `[profile.pgo]` TOML block that mirrors the
/// workspace-root declaration (T62).
///
/// The block contains the LTO + codegen-units baseline that BOTH PGO
/// phases share:
///
/// ```toml
/// [profile.pgo]
/// inherits = "release"
/// lto = "fat"
/// codegen-units = 1
/// ```
///
/// **Why just `inherits = "release"` + LTO knobs**: the actual PGO
/// flags (`-C profile-generate` / `-C profile-use`) are phase-dependent
/// and MUST be passed dynamically by `buff build --pgo` (a static
/// Cargo profile cannot express "instrument on first build, use on
/// second"). This profile therefore only fixes the baseline that both
/// phases share so cargo-driven paths selecting `--profile pgo` get
/// the same groundwork as the single-file rustc path.
///
/// Determinism: this is a pure fixed-string function — same output on
/// every call, no environment dependence, no side effects.
pub fn pgo_profile_toml() -> String {
    "[profile.pgo]\ninherits = \"release\"\nlto = \"fat\"\ncodegen-units = 1\n".to_string()
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

    // -------------------------------------------------------------------------
    // T2: LinkerChoice resolution tests.
    // -------------------------------------------------------------------------

    #[test]
    fn linker_choice_default_is_auto() {
        assert_eq!(LinkerChoice::default(), LinkerChoice::Auto);
    }

    #[test]
    fn linker_from_str_parses_valid_values() {
        assert_eq!(linker_from_str("auto").unwrap(), LinkerChoice::Auto);
        assert_eq!(linker_from_str("mold").unwrap(), LinkerChoice::Mold);
        assert_eq!(linker_from_str("lld").unwrap(), LinkerChoice::Lld);
        assert_eq!(linker_from_str("system").unwrap(), LinkerChoice::System);
        // Case-insensitive.
        assert_eq!(linker_from_str("AUTO").unwrap(), LinkerChoice::Auto);
        assert_eq!(linker_from_str("Mold").unwrap(), LinkerChoice::Mold);
    }

    #[test]
    fn linker_from_str_rejects_unknown() {
        assert!(linker_from_str("garbage").is_err());
        assert!(linker_from_str("").is_err());
    }

    #[test]
    fn resolve_linker_system_returns_none() {
        let resolved = resolve_linker(LinkerChoice::System).unwrap();
        assert!(!resolved.is_fast());
        assert!(resolved.rustc_flags().is_empty());
    }

    #[test]
    fn resolve_linker_auto_never_errors() {
        // Auto must always resolve without error (may be None on any host).
        let resolved = resolve_linker(LinkerChoice::Auto).unwrap();
        // No assertion on is_fast — it depends on what's on PATH.
        // The important thing is it doesn't panic or error.
    }
}
