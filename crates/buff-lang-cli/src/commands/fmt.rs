//! `buff fmt` — format a `.buff` source file into canonical form (T54).
//!
//! Two modes:
//!
//! - **Write mode** (default): rewrites the file in place with the
//!   canonical form produced by [`buff_lang_cli::fmt::format_source`].
//! - **Check mode** (`--check`): does NOT write; signals (via
//!   [`FmtOutcome::NeedsFormat`]) when the file isn't already in
//!   canonical form (mirrors `cargo fmt --check`). The CLI binary
//!   translates that variant into `exit(1)`.

use std::path::Path;

use anyhow::{Context, Result};

use crate::fmt;

/// The outcome of a `buff fmt` invocation.
///
/// The CLI binary translates `NeedsFormat` into `std::process::exit(1)`
/// so a CI `buff fmt --check` step fails the build on un-formatted code.
/// Returning an enum (instead of calling `exit` from inside the library
/// `run`) keeps the function testable: tests can inspect the variant
/// without aborting the test process.
#[derive(Debug, PartialEq, Eq)]
pub enum FmtOutcome {
    /// File is already canonical — nothing to do.
    AlreadyFormatted,
    /// File was rewritten with the canonical form (write mode only).
    Formatted,
    /// File is not canonical and would be reformatted (check mode only).
    /// The CLI binary exits with code 1 in this case.
    NeedsFormat,
}

/// Library entry point for `buff fmt <FILE> [--check]`.
///
/// - Read the file.
/// - Run [`fmt::format_source`] to compute the canonical form.
/// - In check mode: return [`FmtOutcome::NeedsFormat`] if the file
///   differs from canonical (the CLI binary maps this to `exit(1)`).
/// - In write mode: overwrite the file with canonical (if different)
///   and return [`FmtOutcome::Formatted`].
/// - Already-canonical: return [`FmtOutcome::AlreadyFormatted`].
///
/// # Errors
///
/// Propagates any I/O or formatting error. Parse errors surface via
/// [`anyhow::Error`] chain (the user sees the original diagnostic
/// because [`fmt::FormatError`] implements [`std::error::Error`] and
/// `anyhow`'s `?` preserves the source chain).
pub fn run(file: &Path, check: bool) -> Result<FmtOutcome> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read `{}`", file.display()))?;

    let canonical = fmt::format_source(&src)
        .with_context(|| format!("failed to format `{}`", file.display()))?;

    if src == canonical {
        return Ok(FmtOutcome::AlreadyFormatted);
    }

    if check {
        eprintln!(
            "{}: not formatted (run `buff fmt {}` to fix)",
            file.display(),
            file.display()
        );
        return Ok(FmtOutcome::NeedsFormat);
    }

    std::fs::write(file, &canonical)
        .with_context(|| format!("failed to write `{}`", file.display()))?;
    eprintln!("Formatted {}", file.display());
    Ok(FmtOutcome::Formatted)
}
