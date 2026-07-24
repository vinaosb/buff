//! `buff-eval` — a thin evaluation engine over the existing Buff compiler
//! primitives (`tokenize`, `parse`, `TypeInferencer`, `generate_rust`).
//!
//! # Shared rustc-invoke helpers (T35)
//!
//! The `rustc_invoke` module is included via `#[path]` from
//! `buff-lang-cli/src/rustc_invoke.rs` — the single source of truth for
//! PATH probes, Cranelift detection, target-installed checks, and common
//! rustc flag configuration. This avoids duplicating the logic while
//! keeping `buff-eval` free of `clap`/`tokio` transitive dependencies
//! (the shared module itself has zero CLI-specific deps).
//!
//! # Purpose
//!
//! T125-prep extracts a reusable evaluation API consumed by the REPL
//! (T125a/b/c), the Jupyter kernel (T129b), and the Bufflings tutorial
//! runner (T138c). This crate adds NO new compilation logic — it wires
//! the existing lexer/parser/types/codegen-rust crates into a clean
//! eval/eval_line/type_of surface with captured stdout.
//!
//! # Pipeline
//!
//! Each [`Evaluator::eval`] / [`Evaluator::eval_line`] call composes a
//! full Buff program from the accumulated state + the new snippet, then
//! runs the existing compile-to-Rust → rustc → spawn-and-capture pipeline:
//!
//! ```text
//!  snippet
//!     │
//!     ▼  classify (parse_expression / parse)
//!  SnippetKind { Expr | BodyStmt | TopLevelDecl | FullProgram }
//!     │
//!     ▼  compose accumulated state into a single Buff source
//!  full_source : String
//!     │
//!     ▼  buff_lang_lexer::tokenize  →  buff_lang_parser::parse
//!                                 ↓
//!                       buff_lang_codegen_rust::generate_rust
//!                                 ↓
//!                          String (formatted Rust source)
//!     │
//!     ▼  write to <temp dir>/eval-<pid>-<n>.rs
//!  rustc --edition 2021 -O <rs> -o <exe>
//!     │
//!     ▼  Command::new(<exe>).output()   (stdout/stderr captured)
//!  (stdout, stderr, exit_code)
//!     │
//!     ▼  EvalResult
//! ```
//!
//! # State accumulation
//!
//! `let` bindings and `func` declarations persist across calls so a REPL
//! session can build up state incrementally. The accumulated source is
//! held verbatim (re-parsed each call) so the codegen pass always sees a
//! self-contained program. The [`type_of`] introspection path runs the
//! [`TypeInferencer`] over the same accumulated source so its env stays
//! in sync with what a real evaluation would see.
//!
//! # Stdout capture
//!
//! Buff's `print` lowers to Rust's `println!`, which writes to the
//! compiled program's stdout. We spawn the program via
//! [`std::process::Command::output`], which captures the child's stdout
//! into a `Vec<u8>` without ever touching the parent process's stdout.
//! The captured bytes are surfaced as [`EvalResult::stdout`]. Consumers
//! (Jupyter, Bufflings) depend on this — they format the captured buffer
//! into their own display surfaces instead of inheriting process stdio.
//!
//! # No panics
//!
//! Every fallible operation (lex/parse/codegen/rustc/run) is threaded
//! through [`EvalResult::diagnostic`]. There are no `unwrap`/`expect`/
//! `panic!`/`unimplemented!`/`todo!` calls outside `#[cfg(test)]`.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use buff_lang_ast::Decl;
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::{Diagnostic, SourceId, Span};
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse, parse_expression};
use buff_lang_types::{Type, TypeInferencer};

// T35: Include the shared rustc-invoke helpers from buff-lang-cli via
// `#[path]` so we get the single source of truth WITHOUT pulling clap/
// tokio transitively. The shared module has zero CLI-specific deps.
#[path = "../../buff-lang-cli/src/rustc_invoke.rs"]
mod rustc_invoke;

// ---------------------------------------------------------------------------
// T2: Fast-linker selection (mirrors buff_lang_cli::pipeline::LinkerChoice).
// Kept inline to avoid pulling clap/tokio transitively into the eval crate.
// ---------------------------------------------------------------------------

