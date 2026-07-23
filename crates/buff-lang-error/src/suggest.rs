//! T63 — Identifier suggestion engine ("did you mean `print`?").
//!
//! This module extends the T36 single-best-match helpers
//! ([`suggest_close`](crate::diagnostic::suggest_close) /
//! [`format_did_you_mean`](crate::diagnostic::format_did_you_mean)) with two
//! additions the error-quality pass (T63) needed:
//!
//! - [`suggest_identifier`] — returns the **top-3** closest matches (the T36
//!   helpers only ever return one). Useful for "did you mean one of …?" hints
//!   and for tooling (LSP completion, REPL `:help`) that wants a shortlist.
//! - [`suggest_with_message`] — formats a single best match as a lowercase
//!   `did you mean \`X\`?` note, complementing the sentence-case
//!   [`format_did_you_mean`] used by the parser. Both formats appear in
//!   different parts of the diagnostic surface; the lowercase form is the
//!   canonical `help:` line payload.
//!
//! # Determinism
//!
//! Like the T36 helpers, this module never depends on `HashMap` iteration
//! order. Candidates are scored by [`levenshtein`](crate::diagnostic::levenshtein)
//! distance, ties are broken **alphabetically**, and the result is a stable
//! `Vec<String>` / `Option<String>` for the same inputs every run.
//!
//! # No external dependency
//!
//! Levenshtein is reused from [`crate::diagnostic`] (char-based, two-row DP).
//! No `strsim` or other crate is pulled in — the workspace keeps
//! `buff-lang-error` a leaf crate with only `thiserror` as a dependency.

use crate::diagnostic::{levenshtein, SUGGESTION_MAX_DISTANCE};

/// Maximum number of candidates [`suggest_identifier`] returns.
///
/// Three matches the density of rustc / clangd "did you mean" shortlists —
/// enough to cover the common typo cluster (e.g. `prin` → `print`, `println`,
/// `printf`) without flooding the diagnostic with distant noise.
pub const SUGGESTION_TOP_N: usize = 3;

/// Return the top-`N` closest candidates to `input`, sorted by ascending
/// Levenshtein distance with alphabetical tie-breaking.
///
/// `N` is [`SUGGESTION_TOP_N`] (3). Candidates outside
/// [`SUGGESTION_MAX_DISTANCE`] edits of `input` are dropped — the same
/// threshold the single-match [`suggest_close`](crate::diagnostic::suggest_close)
/// uses, so the two helpers agree on what counts as "close enough".
///
/// Returns owned `String`s (not borrowed) so callers can freely mix the
/// result with other owned data (e.g. concatenating into a diagnostic note)
/// without juggling lifetimes across the candidate slice.
///
/// # Examples
///
/// ```
/// # use buff_lang_error::suggest_identifier;
/// let valid = ["print", "println", "printf", "abs", "min"];
/// // `pritn` is within distance 2 of `print` (transposition) and further
/// // from the others — the closest match wins the first slot.
/// let top = suggest_identifier("pritn", &valid);
/// assert_eq!(top.first().map(String::as_str), Some("print"));
/// assert!(top.len() <= 3);
/// ```
///
/// # Determinism
///
/// The sort key is `(distance, candidate_string)` so two candidates at the
/// same distance always come out in lexicographic order — the function is
/// a pure function of its inputs.
pub fn suggest_identifier(input: &str, valid: &[&str]) -> Vec<String> {
    if input.is_empty() || valid.is_empty() {
        return Vec::new();
    }
    // An exact match means the input is already valid — no suggestion needed.
    // Mirrors the `format_did_you_mean` contract: returning an empty Vec here
    // lets callers distinguish "input was already correct" from "nothing was
    // close enough" without an extra round-trip.
    if valid.contains(&input) {
        return Vec::new();
    }

    let input_len = input.chars().count();

    // Score every candidate within the threshold. Collect into a Vec so we
    // can sort by (distance, name) — deterministic ordering.
    let mut scored: Vec<(usize, String)> = Vec::new();
    for &cand in valid {
        let cand_len = cand.chars().count();
        // Cheap length pre-filter: edits needed >= abs(len delta).
        let len_delta = cand_len
            .saturating_sub(input_len)
            .max(input_len.saturating_sub(cand_len));
        if len_delta > SUGGESTION_MAX_DISTANCE {
            continue;
        }
        let d = levenshtein(input, cand);
        if d > SUGGESTION_MAX_DISTANCE {
            continue;
        }
        // Skip exact matches defensively (length pre-filter lets an exact
        // match through; the early-return above already handles the common
        // case, but a duplicate candidate in `valid` would otherwise resurface).
        if d == 0 {
            continue;
        }
        scored.push((d, cand.to_string()));
    }

    // Sort by distance ascending, then alphabetically for deterministic
    // tie-breaking — same policy as `suggest_close`.
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.as_str().cmp(b.1.as_str())));

    scored
        .into_iter()
        .take(SUGGESTION_TOP_N)
        .map(|(_, name)| name)
        .collect()
}

