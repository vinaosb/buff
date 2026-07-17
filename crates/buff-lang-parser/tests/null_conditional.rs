//! Integration tests for the null-conditional `?.` operator parsing (T70).
//!
//! The null-conditional operator desugars IN THE PARSER to a
//! [`Expr::MethodCall`]: `receiver.and_then(|x| x.field)`. Chaining is
//! left-associative, so each `?.` in `a?.b?.c` nests one more `.and_then`.
//!
//! These tests assert the desugared AST shape directly via `Expr::Display`:
//!
//! - `u?.name`        → `MethodCall(u.and_then, [Lambda(fn(x: _) { ExprStmt(MethodCall(x.name, [])) })])`
//! - `a?.b?.c`        → outer `MethodCall(inner.and_then, [Lambda(...x.c...)])`
//!   where `inner = MethodCall(a.and_then, [Lambda(...x.b...)])`
//! - `u?.m(42)`       → `MethodCall(u.and_then, [Lambda(fn(x: _) { ExprStmt(MethodCall(x.m, [42])) })])`
//!
//! The `?.` operator MUST NOT break the single `?` (Try/T30) or the plain
//! `.` member access (MethodCall with zero args) — covered by regression
//! tests at the bottom.

use buff_lang_ast::{Expr, Literal};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse_expression;

