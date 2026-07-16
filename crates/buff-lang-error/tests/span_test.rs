//! Integration tests for span, source map, and diagnostic types.

use buff_lang_error::{
    BuffError, CodegenError, Diagnostic, LexError, ParseError, RuntimeError, Severity, SourceId,
    SourceMap, Span, TypeError,
};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Span tests
// ---------------------------------------------------------------------------

#[test]
fn span_new_constructs_correctly() {
    let id = SourceId(1);
    let span = Span::new(5, 10, id);
    assert_eq!(span.start, 5);
    assert_eq!(span.end, 10);
    assert_eq!(span.source_id, id);
}

#[test]
fn span_dummy_is_zero() {
    let span = Span::dummy();
    assert_eq!(span.start, 0);
    assert_eq!(span.end, 0);
    assert_eq!(span.source_id, SourceId(0));
}

#[test]
fn span_copy_works() {
    let a = Span::new(1, 5, SourceId(2));
    let b = a; // copy
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// SourceMap lookup tests
// ---------------------------------------------------------------------------

#[test]
fn lookup_single_line_ascii() {
    let mut sm = SourceMap::new();
    sm.add_source(
        SourceId(1),
        PathBuf::from("test.buff"),
        "hello world".into(),
    );

    // offset 6 = 'w' (7th char, 1-based)
    let result = sm.lookup(SourceId(1), 6);
    assert_eq!(result, Some((1, 7)));
}

#[test]
fn lookup_multi_line() {
    let mut sm = SourceMap::new();
    sm.add_source(
        SourceId(1),
        PathBuf::from("test.buff"),
        "line1\nline2\nline3".into(),
    );

    // offset 0 = 'l' of line1 → (1, 1)
    assert_eq!(sm.lookup(SourceId(1), 0), Some((1, 1)));
    // offset 6 = 'l' of line2 → (2, 1)
    assert_eq!(sm.lookup(SourceId(1), 6), Some((2, 1)));
    // offset 12 = 'l' of line3 → (3, 1)
    assert_eq!(sm.lookup(SourceId(1), 12), Some((3, 1)));
}

#[test]
fn lookup_unicode() {
    // "olá\nmundo\n✓"
    // bytes: o(0), l(1), á(2-3), \n(4), m(5), u(6), n(7), d(8), o(9), \n(10), ✓(11-13)
    let mut sm = SourceMap::new();
    sm.add_source(
        SourceId(1),
        PathBuf::from("test.buff"),
        "olá\nmundo\n✓".into(),
    );

    // offset 6 = 'u' (2nd char of "mundo", col=2)
    assert_eq!(sm.lookup(SourceId(1), 6), Some((2, 2)));
    // offset 11 = '✓' (line 3, col 1)
    assert_eq!(sm.lookup(SourceId(1), 11), Some((3, 1)));
}

#[test]
fn lookup_out_of_bounds_returns_none() {
    let mut sm = SourceMap::new();
    sm.add_source(SourceId(1), PathBuf::from("test.buff"), "hi".into());

    assert_eq!(sm.lookup(SourceId(1), 99), None);
}

#[test]
fn lookup_unknown_source_returns_none() {
    let sm = SourceMap::new();
    assert_eq!(sm.lookup(SourceId(42), 0), None);
}

// ---------------------------------------------------------------------------
// Diagnostic tests
// ---------------------------------------------------------------------------

#[test]
fn diagnostic_constructors() {
    let span = Span::dummy();

    let err = Diagnostic::error("something broke", span);
    assert_eq!(err.severity, Severity::Error);
    assert_eq!(err.message, "something broke");

    let warn = Diagnostic::warning("be careful", span);
    assert_eq!(warn.severity, Severity::Warning);

    let info = Diagnostic::info("for your info", span);
    assert_eq!(info.severity, Severity::Info);
}

#[test]
fn diagnostic_with_notes() {
    let span = Span::dummy();
    let diag = Diagnostic::error("bad", span)
        .with_note("here is why")
        .with_note("and also this");

    assert_eq!(diag.notes.len(), 2);
    assert_eq!(diag.notes[0], "here is why");
}

#[test]
fn diagnostic_display() {
    let span = Span::dummy();
    let diag = Diagnostic::error("test message", span);
    let display = diag.to_string();
    assert!(display.contains("Error"));
    assert!(display.contains("test message"));
}

// ---------------------------------------------------------------------------
// BuffError tests
// ---------------------------------------------------------------------------

#[test]
fn buff_error_from_lex_error() {
    let diag = Diagnostic::error("unknown token", Span::dummy());
    let lex_err = LexError::new(diag);
    let err: BuffError = lex_err.into();
    assert!(matches!(err, BuffError::Lex(_)));
}

#[test]
fn buff_error_from_parse_error() {
    let diag = Diagnostic::error("expected semicolon", Span::dummy());
    let parse_err = ParseError::new(diag);
    let err: BuffError = parse_err.into();
    assert!(matches!(err, BuffError::Parse(_)));
}

#[test]
fn buff_error_from_type_error() {
    let diag = Diagnostic::error("type mismatch", Span::dummy());
    let type_err = TypeError::new(diag);
    let err: BuffError = type_err.into();
    assert!(matches!(err, BuffError::Type(_)));
}

#[test]
fn buff_error_from_codegen_error() {
    let diag = Diagnostic::error("codegen failed", Span::dummy());
    let cg_err = CodegenError::new(diag);
    let err: BuffError = cg_err.into();
    assert!(matches!(err, BuffError::Codegen(_)));
}

#[test]
fn buff_error_from_runtime_error() {
    let diag = Diagnostic::error("runtime panic", Span::dummy());
    let rt_err = RuntimeError::new(diag);
    let err: BuffError = rt_err.into();
    assert!(matches!(err, BuffError::Runtime(_)));
}

#[test]
fn buff_error_display() {
    let diag = Diagnostic::error("bad things", Span::dummy());
    let lex_err = LexError::new(diag);
    let err: BuffError = lex_err.into();
    let display = err.to_string();
    assert!(display.contains("Lex error"));
    assert!(display.contains("bad things"));
}
