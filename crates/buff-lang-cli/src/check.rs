//! `buff check` — type-checker + naming-convention linter (T55).
//!
//! Runs the compiler's front-end (`tokenize` → `parse`) and the
//! [`TypeInferencer`] over every function body WITHOUT invoking codegen
//! or `rustc`. This makes `buff check` substantially faster than
//! `buff build` (no syn/quote/prettyplease, no rustc compile), at the
//! cost of not catching errors that only surface during Rust codegen
//! (e.g. ownership errors that rustc would catch).
//!
//! # Pipeline
//!
//! ```text
//!   .buff source
//!        │
//!        ▼  tokenize
//!   Vec<Token>      ── on error ──▶ HasErrors
//!        │
//!        ▼  parse
//!   Vec<Decl>       ── on error ──▶ HasErrors
//!        │
//!        ▼  type_check_decls (drive TypeInferencer over each func body)
//!   Vec<TypeError>  ── any? ──────▶ HasErrors
//!        │
//!        ▼  lint_naming
//!   Vec<Diagnostic> (Warnings only)
//!        │
//!        ▼
//!   CheckReport { diagnostics, outcome }
//! ```
//!
//! # Outcome mapping
//!
//! [`CheckOutcome`] is the library-level signal: [`CheckOutcome::Clean`]
//! (no diagnostics), [`CheckOutcome::HasWarnings`] (only warnings — exit
//! 0 by default), [`CheckOutcome::HasErrors`] (at least one error — exit 1).
//! The CLI binary ([`crate::commands::check`]) translates this into an
//! exit code; `--deny-warnings` / `-D` promotes warnings to exit-non-zero.
//!
//! # Why no codegen?
//!
//! `buff build` already runs the full pipeline (lex → parse → codegen →
//! rustc) and surfaces every error rustc finds. `buff check` is the FAST
//! feedback path: it catches the errors the compiler can find on its own
//! (lex/parse/type-check) in a fraction of the time. This mirrors
//! `cargo check` vs `cargo build` for Rust.

// The `mod tests` block sits in the middle of this file (between the
// core check pipeline and the deprecated-call collector helpers at the
// bottom). Reordering would shuffle ~300 lines for purely stylistic
// reasons; clippy's `items_after_test_module` is silenced here until
// the next green-field refactor of this module.
#![allow(clippy::items_after_test_module)]

use std::path::Path;

use buff_lang_ast::{Decl, Expr, Stmt, TypeRef};
use buff_lang_error::{render_diagnostics_json, Diagnostic, Severity, SourceFile, SourceId};
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

// The `mod tests` block sits in the middle of this file (between the
// core check pipeline and the deprecated-call collector helpers at the
// bottom). Reordering would shuffle ~300 lines for purely stylistic
// reasons; clippy's `items_after_test_module` is silenced here until
// the next green-field refactor of this module.
use buff_lang_types::{Type, TypeInferencer};

use crate::naming_lint::{lint_common_mistakes, lint_naming, lint_tab_indentation};

// ---------------------------------------------------------------------------
// Error output format (T1, v1.25 Wave 0).
// ---------------------------------------------------------------------------

/// How `buff check` (and other diagnostic-emitting commands) format their
/// output. Selected via `--error-format <KIND>` on the CLI.
///
/// - [`Human`](Self::Human) (default) renders rustc-style source-line +
///   caret blocks to **stderr** — the pre-T1 behavior, byte-identical.
/// - [`Json`](Self::Json) emits a single JSON array of
///   [`buff_lang_error::DiagnosticJson`] to **stdout** (one line, no
///   trailing newline added by the serializer) so `buff check
///   --error-format json | jq '.[].code'` works.
///
/// Unknown strings resolve to [`Human`](Self::Human) on parse (see
/// [`ErrorFormat::from_str`]) — we never panic on a bad CLI flag value,
/// we just fall back to the safe default. The CLI's clap `value_parser`
/// rejects unknown values up front, so the from_str fallback is a
/// belt-and-braces safety net for library callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorFormat {
    /// Rustc-style human-readable output (default). Emits to stderr.
    #[default]
    Human,
    /// JSON array of diagnostics. Emits to stdout.
    Json,
}

