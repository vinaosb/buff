//! T36 — Error message rendering + "Did you mean?" integration tests.
//!
//! These tests pin down the **public error-message format** of the Buff
//! compiler. Error wording is part of the public API (snapshotted via insta)
//! — see `.snap` files alongside this test for the accepted output.
//!
//! Coverage:
//!
//! - `Diagnostic::render(source)` — rustc-style source line + caret pointer
//! - `levenshtein` distance + `suggest_close` / `format_did_you_mean` helpers
//! - `render_diagnostics` — render multiple diagnostics in one pass
//! - 5 stable snapshots covering: simple error, multi-char caret, did-you-mean
//!   note, multi-error file, and notes rendering

#![allow(clippy::needless_raw_string_hashes)]

use buff_lang_error::{
    format_did_you_mean, levenshtein, render_diagnostics, suggest_close, Diagnostic, SourceId, Span,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an error diagnostic anchored at `start..end` byte offsets in source
/// id `SourceId(0)`. The source id is irrelevant for `render(source)` — only
/// the byte offsets matter for line/caret computation.
fn err_at(start: usize, end: usize, msg: &str) -> Diagnostic {
    Diagnostic::error(msg, Span::new(start, end, SourceId(0)))
}

/// Prelude + keyword candidate set used by the "Did you mean?" tests. Taken
/// from `crates/buff-lang-types/src/prelude.rs` + the 25 keywords; kept here
/// as a static slice so the test does not need to depend on the types crate.
fn candidates() -> Vec<&'static str> {
    let prelude: &[&str] = &[
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
        "assert_eq",
    ];
    let keywords: &[&str] = &[
        "func", "let", "mut", "struct", "enum", "trait", "type", "if", "else", "for", "return",
        "break", "continue", "in", "match", "async", "spawn", "import", "export", "from", "as",
        "true", "false", "extern", "unsafe",
    ];
    let mut all: Vec<&str> = Vec::with_capacity(prelude.len() + keywords.len());
    all.extend_from_slice(prelude);
    all.extend_from_slice(keywords);
    all
}

// ---------------------------------------------------------------------------
// 1. Source-line + caret rendering (rustc-style)
// ---------------------------------------------------------------------------

#[test]
fn error_messages_render_shows_source_line_and_single_caret() {
    //           012345678901
    let src = "let x = 1";
    // Span covers just the `=` at byte offset 6..7.
    let diag = err_at(6, 7, "assignment is not allowed here");
    let rendered = diag.render(src);

    // Header line.
    assert!(
        rendered.contains("[Error] assignment is not allowed here"),
        "missing header in:\n{rendered}"
    );
    // The offending source line must appear.
    assert!(
        rendered.contains("let x = 1"),
        "missing source line in:\n{rendered}"
    );
    // A caret line must appear, pointing at column 6 (0-indexed: 6 spaces then `^`).
    assert!(
        rendered.contains("      ^"),
        "missing/miss-aligned caret in:\n{rendered}"
    );
}

#[test]
fn error_messages_render_multichar_span_uses_caret_width() {
    //           0123456789012345
    let src = "let name = value";
    // Span covers `value` (bytes 10..15, 5 chars).
    let diag = err_at(10, 15, "unknown identifier");
    let rendered = diag.render(src);

    // Five carets aligned to column 10 (10 leading spaces).
    assert!(
        rendered.contains("          ^^^^^"),
        "expected 5 carets at col 10 in:\n{rendered}"
    );
}

#[test]
fn error_messages_render_picks_correct_line_in_multiline_source() {
    // Two-line source.
    let src = "let a = 1\nlet b = value";
    // line1 ends at byte 9 ('\n'); line2 starts at byte 10.
    // `value` on line 2 spans bytes 18..23 ('v' at byte 18).
    let diag = err_at(18, 23, "unknown identifier `value`");
    let rendered = diag.render(src);

    // Must contain line 2 (NOT line 1's content).
    assert!(
        rendered.contains("let b = value"),
        "missing line 2 in:\n{rendered}"
    );
    // Line 1's content must NOT leak into the source-line slot (the caret
    // block renders exactly one line).
    assert!(
        !rendered.contains("let a = 1\nlet b"),
        "leaked line-1 content into caret block in:\n{rendered}"
    );
    // Caret offset is RELATIVE to line 2 start (byte 10), so `value` starts
    // at col 8 (column count from line start: 8 chars `let b = `).
    assert!(
        rendered.contains("        ^^^^^"),
        "expected caret at col 8 of line 2 in:\n{rendered}"
    );
}

#[test]
fn error_messages_render_zero_width_span_uses_single_caret() {
    let src = "let x = 1";
    // start == end → zero-width span → at least one caret.
    let diag = err_at(6, 6, "missing type annotation");
    let rendered = diag.render(src);
    assert!(
        rendered.contains("      ^"),
        "expected single caret for zero-width span in:\n{rendered}"
    );
}

