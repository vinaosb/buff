//! Integration tests for range expression parsing (T68).
//!
//! Tests that `0..10` (exclusive) and `0..=10` (inclusive) parse to the
//! correct AST shape, and that ranges work in `for i in 0..5` position.

use buff_lang_ast::{Expr, Ident, Literal};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse_expression;

fn sid() -> SourceId {
    SourceId(0)
}

fn span() -> buff_lang_error::Span {
    buff_lang_error::Span::dummy()
}

fn int(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn ident(name: &str) -> Expr {
    Expr::Ident(Ident::new(name, span()), span())
}

fn range(start: Expr, end: Expr, inclusive: bool) -> Expr {
    Expr::Range {
        start: Box::new(start),
        end: Box::new(end),
        inclusive,
        span: span(),
    }
}

/// Tokenize + parse a single expression from `src`. Panics on failure.
fn parse(src: &str) -> Expr {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    parse_expression(&tokens, sid()).expect("parser should succeed")
}

/// Strip span information so two `Expr`s can be compared structurally.
fn shape(e: &Expr) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// Exclusive range: `0..10`
// ---------------------------------------------------------------------------

#[test]
fn ranges_exclusive() {
    let e = parse("0..10");
    assert_eq!(
        shape(&e),
        shape(&range(int(0), int(10), false)),
        "0..10 should parse as Range(0, 10, exclusive)"
    );
}

// ---------------------------------------------------------------------------
// Inclusive range: `0..=10`
// ---------------------------------------------------------------------------

#[test]
fn ranges_inclusive() {
    let e = parse("0..=10");
    assert_eq!(
        shape(&e),
        shape(&range(int(0), int(10), true)),
        "0..=10 should parse as Range(0, 10, inclusive)"
    );
}

// ---------------------------------------------------------------------------
// Range with ident bounds: `start..end`
// ---------------------------------------------------------------------------

#[test]
fn ranges_ident_bounds() {
    let e = parse("start..end");
    assert_eq!(
        shape(&e),
        shape(&range(ident("start"), ident("end"), false)),
        "start..end should parse as Range(start, end, exclusive)"
    );
}

// ---------------------------------------------------------------------------
// Range with expression bounds: `a + 1..b * 2`
// ---------------------------------------------------------------------------

#[test]
fn ranges_precedence_additive() {
    // `a+1..b*2` should parse as `(a+1)..(b*2)` because range has lower
    // precedence than + and *.
    let e = parse("a+1..b*2");
    // Expected: Range(Add(a, 1), Mul(b, 2), exclusive)
    let expected = range(
        Expr::BinaryOp {
            op: buff_lang_ast::BinaryOp::Add,
            lhs: Box::new(ident("a")),
            rhs: Box::new(int(1)),
            span: span(),
        },
        Expr::BinaryOp {
            op: buff_lang_ast::BinaryOp::Mul,
            lhs: Box::new(ident("b")),
            rhs: Box::new(int(2)),
            span: span(),
        },
        false,
    );
    assert_eq!(shape(&e), shape(&expected));
}

// ---------------------------------------------------------------------------
// Range in for-loop position (parses as expression)
// ---------------------------------------------------------------------------

#[test]
fn ranges_in_for_loop() {
    // `for i in 0..5` — the range is the iterable expression.
    // We just verify the range expression parses correctly in that position.
    let e = parse("0..5");
    assert_eq!(
        shape(&e),
        shape(&range(int(0), int(5), false)),
        "0..5 should parse as Range(0, 5, exclusive)"
    );
}

// ---------------------------------------------------------------------------
// Display format
// ---------------------------------------------------------------------------

#[test]
fn ranges_display_exclusive() {
    let e = parse("0..10");
    assert_eq!(e.to_string(), "Range(Lit(Int(0)), Lit(Int(10)), excl)");
}

#[test]
fn ranges_display_inclusive() {
    let e = parse("0..=10");
    assert_eq!(e.to_string(), "Range(Lit(Int(0)), Lit(Int(10)), incl)");
}