impl ErrorFormat {
    /// Parse a `--error-format` value. Lowercase, case-sensitive (matches
    /// rustc / cargo conventions: `human` / `json`). Returns
    /// [`Human`](Self::Human) for any unrecognized string — the CLI's clap
    /// layer is responsible for rejecting bad values up front; this is
    /// the library-side safety net.
    #[allow(clippy::should_implement_trait)] // not a true FromStr (no Result); name kept for clarity.
    pub fn from_str(s: &str) -> Self {
        match s {
            "json" => ErrorFormat::Json,
            _ => ErrorFormat::Human,
        }
    }

    /// `true` when this format is JSON.
    pub fn is_json(self) -> bool {
        matches!(self, ErrorFormat::Json)
    }
}

// ---------------------------------------------------------------------------
// Outcome + report.
// ---------------------------------------------------------------------------

/// The library-level outcome of a `buff check` run.
///
/// The CLI binary translates these into process exit codes:
///
/// | Variant         | Default exit | With `--deny-warnings` |
/// |-----------------|--------------|------------------------|
/// | [`Clean`](Self::Clean)            | 0 | 0 |
/// | [`HasWarnings`](Self::HasWarnings) | 0 | 1 |
/// | [`HasErrors`](Self::HasErrors)     | 1 | 1 |
///
/// Returning an enum (rather than calling `std::process::exit` from the
/// library `run`) keeps the function testable: tests can inspect the
/// variant without aborting the test process. This is the T54 lesson
/// applied to T55.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    /// No diagnostics at all — the source is clean.
    Clean,
    /// Only warning-level diagnostics (lint warnings). Default exit code 0.
    HasWarnings,
    /// At least one error-level diagnostic (lex/parse/type error). Exit 1.
    HasErrors,
}

/// The full report returned by [`check_source`]: all collected diagnostics
/// (errors first, then warnings) plus the derived outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// All diagnostics collected during the check, in the order they were
    /// produced. Errors come before warnings within each phase.
    pub diagnostics: Vec<Diagnostic>,
    /// The derived outcome (clean / warnings / errors).
    pub outcome: CheckOutcome,
}

// ---------------------------------------------------------------------------
// Entry points.
// ---------------------------------------------------------------------------

/// Run the full check pipeline on a source string.
///
/// Drives lex → parse → type-check → naming-lint, collecting diagnostics
/// from each phase. The phase ordering is fail-soft within a phase but
/// fail-fast between phases: lex errors short-circuit parsing (there are
/// no tokens to feed), parse errors short-circuit type-check (there's no
/// AST to walk). Type-check errors and lint warnings are collected
/// together at the end.
///
/// This entry is the test surface: integration tests pass source strings
/// and assert the [`CheckOutcome`] / diagnostic contents.
pub fn check_source(src: &str) -> CheckReport {
    let source_id = SourceId(0);
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // 1. Lex.
    let tokens = match tokenize(src, source_id) {
        Ok(t) => t,
        Err(e) => {
            diagnostics.push(e.inner.diagnostic);
            return CheckReport {
                diagnostics,
                outcome: CheckOutcome::HasErrors,
            };
        }
    };

    // 2. Parse.
    let decls = match parse(&tokens, source_id) {
        Ok(d) => d,
        Err(e) => {
            diagnostics.push(e.diagnostic);
            return CheckReport {
                diagnostics,
                outcome: CheckOutcome::HasErrors,
            };
        }
    };

    // 3. Type-check: drive TypeInferencer over each function body. Errors
    //    are collected (not short-circuited) so a program with multiple
    //    type errors reports them all in one pass.
    let type_errors = type_check_decls(&decls);
    for err in type_errors {
        diagnostics.push(err.diagnostic);
    }

    // 4. Naming-convention lint.
    let lint_warnings = lint_naming(&decls);
    diagnostics.extend(lint_warnings);

    // 5. T63: common-mistake linter — prelude typos (Print/prin) + tab
    //    indentation. Runs after parse (needs the AST) and over the raw
    //    source (tab scan is byte-level). Both emit warnings only.
    let mistake_warnings = lint_common_mistakes(&decls);
    diagnostics.extend(mistake_warnings);
    let tab_warnings = lint_tab_indentation(src);
    diagnostics.extend(tab_warnings);

    // 6. T0-G3: @deprecated call-site warnings. Walks the AST to find
    //    FuncCalls whose callee resolves to a fn marked `@deprecated`.
    //    Each call site gets a warning naming the fn + the `since` and
    //    `replacement` (when provided).
    let deprecated_warnings = collect_deprecated_call_warnings(&decls);
    diagnostics.extend(deprecated_warnings);

    // 7. T72: Compiler plugin dispatch. Calls into the global plugin
    //    registry (env-var-loaded via BUFF_PLUGIN_DIR /
    //    BUFF_PLUGIN_PATH). Empty registry → empty Vec → no-op (no
    //    diagnostics added, no perf overhead beyond a Mutex lock).
    //    Each LintWarning is converted to a warning-severity
    //    Diagnostic; the plugin's `name()` is used as the code when
    //    the warning's own `code` field is None.
    let plugin_warnings = buff_plugins::dispatch_global_compiler_lint(&decls);
    for warning in plugin_warnings {
        let code = warning.code.unwrap_or_else(|| "plugin".to_string());
        diagnostics.push(Diagnostic::warning(
            format!("[{code}] {}", warning.message),
            warning.span,
        ));
    }

    let outcome = compute_outcome(&diagnostics);
    CheckReport {
        diagnostics,
        outcome,
    }
}

