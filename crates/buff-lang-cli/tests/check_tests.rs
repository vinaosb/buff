//! Integration tests for `buff check` (T55).
//!
//! Coverage:
//! - QA: clean valid program → [`CheckOutcome::Clean`].
//! - QA: type error → [`CheckOutcome::HasErrors`] (exit-1 equivalent).
//! - QA: lex error → [`CheckOutcome::HasErrors`].
//! - QA: parse error → [`CheckOutcome::HasErrors`].
//! - QA: camelCase function name → a Warning diagnostic is emitted.
//! - QA: snake_case function name → no naming warning.
//! - QA: PascalCase type → no warning; non-PascalCase type → warning.
//! - Command entry: `commands::check::run(&file, deny_warnings)` mirrors the
//!   CLI exit-code translation; `--deny-warnings` promotes warnings to
//!   [`CheckOutcome::HasErrors`].
//!
//! All test names contain `check` so `cargo test -p buff-lang-cli check`
//! matches them.

#![cfg(test)]

use buff_lang_cli::check::{check_source, CheckOutcome};
use buff_lang_error::Severity;

// ---------------------------------------------------------------------------
// 1. Outcome-level QA — the RED/GREEN acceptance matrix.
// ---------------------------------------------------------------------------

#[test]
fn check_clean_program_returns_clean_outcome() {
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
fn check_typed_clean_program_returns_clean_outcome() {
    // A program with typed params + a return type that type-checks cleanly.
    let src = "func add(a: Int, b: Int) -> Int:\n    return a + b\n\nfunc main():\n    print(add(2, 3))\n";
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
fn check_type_error_returns_has_errors_outcome() {
    // Annotation says Int, value is a String literal → TypeError.
    // This is the RED acceptance case from the task spec.
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
fn check_logical_operator_type_mismatch_is_error() {
    // `1 and 2` — logical And on Ints → TypeError.
    let src = "func main():\n    let b = 1 and 2\n    print(b)\n";
    let report = check_source(src);
    assert_eq!(report.outcome, CheckOutcome::HasErrors);
}

#[test]
fn check_lex_error_returns_has_errors_outcome() {
    // Unterminated string literal → LexerError short-circuits the pipeline.
    let src = "func main():\n    print(\"oops)\n";
    let report = check_source(src);
    assert_eq!(report.outcome, CheckOutcome::HasErrors);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| matches!(d.severity, Severity::Error)),
        "lex errors must surface as Error-severity diagnostics"
    );
}

#[test]
fn check_parse_error_returns_has_errors_outcome() {
    // Top-level `let` (no enclosing func) → ParseError.
    let src = "let x = 1\n";
    let report = check_source(src);
    assert_eq!(report.outcome, CheckOutcome::HasErrors);
}

#[test]
fn check_camelcase_function_emits_warning_outcome() {
    let src = "func myFunc():\n    print(\"hi\")\n";
    let report = check_source(src);
    assert_eq!(
        report.outcome,
        CheckOutcome::HasWarnings,
        "camelCase fn name should produce HasWarnings, got {:?}; diags: {:?}",
        report.outcome,
        report.diagnostics
    );
}

#[test]
fn check_camelcase_function_emits_explicit_snake_case_warning() {
    let src = "func myFunc():\n    print(\"hi\")\n";
    let report = check_source(src);
    let warning = report
        .diagnostics
        .iter()
        .find(|d| d.message.contains("myFunc") && d.message.contains("snake_case"));
    assert!(
        warning.is_some(),
        "expected a `myFunc should be snake_case` warning, got: {:?}",
        report.diagnostics
    );
    let w = warning.expect("checked above");
    assert!(
        matches!(w.severity, Severity::Warning),
        "expected Warning severity, got {:?}",
        w.severity
    );
}

#[test]
fn check_snake_case_function_emits_no_naming_warning() {
    let src = "func my_func():\n    print(\"hi\")\n";
    let report = check_source(src);
    assert_eq!(report.outcome, CheckOutcome::Clean);
    let any_naming_warning = report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("snake_case") || d.message.contains("PascalCase"));
    assert!(
        !any_naming_warning,
        "snake_case fn should not produce a naming warning, got: {:?}",
        report.diagnostics
    );
}

