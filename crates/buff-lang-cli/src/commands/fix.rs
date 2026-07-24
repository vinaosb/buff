//! `buff fix <FILE> [--dry-run]` — auto-apply machine-applicable fix
//! suggestions from diagnostics (T98).
//!
//! Like `cargo fix` / `rustfix` — runs `buff check` on the file, collects
//! diagnostics with `MachineApplicable` suggestions, applies the suggested
//! text replacements, and writes the fixed file.
//!
//! # Safety
//!
//! Only suggestions marked [`Applicability::MachineApplicable`] are applied.
//! Suggestions with `MaybeIncorrect` or `HasPlaceholders` are skipped and
//! reported in the summary.
//!
//! # Byte-offset stability
//!
//! Replacements are applied in **reverse order** (rightmost span first) so
//! earlier byte offsets remain valid after each replacement.

use std::path::Path;

use anyhow::{Context, Result};

use buff_lang_error::Applicability;

use crate::check::check_source;

/// Library entry point for `buff fix <FILE> [--dry-run]`.
///
/// Returns the number of applied suggestions (0 if none) and prints a
/// summary to stderr. When `dry_run` is true, the file is NOT modified;
/// instead a diff-like report is printed to stdout.
///
/// # Errors
///
/// Returns `Err` when the file cannot be read or written. Compile
/// diagnostics are NOT errors — they are filtered for suggestions.
pub fn run(file: &Path, dry_run: bool) -> Result<usize> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read `{}`", file.display()))?;

    let report = check_source(&src);

    // Collect all MachineApplicable suggestions with their diagnostic message.
    let mut suggestions: Vec<(buff_lang_error::Span, String, String)> = Vec::new();
    let mut skipped = 0usize;

    for diag in &report.diagnostics {
        for sug in &diag.suggestions {
            if sug.applicability == Applicability::MachineApplicable {
                suggestions.push((sug.span, sug.replacement.clone(), diag.message.clone()));
            } else {
                skipped += 1;
            }
        }
    }

    if suggestions.is_empty() {
        if skipped > 0 {
            eprintln!(
                "{}: no machine-applicable suggestions ({} non-applicable skipped)",
                file.display(),
                skipped
            );
        } else {
            eprintln!("{}: no fix suggestions found", file.display());
        }
        return Ok(0);
    }

    // Sort by span.start DESCENDING (rightmost first) so byte offsets stay
    // valid as we apply replacements left-to-right in the source string.
    suggestions.sort_by(|a, b| b.0.start.cmp(&a.0.start));

    if dry_run {
        // Print a diff-like preview.
        println!("--- a/{}", file.display());
        println!("+++ b/{}", file.display());
        for (span, replacement, msg) in &suggestions {
            let old_text = &src[span.start..span.end];
            println!(
                "@@ -{},{} +{},{} @@ // {}",
                span.start,
                span.end.saturating_sub(span.start),
                span.start,
                replacement.len(),
                msg
            );
            println!("-{}", old_text);
            println!("+{}", replacement);
        }
        eprintln!(
            "{}: {} suggestion(s) would be applied ({} skipped)",
            file.display(),
            suggestions.len(),
            skipped
        );
        return Ok(suggestions.len());
    }

    // Apply replacements right-to-left.
    let mut result = src.to_string();
    for (span, replacement, _msg) in &suggestions {
        result.replace_range(span.start..span.end, replacement);
    }

    std::fs::write(file, &result)
        .with_context(|| format!("failed to write `{}`", file.display()))?;

    eprintln!(
        "{}: applied {} suggestion(s) ({} skipped)",
        file.display(),
        suggestions.len(),
        skipped
    );
    Ok(suggestions.len())
}
