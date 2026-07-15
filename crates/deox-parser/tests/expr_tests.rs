//! Integration tests for the deox-parser expression parser (T7).
//!
//! These tests exercise the parser end-to-end by feeding source strings
//! through the T6 lexer and then through [`deox_parser::parse_expression`].
//! They cover:
//!
//! - All literal kinds (int / float / double / bool / byte / string)
//! - Identifiers
//! - Binary operators with precedence and associativity
//! - Unary prefix operators
//! - Function calls (no-arg, multi-arg, nested)
//! - Method calls (single and chained)
//! - Parenthesized expressions
//!
//! String interpolation is intentionally unsupported in T7 and gets a
//! dedicated error test.

#![allow(clippy::approx_constant)]

use deox_ast::{BinaryOp, Expr, Ident, Literal, UnaryOp};
use deox_error::SourceId;
use deox_lexer::tokenize;
use deox_parser::parse_expression;

// ---------------------------------------------------------------------------
// Helpers for building expected AST nodes (with dummy spans).
// ---------------------------------------------------------------------------

fn sid() -> SourceId {
    SourceId(0)
}

fn int(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), deox_error::Span::dummy())
}

fn ident(name: &str) -> Expr {
    Expr::Ident(
        Ident::new(name, deox_error::Span::dummy()),
        deox_error::Span::dummy(),
    )
}

fn binop(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: deox_error::Span::dummy(),
    }
}

fn unop(op: UnaryOp, operand: Expr) -> Expr {
    Expr::UnaryOp {
        op,
        operand: Box::new(operand),
        span: deox_error::Span::dummy(),
    }
}

fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(callee),
        args,
        span: deox_error::Span::dummy(),
    }
}

fn mcall(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: Ident::new(method, deox_error::Span::dummy()),
        args,
        span: deox_error::Span::dummy(),
    }
}

/// Tokenize + parse a single expression from `src`. Panics on lexer or
/// parser failure so tests stay terse.
fn parse(src: &str) -> Expr {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    parse_expression(&tokens, sid()).expect("parser should succeed")
}

/// Like [`parse`] but asserts the parser produces an error.
fn parse_err(src: &str) -> deox_error::ParseError {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    parse_expression(&tokens, sid()).expect_err("parser should fail")
}

/// Strip span information so two `Expr`s can be compared structurally.
/// We do this by re-parsing both via Display, which already discards spans.
fn shape(e: &Expr) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// 1. Literal tests
// ---------------------------------------------------------------------------

#[test]
fn test_int_literal() {
    let e = parse("42");
    assert_eq!(shape(&e), "Lit(Int(42))");
    assert!(matches!(e, Expr::Literal(Literal::Int(42), _)));
}

#[test]
fn test_negative_int_literal() {
    // `-42` parses as UnaryOp(Neg, 42) — there are no negative literals.
    let e = parse("-42");
    assert_eq!(shape(&e), "UnaryOp(-, Lit(Int(42)))");
}

#[test]
fn test_float_literal() {
    let e = parse("2.5");
    assert_eq!(shape(&e), "Lit(Float(2.5))");
}

#[test]
fn test_double_literal() {
    let e = parse("99.9d");
    assert_eq!(shape(&e), "Lit(Double(99.9))");
}

#[test]
fn test_bool_literals() {
    assert_eq!(shape(&parse("true")), "Lit(Bool(true))");
    assert_eq!(shape(&parse("false")), "Lit(Bool(false))");
}

#[test]
fn test_byte_hex_literal() {
    let e = parse("0xFF");
    assert_eq!(shape(&e), "Lit(Byte(0xFF))");
}

#[test]
fn test_byte_binary_literal() {
    let e = parse("0b1010");
    // 0b1010 = 10 decimal = 0x0A
    assert_eq!(shape(&e), "Lit(Byte(0x0A))");
}