/// Run the check pipeline on a file, render diagnostics to stderr, and
/// return the outcome. Thin wrapper around [`run_check_file_with_format`]
/// with [`ErrorFormat::Human`] — byte-identical to the pre-T1 behavior.
///
/// File-read failures propagate as `Err` (they are I/O errors, not
/// diagnostics). All compile diagnostics are rendered to stderr via
/// [`Diagnostic::render`] (rustc-style with caret) and the outcome is
/// returned for the caller to translate into an exit code.
///
/// **T133:** Dispatches on file extension. `.buffhtml` files are checked
/// via [`check_buffhtml_source`] (parse + codegen; rustc-level errors are
/// surfaced via `buff build` not `buff check` — type-inference-on-codegen
/// is integrated inside `buff_lang_codegen_buffhtml::generate`).
///
/// # Errors
///
/// Returns `Err` only when the file cannot be read. Compile diagnostics
/// are NOT errors here — they become the returned [`CheckReport`].
pub fn run_check_file(file: &Path) -> anyhow::Result<CheckReport> {
    run_check_file_with_format(file, ErrorFormat::Human, false)
}

/// Run the full check pipeline on a `.buff` or `.buffhtml` file and emit
/// diagnostics in the requested [`ErrorFormat`] (T1, v1.25 Wave 0).
///
/// - [`ErrorFormat::Human`] (default) — rustc-style source-line + caret
///   blocks to **stderr**. Byte-identical to pre-T1 output.
/// - [`ErrorFormat::Json`] — single JSON array of
///   [`buff_lang_error::DiagnosticJson`] to **stdout**. Each entry carries
///   byte offsets + 1-based line/col + suggestions + applicability. Empty
///   diagnostics → `[]`. The "no issues found" stderr note is suppressed
///   in JSON mode (would pollute the JSON stream consumers parse).
///
/// The JSON mode exists for tooling (LSP CodeAction round-trip, CI
/// reporters, future `buff fix`). See `crates/buff-lang-error/src/json.rs`
/// for the shape contract.
///
/// T43: `no_color` forces human-readable output without ANSI escape codes.
///
/// # Errors
///
/// Returns `Err` only when the file cannot be read. Compile diagnostics
/// are NOT errors here — they become the returned [`CheckReport`].
pub fn run_check_file_with_format(
    file: &Path,
    format: ErrorFormat,
    no_color: bool,
) -> anyhow::Result<CheckReport> {
    let is_buffhtml = file
        .extension()
        .is_some_and(|e| e == crate::pipeline::BUFFHTML_EXT);
    if is_buffhtml {
        return run_check_buffhtml_file_with_format(file, format, no_color);
    }
    let src = std::fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("failed to read `{}`: {e}", file.display()))?;
    let report = check_source(&src);

    emit_diagnostics(&report.diagnostics, &src, file, format, no_color);

    if matches!(report.outcome, CheckOutcome::Clean) && !format.is_json() {
        eprintln!("{}: no issues found", file.display());
    }
    Ok(report)
}

