//! `buff test` — discover and run `@test` functions in a `.buff` file (T35).
//!
//! Pipeline: parse → discover `@test` funcs → filter by `--pattern` →
//! generate Rust test harness → compile via `rustc` → run → report counts.
//! See [`crate::test_runner`] for the real logic; this module is the thin
//! CLI entry point that prints the summary and sets the exit code.

use std::path::Path;

use anyhow::Result;

use crate::test_runner;

/// Entry point for `buff test <FILE> [--pattern <PATTERN>]`.
///
/// Returns `Ok(())` if all tests pass (exit 0). If any test fails, the
/// process exits with code `1` directly (via [`std::process::exit`]) so the
/// exit code is preserved even though the function's return type is
/// `Result<()>`.
///
/// # Errors
///
/// Propagates any pipeline error (file-not-found, lex/parse/codegen
/// failure, rustc invocation failure). A failing TEST (assertion panic
/// inside a `@test` fn) is NOT an `Err` here — it's reflected in the
/// [`test_runner::TestReport`] counts and triggers an `exit(1)`.
pub fn run(file: &Path, pattern: Option<&str>) -> Result<()> {
    let pat = pattern.unwrap_or("");
    let report = test_runner::run_tests(file, pat)?;

    eprintln!("{}", report.summary_line());

    if report.failed > 0 {
        std::process::exit(report.exit_code());
    }
    Ok(())
}