/// Fast-linker selection for the eval crate's rustc invocation.
///
/// Mirrors `buff_lang_cli::pipeline::LinkerChoice` without depending on the
/// CLI crate. See AGENTS.md: "Keep the two copies in sync manually."
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum EvalLinker {
    /// Auto-detect: probe PATH for mold (Linux) → rust-lld → system default.
    #[default]
    Auto,
    /// Use rustc's default system linker (no `-C link-arg=-fuse-ld` flag).
    System,
}

/// Resolve an [`EvalLinker`] to rustc `-C link-arg=-fuse-ld` flags.
///
/// Returns an empty vec for [`EvalLinker::System`] (let rustc pick its
/// default). For [`EvalLinker::Auto`], probes PATH for mold (Linux) →
/// rust-lld → lld → system default (silent fallback).
fn resolve_eval_linker_flags(linker: EvalLinker) -> Vec<&'static str> {
    match linker {
        EvalLinker::Auto => {
            // mold is Linux-only in practice.
            if cfg!(target_os = "linux") && on_path("mold") {
                vec!["-C", "link-arg=-fuse-ld=mold"]
            } else if on_path("rust-lld") || on_path("lld") {
                vec!["-C", "link-arg=-fuse-ld=lld"]
            } else {
                Vec::new()
            }
        }
        EvalLinker::System => Vec::new(),
    }
}

/// Returns `true` when `name` (an executable basename) is found on `PATH`.
/// Delegates to the shared [`rustc_invoke::on_path`] (T35).
fn on_path(name: &str) -> bool {
    rustc_invoke::on_path(name)
}

// ---------------------------------------------------------------------------
// T4: Cranelift dev backend (mirrors buff_lang_cli::pipeline::BackendChoice).
// Kept inline to avoid pulling clap/tokio transitively into the eval crate.
// ---------------------------------------------------------------------------

/// Returns `true` when `sccache` is installed and on `PATH` (T9).
///
/// Mirrors `buff_lang_cli::compile_speed::sccache_available` without
/// depending on the CLI crate. sccache wraps rustc invocations to cache
/// compiled artefacts across projects. Opt-in via `BUFF_EVAL_SCCACHE=1`
/// env var (no CLI surface in eval).
fn sccache_available() -> bool {
    on_path("sccache")
}

/// Probe whether the Cranelift codegen backend is available (T4).
///
/// Delegates to the shared [`rustc_invoke::cranelift_available`] (T35).
fn cranelift_available() -> bool {
    rustc_invoke::cranelift_available()
}

/// Decide whether to set `CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift` on
/// the spawned rustc process for an eval call (T4).
///
/// `buff-eval` has no CLI surface (it is consumed in-process by REPL /
/// Jupyter / Bufflings), so the backend choice is driven by an env var
/// instead of a flag:
///
/// - `BUFF_EVAL_BACKEND=cranelift` → opt in. Probes via
///   [`cranelift_available`]; sets the env var on the rustc `Command`
///   when the probe succeeds, falls back silently to LLVM otherwise.
/// - Any other value (or unset) → LLVM (rustc default). No probe, no
///   env var, no overhead.
///
/// This mirrors the CLI's `--backend=cranelift` opt-in nature without
/// requiring a plumbing change through REPL/Jupyter/Bufflings. The env
/// var is read fresh on every `run_full_program` call so a user can
/// toggle it mid-session (`:set`-style commands in a future REPL
/// revision would just `set_var("BUFF_EVAL_BACKEND", ...)`).
///
/// Returns `true` when the caller should set the env var on the rustc
/// `Command`.
fn should_eval_use_cranelift() -> bool {
    match std::env::var("BUFF_EVAL_BACKEND") {
        Ok(v) if v.eq_ignore_ascii_case("cranelift") => cranelift_available(),
        _ => false,
    }
}

/// Read the cross-compilation target from the `BUFF_EVAL_TARGET` env var
/// (T112).
///
/// Returns `None` when unset or empty (native compilation). The env var
/// is read fresh on every `run_full_program` call so a user can toggle it
/// mid-session.
fn eval_target() -> Option<String> {
    let val = std::env::var("BUFF_EVAL_TARGET").ok()?;
    if val.trim().is_empty() {
        return None;
    }
    Some(val.trim().to_string())
}