#[test]
fn error_messages_render_keeps_notes_below_caret() {
    let src = "let x = 1";
    let diag = err_at(6, 7, "assignment is not allowed here")
        .with_note("consider using `mut` if you need to rebind")
        .with_note("see the Buff manual, §3 for details");
    let rendered = diag.render(src);
    assert!(
        rendered.contains("  note: consider using `mut` if you need to rebind"),
        "missing first note in:\n{rendered}"
    );
    assert!(
        rendered.contains("  note: see the Buff manual, §3 for details"),
        "missing second note in:\n{rendered}"
    );
    // Caret still present.
    assert!(
        rendered.contains("      ^"),
        "missing caret in:\n{rendered}"
    );
}

#[test]
fn error_messages_render_unicode_columns_count_chars_not_bytes() {
    // `Olá` — 'O' (1 byte), 'l' (1 byte), 'á' (2 bytes). Total bytes = 4 but
    // char count = 3. Span covers `á` (bytes 2..4). Column must be 2
    // (0-indexed: O=0, l=1, á=2).
    let src = "Olá mundo";
    let diag = err_at(2, 4, "unexpected character");
    let rendered = diag.render(src);
    // Two spaces (cols 0,1) then one caret at col 2.
    assert!(
        rendered.contains("  ^"),
        "expected caret at col 2 (char-counted) in:\n{rendered}"
    );
}

#[test]
fn error_messages_render_handles_utf8_bom() {
    // A UTF-8 BOM (`\u{feff}`, 3 bytes) at the start of the source must not
    // panic the diagnostic renderer. Before the fix, byte-index slicing
    // panicked because byte index 1 is inside the 3-byte BOM sequence.
    let src = "\u{feff}let x = 1";
    // Use a span at byte offset 0..3 — without BOM stripping this lands
    // inside the BOM (byte 1 is not a char boundary) and panics.
    let diag = err_at(0, 3, "test error");
    let rendered = diag.render(src);
    // Should not panic and should contain the source text or the header.
    assert!(
        rendered.contains("let x = 1") || rendered.contains("test error"),
        "BOM-prefixed source must not panic the renderer; got:\n{rendered}"
    );
}

#[test]
fn error_messages_render_out_of_bounds_span_omits_source_line() {
    let src = "short";
    let diag = err_at(99, 100, "post-EOF error").with_note("recoverable");
    let rendered = diag.render(src);
    assert!(
        rendered.contains("[Error] post-EOF error"),
        "missing header in:\n{rendered}"
    );
    assert!(
        rendered.contains("  note: recoverable"),
        "missing note in:\n{rendered}"
    );
    // No caret line.
    assert!(
        !rendered.contains('^'),
        "unexpected caret for out-of-bounds span in:\n{rendered}"
    );
}

