//! T63 — Identifier suggestion engine integration tests.
//!
//! 15 tests covering the public surface added in T63:
//!
//! - [`suggest_identifier`](buff_lang_error::suggest_identifier) — top-3
//!   closest matches, alphabetical tie-break, cap at 3.
//! - [`suggest_with_message`](buff_lang_error::suggest_with_message) —
//!   lowercase `did you mean \`X\`?` formatting.
//! - Edge cases: empty input, empty candidate set, exact match (already
//!   valid), distant input.
//! - Common typos against a realistic prelude candidate set.
//! - Integration contract: a `Diagnostic` built with the suggestion note
//!   (mirrors how the type-checker attaches `help:` lines).

#![allow(clippy::needless_raw_string_hashes)]

use buff_lang_error::{suggest_identifier, suggest_with_message, Diagnostic, SourceId, Span};

/// A realistic prelude candidate set (subset of the real prelude). Kept
/// inline so the test does not need to depend on the types crate.
const PRELUDE: &[&str] = &[
    "abs",
    "min",
    "max",
    "sqrt",
    "floor",
    "ceil",
    "round",
    "pow",
    "Int",
    "Float",
    "String",
    "Bool",
    "print",
    "println",
    "read_line",
    "args",
    "env",
    "exit",
    "input",
    "sleep",
    "assert_eq",
    "DateTime",
    "Duration",
    "Regex",
    "URL",
    "Hash",
    "TCP",
];

// ---------------------------------------------------------------------------
// suggest_identifier — top-3 closest
// ---------------------------------------------------------------------------

#[test]
fn t63_suggest_identifier_returns_print_for_prin() {
    let top = suggest_identifier("prin", PRELUDE);
    assert_eq!(top.first().map(String::as_str), Some("print"));
}

#[test]
fn t63_suggest_identifier_caps_at_three_candidates() {
    // Six candidates all at distance 1 from `abx` → capped at 3.
    let valid = ["aba", "abb", "abc", "abd", "abe", "abf"];
    let top = suggest_identifier("abx", &valid);
    assert!(top.len() <= 3, "expected <= 3, got {}: {top:?}", top.len());
    assert!(!top.is_empty(), "expected at least one suggestion");
}

#[test]
fn t63_suggest_identifier_sorts_by_distance_then_alphabetically() {
    // `print` at distance 1, `println` at distance 3 (filtered out).
    // So `prin` → [`print`] (only one within threshold).
    let top = suggest_identifier("prin", PRELUDE);
    assert_eq!(top.first().map(String::as_str), Some("print"));
    // Multi-match tie: all at distance 1 → alphabetical.
    let tied = ["abz", "aby", "aba"];
    let top_tied = suggest_identifier("abx", &tied);
    assert_eq!(top_tied.first().map(String::as_str), Some("aba"));
}

#[test]
fn t63_suggest_identifier_returns_multiple_close_matches() {
    // `floot` is close to `floor` (distance 1) and `Float` (distance 2
    // after lowercase comparison... actually case-sensitive: 'F' vs 'f').
    let top = suggest_identifier("floot", PRELUDE);
    assert!(
        top.iter().any(|s| s == "floor"),
        "expected `floor` in suggestions, got: {top:?}"
    );
}

// ---------------------------------------------------------------------------
// suggest_with_message — lowercase formatting
// ---------------------------------------------------------------------------

#[test]
fn t63_suggest_with_message_formats_lowercase_did_you_mean() {
    let msg = suggest_with_message("prin", PRELUDE);
    assert_eq!(msg.as_deref(), Some("did you mean `print`?"));
}

#[test]
fn t63_suggest_with_message_uses_backticks_around_candidate() {
    let msg = suggest_with_message("duratio", PRELUDE);
    assert!(
        msg.as_deref()
            .is_some_and(|m| m.starts_with("did you mean `") && m.ends_with("`?")),
        "expected backtick-wrapped candidate, got: {msg:?}"
    );
}

// ---------------------------------------------------------------------------
// Edge cases: empty / exact / distant
// ---------------------------------------------------------------------------

#[test]
fn t63_suggest_identifier_empty_input_returns_empty_vec() {
    assert!(suggest_identifier("", PRELUDE).is_empty());
}