/// Probe whether a rustc target triple is installed (T112).
///
/// Delegates to the shared [`rustc_invoke::target_is_installed`] (T35).
fn eval_target_is_installed(triple: &str) -> bool {
    rustc_invoke::target_is_installed(triple)
}

/// Re-export of the resolved [`Type`] so consumers can refer to it as
/// `buff_eval::Type` without depending on `buff-lang-types` directly.
pub use buff_lang_types::Type as ResolvedType;

/// Result of evaluating a Buff snippet.
///
/// All fallible phases (lex, parse, codegen, rustc, exec) surface their
/// errors through [`EvalResult::diagnostic`] rather than `Result<>` — the
/// caller checks `diagnostic.is_some()` to detect failure. `value`,
/// `stdout`, and `stderr` are always populated (to empty strings when the
/// pipeline failed before producing output), so partial output from a
/// runtime panic is still observable.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalResult {
    /// The evaluated value of a bare-expression snippet, captured by
    /// wrapping the expression in `print(...)` and reading the spawned
    /// program's stdout. `None` for statements / declarations / errors.
    ///
    /// The value is the captured stdout `trim`-ed of surrounding
    /// whitespace (so `print(2 + 3)` → `"5"`, not `"5\n"`).
    pub value: Option<String>,
    /// Everything the spawned program wrote to its stdout, verbatim.
    /// This is where Buff's `print` output lands.
    pub stdout: String,
    /// Everything the spawned program wrote to its stderr, verbatim.
    /// Includes Rust panic messages from runtime errors.
    pub stderr: String,
    /// The diagnostic for any pipeline error (lex / parse / codegen /
    /// rustc / spawn failure). `None` on clean evaluation.
    pub diagnostic: Option<Diagnostic>,
    /// The spawned program's exit code. `None` if the program never ran
    /// (compile error) or the OS did not report a code (signal death).
    pub exit_code: Option<i32>,
}

impl EvalResult {
    /// `true` when the pipeline reached a successful program execution.
    /// Mirrors `diagnostic.is_none() && exit_code == Some(0)` but is the
    /// idiomatic "did it work?" predicate.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.diagnostic.is_none() && self.exit_code == Some(0)
    }

    /// Construct an error result with the given diagnostic and empty
    /// captured output. Convenience for the error paths.
    fn err(diagnostic: Diagnostic) -> Self {
        Self {
            value: None,
            stdout: String::new(),
            stderr: String::new(),
            diagnostic: Some(diagnostic),
            exit_code: None,
        }
    }

    /// Construct a successful result with the given captured output.
    fn ok(stdout: String, stderr: String, exit_code: Option<i32>, value: Option<String>) -> Self {
        Self {
            value,
            stdout,
            stderr,
            diagnostic: None,
            exit_code,
        }
    }
}