/// Build a lowercase `did you mean \`X\`?` note for the closest candidate,
/// or `None` when no candidate is close enough / the input is already valid.
///
/// This is the lowercase sibling of
/// [`format_did_you_mean`](crate::diagnostic::format_did_you_mean): the
/// parser emits the sentence-case `Did you mean \`X\`?` form as a `note:`
/// line, while the type-checker and `buff check` linter emit the lowercase
/// form as a `help:` line payload (see T63 spec: *"help: did you mean
/// \`print\`?"*). Both forms resolve to the same best candidate via the same
/// [`levenshtein`] scoring.
///
/// Returns `None` when:
/// - `input` is empty, or
/// - `valid` is empty, or
/// - `input` exactly matches a candidate (already valid — no suggestion), or
/// - no candidate is within [`SUGGESTION_MAX_DISTANCE`] edits.
///
/// # Examples
///
/// ```
/// # use buff_lang_error::suggest_with_message;
/// let valid = ["print", "println", "abs", "min"];
/// assert_eq!(
///     suggest_with_message("prin", &valid).as_deref(),
///     Some("did you mean `print`?"),
/// );
/// assert_eq!(suggest_with_message("print", &valid), None); // already valid
/// assert_eq!(suggest_with_message("zzzzzzz", &valid), None); // too far
/// ```
pub fn suggest_with_message(input: &str, valid: &[&str]) -> Option<String> {
    if input.is_empty() || valid.is_empty() {
        return None;
    }
    // Already-valid input → no suggestion (mirrors suggest_identifier).
    if valid.contains(&input) {
        return None;
    }
    // Reuse the top-N shortlist, take the head. This keeps the two public
    // helpers consistent: the best candidate is always the first element of
    // suggest_identifier(input, valid).
    suggest_identifier(input, valid)
        .into_iter()
        .next()
        .map(|cand| format!("did you mean `{cand}`?"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggest_identifier_returns_top_closest() {
        let valid = ["print", "println", "printf", "abs", "min"];
        let top = suggest_identifier("prin", &valid);
        // `print` is distance 1; `println` / `printf` are distance 3+ (filtered).
        assert_eq!(top.first().map(String::as_str), Some("print"));
    }

    #[test]
    fn suggest_identifier_caps_at_three() {
        // Six candidates all at distance 1 from `abx`.
        let valid = ["aby", "abz", "abc", "abd", "abe", "abf"];
        let top = suggest_identifier("abx", &valid);
        assert!(top.len() <= SUGGESTION_TOP_N, "got {}: {top:?}", top.len());
    }

    #[test]
    fn suggest_identifier_breaks_ties_alphabetically() {
        let valid = ["abz", "aby", "aba"]; // reverse insertion order
        let top = suggest_identifier("abx", &valid);
        // All at distance 1 → sorted alphabetically: aba, aby, abz.
        assert_eq!(top.first().map(String::as_str), Some("aba"));
    }

    #[test]
    fn suggest_identifier_empty_input_returns_empty() {
        let valid = ["print", "abs"];
        assert!(suggest_identifier("", &valid).is_empty());
    }

    #[test]
    fn suggest_identifier_empty_valid_returns_empty() {
        assert!(suggest_identifier("print", &[]).is_empty());
    }

    #[test]
    fn suggest_identifier_exact_match_returns_empty() {
        let valid = ["print", "abs"];
        assert!(suggest_identifier("print", &valid).is_empty());
    }

    #[test]
    fn suggest_identifier_rejects_distant_input() {
        let valid = ["print", "abs", "min"];
        assert!(suggest_identifier("zzzzzzz", &valid).is_empty());
    }

    #[test]
    fn suggest_with_message_formats_lowercase() {
        let valid = ["print", "println", "abs"];
        assert_eq!(
            suggest_with_message("prin", &valid).as_deref(),
            Some("did you mean `print`?"),
        );
    }

    #[test]
    fn suggest_with_message_none_for_exact_match() {
        let valid = ["print", "abs"];
        assert_eq!(suggest_with_message("print", &valid), None);
    }

    #[test]
    fn suggest_with_message_none_for_empty_input() {
        let valid = ["print"];
        assert_eq!(suggest_with_message("", &valid), None);
    }

    #[test]
    fn suggest_with_message_none_for_empty_valid() {
        assert_eq!(suggest_with_message("print", &[]), None);
    }

    #[test]
    fn suggest_with_message_none_for_distant_input() {
        let valid = ["print", "abs"];
        assert_eq!(suggest_with_message("qwertyuiop", &valid), None);
    }
}