/// Emit a slice of diagnostics in the requested [`ErrorFormat`].
///
/// Pulled out of [`run_check_file_with_format`] so the same logic serves
/// the `.buff` and `.buffhtml` check paths without duplication. `source`
/// is the raw file content (the same `&str` you'd pass to
/// [`Diagnostic::render`]).
///
/// T43: `no_color` controls ANSI escape codes in human-readable output.
fn emit_diagnostics(
    diagnostics: &[Diagnostic],
    source: &str,
    file: &Path,
    format: ErrorFormat,
    no_color: bool,
) {
    match format {
        ErrorFormat::Json => {
            // Single JSON array on stdout. Empty input → `[]` (clean file).
            let json = render_diagnostics_json(diagnostics, source);
            println!("{json}");
        }
        ErrorFormat::Human => {
            // Human-readable render to stderr.
            let use_color = !no_color && buff_lang_error::should_use_color();
            let source_file = SourceFile::new(file.to_path_buf(), source.to_string());
            for d in diagnostics {
                let rendered = render_diagnostic(d, &source_file, use_color);
                // Pre-T1 behavior: both Error and Warning go to stderr.
                eprint!("{rendered}");
            }
        }
    }
}

/// T133: run the `.buffhtml` check pipeline on a file. Thin wrapper around
/// [`run_check_buffhtml_file_with_format`] with [`ErrorFormat::Human`] and
/// no color.
#[allow(dead_code)] // T133 floor; wired in by `buff check <file>.buffhtml` once the dispatcher lands.
fn run_check_buffhtml_file(file: &Path) -> anyhow::Result<CheckReport> {
    run_check_buffhtml_file_with_format(file, ErrorFormat::Human, false)
}

/// T133 + T1: run the `.buffhtml` check pipeline on a file with a
/// configurable output format.
fn run_check_buffhtml_file_with_format(
    file: &Path,
    format: ErrorFormat,
    no_color: bool,
) -> anyhow::Result<CheckReport> {
    let src = std::fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("failed to read `{}`: {e}", file.display()))?;
    let report = check_buffhtml_source(&src, file);
    emit_diagnostics(&report.diagnostics, &src, file, format, no_color);
    if matches!(report.outcome, CheckOutcome::Clean) && !format.is_json() {
        eprintln!("{}: no issues found", file.display());
    }
    Ok(report)
}

/// T133: run the check pipeline on a `.buffhtml` source string.
///
/// Returns a [`CheckReport`] populated from the `.buffhtml` parser +
/// codegen. The script-block contents are NOT type-checked at this layer
/// (T133 floor: Rust-in-script-block pass-through — type-checking them
/// would require running rustc on the spliced `.rs`, which is `buff
/// build`'s job).
pub fn check_buffhtml_source(src: &str, file: &Path) -> CheckReport {
    let source_id = SourceId(0);
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let component_name = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Component");

    match buff_lang_buffhtml_parser::parse(src, source_id) {
        Ok(template) => {
            if let Err(e) = buff_lang_codegen_buffhtml::generate(&template, component_name) {
                diagnostics.push(Diagnostic::error(
                    format!("buffhtml codegen: {e}"),
                    buff_lang_error::Span::dummy(),
                ));
            }
        }
        Err(e) => {
            diagnostics.push(Diagnostic::error(format!("buffhtml {e}"), e.span()));
        }
    }

    let outcome = compute_outcome(&diagnostics);
    CheckReport {
        diagnostics,
        outcome,
    }
}

// ---------------------------------------------------------------------------
// Type-checking: drive TypeInferencer over each function body WITHOUT codegen.
// ---------------------------------------------------------------------------

/// Walk every function in the program and run inference over its body,
/// collecting all [`buff_lang_error::TypeError`]s.
///
/// A fresh [`TypeInferencer`] is created per function so the env doesn't
/// leak between sibling functions. Parameters are pre-bound using
/// [`typeref_to_type`] — a minimal reimplementation of the private helper
/// in `buff-lang-types::infer` that covers the primitive + Option/Result
/// cases. User-defined types (struct/enum names) fall back to
/// [`Type::Unknown`], which is permissive in the inference rules.
fn type_check_decls(decls: &[Decl]) -> Vec<buff_lang_error::TypeError> {
    let mut errors = Vec::new();
    for d in decls {
        type_check_decl(d, &mut errors);
    }
    errors
}