/// A reusable Buff evaluator that accumulates `let` bindings and `func`
/// declarations across calls.
///
/// Clone is NOT derived because the evaluator owns mutable session state
/// that callers usually want to keep singular. Consumers wanting a fork
/// can construct a fresh `Evaluator::new()`.
#[derive(Debug)]
pub struct Evaluator {
    /// Accumulated source for top-level declarations (e.g. `func helper()
    /// -> Int: ...`). Stored verbatim so the next call can compose a fresh
    /// self-contained program.
    top_level_src: String,
    /// Accumulated source for body statements inside the synthetic `main`
    /// (e.g. `let x = 42`). Each entry is one Buff source line (or block)
    /// already stripped of leading indentation — `compose_program`
    /// re-indents when emitting.
    body_stmts_src: String,
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl Evaluator {
    /// Construct a fresh evaluator with no accumulated state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            top_level_src: String::new(),
            body_stmts_src: String::new(),
        }
    }

    /// Evaluate a full Buff source snippet.
    ///
    /// The snippet may be:
    ///
    /// - a bare expression (`2 + 3`) — the spawned program prints its
    ///   value, surfaced as [`EvalResult::value`] and [`EvalResult::stdout`],
    /// - a body statement (`let x = 42`) — accumulated into the session
    ///   state, no value returned,
    /// - a top-level declaration (`func helper(): ...`) — accumulated into
    ///   the session state, no value returned,
    /// - a full Buff program (`func main(): print("hi")`) — evaluated
    ///   verbatim; not accumulated (the caller owns `main`).
    ///
    /// State persists across `eval` and [`Self::eval_line`] calls.
    #[allow(clippy::needless_pass_by_value)]
    pub fn eval(&mut self, source: &str) -> EvalResult {
        self.evaluate(source)
    }

    /// Evaluate a single Buff line, incrementally accumulating `let`
    /// bindings and `func` declarations.
    ///
    /// Functionally identical to [`Self::eval`] — the `eval` / `eval_line`
    /// split documents intent (`eval` for whole snippets, `eval_line`
    /// for one-statement-at-a-time REPL input). Both share the same
    /// classification + composition + compile + run path.
    #[allow(clippy::needless_pass_by_value)]
    pub fn eval_line(&mut self, line: &str) -> EvalResult {
        self.evaluate(line)
    }

    /// Type-introspect `expr` against the accumulated environment.
    ///
    /// Returns `None` on any lex/parse/inference error (no diagnostic is
    /// surfaced — the caller treats `None` as "type unknown"). The
    /// accumulated state is consulted but NOT modified, so a `type_of`
    /// query has no side effects on subsequent `eval` calls.
    #[must_use]
    pub fn type_of(&self, expr: &str) -> Option<Type> {
        // Compose a synthetic program whose main body runs the accumulated
        // statements and then evaluates the queried expression as the
        // trailing ExprStmt. Walk all-but-last through the inferencer to
        // populate env, then infer the trailing expr's type.
        let body = indent_lines(&self.body_stmts_src);
        let composed = if body.is_empty() {
            format!("func __typecheck__():\n    {expr}\n")
        } else {
            format!("func __typecheck__():\n{body}    {expr}\n")
        };
        let tokens = tokenize(&composed, SourceId(0)).ok()?;
        let decls = parse(&tokens, SourceId(0)).ok()?;
        if decls.len() != 1 {
            return None;
        }
        let func = match &decls[0] {
            Decl::FuncDecl(f) => f,
            _ => return None,
        };
        let stmts = &func.body.stmts;
        if stmts.is_empty() {
            return None;
        }
        let mut inf = TypeInferencer::new();
        // Walk all-but-last to populate env. The trailing stmt is the
        // expression whose type we want.
        for stmt in &stmts[..stmts.len() - 1] {
            let _ = inf.infer_stmt(stmt);
        }
        match stmts.last() {
            Some(buff_lang_ast::Stmt::ExprStmt(e, _)) => inf.infer_expr(e).ok(),
            _ => None,
        }
    }

    /// Shared driver for `eval` / `eval_line`.
    fn evaluate(&mut self, snippet: &str) -> EvalResult {
        match classify(snippet) {
            SnippetKind::Empty => EvalResult::ok(String::new(), String::new(), Some(0), None),
            SnippetKind::FullProgram(src) => {
                // User provided their own `main` — evaluate verbatim. We do
                // NOT accumulate full programs into session state because
                // the user owns `main`; a later `eval_line` would produce
                // a duplicate `main` and a rustc error.
                run_full_program(&src)
            }
            SnippetKind::TopLevelDecl(src) => {
                // Accumulate the decl, then run the composed program.
                self.append_top_level(&src);
                let composed = self.compose_program_body(None);
                run_full_program(&composed)
            }
            SnippetKind::BodyStmt(src) => {
                // Accumulate the body stmt, then run the composed program.
                self.append_body_stmt(&src);
                let composed = self.compose_program_body(None);
                run_full_program(&composed)
            }
            SnippetKind::BareExpr(expr_src, is_print) => {
                // Bare expression: do NOT accumulate (state unchanged).
                // Wrap as `print(<expr>)` to capture the value, unless the
                // expression is itself a `print(...)` call (which returns
                // Void — wrapping would yield `print(print(...))` and a
                // rustc type error).
                let contribution = if is_print {
                    expr_src.clone()
                } else {
                    format!("print({expr_src})")
                };
                let composed = self.compose_program_body(Some(&contribution));
                let mut result = run_full_program(&composed);
                // If the run succeeded and the snippet was NOT a print
                // call, surface the trimmed stdout as the evaluated value.
                if !is_print && result.diagnostic.is_none() {
                    result.value = Some(result.stdout.trim().to_string());
                }
                result
            }
        }
    }

    /// Append a top-level declaration source string to the accumulated
    /// state, ensuring a trailing newline separator.
    fn append_top_level(&mut self, src: &str) {
        if !self.top_level_src.is_empty() && !self.top_level_src.ends_with('\n') {
            self.top_level_src.push('\n');
        }
        self.top_level_src.push_str(src);
        if !self.top_level_src.ends_with('\n') {
            self.top_level_src.push('\n');
        }
    }

    /// Append a body statement source string to the accumulated state,
    /// ensuring a trailing newline separator.
    fn append_body_stmt(&mut self, src: &str) {
        if !self.body_stmts_src.is_empty() && !self.body_stmts_src.ends_with('\n') {
            self.body_stmts_src.push('\n');
        }
        self.body_stmts_src.push_str(src);
        if !self.body_stmts_src.ends_with('\n') {
            self.body_stmts_src.push('\n');
        }
    }

    /// Compose a full Buff program from the accumulated state, optionally
    /// appending a trailing contribution (already an un-indented Buff
    /// statement source like `print(x + 8)` or `let y = 10`).
    fn compose_program_body(&self, trailing: Option<&str>) -> String {
        let mut out = String::new();
        // Top-level decls (functions, structs, etc.) — no indentation.
        if !self.top_level_src.is_empty() {
            out.push_str(&self.top_level_src);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        // Synthetic main. Buff layout: `func main():\n` followed by
        // 4-space-indented body statements.
        out.push_str("func main():\n");
        let body = indent_lines(&self.body_stmts_src);
        out.push_str(&body);
        if let Some(t) = trailing {
            let indented = indent_lines(t);
            out.push_str(&indented);
            if !indented.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }
}

/// Classification of a user-supplied snippet.
///
/// Produced by [`classify`] — drives the accumulation + composition
/// strategy in [`Evaluator::evaluate`].
#[derive(Debug, Clone)]
enum SnippetKind {
    /// Whitespace-only input — no-op.
    Empty,
    /// A bare expression (`2 + 3`). Carries the original source string
    /// and a flag indicating whether the expression is itself a
    /// `print(...)` / `println(...)` call (Void-returning — must NOT be
    /// wrapped again).
    BareExpr(String, bool),
    /// A body statement (`let x = 42`). Carries the original source.
    BodyStmt(String),
    /// A top-level declaration (`func helper(): ...`). Carries the
    /// original source.
    TopLevelDecl(String),
    /// A full Buff program with its own `func main(): ...`. Evaluated
    /// verbatim; not accumulated.
    FullProgram(String),
}

/// Classify a snippet by trying each parser entry point in turn.
///
/// Strategy:
/// 1. Trim-test for empty input.
/// 2. Tokenize. If lex fails, fall back to BodyStmt (we can't say more).
/// 3. Try `parse_expression` (strict — requires all tokens consumed).
///    If Ok, it's a [`SnippetKind::BareExpr`].
/// 4. Try `parse`. If Ok:
///    - If any decl is `func main`, it's a [`SnippetKind::FullProgram`].
///    - Otherwise it's a [`SnippetKind::TopLevelDecl`].
/// 5. Else, fall back to [`SnippetKind::BodyStmt`] (most likely a `let`
///    or `print(...)` line, neither of which is a top-level form).
fn classify(snippet: &str) -> SnippetKind {
    if snippet.trim().is_empty() {
        return SnippetKind::Empty;
    }
    let tokens = match tokenize(snippet, SourceId(0)) {
        Ok(t) => t,
        Err(_) => return SnippetKind::BodyStmt(snippet.to_string()),
    };
    // Try as a single expression (strict — requires all tokens consumed).
    if let Ok(expr) = parse_expression(&tokens, SourceId(0)) {
        let is_print = is_print_call(&expr);
        return SnippetKind::BareExpr(snippet.trim().to_string(), is_print);
    }
    // Try as a top-level program.
    if let Ok(decls) = parse(&tokens, SourceId(0)) {
        let has_main = decls.iter().any(|d| {
            matches!(
                d,
                Decl::FuncDecl(f) if f.name.name == "main"
            )
        });
        if has_main {
            return SnippetKind::FullProgram(snippet.to_string());
        }
        return SnippetKind::TopLevelDecl(snippet.to_string());
    }
    // Fall back: treat as a body statement (let, assignment, print line).
    SnippetKind::BodyStmt(snippet.to_string())
}

/// `true` if `expr` is a `print(...)` / `println(...)` call.
///
/// Such expressions return `Void` in Buff's prelude, so wrapping them
/// again as `print(print(...))` would yield a rustc type error. The
/// caller emits them as a statement instead.
fn is_print_call(expr: &buff_lang_ast::Expr) -> bool {
    if let buff_lang_ast::Expr::FuncCall { callee, .. } = expr {
        if let buff_lang_ast::Expr::Ident(name, _) = callee.as_ref() {
            return name.name == "print" || name.name == "println";
        }
    }
    false
}

/// Indent every line of `src` by 4 spaces. Empty input → empty output.
/// Trailing newline is preserved.
fn indent_lines(src: &str) -> String {
    if src.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(src.len() + src.lines().count() * 4);
    for line in src.split_inclusive('\n') {
        if line == "\n" || line.is_empty() {
            // Preserve blank lines without indent (Buff fmt rule).
            out.push('\n');
            continue;
        }
        out.push_str("    ");
        out.push_str(line);
    }
    out
}

/// Run the full Buff source through the compile-and-spawn pipeline.
///
/// Returns an [`EvalResult`] populated with the spawned program's stdout
/// / stderr / exit code on success, or a diagnostic on any pipeline
/// error (lex / parse / codegen / rustc / spawn).
fn run_full_program(source: &str) -> EvalResult {
    let source_id = SourceId(0);
    let tokens = match tokenize(source, source_id) {
        Ok(t) => t,
        Err(e) => {
            return EvalResult::err(e.inner.diagnostic);
        }
    };
    let decls = match parse(&tokens, source_id) {
        Ok(d) => d,
        Err(e) => {
            return EvalResult::err(e.diagnostic);
        }
    };
    let rust_source = match generate_rust(&decls) {
        Ok(s) => s,
        Err(e) => {
            return EvalResult::err(e.diagnostic);
        }
    };

    // Acquire unique temp paths. Use a process-wide counter + PID so
    // parallel test processes don't collide.
    let dir = temp_dir_for_eval();
    let stem = unique_stem();
    let rust_path = dir.join(format!("{stem}.rs"));
    let exe_path = dir.join(with_exe_extension(&PathBuf::from(stem)));

    // Write the .rs file. If this fails, surface as a diagnostic.
    if let Err(e) = std::fs::write(&rust_path, &rust_source) {
        return EvalResult::err(Diagnostic::error(
            format!(
                "eval: failed to write temp rust file `{}`: {e}",
                rust_path.display()
            ),
            Span::dummy(),
        ));
    }

    // rustc invocation: edition 2021 + -O (debug mode = fast compile, no
    // LTO). Mirrors pipeline.rs `BuildMode::Debug` exactly.
    // T2: auto-detect fast linker (mold → rust-lld → system default).
    // T3: line-tables-only debuginfo (-C debuginfo=1) for backtraces.
    // T4: opt-in Cranelift dev backend via BUFF_EVAL_BACKEND=cranelift
    // (probe + set CARGO_PROFILE_DEV_CODEGEN_BACKEND env var on the
    // child rustc process — scoped to the subprocess, no parent leak).
    // T9: opt-in sccache via BUFF_EVAL_SCCACHE=1 env var. When set AND
    // sccache is on PATH, sets RUSTC_WRAPPER=sccache on the child rustc
    // process. Falls back silently when sccache is missing.
    // T112: cross-compilation target via BUFF_EVAL_TARGET env var.
    // When set, verifies the target is installed via `rustup target list
    // --installed` and passes `--target <triple>` to rustc.
    let mut rustc_cmd = Command::new("rustc");
    // T9: sccache wrapper — opt-in via BUFF_EVAL_SCCACHE=1 env var.
    if std::env::var("BUFF_EVAL_SCCACHE").as_deref() == Ok("1") && sccache_available() {
        rustc_cmd.env("RUSTC_WRAPPER", "sccache");
    }
    // T35: delegate common flag configuration to the shared helper.
    let linker_flags = resolve_eval_linker_flags(EvalLinker::Auto);
    let target = eval_target();
    if let Some(ref triple) = target {
        if !eval_target_is_installed(triple) {
            let _ = std::fs::remove_file(&rust_path);
            return EvalResult::err(Diagnostic::error(
                format!(
                    "Target `{triple}` is not installed.\n\
                     Run: rustup target add {triple}"
                ),
                Span::dummy(),
            ));
        }
    }
    if let Err(msg) = rustc_invoke::configure_rustc_command(
        &mut rustc_cmd,
        &["-O"],
        &linker_flags,
        should_eval_use_cranelift(),
        "debuginfo=1",
        target.as_deref(),
    ) {
        let _ = std::fs::remove_file(&rust_path);
        return EvalResult::err(Diagnostic::error(msg, Span::dummy()));
    }
    rustc_cmd.arg(&rust_path).arg("-o").arg(&exe_path);
    let compile_out = match rustc_cmd.output()
    {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&rust_path);
            return EvalResult::err(Diagnostic::error(
                format!("eval: failed to invoke `rustc`: {e}"),
                Span::dummy(),
            ));
        }
    };

    if !compile_out.status.success() {
        let stderr = String::from_utf8_lossy(&compile_out.stderr).into_owned();
        let _ = std::fs::remove_file(&rust_path);
        let _ = std::fs::remove_file(&exe_path);
        return EvalResult {
            value: None,
            stdout: String::new(),
            stderr,
            diagnostic: Some(Diagnostic::error(
                "eval: `rustc` exited non-zero (see stderr for rustc diagnostics)",
                Span::dummy(),
            )),
            exit_code: compile_out.status.code(),
        };
    }

    // Run the compiled exe, capturing stdout/stderr.
    let run_out = match Command::new(&exe_path).output() {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&rust_path);
            let _ = std::fs::remove_file(&exe_path);
            return EvalResult::err(Diagnostic::error(
                format!("eval: failed to spawn compiled program: {e}",),
                Span::dummy(),
            ));
        }
    };

    let stdout = String::from_utf8_lossy(&run_out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&run_out.stderr).into_owned();
    let exit_code = run_out.status.code();

    // Best-effort cleanup. The compiled program files are useless after
    // we've captured their output.
    let _ = std::fs::remove_file(&rust_path);
    let _ = std::fs::remove_file(&exe_path);

    if exit_code != Some(0) {
        // Runtime panic / non-zero exit. Surface as a diagnostic so the
        // caller sees a structured error; stderr carries the panic msg.
        return EvalResult {
            value: None,
            stdout,
            stderr,
            diagnostic: Some(Diagnostic::error(
                format!(
                    "eval: program exited with code {}",
                    exit_code
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "<signal>".to_string())
                ),
                Span::dummy(),
            )),
            exit_code,
        };
    }

    EvalResult::ok(stdout, stderr, exit_code, None)
}

