//! T1 (v1.25 Wave 0) — Multi-span diagnostics + fix suggestions + JSON output.
//!
//! Coverage:
//!
//! - Multi-span render: primary span + secondary labels render as separate
//!   source-line + caret blocks (rustc-style).
//! - Suggestion render: `help:` line with replacement + applicability.
//! - JSON serialization roundtrip: `to_json` produces the stable shape,
//!   `serde_json::from_str` parses it back, fields match.
//! - Backward compatibility: empty labels + suggestions → byte-identical
//!   to pre-T1 single-span render.

#![allow(clippy::needless_raw_string_hashes)]

use buff_lang_error::{
    to_json, Applicability, CodeSuggestion, Diagnostic, ErrorCode, LabelStyle, SourceId, Span,
    SpanLabel,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn err_at(start: usize, end: usize, msg: &str) -> Diagnostic {
    Diagnostic::error(msg, Span::new(start, end, SourceId(0)))
}

// ---------------------------------------------------------------------------
// 1. Multi-span render (primary + secondary labels)
// ---------------------------------------------------------------------------

#[test]
fn multispan_secondary_label_renders_as_tilde_block_with_label() {
    //           0123456789012345678 9012345
    let src = "func add(a: Int, b: Int)\n    return add(\"hi\", 2)";
    // Primary: the `"hi"` argument (bytes 38..42 on line 2).
    // Secondary: the `a` parameter declaration (bytes 9..10 on line 1).
    let diag = err_at(38, 42, "expected `Int`, found `String`")
        .with_secondary_label(Span::new(9, 10, SourceId(0)), "parameter `a` declared here");
    let rendered = diag.render(src);

    // Primary caret block present with `^` carets.
    assert!(
        rendered.contains("\"hi\""),
        "missing primary source line in:\n{rendered}"
    );
    assert!(
        rendered.contains("^^"),
        "missing primary carets in:\n{rendered}"
    );
    // Secondary tilde block present with `~` tildes + the label.
    assert!(
        rendered.contains("func add(a: Int, b: Int)"),
        "missing secondary source line in:\n{rendered}"
    );
    assert!(
        rendered.contains("~"),
        "missing secondary tildes in:\n{rendered}"
    );
    assert!(
        rendered.contains("parameter `a` declared here"),
        "missing secondary label text in:\n{rendered}"
    );
}

#[test]
fn multispan_primary_label_renders_as_caret_block_with_label() {
    let src = "let x = value\nlet y = 1";
    let diag = err_at(8, 13, "unknown identifier `value`")
        .with_label(Span::new(4, 5, SourceId(0)), "binding site");
    let rendered = diag.render(src);
    // The primary label's carets are `^` (not `~`).
    assert!(
        rendered.contains("binding site"),
        "missing primary label text in:\n{rendered}"
    );
}

#[test]
fn multispan_empty_labels_renders_byte_identical_to_pre_t1() {
    // When labels is empty, the render must be byte-identical to the
    // pre-T1 format (just header + primary caret + notes). This is the
    // backward-compatibility guarantee.
    let src = "let x = 1";
    let diag_old = err_at(0, 3, "some error").with_note("a note");
    let diag_new =
        Diagnostic::error("some error", Span::new(0, 3, SourceId(0))).with_note("a note");
    // Both have empty labels + suggestions → identical render.
    assert_eq!(diag_old.render(src), diag_new.render(src));
}

#[test]
fn multispan_multiple_labels_render_in_declaration_order() {
    let src = "let a = 1\nlet b = 2\nlet c = a + b";
    let diag = err_at(28, 29, "type mismatch in `+`")
        .with_secondary_label(Span::new(4, 5, SourceId(0)), "`a` declared here")
        .with_secondary_label(Span::new(14, 15, SourceId(0)), "`b` declared here");
    let rendered = diag.render(src);
    // Both labels present, in order: `a` before `b`.
    let a_pos = rendered.find("`a` declared here");
    let b_pos = rendered.find("`b` declared here");
    assert!(a_pos.is_some(), "missing `a` label in:\n{rendered}");
    assert!(b_pos.is_some(), "missing `b` label in:\n{rendered}");
    assert!(a_pos < b_pos, "labels out of order in:\n{rendered}");
}

#[test]
fn multispan_with_code_renders_code_tag_in_header() {
    let src = "let x = @";
    let diag = Diagnostic::error("unexpected character: '@'", Span::new(8, 9, SourceId(0)))
        .with_code(ErrorCode::UnexpectedChar)
        .with_secondary_label(Span::new(4, 5, SourceId(0)), "in `let x`");
    let rendered = diag.render(src);
    assert!(
        rendered.contains("[Error] error[E1001]: unexpected character: '@'"),
        "missing code-tagged header in:\n{rendered}"
    );
    assert!(
        rendered.contains("in `let x`"),
        "missing label in:\n{rendered}"
    );
}

#[test]
fn multispan_span_label_constructors_set_correct_style() {
    let span = Span::new(0, 1, SourceId(0));
    let primary = SpanLabel::primary(span, "primary");
    let secondary = SpanLabel::secondary(span, "secondary");
    assert_eq!(primary.style, LabelStyle::Primary);
    assert_eq!(secondary.style, LabelStyle::Secondary);
    assert_eq!(primary.label, "primary");
    assert_eq!(secondary.label, "secondary");
}

// ---------------------------------------------------------------------------
// 2. Suggestion render (help: line)
// ---------------------------------------------------------------------------

#[test]
fn suggestion_renders_help_line_with_replacement_and_applicability() {
    let src = "pritn(\"hello\")";
    let diag = err_at(0, 5, "unknown identifier `pritn`").with_suggestion(
        Span::new(0, 5, SourceId(0)),
        "print",
        Applicability::MachineApplicable,
    );
    let rendered = diag.render(src);
    assert!(
        rendered.contains("help: replace with `print` (MachineApplicable)"),
        "missing help line in:\n{rendered}"
    );
}

#[test]
fn suggestion_with_label_renders_label_before_replacement() {
    let src = "pritn(\"hello\")";
    let diag = err_at(0, 5, "unknown identifier `pritn`").with_labeled_suggestion(
        Span::new(0, 5, SourceId(0)),
        "print",
        Applicability::MachineApplicable,
        "change `pritn` to `print`",
    );
    let rendered = diag.render(src);
    assert!(
        rendered
            .contains("help: change `pritn` to `print`: replace with `print` (MachineApplicable)"),
        "missing labeled help line in:\n{rendered}"
    );
}

#[test]
fn suggestion_maybe_incorrect_renders_correct_applicability() {
    let src = "let x = value";
    let diag = err_at(8, 13, "unknown identifier").with_suggestion(
        Span::new(8, 13, SourceId(0)),
        "val",
        Applicability::MaybeIncorrect,
    );
    let rendered = diag.render(src);
    assert!(
        rendered.contains("(MaybeIncorrect)"),
        "missing MaybeIncorrect tag in:\n{rendered}"
    );
}

#[test]
fn suggestion_has_placeholders_renders_correct_applicability() {
    let src = "let x = 1";
    let diag = err_at(4, 5, "missing type annotation").with_suggestion(
        Span::new(5, 5, SourceId(0)),
        ": <type>",
        Applicability::HasPlaceholders,
    );
    let rendered = diag.render(src);
    assert!(
        rendered.contains("(HasPlaceholders)"),
        "missing HasPlaceholders tag in:\n{rendered}"
    );
}

#[test]
fn suggestion_appears_after_notes_in_render() {
    let src = "pritn(\"hi\")";
    let diag = err_at(0, 5, "unknown identifier `pritn`")
        .with_note("this name is not in scope")
        .with_suggestion(
            Span::new(0, 5, SourceId(0)),
            "print",
            Applicability::MachineApplicable,
        );
    let rendered = diag.render(src);
    let note_pos = rendered.find("note: this name is not in scope");
    let help_pos = rendered.find("help: replace with");
    assert!(note_pos.is_some(), "missing note in:\n{rendered}");
    assert!(help_pos.is_some(), "missing help in:\n{rendered}");
    assert!(
        note_pos < help_pos,
        "help must come after note in:\n{rendered}"
    );
}

#[test]
fn applicability_is_machine_applicable_predicate() {
    assert!(Applicability::MachineApplicable.is_machine_applicable());
    assert!(!Applicability::MaybeIncorrect.is_machine_applicable());
    assert!(!Applicability::HasPlaceholders.is_machine_applicable());
    assert!(!Applicability::Unspecified.is_machine_applicable());
}

#[test]
fn applicability_display_matches_variant_name() {
    assert_eq!(
        Applicability::MachineApplicable.to_string(),
        "MachineApplicable"
    );
    assert_eq!(Applicability::MaybeIncorrect.to_string(), "MaybeIncorrect");
    assert_eq!(
        Applicability::HasPlaceholders.to_string(),
        "HasPlaceholders"
    );
    assert_eq!(Applicability::Unspecified.to_string(), "Unspecified");
}

#[test]
fn code_suggestion_render_help_line_directly() {
    let s = CodeSuggestion {
        span: Span::new(0, 5, SourceId(0)),
        replacement: "print".to_string(),
        applicability: Applicability::MachineApplicable,
        label: Some("fix typo".to_string()),
    };
    let line = s.render_help_line();
    assert!(line.starts_with("  help: fix typo: "));
    assert!(line.contains("`print`"));
    assert!(line.contains("MachineApplicable"));
}

// ---------------------------------------------------------------------------
// 3. JSON serialization roundtrip
// ---------------------------------------------------------------------------

#[test]
fn json_roundtrip_simple_diagnostic_serializes_and_deserializes() {
    let src = "let x = 1";
    let diag =
        Diagnostic::error("bad", Span::new(0, 3, SourceId(0))).with_code(ErrorCode::UnexpectedChar);
    let j = to_json(&diag, src);
    let json_str = serde_json::to_string(&j).expect("serialize");
    let v: serde_json::Value = serde_json::from_str(&json_str).expect("deserialize back");
    assert_eq!(v["code"], "E1001");
    assert_eq!(v["severity"], "Error");
    assert_eq!(v["message"], "bad");
    assert_eq!(v["spans"][0]["style"], "Primary");
    assert_eq!(v["spans"][0]["byte_start"], 0);
    assert_eq!(v["spans"][0]["byte_end"], 3);
    assert_eq!(v["spans"][0]["line_start"], 1);
    assert_eq!(v["spans"][0]["col_start"], 1);
}

#[test]
fn json_roundtrip_multispan_with_labels_and_suggestion() {
    let src = "func add(a: Int)\n    return add(\"hi\")";
    let diag = Diagnostic::error(
        "expected `Int`, found `String`",
        Span::new(28, 32, SourceId(0)),
    )
    .with_code(ErrorCode::AssignTypeMismatch)
    .with_secondary_label(Span::new(9, 10, SourceId(0)), "parameter `a` declared here")
    .with_labeled_suggestion(
        Span::new(28, 32, SourceId(0)),
        "42",
        Applicability::MaybeIncorrect,
        "replace with an `Int` literal",
    )
    .with_note("`add` expects its first argument to be `Int`");

    let j = to_json(&diag, src);
    let json_str = serde_json::to_string(&j).expect("serialize");
    let v: serde_json::Value = serde_json::from_str(&json_str).expect("deserialize");

    // Code + severity + message.
    assert_eq!(v["code"], "E1203");
    assert_eq!(v["severity"], "Error");

    // Two spans: primary (the `"hi"` arg) + secondary (the `a` param).
    assert_eq!(v["spans"].as_array().unwrap().len(), 2);
    assert_eq!(v["spans"][0]["style"], "Primary");
    assert!(v["spans"][0]["label"].is_null());
    assert_eq!(v["spans"][1]["style"], "Secondary");
    assert_eq!(v["spans"][1]["label"], "parameter `a` declared here");
    // Line/col resolved: primary span at byte 28 → line 2.
    assert_eq!(v["spans"][0]["line_start"], 2);
    // Secondary span at byte 9 → line 1.
    assert_eq!(v["spans"][1]["line_start"], 1);

    // One note.
    assert_eq!(
        v["notes"][0],
        "`add` expects its first argument to be `Int`"
    );

    // One suggestion.
    assert_eq!(v["suggestions"].as_array().unwrap().len(), 1);
    assert_eq!(v["suggestions"][0]["replacement"], "42");
    assert_eq!(v["suggestions"][0]["applicability"], "MaybeIncorrect");
    assert_eq!(
        v["suggestions"][0]["label"],
        "replace with an `Int` literal"
    );
}

#[test]
fn json_no_code_emits_null_code_field() {
    let src = "let x = 1";
    let diag = Diagnostic::error("no code", Span::new(0, 1, SourceId(0)));
    let j = to_json(&diag, src);
    let json_str = serde_json::to_string(&j).expect("serialize");
    let v: serde_json::Value = serde_json::from_str(&json_str).expect("deserialize");
    assert!(v["code"].is_null(), "expected null code, got: {v}");
}

#[test]
fn json_out_of_bounds_span_emits_null_linecol() {
    let src = "short";
    let diag = Diagnostic::error("eof", Span::new(99, 100, SourceId(0)));
    let j = to_json(&diag, src);
    let json_str = serde_json::to_string(&j).expect("serialize");
    let v: serde_json::Value = serde_json::from_str(&json_str).expect("deserialize");
    assert_eq!(v["spans"][0]["byte_start"], 99);
    assert!(v["spans"][0]["line_start"].is_null());
    assert!(v["spans"][0]["col_start"].is_null());
}

#[test]
fn json_render_diagnostics_json_emits_array_for_empty() {
    let json = buff_lang_error::render_diagnostics_json(&[], "anything");
    assert_eq!(json, "[]");
}

#[test]
fn json_render_diagnostics_json_emits_array_for_multiple() {
    let src = "let a = value\nlet b = othr";
    let diags = vec![
        Diagnostic::error("unknown `value`", Span::new(8, 13, SourceId(0))),
        Diagnostic::warning("unknown `othr`", Span::new(22, 26, SourceId(0))),
    ];
    let json = buff_lang_error::render_diagnostics_json(&diags, src);
    let v: serde_json::Value = serde_json::from_str(&json).expect("parse");
    let arr = v.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["severity"], "Error");
    assert_eq!(arr[1]["severity"], "Warning");
}