fn type_check_decl(decl: &Decl, errors: &mut Vec<buff_lang_error::TypeError>) {
    match decl {
        Decl::FuncDecl(f) => type_check_func(f, errors),
        Decl::TraitDecl(t) => {
            // Default methods carry real bodies that need checking.
            for d in &t.defaults {
                type_check_func(d, errors);
            }
        }
        Decl::ExtendBlock(b) => {
            for m in &b.methods {
                type_check_func(m, errors);
            }
        }
        Decl::ExportDecl(inner) => type_check_decl(&inner.inner, errors),
        // Struct / Enum / Import / Module / Reexport / ExternCrate: no
        // function bodies to type-check at this layer (struct/enum field
        // types are checked at codegen in v1.0).
        _ => {}
    }
}

fn type_check_func(f: &buff_lang_ast::FuncDecl, errors: &mut Vec<buff_lang_error::TypeError>) {
    let mut inferencer = TypeInferencer::new();
    // Pre-bind parameters using the same primitive mapping the codegen
    // uses internally. User-defined types fall back to Unknown (permissive).
    for p in &f.params {
        if let Some(ty) = typeref_to_type(&p.ty) {
            inferencer.bind(&p.name.name, ty);
        }
    }
    // Walk body statements via the public infer_stmt API. Errors are
    // collected (not propagated) so multiple type errors per function are
    // reported in one pass.
    for stmt in &f.body.stmts {
        if let Err(e) = inferencer.infer_stmt(stmt) {
            errors.push(e);
        }
    }
}

// ---------------------------------------------------------------------------
// TypeRef → Type (minimal reimplementation).
// ---------------------------------------------------------------------------

/// Convert a parse-time [`TypeRef`] into a resolved [`Type`] for the
/// primitive names + Option/Result wrappers recognised in v1.0.
///
/// This mirrors the private `typeref_to_type` helper in
/// `crates/buff-lang-types/src/infer.rs` so the CLI's check pass can pre-bind
/// function parameters without modifying the types crate (which would be a
/// cross-crate ripple for T55). User-defined type names and generic
/// applications other than Option/Result fall back to [`Type::Unknown`] — a
/// permissive type that doesn't trigger spurious errors in inference.
fn typeref_to_type(ty: &TypeRef) -> Option<Type> {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "Int" => Some(Type::int_default()),
            "Float" => Some(Type::float_default()),
            "Double" => Some(Type::double()),
            "Bool" => Some(Type::bool()),
            "String" => Some(Type::string()),
            "Char" => Some(Type::char()),
            "Byte" => Some(Type::byte()),
            "Decimal" => Some(Type::Decimal),
            "Void" => Some(Type::Void),
            _ => None,
        },
        TypeRef::Option(inner, _) => Some(Type::option(
            typeref_to_type(inner).unwrap_or(Type::Unknown),
        )),
        TypeRef::Generic { base, args, .. } => {
            if let TypeRef::Named { name, .. } = base.as_ref() {
                if name.name == "Option" && args.len() == 1 {
                    let inner = typeref_to_type(&args[0]).unwrap_or(Type::Unknown);
                    return Some(Type::option(inner));
                }
                if name.name == "Result" && args.len() == 2 {
                    let ok_ty = typeref_to_type(&args[0]).unwrap_or(Type::Unknown);
                    let err_ty = typeref_to_type(&args[1]).unwrap_or(Type::Unknown);
                    return Some(Type::result(ok_ty, err_ty));
                }
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Outcome derivation + rendering.
// ---------------------------------------------------------------------------

fn compute_outcome(diagnostics: &[Diagnostic]) -> CheckOutcome {
    let has_error = diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));
    if has_error {
        return CheckOutcome::HasErrors;
    }
    if diagnostics.is_empty() {
        CheckOutcome::Clean
    } else {
        CheckOutcome::HasWarnings
    }
}