/// Return the per-eval temp directory (`<tmp>/buff-eval`), creating it
/// if missing. Failure to create is silent — the caller's `std::fs::write`
/// will surface the error path with full context.
fn temp_dir_for_eval() -> PathBuf {
    let dir = std::env::temp_dir().join("buff-eval");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Process-wide counter guaranteeing unique temp-file names per call.
static STEM_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Build a unique stem like `eval-<pid>-<n>`.
fn unique_stem() -> String {
    let n = STEM_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("eval-{pid}-{n}")
}

/// Apply the platform's executable extension to `path` (`.exe` on
/// Windows, no-op on Unix). Mirrors `buff_lang_cli::pipeline::
/// with_exe_extension` without taking a dep on the CLI crate (which
/// would pull in `clap` / `tokio` etc. for a single helper).
fn with_exe_extension(path: &std::path::Path) -> PathBuf {
    let ext = std::env::consts::EXE_EXTENSION;
    if ext.is_empty() {
        return path.to_path_buf();
    }
    if path.extension().is_some_and(|e| e == ext) {
        return path.to_path_buf();
    }
    let mut p = path.to_path_buf();
    p.set_extension(ext);
    p
}

// ---------------------------------------------------------------------------
// Smoke tests — exercise the public API end-to-end. The acceptance
// scenarios (expression eval, state accumulation, type introspection,
// stdout capture, error handling) live in `tests/eval_tests.rs`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_empty_and_bare_expr_and_body_stmt() {
        assert!(matches!(classify(""), SnippetKind::Empty));
        assert!(matches!(classify("   \n  "), SnippetKind::Empty));
        // Bare expr: tokenize + parse_expression succeeds.
        match classify("2 + 3") {
            SnippetKind::BareExpr(src, false) => assert_eq!(src, "2 + 3"),
            other => panic!("expected BareExpr, got {other:?}"),
        }
        // Body stmt: `let` is not a top-level form, falls through to BodyStmt.
        assert!(matches!(classify("let x = 42"), SnippetKind::BodyStmt(_)));
    }

    #[test]
    fn classify_top_level_decl_and_full_program() {
        // Top-level decl: function WITHOUT name "main".
        match classify("func helper():\n    return 1") {
            SnippetKind::TopLevelDecl(_) => {}
            other => panic!("expected TopLevelDecl, got {other:?}"),
        }
        // Full program: function WITH name "main".
        match classify("func main():\n    print(\"hi\")") {
            SnippetKind::FullProgram(_) => {}
            other => panic!("expected FullProgram, got {other:?}"),
        }
    }

    #[test]
    fn indent_lines_prepends_four_spaces() {
        assert_eq!(indent_lines(""), "");
        assert_eq!(indent_lines("let x = 1\n"), "    let x = 1\n");
        assert_eq!(
            indent_lines("let x = 1\nlet y = 2\n"),
            "    let x = 1\n    let y = 2\n"
        );
        // Blank lines stay blank (Buff fmt rule).
        assert_eq!(indent_lines("\n"), "\n");
    }

    #[test]
    fn evaluator_compose_program_body_indents_accumulated() {
        let mut ev = Evaluator::new();
        ev.append_body_stmt("let x = 42");
        let composed = ev.compose_program_body(Some("print(x)"));
        assert!(
            composed.contains("func main():\n"),
            "missing main header in: {composed}"
        );
        assert!(
            composed.contains("    let x = 42\n"),
            "missing indented body in: {composed}"
        );
        assert!(
            composed.contains("    print(x)\n"),
            "missing trailing contribution in: {composed}"
        );
    }

    #[test]
    fn evaluator_appends_top_level_and_body() {
        let mut ev = Evaluator::new();
        ev.append_top_level("func helper():\n    return 1");
        ev.append_body_stmt("let x = 2");
        let composed = ev.compose_program_body(None);
        assert!(
            composed.contains("func helper():\n    return 1\n"),
            "missing top-level decl in: {composed}"
        );
        assert!(
            composed.contains("func main():\n    let x = 2\n"),
            "missing main + body in: {composed}"
        );
    }

    #[test]
    fn is_print_call_detects_print_and_println() {
        // print(...) — bare call expression
        let tokens = tokenize("print(42)", SourceId(0)).expect("tokenize");
        let expr = parse_expression(&tokens, SourceId(0)).expect("parse_expression");
        assert!(is_print_call(&expr));

        // println(...) — same.
        let tokens = tokenize("println(42)", SourceId(0)).expect("tokenize");
        let expr = parse_expression(&tokens, SourceId(0)).expect("parse_expression");
        assert!(is_print_call(&expr));

        // Non-print call (e.g. user-defined foo(...)) — false.
        // (We can't easily parse a non-prelude call here without a
        // supporting decl, so test the negative case with a bare ident
        // expression.)
        let tokens = tokenize("x", SourceId(0)).expect("tokenize");
        let expr = parse_expression(&tokens, SourceId(0)).expect("parse_expression");
        assert!(!is_print_call(&expr));
    }

    #[test]
    fn eval_result_ok_and_err_helpers() {
        let ok = EvalResult::ok(
            String::from("out"),
            String::new(),
            Some(0),
            Some(String::from("5")),
        );
        assert!(ok.is_ok());
        assert_eq!(ok.value.as_deref(), Some("5"));

        let err = EvalResult::err(Diagnostic::error("boom", Span::dummy()));
        assert!(!err.is_ok());
        assert!(err.diagnostic.is_some());
    }

    #[test]
    fn with_exe_extension_unix_passthrough_or_windows_appends() {
        let p = PathBuf::from("eval-1");
        let ext = std::env::consts::EXE_EXTENSION;
        let with_ext = with_exe_extension(&p);
        if ext.is_empty() {
            assert_eq!(with_ext, p);
        } else {
            assert_eq!(
                with_ext.file_name().and_then(|n| n.to_str()),
                Some("eval-1.exe")
            );
        }
    }
}