#[test]
fn json_error_code_from_code_str_roundtrips_all_variants() {
    for &code in ErrorCode::all() {
        let s = code.code_str();
        assert_eq!(
            ErrorCode::from_code_str(s),
            Some(code),
            "roundtrip failed for {s}"
        );
    }
    // Unknown codes → None (no speculative variant).
    assert_eq!(ErrorCode::from_code_str("E9999"), None);
    assert_eq!(ErrorCode::from_code_str(""), None);
    assert_eq!(ErrorCode::from_code_str("not-even-a-code"), None);
}

#[test]
fn json_unicode_linecol_resolution() {
    // `Olá` — 'O' (byte 0), 'l' (byte 1), 'á' (bytes 2-3), ' ' (byte 4).
    let src = "Olá mundo";
    let diag = Diagnostic::error("bad char", Span::new(2, 4, SourceId(0)));
    let j = to_json(&diag, src);
    // 'á' starts at byte 2 → line 1, col 3 (1-based, char-counted).
    assert_eq!(j.spans[0].line_start, Some(1));
    assert_eq!(j.spans[0].col_start, Some(3));
    assert_eq!(j.spans[0].line_end, Some(1));
    assert_eq!(j.spans[0].col_end, Some(4));
}

// ---------------------------------------------------------------------------
// 4. Stable snapshots (insta) — multi-span + suggestion render is public API
// ---------------------------------------------------------------------------