#[test]
fn error_messages_render_severity_warning_uses_warning_header() {
    let src = "let x = 1";
    let diag = Diagnostic::warning("unused variable", Span::new(4, 5, SourceId(0)));
    let rendered = diag.render(src);
    assert!(
        rendered.contains("[Warning] unused variable"),
        "missing warning header in:\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 2. Levenshtein distance
// ---------------------------------------------------------------------------

#[test]
fn error_messages_levenshtein_identical_strings_distance_zero() {
    assert_eq!(levenshtein("print", "print"), 0);
    assert_eq!(levenshtein("", ""), 0);
}

#[test]
fn error_messages_levenshtein_single_substitution() {
    // 'pritn' vs 'print' — transposition counts as 2 in classic Levenshtein
    // (substitute i->n then n->i).
    assert_eq!(levenshtein("pritn", "print"), 2);
    // Single substitution.
    assert_eq!(levenshtein("print", "prink"), 1);
}

#[test]
fn error_messages_levenshtein_insertion_and_deletion() {
    assert_eq!(levenshtein("print", "printx"), 1); // insertion
    assert_eq!(levenshtein("print", "prin"), 1); // deletion
    assert_eq!(levenshtein("kitten", "sitting"), 3); // classic example
}

// ---------------------------------------------------------------------------
// 3. Did-you-mean suggestion
// ---------------------------------------------------------------------------

#[test]
fn error_messages_suggest_close_finds_print_for_pritn() {
    let cands = candidates();
    let suggestion = suggest_close("pritn", &cands);
    assert_eq!(suggestion, Some("print"));
}

#[test]
fn error_messages_suggest_close_rejects_distant_input() {
    let cands = candidates();
    // 'xyzzy' is nowhere close to any candidate.
    assert_eq!(suggest_close("xyzzy", &cands), None);
    assert_eq!(suggest_close("xyzzy", &[]), None);
}

#[test]
fn error_messages_suggest_close_breaks_ties_alphabetically() {
    // Two candidates at distance 1 from 'floo': 'floor' (substitute o->r at
    // the end... actually floor vs floo = 1 deletion) and 'Float' (uppercase
    // F is 1 substitution, but case matters — 'Float' starts with 'F').
    // Make a controlled tie: 'abx' vs ['aby', 'abz'] — both at distance 1.
    let cands = vec!["abz", "aby"]; // reverse insertion order on purpose
    let suggestion = suggest_close("abx", &cands);
    // Tie broken alphabetically → 'aby' < 'abz'.
    assert_eq!(suggestion, Some("aby"));
}

#[test]
fn error_messages_format_did_you_mean_renders_backticks() {
    let cands = candidates();
    let rendered = format_did_you_mean("pritn", &cands);
    assert_eq!(rendered.as_deref(), Some("Did you mean `print`?"));
}

#[test]
fn error_messages_format_did_you_mean_returns_none_when_no_close_match() {
    let cands = candidates();
    let rendered = format_did_you_mean("zzzzzzz", &cands);
    assert!(rendered.is_none());
}

#[test]
fn error_messages_did_you_mean_attaches_as_note() {
    // A common ergonomic pattern: take an unknown identifier, build the
    // suggestion, attach as a note on the diagnostic. Verifies the helper
    // integrates with the Diagnostic builder API.
    let cands = candidates();
    let mut diag = err_at(0, 5, "unknown identifier `pritn`");
    if let Some(note) = format_did_you_mean("pritn", &cands) {
        diag = diag.with_note(note);
    }
    assert_eq!(diag.notes.len(), 1);
    assert_eq!(diag.notes[0], "Did you mean `print`?");
}

// ---------------------------------------------------------------------------
// 4. Multi-error rendering
// ---------------------------------------------------------------------------

#[test]
fn error_messages_render_diagnostics_renders_all_in_order() {
    let src = "let a = value\nlet b = 1\nlet c = othr";
    // Two errors: line 1 `value` (bytes 8..13), line 3 `othr` (bytes 32..36).
    let diags = vec![
        err_at(8, 13, "unknown identifier `value`"),
        err_at(32, 36, "unknown identifier `othr`"),
    ];
    let rendered = render_diagnostics(&diags, src);

    // Both messages present, in order.
    let first = rendered.find("unknown identifier `value`");
    let second = rendered.find("unknown identifier `othr`");
    assert!(first.is_some(), "missing first error in:\n{rendered}");
    assert!(second.is_some(), "missing second error in:\n{rendered}");
    assert!(first < second, "errors out of order in:\n{rendered}");
    // Both source lines present.
    assert!(rendered.contains("let a = value"));
    assert!(rendered.contains("let c = othr"));
}

#[test]
fn error_messages_render_diagnostics_empty_returns_empty_string() {
    assert_eq!(render_diagnostics(&[], "anything"), "");
}

// ---------------------------------------------------------------------------
// 5. Stable snapshots (insta) — error wording is public API
// ---------------------------------------------------------------------------

#[test]
fn error_messages_snapshot_simple_type_error() {
    let src = "let x = 1\nx = 2";
    let diag = err_at(10, 11, "cannot mutate immutable variable `x`")
        .with_note("declare with `let mut x = ...` to allow mutation");
    insta::assert_snapshot!(diag.render(src), @r#"
    [Error] cannot mutate immutable variable `x`
      |
    2 | x = 2
      | ^
      |
      note: declare with `let mut x = ...` to allow mutation
    "#);
}

#[test]
fn error_messages_snapshot_multi_char_caret() {
    // Span covers `value` (5 chars).
    let src = "let name = value";
    let diag = err_at(10, 15, "unknown identifier `value`");
    insta::assert_snapshot!(diag.render(src), @r#"
    [Error] unknown identifier `value`
      |
    1 | let name = value
      |           ^^^^^
      |
    "#);
}

#[test]
fn error_messages_snapshot_did_you_mean() {
    let src = "pritn(\"hello\")";
    // Span covers `pritn` (bytes 0..5).
    let mut diag = err_at(0, 5, "unknown identifier `pritn`");
    let cands = candidates();
    if let Some(note) = format_did_you_mean("pritn", &cands) {
        diag = diag.with_note(note);
    }
    insta::assert_snapshot!(diag.render(src), @r#"
    [Error] unknown identifier `pritn`
      |
    1 | pritn("hello")
      | ^^^^^
      |
      note: Did you mean `print`?
    "#);
}

#[test]
fn error_messages_snapshot_multi_error_file() {
    let src = "let a = value\nlet b = 1\nlet c = othr";
    let diags = vec![
        err_at(8, 13, "unknown identifier `value`"),
        err_at(32, 36, "unknown identifier `othr`"),
    ];
    insta::assert_snapshot!(render_diagnostics(&diags, src), @r#"
    [Error] unknown identifier `value`
      |
    1 | let a = value
      |         ^^^^^
      |

    [Error] unknown identifier `othr`
      |
    3 | let c = othr
      |         ^^^^
      |
    "#);
}

#[test]
fn error_messages_snapshot_warning_with_notes() {
    let src = "let x = 1";
    let diag = Diagnostic::warning("unused variable `x`", Span::new(4, 5, SourceId(0)))
        .with_note("prefix with `_` to silence: `let _x = 1`");
    insta::assert_snapshot!(diag.render(src), @r#"
    [Warning] unused variable `x`
      |
    1 | let x = 1
      |     ^
      |
      note: prefix with `_` to silence: `let _x = 1`
    "#);
}