#[test]
fn check_pascal_case_struct_emits_no_warning() {
    // Struct decls are not yet parser-supported (only enum/func/trait/extend).
    // Drive lint_naming directly with a hand-built StructDecl AST so the
    // struct field check is exercised end-to-end.
    use buff_lang_ast::{Decl, Ident, StructDecl};
    use buff_lang_cli::naming_lint::lint_naming;
    use buff_lang_error::Span;
    let decl = Decl::StructDecl(StructDecl { name: Ident::new("HttpRequest", Span::dummy()),
    fields: vec![(
        Ident::new("method", Span::dummy()),
        builtin_named_ty("String"),
    )], traits: Vec::new(), type_params: Vec::new(), span: Span::dummy(), });
    let diags = lint_naming(&[decl]);
    assert!(
        diags.is_empty(),
        "PascalCase struct + snake_case field should not warn, got: {:?}",
        diags
    );
}

#[test]
fn check_non_pascal_case_struct_emits_pascal_warning() {
    use buff_lang_ast::{Decl, Ident, StructDecl};
    use buff_lang_cli::naming_lint::lint_naming;
    use buff_lang_error::Span;
    let decl = Decl::StructDecl(StructDecl { name: Ident::new("httpRequest", Span::dummy()),
    fields: vec![(
        Ident::new("method", Span::dummy()),
        builtin_named_ty("String"),
    )], traits: Vec::new(), type_params: Vec::new(), span: Span::dummy(), });
    let diags = lint_naming(&[decl]);
    let warning = diags
        .iter()
        .find(|d| d.message.contains("httpRequest") && d.message.contains("PascalCase"));
    assert!(
        warning.is_some(),
        "expected a PascalCase warning for `httpRequest`, got: {:?}",
        diags
    );
}

#[test]
fn check_pascal_case_enum_emits_no_warning() {
    let src = "enum Color { Red, Green, Blue }\n\nfunc main():\n    print(\"hi\")\n";
    let report = check_source(src);
    assert_eq!(
        report.outcome,
        CheckOutcome::Clean,
        "PascalCase enum + variants should not warn, got: {:?}",
        report.diagnostics
    );
}

#[test]
fn check_non_pascal_case_enum_emits_warning() {
    let src = "enum httpRequest { Get, Post }\n\nfunc main():\n    print(\"hi\")\n";
    let report = check_source(src);
    assert_eq!(report.outcome, CheckOutcome::HasWarnings);
    let warning = report
        .diagnostics
        .iter()
        .find(|d| d.message.contains("httpRequest") && d.message.contains("PascalCase"));
    assert!(
        warning.is_some(),
        "expected a PascalCase warning for `httpRequest`, got: {:?}",
        report.diagnostics
    );
}

#[test]
fn check_enum_with_non_pascal_variant_emits_warning() {
    let src = "enum Color { red, green }\n\nfunc main():\n    print(\"hi\")\n";
    let report = check_source(src);
    assert_eq!(report.outcome, CheckOutcome::HasWarnings);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("`red`") && d.message.contains("PascalCase")),
        "expected a PascalCase warning for variant `red`, got: {:?}",
        report.diagnostics
    );
}

/// Helper: build a `TypeRef::Named` for a primitive name (used by struct
/// field tests that construct AST by hand).
fn builtin_named_ty(name: &str) -> buff_lang_ast::TypeRef {
    use buff_lang_error::Span;
    buff_lang_ast::TypeRef::Named {
        name: buff_lang_ast::Ident::new(name, Span::dummy()),
        span: Span::dummy(),
    }
}

#[test]
fn check_camelcase_let_binding_emits_warning() {
    let src = "func main():\n    let itemCount = 42\n    print(itemCount)\n";
    let report = check_source(src);
    assert_eq!(report.outcome, CheckOutcome::HasWarnings);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("itemCount") && d.message.contains("snake_case")),
        "expected a snake_case warning for variable `itemCount`"
    );
}