/// Render a diagnostic against the source file, prepended with the file
/// path so the user sees `<path>: line:col` rustc-style.
///
/// T43: when `use_color` is true, the severity tag and diagnostic body
/// are rendered with ANSI escape codes.
fn render_diagnostic(d: &Diagnostic, source_file: &SourceFile, use_color: bool) -> String {
    let header = match source_file.lookup(d.span.start) {
        Some((line, col)) => {
            if use_color {
                let sev = format!("[{:?}]", d.severity);
                let colored_sev = buff_lang_error::color_severity(sev.as_str(), d.severity);
                format!(
                    "{}:{}:{}: {} {}",
                    source_file.path.display(),
                    line,
                    col,
                    colored_sev,
                    d.message
                )
            } else {
                format!(
                    "{}:{}:{}: [{:?}] {}",
                    source_file.path.display(),
                    line,
                    col,
                    d.severity,
                    d.message
                )
            }
        }
        None => {
            if use_color {
                let sev = format!("[{:?}]", d.severity);
                let colored_sev = buff_lang_error::color_severity(sev.as_str(), d.severity);
                format!(
                    "{}: {} {}",
                    source_file.path.display(),
                    colored_sev,
                    d.message
                )
            } else {
                format!(
                    "{}: [{:?}] {}",
                    source_file.path.display(),
                    d.severity,
                    d.message
                )
            }
        }
    };
    let mut out = header;
    out.push('\n');
    // The body of the render (source line + caret) — reuse the diagnostic's
    // render helper from buff_lang_error.
    let body = if use_color {
        d.render_with_color(&source_file.content, true)
    } else {
        d.render(&source_file.content)
    };
    // d.render already includes the header line too; strip it so we don't
    // duplicate. The header always starts with `[{severity:?}]` (uncolored)
    // or `\x1b[3Xm[{severity:?}]\x1b[0m` (colored) on line 1.
    if let Some(rest_start) = body.find('\n') {
        let body_rest = &body[rest_start + 1..];
        out.push_str(body_rest);
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_clean_program_returns_clean() {
        let src = "func main():\n    print(\"hello\")\n";
        let report = check_source(src);
        assert_eq!(
            report.outcome,
            CheckOutcome::Clean,
            "expected Clean, got {:?}; diagnostics: {:?}",
            report.outcome,
            report.diagnostics
        );
    }

    #[test]
    fn check_type_error_returns_has_errors() {
        // Annotation says Int, value is String → TypeError.
        let src = "func main():\n    let x: Int = \"hello\"\n    print(x)\n";
        let report = check_source(src);
        assert_eq!(
            report.outcome,
            CheckOutcome::HasErrors,
            "expected HasErrors for annotation mismatch"
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| matches!(d.severity, Severity::Error)),
            "expected at least one Error-severity diagnostic"
        );
    }

    #[test]
    fn check_lex_error_returns_has_errors() {
        // Unterminated string literal → LexerError.
        let src = "func main():\n    print(\"oops)\n";
        let report = check_source(src);
        assert_eq!(
            report.outcome,
            CheckOutcome::HasErrors,
            "expected HasErrors on unterminated string"
        );
    }

    #[test]
    fn check_parse_error_returns_has_errors() {
        // Top-level let (no enclosing func) → ParseError.
        let src = "let x = 1\n";
        let report = check_source(src);
        assert_eq!(
            report.outcome,
            CheckOutcome::HasErrors,
            "expected HasErrors on top-level let"
        );
    }

    #[test]
    fn check_camelcase_function_emits_warning() {
        let src = "func myFunc():\n    print(\"hi\")\n";
        let report = check_source(src);
        assert_eq!(report.outcome, CheckOutcome::HasWarnings);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.message.contains("myFunc")
                    && d.message.contains("snake_case")
                    && matches!(d.severity, Severity::Warning)),
            "expected a snake_case warning for `myFunc`, got: {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn check_snake_case_function_emits_no_warning() {
        let src = "func my_func():\n    print(\"hi\")\n";
        let report = check_source(src);
        assert_eq!(report.outcome, CheckOutcome::Clean);
    }

    #[test]
    fn check_pascal_type_emits_no_warning() {
        // Enums ARE parser-supported (struct decls are not yet — T54 lessons).
        let src = "enum HttpRequest { Get, Post }\n\nfunc main():\n    print(\"hi\")\n";
        let report = check_source(src);
        assert_eq!(report.outcome, CheckOutcome::Clean);
    }

    #[test]
    fn check_non_pascal_type_emits_warning() {
        let src = "enum httpRequest { Get, Post }\n\nfunc main():\n    print(\"hi\")\n";
        let report = check_source(src);
        assert_eq!(report.outcome, CheckOutcome::HasWarnings);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.message.contains("httpRequest") && d.message.contains("PascalCase")),
            "expected a PascalCase warning for `httpRequest`"
        );
    }

    #[test]
    fn check_outcome_derivation_prioritises_errors() {
        let diags = vec![
            Diagnostic::warning("w1", buff_lang_error::Span::dummy()),
            Diagnostic::error("e1", buff_lang_error::Span::dummy()),
            Diagnostic::warning("w2", buff_lang_error::Span::dummy()),
        ];
        assert_eq!(compute_outcome(&diags), CheckOutcome::HasErrors);
    }

    #[test]
    fn check_outcome_derivation_clean_on_empty() {
        let diags: Vec<Diagnostic> = vec![];
        assert_eq!(compute_outcome(&diags), CheckOutcome::Clean);
    }

    #[test]
    fn check_outcome_derivation_warnings_only() {
        let diags = vec![
            Diagnostic::warning("w1", buff_lang_error::Span::dummy()),
            Diagnostic::warning("w2", buff_lang_error::Span::dummy()),
        ];
        assert_eq!(compute_outcome(&diags), CheckOutcome::HasWarnings);
    }

    // -----------------------------------------------------------------------
    // T0-G3 — @deprecated call-site warnings
    // -----------------------------------------------------------------------

    #[test]
    fn deprecated_call_emits_warning() {
        let src = r#"
@deprecated(since = "2.0", replacement = "new_fn")
func old_fn():
    return 0

func caller():
    let r = old_fn()
    print(r)
"#;
        let report = check_source(src);
        let has_dep_warning = report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("old_fn") && d.message.contains("deprecated"));
        assert!(
            has_dep_warning,
            "expected a deprecated-call warning, got: {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn deprecated_call_warning_includes_since_and_replacement() {
        let src = r#"
@deprecated(since = "3.5", replacement = "shiny_new")
func legacy():
    return 0

func main():
    legacy()
"#;
        let report = check_source(src);
        let dep = report
            .diagnostics
            .iter()
            .find(|d| d.message.contains("deprecated"))
            .expect("at least one deprecated warning");
        assert!(dep.message.contains("3.5"), "since in: {}", dep.message);
        assert!(
            dep.message.contains("shiny_new"),
            "replacement in: {}",
            dep.message
        );
    }

    #[test]
    fn non_deprecated_call_emits_no_warning() {
        let src = r#"
func normal_fn():
    return 0

func caller():
    let r = normal_fn()
    print(r)
"#;
        let report = check_source(src);
        let has_dep_warning = report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("deprecated"));
        assert!(
            !has_dep_warning,
            "no deprecated warning for non-deprecated call"
        );
    }

    #[test]
    fn deprecated_definition_alone_emits_no_warning() {
        // A deprecated fn that is never CALLED doesn't warn — the
        // warning is at call sites, not at the definition.
        let src = r#"
@deprecated(since = "1.0", replacement = "other")
func unused_old():
    return 0
"#;
        let report = check_source(src);
        let has_dep_warning = report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("deprecated"));
        assert!(!has_dep_warning, "no warning without a call site");
    }
}