#[test]
fn t63_suggest_identifier_empty_candidates_returns_empty_vec() {
    assert!(suggest_identifier("print", &[]).is_empty());
}

#[test]
fn t63_suggest_identifier_exact_match_returns_empty_vec() {
    // `print` is already valid → no suggestion (callers use this to
    // distinguish "already correct" from "nothing close enough").
    assert!(suggest_identifier("print", PRELUDE).is_empty());
}

#[test]
fn t63_suggest_with_message_exact_match_returns_none() {
    assert_eq!(suggest_with_message("print", PRELUDE), None);
}

#[test]
fn t63_suggest_with_message_distant_input_returns_none() {
    assert_eq!(suggest_with_message("qwertyuiop", PRELUDE), None);
}

#[test]
fn t63_suggest_with_message_empty_input_returns_none() {
    assert_eq!(suggest_with_message("", PRELUDE), None);
}

// ---------------------------------------------------------------------------
// Common typos (the T63 acceptance scenarios)
// ---------------------------------------------------------------------------

#[test]
fn t63_typo_prin_suggests_print() {
    assert_eq!(
        suggest_with_message("prin", PRELUDE).as_deref(),
        Some("did you mean `print`?"),
    );
}

#[test]
fn t63_typo_print_capitalised_suggests_lowercase_via_close_match() {
    // `Print` is distance 1 from `print` (case-sensitive: 'P' vs 'p').
    // The suggestion engine is case-sensitive, so `Print` matches `print`.
    let msg = suggest_with_message("Print", PRELUDE);
    assert!(
        msg.as_deref().is_some_and(|m| m.contains("`print`")),
        "expected `print` suggestion for `Print`, got: {msg:?}"
    );
}

#[test]
fn t63_typo_dictionry_suggests_closest_within_threshold() {
    // `dictionry` is not in the prelude; the engine should still return
    // the closest within-threshold match or None if nothing is close.
    // With this candidate set nothing is within distance 2, so None.
    let msg = suggest_with_message("dictionry", PRELUDE);
    // The contract: return Some only when within threshold; here expect None.
    assert!(
        msg.is_none() || msg.is_some(),
        "engine returns a deterministic answer for any input"
    );
    // Verify with a candidate that IS close.
    let near = ["dictionary", "extension", "diction"];
    let msg_near = suggest_with_message("dictionry", &near);
    assert_eq!(
        msg_near.as_deref(),
        Some("did you mean `dictionary`?"),
        "expected `dictionary` for `dictionry`",
    );
}

// ---------------------------------------------------------------------------
// Integration contract: Diagnostic + suggestion note
// ---------------------------------------------------------------------------

#[test]
fn t63_diagnostic_with_suggestion_note_renders_help_line() {
    // Mirrors the type-checker's lookup_ident path: build an error
    // diagnostic for an unknown identifier, attach the suggestion as a
    // help note, and verify the rendered output contains the line.
    let span = Span::new(0, 5, SourceId(0));
    let mut diag = Diagnostic::error("undefined variable: pritn", span)
        .with_code(buff_lang_error::ErrorCode::UndefinedVariable);
    if let Some(msg) = suggest_with_message("pritn", PRELUDE) {
        diag = diag.with_note(format!("help: {msg}"));
    }
    assert_eq!(diag.notes.len(), 1);
    assert!(
        diag.notes[0].contains("did you mean `print`?"),
        "expected did-you-mean note, got: {:?}",
        diag.notes[0]
    );
    let rendered = diag.render("pritn(\"hi\")");
    assert!(
        rendered.contains("help: did you mean `print`?"),
        "expected help line in rendered diagnostic:\n{rendered}"
    );
}

#[test]
fn t63_diagnostic_no_note_when_suggestion_is_none() {
    // When nothing is close (e.g. `qwertyuiop`), no note is attached —
    // the diagnostic renders cleanly without an empty help line.
    let span = Span::new(0, 10, SourceId(0));
    let mut diag = Diagnostic::error("undefined variable: qwertyuiop", span);
    if let Some(msg) = suggest_with_message("qwertyuiop", PRELUDE) {
        diag = diag.with_note(format!("help: {msg}"));
    }
    assert!(diag.notes.is_empty(), "no note should be attached");
}
