//! `buff check` — type-checker + naming-convention linter command (T55).
//!
//! Thin wrapper around [`crate::check::run_check_file`] that translates the
//! returned [`CheckOutcome`] into a library-level result the CLI binary
//! ([`main.rs`](../../main.rs)) maps to an exit code.
//!
//! Like [`crate::commands::fmt`], the library `run` does NOT call
//! [`std::process::exit`]; that would abort the test harness. The CLI
//! binary inspects the returned outcome and exits accordingly:
//!
//! | Outcome                              | Default exit | With `-D` |
//! |--------------------------------------|--------------|-----------|
//! | [`CheckOutcome::Clean`]              | 0            | 0         |
//! | [`CheckOutcome::HasWarnings`]        | 0            | 1         |
//! | [`CheckOutcome::HasErrors`]          | 1            | 1         |
//!
//! `--deny-warnings` / `-D` promotes lint warnings to exit-non-zero (mirrors
//! `rustc -D warnings` / `cargo clippy -- -D warnings`). Type errors always
//! fail the exit code regardless of the flag.
//!
//! **T1 (v1.25 Wave 0):** `--error-format <human|json>` selects the output
//! format. `json` emits a single JSON array on stdout (see
//! [`crate::check::ErrorFormat`] for the shape).

use std::path::Path;

use anyhow::Result;

use crate::check::{run_check_file_with_format, CheckOutcome, ErrorFormat};

/// Library entry point for `buff check <FILE> [--deny-warnings/-D]
/// [--error-format <human|json>] [--target <TRIPLE>] [--no-color]`.
///
/// Returns the outcome directly (no process::exit) so tests can inspect it
/// and the CLI binary can translate it to an exit code.
///
/// T112: `--target <TRIPLE>` is accepted for CLI compatibility but is a
/// no-op in check mode — `buff check` runs the standalone typechecker
/// (T55) which does NOT invoke rustc, so there is no cross-compilation
/// to perform. The flag is parsed and validated but has no effect on the
/// check outcome.
///
/// T43: `--no-color` disables ANSI color in human-readable output.
///
/// # Errors
///
/// Propagates file-read errors. Compile diagnostics are NOT errors at this
/// layer — they are returned as part of the [`CheckReport`] inside the
/// outcome.
pub fn run(
    file: &Path,
    deny_warnings: bool,
    format: ErrorFormat,
    _target: Option<&str>,
    no_color: bool,
) -> Result<CheckOutcome> {
    let report = run_check_file_with_format(file, format, no_color)?;
    let outcome = if deny_warnings && matches!(report.outcome, CheckOutcome::HasWarnings) {
        CheckOutcome::HasErrors
    } else {
        report.outcome
    };
    Ok(outcome)
}