#[test]
fn test_string_simple() {
    let e = parse("\"hello\"");
    assert_eq!(shape(&e), "Lit(String(\"hello\"))");
}

#[test]
fn test_string_empty() {
    let e = parse("\"\"");
    assert_eq!(shape(&e), "Lit(String(\"\"))");
}

// ---------------------------------------------------------------------------
// 2. Identifier test
// ---------------------------------------------------------------------------

#[test]
fn test_identifier() {
    let e = parse("x");
    assert_eq!(shape(&e), "Ident(x)");
}

#[test]
fn test_identifier_multi_char() {
    let e = parse("foo_bar_baz");
    assert_eq!(shape(&e), "Ident(foo_bar_baz)");
}

// ---------------------------------------------------------------------------
// 3. Precedence and associativity
// ---------------------------------------------------------------------------

#[test]
fn test_precedence_mul_over_add() {
    // 2 + 3 * 4  ==>  Add(2, Mul(3, 4))
    let e = parse("2 + 3 * 4");
    assert_eq!(
        shape(&e),
        "BinaryOp(+, Lit(Int(2)), BinaryOp(*, Lit(Int(3)), Lit(Int(4))))"
    );
}

#[test]
fn test_precedence_parens_override() {
    // (2 + 3) * 4  ==>  Mul(Add(2,3), 4)
    let e = parse("(2 + 3) * 4");
    assert_eq!(
        shape(&e),
        "BinaryOp(*, BinaryOp(+, Lit(Int(2)), Lit(Int(3))), Lit(Int(4)))"
    );
}