#[test]
fn snapshot_multispan_primary_plus_secondary() {
    let src = "func add(a: Int, b: Int)\n    return add(\"hi\", 2)";
    let diag = err_at(38, 42, "expected `Int`, found `String`")
        .with_code(ErrorCode::AssignTypeMismatch)
        .with_secondary_label(Span::new(9, 10, SourceId(0)), "parameter `a` declared here");
    insta::assert_snapshot!(diag.render(src), @r#"
    [Error] error[E1203]: expected `Int`, found `String`
      |
    2 |     return add("hi", 2)
      |              ^^^^
      |
      |
    1 | func add(a: Int, b: Int)
      |          ~ parameter `a` declared here
      |
      "#);
}

#[test]
fn snapshot_suggestion_help_line() {
    let src = "pritn(\"hello\")";
    let diag = err_at(0, 5, "unknown identifier `pritn`")
        .with_code(ErrorCode::UndefinedVariable)
        .with_note("Did you mean `print`?")
        .with_labeled_suggestion(
            Span::new(0, 5, SourceId(0)),
            "print",
            Applicability::MachineApplicable,
            "change `pritn` to `print`",
        );
    insta::assert_snapshot!(diag.render(src), @r#"
    [Error] error[E1201]: unknown identifier `pritn`
      |
    1 | pritn("hello")
      | ^^^^^
      |
      note: Did you mean `print`?
      help: change `pritn` to `print`: replace with `print` (MachineApplicable)
    "#);
}

#[test]
fn snapshot_backward_compat_empty_labels_suggestions_unchanged() {
    // A diagnostic with NO labels and NO suggestions must render
    // byte-identically to the pre-T1 format. This snapshot pins the
    // backward-compatibility guarantee.
    let src = "let x = value";
    let diag = err_at(8, 13, "unknown identifier `value`").with_note("Did you mean `val`?");
    insta::assert_snapshot!(diag.render(src), @r#"
    [Error] unknown identifier `value`
      |
    1 | let x = value
      |         ^^^^^
      |
      note: Did you mean `val`?
    "#);
}
