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

use std::path::Path;

use buff_lang_ast::{Decl, TypeRef};
use buff_lang_error::{Diagnostic, Severity, SourceFile, SourceId};
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;
use buff_lang_types::{Type, TypeInferencer};

use crate::naming_lint::lint_naming;

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

    let outcome = compute_outcome(&diagnostics);
    CheckReport {
        diagnostics,
        outcome,
    }
}

/// Run the check pipeline on a file, render diagnostics to stderr, and
/// return the outcome.
///
/// File-read failures propagate as `Err` (they are I/O errors, not
/// diagnostics). All compile diagnostics are rendered to stderr via
/// [`Diagnostic::render`] (rustc-style with caret) and the outcome is
/// returned for the caller to translate into an exit code.
///
/// # Errors
///
/// Returns `Err` only when the file cannot be read. Compile diagnostics
/// are NOT errors here — they become the returned [`CheckReport`].
pub fn run_check_file(file: &Path) -> anyhow::Result<CheckReport> {
    let src = std::fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("failed to read `{}`: {e}", file.display()))?;
    let report = check_source(&src);

    // Build a SourceFile so render() can resolve byte offsets to line/col.
    let source_file = SourceFile::new(file.to_path_buf(), src.clone());
    for d in &report.diagnostics {
        let rendered = render_diagnostic(d, &source_file);
        match d.severity {
            Severity::Error => eprint!("{rendered}"),
            Severity::Warning | Severity::Info => eprint!("{rendered}"),
        }
    }

    if matches!(report.outcome, CheckOutcome::Clean) {
        eprintln!("{}: no issues found", file.display());
    }
    Ok(report)
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
        // types are checked at codegen in v0.1).
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
/// primitive names + Option/Result wrappers recognised in v0.1.
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
fn render_diagnostic(d: &Diagnostic, source_file: &SourceFile) -> String {
    let header = match source_file.lookup(d.span.start) {
        Some((line, col)) => format!(
            "{}:{}:{}: [{:?}] {}",
            source_file.path.display(),
            line,
            col,
            d.severity,
            d.message
        ),
        None => format!(
            "{}: [{:?}] {}",
            source_file.path.display(),
            d.severity,
            d.message
        ),
    };
    let mut out = header;
    out.push('\n');
    // The body of the render (source line + caret) — reuse the diagnostic's
    // render-in-source helper from buff_lang_error.
    let body = d.render(&source_file.content);
    // d.render already includes the header line too; strip it so we don't
    // duplicate. The header always starts with `[{severity:?}]` on line 1.
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
}