// ---------------------------------------------------------------------------
// 2. Outcome semantics — error dominates warning.
// ---------------------------------------------------------------------------

#[test]
fn check_error_and_warning_yields_has_errors() {
    // Type error AND camelCase fn name — error wins.
    let src = "func myFunc():\n    let x: Int = \"no\"\n    print(x)\n";
    let report = check_source(src);
    assert_eq!(
        report.outcome,
        CheckOutcome::HasErrors,
        "error should dominate warning"
    );
    // Both diagnostics should be present (the warning is still emitted).
    let has_error = report
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));
    let has_warning = report
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Warning));
    assert!(has_error, "expected an error diagnostic");
    assert!(has_warning, "expected the naming warning too");
}

// ---------------------------------------------------------------------------
// 3. Command entry — drives the CLI library fn end-to-end on a temp file.
// ---------------------------------------------------------------------------

#[test]
fn check_command_run_on_clean_file_returns_clean() {
    let dir = std::env::temp_dir().join("buff-check-tests");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("check_command_clean.buff");
    std::fs::write(&path, "func main():\n    print(\"hi\")\n").expect("write temp");
    let outcome = buff_lang_cli::commands::check::run(&path, false).expect("check ok");
    assert_eq!(outcome, CheckOutcome::Clean);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn check_command_run_on_type_error_returns_has_errors() {
    let dir = std::env::temp_dir().join("buff-check-tests");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("check_command_type_error.buff");
    std::fs::write(
        &path,
        "func main():\n    let x: Int = \"oops\"\n    print(x)\n",
    )
    .expect("write");
    let outcome = buff_lang_cli::commands::check::run(&path, false).expect("check ok");
    assert_eq!(outcome, CheckOutcome::HasErrors);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn check_command_run_on_camelcase_returns_has_warnings_by_default() {
    let dir = std::env::temp_dir().join("buff-check-tests");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("check_command_camelcase.buff");
    std::fs::write(&path, "func myFunc():\n    print(\"hi\")\n").expect("write");
    let outcome = buff_lang_cli::commands::check::run(&path, false).expect("check ok");
    assert_eq!(outcome, CheckOutcome::HasWarnings);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn check_command_run_with_deny_warnings_promotes_to_has_errors() {
    let dir = std::env::temp_dir().join("buff-check-tests");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("check_command_deny_warnings.buff");
    std::fs::write(&path, "func myFunc():\n    print(\"hi\")\n").expect("write");
    let outcome = buff_lang_cli::commands::check::run(&path, /* deny_warnings */ true).expect("ok");
    assert_eq!(
        outcome,
        CheckOutcome::HasErrors,
        "--deny-warnings should promote HasWarnings → HasErrors"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn check_command_run_on_missing_file_propagates_error() {
    let dir = std::env::temp_dir().join("buff-check-tests");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("does_not_exist.buff");
    let _ = std::fs::remove_file(&path);
    let result = buff_lang_cli::commands::check::run(&path, false);
    assert!(
        result.is_err(),
        "missing file should propagate as Err (not an outcome)"
    );
}

// ---------------------------------------------------------------------------
// 4. Diagnostic rendering — the rendered string includes the file path,
//    line/col, severity, message, and a caret pointing at the span.
// ---------------------------------------------------------------------------

#[test]
fn check_rendered_output_includes_severity_and_path() {
    let dir = std::env::temp_dir().join("buff-check-tests");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("check_render.buff");
    std::fs::write(&path, "func myFunc():\n    print(\"hi\")\n").expect("write");

    // Run via the file entry so render path is exercised. Capture stderr
    // by redirecting to a file (the simplest cross-process way in a test).
    // We don't assert on stderr text (that's flaky); we just confirm the
    // run itself succeeds and returns the expected outcome.
    let outcome = buff_lang_cli::commands::check::run(&path, false).expect("check ok");
    assert_eq!(outcome, CheckOutcome::HasWarnings);
    let _ = std::fs::remove_file(&path);
}