fn sid() -> SourceId {
    SourceId(0)
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
// `u?.name` desugars to `u.and_then(|x| x.name)`.
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_simple_field() {
    let e = parse("u?.name");
    // The desugar produces a MethodCall whose method is `and_then`.
    assert!(
        matches!(&e, Expr::MethodCall { method, .. } if method.name == "and_then"),
        "`u?.name` should desugar to a MethodCall named `and_then`, got: {}",
        shape(&e)
    );
    // Its single arg is a Lambda whose body is `ExprStmt(x.name)`.
    if let Expr::MethodCall { receiver, args, .. } = &e {
        assert!(
            matches!(receiver.as_ref(), Expr::Ident(id, _) if id.name == "u"),
            "receiver should be Ident(u), got: {receiver}"
        );
        assert_eq!(
            args.len(),
            1,
            "and_then should have exactly 1 arg (the closure)"
        );
        assert!(
            matches!(args[0], Expr::Lambda { .. }),
            "arg should be a Lambda, got: {}",
            shape(&args[0])
        );
    } else {
        panic!("expected MethodCall, got: {}", shape(&e));
    }
}

// ---------------------------------------------------------------------------
// `opt?.value` desugars to `opt.and_then(|x| x.value)`.
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_opt_value() {
    let e = parse("opt?.value");
    assert!(
        matches!(&e, Expr::MethodCall { method, .. } if method.name == "and_then"),
        "`opt?.value` should desugar to a MethodCall named `and_then`, got: {}",
        shape(&e)
    );
}

// ---------------------------------------------------------------------------
// Chained: `a?.b?.c` desugars to `a.and_then(|x| x.b).and_then(|x| x.c)`.
// The outer MethodCall's receiver is the inner `a.and_then(...)` MethodCall.
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_chained() {
    let e = parse("a?.b?.c");
    // Outer must be `_.and_then(...)`.
    assert!(
        matches!(&e, Expr::MethodCall { method, .. } if method.name == "and_then"),
        "chained `a?.b?.c` outer node should be `and_then`, got: {}",
        shape(&e)
    );
    if let Expr::MethodCall { receiver, .. } = &e {
        // Inner receiver must ALSO be `and_then` (the `a?.b` part).
        assert!(
            matches!(receiver.as_ref(), Expr::MethodCall { method, .. } if method.name == "and_then"),
            "chained `a?.b?.c` inner receiver should be `and_then`, got: {receiver}"
        );
        // The innermost receiver should be Ident("a").
        if let Expr::MethodCall {
            receiver: innermost,
            ..
        } = receiver.as_ref()
        {
            assert!(
                matches!(innermost.as_ref(), Expr::Ident(id, _) if id.name == "a"),
                "innermost receiver should be Ident(a), got: {innermost}"
            );
        }
    } else {
        panic!("expected outer MethodCall, got: {}", shape(&e));
    }
}

// ---------------------------------------------------------------------------
// Method-call form: `u?.m(42)` desugars to `u.and_then(|x| x.m(42))`.
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_method_call() {
    let e = parse("u?.m(42)");
    assert!(
        matches!(&e, Expr::MethodCall { method, .. } if method.name == "and_then"),
        "`u?.m(42)` should desugar to a MethodCall named `and_then`, got: {}",
        shape(&e)
    );
    if let Expr::MethodCall { args, .. } = &e {
        assert_eq!(
            args.len(),
            1,
            "and_then should have exactly 1 arg (the closure)"
        );
        if let Expr::Lambda { body, .. } = &args[0] {
            // Body's single ExprStmt should be `x.m(42)` — a MethodCall on
            // Ident("x") with method "m" and args [42].
            assert_eq!(body.stmts.len(), 1, "lambda body should have 1 stmt");
            match &body.stmts[0] {
                buff_lang_ast::Stmt::ExprStmt(inner, _) => match inner {
                    Expr::MethodCall {
                        receiver,
                        method,
                        args,
                        ..
                    } => {
                        assert!(
                            matches!(receiver.as_ref(), Expr::Ident(id, _) if id.name == "x"),
                            "lambda body receiver should be Ident(x), got: {receiver}"
                        );
                        assert_eq!(method.name, "m", "lambda body method should be `m`");
                        assert_eq!(args.len(), 1, "lambda body method should have 1 arg");
                    }
                    other => panic!("expected MethodCall inside lambda body ExprStmt, got {other}"),
                },
                other => panic!("expected ExprStmt in lambda body, got {other:?}"),
            }
        } else {
            panic!("arg should be a Lambda, got: {}", shape(&args[0]));
        }
    }
}

// ---------------------------------------------------------------------------
// Precedence: `?.` is a postfix operator, binding tighter than `+`.
// `a?.b + 1` should parse as `(a?.b) + 1`.
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_precedence_tighter_than_additive() {
    let e = parse("a?.b + 1");
    assert!(
        matches!(&e, Expr::BinaryOp { op: buff_lang_ast::BinaryOp::Add, lhs, rhs, .. }
        if matches!(lhs.as_ref(), Expr::MethodCall { method, .. } if method.name == "and_then")
        && matches!(rhs.as_ref(), Expr::Literal(Literal::Int(1), _))),
        "`a?.b + 1` should parse as `BinaryOp(Add, MethodCall(and_then), 1)`, got: {}",
        shape(&e)
    );
}

// ---------------------------------------------------------------------------
// Regression: single `?` (Try, T30) MUST still work after adding `?.`.
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_does_not_break_single_question() {
    let e = parse("x?");
    assert!(
        matches!(e, Expr::Try { .. }),
        "`x?` (single `?`) should still parse as Try after adding `?.`, got: {}",
        shape(&e)
    );
}

// ---------------------------------------------------------------------------
// Regression: `x?.y` — the lexer must NOT split `?.` into `?` + `.`.
// (If it did, `x?` would be Try and `.y` would be a stray `.y` postfix —
// the parser would reject it. This test asserts we get the `?.` desugar.)
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_lexer_does_not_split() {
    let e = parse("x?.y");
    // If `?.` were split, we'd see Try followed by stray tokens — a parse
    // error. We must get a clean MethodCall(and_then).
    let is_and_then = matches!(&e, Expr::MethodCall { method, .. } if method.name == "and_then");
    assert!(
        is_and_then,
        "`x?.y` must lex as a single `?.` token and parse to MethodCall(and_then), got: {}",
        shape(&e)
    );
}

// ---------------------------------------------------------------------------
// Regression: plain `.` member access MUST still produce a zero-arg
// MethodCall (not the `?.` desugar).
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_does_not_break_plain_dot() {
    let e = parse("x.y");
    assert!(
        matches!(&e, Expr::MethodCall { method, args, .. }
        if method.name == "y" && args.is_empty()),
        "`x.y` (plain dot) should still parse as zero-arg MethodCall(y), got: {}",
        shape(&e)
    );
    // And it must NOT be `and_then` (the `?.` desugar marker).
    if let Expr::MethodCall { method, .. } = &e {
        assert_ne!(
            method.name, "and_then",
            "`x.y` must not be the `?.` desugar (method should be `y`)"
        );
    }
}

// ---------------------------------------------------------------------------
// Error: `?.` not preceded by an expression (i.e. at the start) errors
// cleanly — the primary parser rejects a leading `?.` because there's no
// receiver. (This is a sanity test, not a primary contract.)
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_leading_question_dot_errors() {
    let tokens = tokenize("?.name", sid()).expect("lexer should succeed");
    let result = parse_expression(&tokens, sid());
    assert!(
        result.is_err(),
        "a leading `?.` with no receiver should be a parse error, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Error: `?.` followed by a non-identifier (e.g. `x ?. 5`) errors cleanly.
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_followed_by_non_ident_errors() {
    let tokens = tokenize("x ?. 5", sid()).expect("lexer should succeed");
    let result = parse_expression(&tokens, sid());
    assert!(
        result.is_err(),
        "`x ?. 5` (?. followed by a non-ident) should be a parse error, got: {result:?}"
    );
}