// ---------------------------------------------------------------------------
// T0-G3 — @deprecated call-site walker
// ---------------------------------------------------------------------------

/// Walk `decls` looking for calls to functions marked `@deprecated`.
/// Returns one [`Diagnostic::warning`] per call site, naming the fn
/// plus the `since` and `replacement` (when provided).
///
/// The walker is single-pass: first build a map of deprecated fn names
/// → their `(since, replacement)` info, then visit every Stmt/Expr in
/// every other FuncDecl body, looking for `Expr::FuncCall` whose
/// callee is an `Expr::Ident` matching a deprecated name.
///
/// Limitations (acceptable for v1.13):
/// - Does NOT resolve through module imports (a deprecated fn imported
///   from another file is invisible to this walker — full resolution
///   arrives with T1's multi-file linker).
/// - Does NOT walk into match arms / lambda bodies (deferred to v1.18+
///   — adds a small recursive visitor; the common case of top-level
///   calls in `func` bodies is covered).
pub fn collect_deprecated_call_warnings(decls: &[Decl]) -> Vec<Diagnostic> {
    use buff_lang_ast::FuncDecl;
    use std::collections::BTreeMap;

    // Pass 1: build the deprecated-fn map.
    let mut deprecated: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
    for decl in decls {
        if let Decl::FuncDecl(FuncDecl {
            name, attributes, ..
        }) = decl
        {
            for attr in attributes {
                if attr.name.name == "deprecated" {
                    let since = attr.named_args.get("since").cloned();
                    let replacement = attr.named_args.get("replacement").cloned();
                    deprecated.insert(name.name.clone(), (since, replacement));
                    break;
                }
            }
        }
    }
    if deprecated.is_empty() {
        return Vec::new();
    }

    // Pass 2: walk each fn body, collect call-site warnings.
    let mut warnings = Vec::new();
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            for stmt in &f.body.stmts {
                collect_deprecated_calls_in_stmt(stmt, &deprecated, &mut warnings);
            }
        }
    }
    warnings
}

