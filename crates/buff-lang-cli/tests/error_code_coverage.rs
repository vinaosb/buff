//! T51 — Track E Invalid Test Fixture Coverage.
//!
//! Verifies that each invalid `.buff` fixture in `tests/fixtures/invalid/`
//! produces the expected [`ErrorCode`] when compiled. This is a regression
//! gate: if a fixture stops producing its expected code, the test fails.
//!
//! Fixtures are grouped by phase:
//!
//! | Fixture              | Expected code | Phase       |
//! |----------------------|---------------|-------------|
//! | `lex_errors.buff`    | E1001         | Lexing      |
//! | `parse_errors.buff`  | E1102         | Parsing     |
//! | `type_errors.buff`   | E1202         | Type-check  |
//! | `codegen_errors.buff`| E1301         | Codegen     |
//! | `runtime_errors.buff`| (doc only)    | Runtime     |
//! | `warning_deprecated.buff`| E1501     | Warnings    |
//!
//! Runtime errors (E14xx) are NOT compile-time detectable — they surface
//! at runtime from `buff-lang-runtime`. The fixture exists for documentation
//! and to ensure the ErrorCode enum stays in sync.

#![cfg(test)]

use buff_lang_cli::check::{check_source, CheckOutcome};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::{ErrorCode, Severity};
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

/// Root of the workspace-level test fixtures directory.
fn fixture_dir() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR is the crate dir (buff-lang-cli). Go up to workspace root.
    p.pop(); // buff-lang-cli
    p.pop(); // crates
    p.push("tests");
    p.push("fixtures");
    p.push("invalid");
    p
}

fn read_fixture(name: &str) -> String {
    let path = fixture_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture `{}`: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// E10xx — Lexing
// ---------------------------------------------------------------------------

#[test]
fn lex_errors_fixture_emits_e1001() {
    let src = read_fixture("lex_errors.buff");
    let report = check_source(&src);
    assert_eq!(
        report.outcome,
        CheckOutcome::HasErrors,
        "lex fixture should produce errors"
    );
    let has_e1001 = report.diagnostics.iter().any(|d| {
        d.code == Some(ErrorCode::UnexpectedChar) && matches!(d.severity, Severity::Error)
    });
    assert!(
        has_e1001,
        "expected E1001 (UnexpectedChar) in lex fixture, got: {:?}",
        report.diagnostics
    );
}

// ---------------------------------------------------------------------------
// E11xx — Parsing
// ---------------------------------------------------------------------------

#[test]
fn parse_errors_fixture_emits_e1102() {
    let src = read_fixture("parse_errors.buff");
    let report = check_source(&src);
    assert_eq!(
        report.outcome,
        CheckOutcome::HasErrors,
        "parse fixture should produce errors"
    );
    let has_e1102 = report.diagnostics.iter().any(|d| {
        d.code == Some(ErrorCode::UnexpectedToken) && matches!(d.severity, Severity::Error)
    });
    assert!(
        has_e1102,
        "expected E1102 (UnexpectedToken) in parse fixture, got: {:?}",
        report.diagnostics
    );
}

// ---------------------------------------------------------------------------
// E12xx — Type-checking
// ---------------------------------------------------------------------------

#[test]
fn type_errors_fixture_emits_e1202() {
    let src = read_fixture("type_errors.buff");
    let report = check_source(&src);
    assert_eq!(
        report.outcome,
        CheckOutcome::HasErrors,
        "type fixture should produce errors"
    );
    let has_e1202 = report.diagnostics.iter().any(|d| {
        d.code == Some(ErrorCode::BinaryOpTypeMismatch) && matches!(d.severity, Severity::Error)
    });
    assert!(
        has_e1202,
        "expected E1202 (BinaryOpTypeMismatch) in type fixture, got: {:?}",
        report.diagnostics
    );
}

// ---------------------------------------------------------------------------
// E13xx — Codegen
// ---------------------------------------------------------------------------

#[test]
fn codegen_errors_fixture_emits_e1301() {
    let src = read_fixture("codegen_errors.buff");
    // Codegen errors are NOT caught by `buff check` (which stops after
    // type-check). We must drive the full lex → parse → codegen pipeline.
    let source_id = buff_lang_error::SourceId(0);
    let tokens = tokenize(&src, source_id).expect("codegen fixture should lex cleanly");
    let decls = parse(&tokens, source_id).expect("codegen fixture should parse cleanly");
    let result = generate_rust(&decls);
    let err = result.expect_err("codegen fixture should produce a codegen error");
    assert_eq!(
        err.diagnostic.code,
        Some(ErrorCode::UnsupportedCodegen),
        "expected E1301 (UnsupportedCodegen) in codegen fixture, got: {:?}",
        err.diagnostic
    );
}

// ---------------------------------------------------------------------------
// E14xx — Runtime (documentation only)
// ---------------------------------------------------------------------------

#[test]
fn runtime_errors_fixture_is_clean_at_compile_time() {
    // Runtime errors are NOT compile-time detectable. The fixture should
    // pass `buff check` cleanly — it's a valid program.
    let src = read_fixture("runtime_errors.buff");
    let report = check_source(&src);
    assert_eq!(
        report.outcome,
        CheckOutcome::Clean,
        "runtime fixture should be clean at compile time, got: {:?}",
        report.diagnostics
    );
}

// ---------------------------------------------------------------------------
// E15xx — Warnings
// ---------------------------------------------------------------------------

#[test]
fn warning_deprecated_fixture_emits_e1501() {
    let src = read_fixture("warning_deprecated.buff");
    let report = check_source(&src);
    assert_eq!(
        report.outcome,
        CheckOutcome::HasWarnings,
        "deprecated fixture should produce warnings"
    );
    let has_e1501 = report.diagnostics.iter().any(|d| {
        d.code == Some(ErrorCode::DeprecatedApiUsed) && matches!(d.severity, Severity::Warning)
    });
    assert!(
        has_e1501,
        "expected E1501 (DeprecatedApiUsed) in deprecated fixture, got: {:?}",
        report.diagnostics
    );
}