#[test]
fn test_left_assoc_sub() {
    // 10 - 3 - 2  ==>  Sub(Sub(10, 3), 2)   [left-assoc]
    let e = parse("10 - 3 - 2");
    let expected = binop(BinaryOp::Sub, binop(BinaryOp::Sub, int(10), int(3)), int(2));
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_right_assoc_assignment() {
    // a = b = 7  ==>  Assign(a, Assign(b, 7))   [right-assoc]
    let e = parse("a = b = 7");
    let expected = binop(
        BinaryOp::Assign,
        ident("a"),
        binop(BinaryOp::Assign, ident("b"), int(7)),
    );
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_mixed_precedence_full() {
    // a + b * c == d && e < f
    // ==>  And( Eq( Add(a, Mul(b, c)), d ), Lt(e, f) )
    let e = parse("a + b * c == d && e < f");
    let expected = binop(
        BinaryOp::And,
        binop(
            BinaryOp::Eq,
            binop(
                BinaryOp::Add,
                ident("a"),
                binop(BinaryOp::Mul, ident("b"), ident("c")),
            ),
            ident("d"),
        ),
        binop(BinaryOp::Lt, ident("e"), ident("f")),
    );
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_or_lower_than_and() {
    // a || b && c  ==>  Or(a, And(b, c))
    let e = parse("a || b && c");
    let expected = binop(
        BinaryOp::Or,
        ident("a"),
        binop(BinaryOp::And, ident("b"), ident("c")),
    );
    assert_eq!(shape(&e), shape(&expected));
}

// ---------------------------------------------------------------------------
// 4. Comparison operators
// ---------------------------------------------------------------------------

#[test]
fn test_comparison_operators() {
    assert_eq!(shape(&parse("a < b")), "BinaryOp(<, Ident(a), Ident(b))");
    assert_eq!(shape(&parse("a > b")), "BinaryOp(>, Ident(a), Ident(b))");
    assert_eq!(shape(&parse("a == b")), "BinaryOp(==, Ident(a), Ident(b))");
    assert_eq!(shape(&parse("a != b")), "BinaryOp(!=, Ident(a), Ident(b))");
    assert_eq!(shape(&parse("a <= b")), "BinaryOp(<=, Ident(a), Ident(b))");
    assert_eq!(shape(&parse("a >= b")), "BinaryOp(>=, Ident(a), Ident(b))");
}

// ---------------------------------------------------------------------------
// 5. Unary operators
// ---------------------------------------------------------------------------

#[test]
fn test_unary_neg() {
    let e = parse("-x");
    let expected = unop(UnaryOp::Neg, ident("x"));
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_unary_not() {
    let e = parse("!flag");
    let expected = unop(UnaryOp::Not, ident("flag"));
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_unary_bitnot() {
    let e = parse("~mask");
    let expected = unop(UnaryOp::BitNot, ident("mask"));
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_unary_double_neg() {
    // - -x  ==>  Neg(Neg(x))
    let e = parse("- -x");
    let expected = unop(UnaryOp::Neg, unop(UnaryOp::Neg, ident("x")));
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_unary_binds_tighter_than_mul_in_postfix() {
    // -a * b  ==>  Mul(Neg(a), b)   — because unary > multiplicative in
    // the ladder: parse_mul calls parse_unary for each operand.
    let e = parse("-a * b");
    let expected = binop(BinaryOp::Mul, unop(UnaryOp::Neg, ident("a")), ident("b"));
    assert_eq!(shape(&e), shape(&expected));
}

// ---------------------------------------------------------------------------
// 6. Function calls
// ---------------------------------------------------------------------------

#[test]
fn test_function_call_no_args() {
    let e = parse("foo()");
    let expected = call(ident("foo"), vec![]);
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_function_call_one_arg() {
    let e = parse("foo(a)");
    let expected = call(ident("foo"), vec![ident("a")]);
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_function_call_multi_args() {
    let e = parse("foo(a, b, c)");
    let expected = call(ident("foo"), vec![ident("a"), ident("b"), ident("c")]);
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_function_call_trailing_comma() {
    let e = parse("foo(a, b,)");
    let expected = call(ident("foo"), vec![ident("a"), ident("b")]);
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_nested_function_call() {
    let e = parse("foo(bar())");
    let expected = call(ident("foo"), vec![call(ident("bar"), vec![])]);
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_function_call_with_complex_args() {
    // foo(1 + 2, bar(3))
    let e = parse("foo(1 + 2, bar(3))");
    let expected = call(
        ident("foo"),
        vec![
            binop(BinaryOp::Add, int(1), int(2)),
            call(ident("bar"), vec![int(3)]),
        ],
    );
    assert_eq!(shape(&e), shape(&expected));
}

// ---------------------------------------------------------------------------
// 7. Method calls
// ---------------------------------------------------------------------------

#[test]
fn test_method_call_no_args() {
    let e = parse("obj.method()");
    let expected = mcall(ident("obj"), "method", vec![]);
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_method_call_with_arg() {
    let e = parse("obj.method(x)");
    let expected = mcall(ident("obj"), "method", vec![ident("x")]);
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_chained_method_calls() {
    // obj.foo().bar()  ==>  MethodCall( MethodCall(obj, foo, []), bar, [] )
    let e = parse("obj.foo().bar()");
    let expected = mcall(mcall(ident("obj"), "foo", vec![]), "bar", vec![]);
    assert_eq!(shape(&e), shape(&expected));
}

// ---------------------------------------------------------------------------
// 8. Parenthesized expressions
// ---------------------------------------------------------------------------

#[test]
fn test_parenthesized_passthrough() {
    // (a)  ==>  just Ident(a)
    let e = parse("(a)");
    assert_eq!(shape(&e), "Ident(a)");
}

#[test]
fn test_parenthesized_deeply_nested() {
    // ((a))  ==>  just Ident(a)
    let e = parse("((a))");
    assert_eq!(shape(&e), "Ident(a)");
}

#[test]
fn test_parenthesized_in_expression() {
    // (a + b) * (c + d)
    let e = parse("(a + b) * (c + d)");
    let expected = binop(
        BinaryOp::Mul,
        binop(BinaryOp::Add, ident("a"), ident("b")),
        binop(BinaryOp::Add, ident("c"), ident("d")),
    );
    assert_eq!(shape(&e), shape(&expected));
}

// ---------------------------------------------------------------------------
// 9. Bitwise + shift
// ---------------------------------------------------------------------------

#[test]
fn test_bitwise_operators() {
    assert_eq!(shape(&parse("a | b")), "BinaryOp(|, Ident(a), Ident(b))");
    assert_eq!(shape(&parse("a & b")), "BinaryOp(&, Ident(a), Ident(b))");
    assert_eq!(shape(&parse("a ^ b")), "BinaryOp(^, Ident(a), Ident(b))");
}

#[test]
fn test_shift_operators() {
    assert_eq!(shape(&parse("a << b")), "BinaryOp(<<, Ident(a), Ident(b))");
    assert_eq!(shape(&parse("a >> b")), "BinaryOp(>>, Ident(a), Ident(b))");
}

#[test]
fn test_shift_lower_than_additive() {
    // 1 + 2 << 3  ==>  Shl(Add(1,2), 3)
    let e = parse("1 + 2 << 3");
    let expected = binop(BinaryOp::Shl, binop(BinaryOp::Add, int(1), int(2)), int(3));
    assert_eq!(shape(&e), shape(&expected));
}

// ---------------------------------------------------------------------------
// 10. Compound assignment
// ---------------------------------------------------------------------------

#[test]
fn test_compound_assignment() {
    let e = parse("x += 5");
    let expected = binop(BinaryOp::AddAssign, ident("x"), int(5));
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn test_all_compound_assignments() {
    for (src, op) in [
        ("x = 1", BinaryOp::Assign),
        ("x += 1", BinaryOp::AddAssign),
        ("x -= 1", BinaryOp::SubAssign),
        ("x *= 1", BinaryOp::MulAssign),
        ("x /= 1", BinaryOp::DivAssign),
        ("x %= 1", BinaryOp::ModAssign),
    ] {
        let e = parse(src);
        let expected = binop(op, ident("x"), int(1));
        assert_eq!(shape(&e), shape(&expected), "failed for src `{src}`");
    }
}

// ---------------------------------------------------------------------------
// 11. Error handling
// ---------------------------------------------------------------------------

#[test]
fn test_string_interpolation_unsupported() {
    let err = parse_err("\"hello {name}\"");
    assert!(
        err.diagnostic.message.contains("interpolation"),
        "got message: {}",
        err.diagnostic.message
    );
}

#[test]
fn test_unexpected_token_errors() {
    // `)` alone is not a valid expression.
    let err = parse_err(")");
    assert!(err.diagnostic.message.contains("expected"));
}

#[test]
fn test_leftover_tokens_error() {
    // `foo bar` parses `foo` then leaves `bar` — parse_expression should
    // reject it via the leftover-tokens check.
    let tokens = tokenize("foo bar", sid()).unwrap();
    let result = parse_expression(&tokens, sid());
    assert!(result.is_err(), "leftover tokens should error");
}

#[test]
fn test_empty_input_errors() {
    let tokens = tokenize("", sid()).unwrap();
    let result = parse_expression(&tokens, sid());
    assert!(result.is_err(), "empty input should error");
}

#[test]
fn test_unclosed_paren_errors() {
    let err = parse_err("(1 + 2");
    assert!(err.diagnostic.message.contains("expected"));
}

// ---------------------------------------------------------------------------
// 12. parse() entry point smoke test (T7 stub returns empty Vec)
// ---------------------------------------------------------------------------

#[test]
fn test_parse_entrypoint_returns_empty_for_t7() {
    // The T7 top-level parse() is a stub returning an empty Vec<Decl>.
    // T8 will implement real declaration parsing.
    let tokens = tokenize("1 + 2", sid()).unwrap();
    let decls = deox_parser::parse(&tokens, sid()).unwrap();
    assert!(decls.is_empty(), "T7 parse() should return empty Vec");
}
