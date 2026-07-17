//! Integration tests for the pipeline operator `|>` parsing (T69).
//!
//! The pipeline operator desugars IN THE PARSER to a [`Expr::FuncCall`]: the
//! left operand is prepended as the first argument of the right-hand call.
//! Chaining is left-associative.
//!
//! These tests assert the desugared AST shape directly (via `Expr::Display`,
//! which for `FuncCall` renders `Call(callee, [args])`).

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

/// Build a `callee(args...)` FuncCall.
fn call(callee: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident(callee)),
        args,
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
// `x |> f()` → `Call(f, [x])`  (the canonical spec example)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_simple() {
    let e = parse("x |> f()");
    assert_eq!(
        shape(&e),
        shape(&call("f", vec![ident("x")])),
        "`x |> f()` should desugar to Call(f, [x])"
    );
}

// ---------------------------------------------------------------------------
// `"hello" |> print()` → `Call(print, ["hello"])`  (QA mirror at AST level)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_string_to_print() {
    let e = parse("\"hello\" |> print()");
    assert_eq!(
        shape(&e),
        shape(&call(
            "print",
            vec![Expr::Literal(Literal::String("hello".to_string()), span())]
        )),
        "`\"hello\" |> print()` should desugar to Call(print, [\"hello\"])"
    );
}

// ---------------------------------------------------------------------------
// `data |> process() |> filter()` → `Call(filter, [Call(process, [data])])`
// (left-associative chaining)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_chained() {
    let e = parse("data |> process() |> filter()");
    let expected = call("filter", vec![call("process", vec![ident("data")])]);
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`data |> process() |> filter()` should desugar to Call(filter, [Call(process, [data])])"
    );
}

// ---------------------------------------------------------------------------
// `x |> f(a, b)` → `Call(f, [x, a, b])`  (extra args preserved after LHS)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_extra_args() {
    let e = parse("x |> f(a, b)");
    assert_eq!(
        shape(&e),
        shape(&call("f", vec![ident("x"), ident("a"), ident("b")])),
        "`x |> f(a, b)` should desugar to Call(f, [x, a, b])"
    );
}

// ---------------------------------------------------------------------------
// `x |> f` (bare callee, no parens) → `Call(f, [x])`
// ---------------------------------------------------------------------------

#[test]
fn pipeline_bare_callee() {
    let e = parse("x |> f");
    assert_eq!(
        shape(&e),
        shape(&call("f", vec![ident("x")])),
        "`x |> f` (bare callee) should desugar to Call(f, [x])"
    );
}

// ---------------------------------------------------------------------------
// Precedence: `a + b |> f()` → `Call(f, [a + b])`
// Pipeline binds looser than `+`, so the whole `a + b` becomes the argument.
// ---------------------------------------------------------------------------

#[test]
fn pipeline_precedence_looser_than_additive() {
    let e = parse("a + b |> f()");
    let inner = Expr::BinaryOp {
        op: buff_lang_ast::BinaryOp::Add,
        lhs: Box::new(ident("a")),
        rhs: Box::new(ident("b")),
        span: span(),
    };
    let expected = call("f", vec![inner]);
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`a + b |> f()` should desugar to Call(f, [a + b]) (pipeline binds looser than +)"
    );
}

// ---------------------------------------------------------------------------
// Precedence: `x |> f() * 2` → ParseError (RHS is `f() * 2`, not a call).
//
// Pipeline is the LOWEST-precedence binary operator (just below range), so
// its RHS greedily consumes a full range-level expression. `f() * 2` is a
// BinaryOp, not a call, so the desugar rejects it. Users who want the
// `(x |> f()) * 2` grouping must parenthesize — see the next test.
// ---------------------------------------------------------------------------

#[test]
fn pipeline_rhs_consumed_full_expr_errors() {
    let tokens = tokenize("x |> f() * 2", sid()).expect("lexer should succeed");
    let err = parse_expression(&tokens, sid()).expect_err("should fail: RHS is not a call");
    assert!(
        err.diagnostic.message.contains("|>"),
        "error message should mention `|>`, got: {}",
        err.diagnostic.message
    );
}

// ---------------------------------------------------------------------------
// `(x |> f()) * 2` → `Call(f, [x]) * 2`  (parens disambiguate)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_parens_then_multiply() {
    let e = parse("(x |> f()) * 2");
    let call_part = call("f", vec![ident("x")]);
    let expected = Expr::BinaryOp {
        op: buff_lang_ast::BinaryOp::Mul,
        lhs: Box::new(call_part),
        rhs: Box::new(int(2)),
        span: span(),
    };
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`(x |> f()) * 2` should parse as `Call(f, [x]) * 2`"
    );
}

// ---------------------------------------------------------------------------
// Error: `x |> 5` — RHS is not a call → ParseError (not a panic).
// ---------------------------------------------------------------------------

#[test]
fn pipeline_rhs_not_a_call_errors() {
    let tokens = tokenize("x |> 5", sid()).expect("lexer should succeed");
    let err = parse_expression(&tokens, sid()).expect_err("should fail: RHS is not a call");
    assert!(
        err.diagnostic.message.contains("|>"),
        "error message should mention `|>`, got: {}",
        err.diagnostic.message
    );
}

// ---------------------------------------------------------------------------
// Error: `x |> a + b` — RHS is not a call (it's a binary op) → ParseError.
// ---------------------------------------------------------------------------

#[test]
fn pipeline_rhs_binary_op_errors() {
    let tokens = tokenize("x |> a + b", sid()).expect("lexer should succeed");
    assert!(
        parse_expression(&tokens, sid()).is_err(),
        "RHS of `|>` being a binary op (not a call) should error"
    );
}