/// Walk a single Stmt, recursing into any embedded Exprs.
fn collect_deprecated_calls_in_stmt(
    stmt: &Stmt,
    deprecated: &std::collections::BTreeMap<String, (Option<String>, Option<String>)>,
    out: &mut Vec<Diagnostic>,
) {
    match stmt {
        Stmt::LetDecl { value, .. } => {
            collect_deprecated_calls_in_expr(value, deprecated, out);
        }
        Stmt::ExprStmt(e, _) | Stmt::Return(Some(e), _) => {
            collect_deprecated_calls_in_expr(e, deprecated, out);
        }
        Stmt::Return(None, _) => {}
        _ => {}
    }
}

/// Walk a single Expr, recursing into sub-Exprs. When a FuncCall whose
/// callee is an Ident matching a deprecated fn name is found, emit a
/// warning at the call's span.
fn collect_deprecated_calls_in_expr(
    expr: &Expr,
    deprecated: &std::collections::BTreeMap<String, (Option<String>, Option<String>)>,
    out: &mut Vec<Diagnostic>,
) {
    match expr {
        Expr::FuncCall { callee, args, span } => {
            if let Expr::Ident(ident, _) = callee.as_ref() {
                if let Some((since, replacement)) = deprecated.get(&ident.name) {
                    let msg = format_deprecated_warning(
                        &ident.name,
                        since.as_deref(),
                        replacement.as_deref(),
                    );
                    out.push(Diagnostic::warning(msg, *span));
                }
            }
            // Recurse into callee + args for nested calls.
            collect_deprecated_calls_in_expr(callee, deprecated, out);
            for arg in args {
                collect_deprecated_calls_in_expr(arg, deprecated, out);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            // Both sides may contain deprecated calls.
            collect_deprecated_calls_in_expr(lhs, deprecated, out);
            collect_deprecated_calls_in_expr(rhs, deprecated, out);
        }
        Expr::UnaryOp { operand, .. } => {
            collect_deprecated_calls_in_expr(operand, deprecated, out);
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_deprecated_calls_in_expr(receiver, deprecated, out);
            for arg in args {
                collect_deprecated_calls_in_expr(arg, deprecated, out);
            }
        }
        _ => {}
    }
}

/// Format the deprecated-call warning message. Examples:
///
/// - With both: `call to deprecated function 'old_fn' (since 2.0, use 'new_fn')`
/// - Since only: `call to deprecated function 'old_fn' (since 2.0)`
/// - Replacement only: `call to deprecated function 'old_fn' (use 'new_fn')`
/// - Neither: `call to deprecated function 'old_fn'`
fn format_deprecated_warning(name: &str, since: Option<&str>, replacement: Option<&str>) -> String {
    let mut msg = format!("call to deprecated function '{name}'");
    match (since, replacement) {
        (Some(s), Some(r)) => msg.push_str(&format!(" (since {s}, use '{r}')")),
        (Some(s), None) => msg.push_str(&format!(" (since {s})")),
        (None, Some(r)) => msg.push_str(&format!(" (use '{r}')")),
        (None, None) => {}
    }
    msg
}
