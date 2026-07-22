//! Integration tests for the T57 mathematical syntax edition
//! (`edition = "scientific"`).
//!
//! These tests exercise every T57 feature against BOTH editions:
//! - The scientific edition ACCEPTS the new syntax (returns the expected
//!   desugared AST).
//! - The default standard edition REJECTS the new syntax (returns a
//!   `ParseError`) — except for the four pure-alias Unicode comparison
//!   operators (`≤ ≥ ≠ →`) which are spelling variants of existing tokens
//!   and therefore work in BOTH editions.
//!
//! The tests assert the AST shape via [`Expr::Display`] (the same approach
//! used by `pipeline.rs` and `null_conditional.rs`), not via raw struct
//! equality — span positions vary across the two editions but the
//! structural shape is stable.

use buff_lang_ast::{Expr, Ident, Literal};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse_expression_with_edition, Edition};

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

fn mul(lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op: buff_lang_ast::BinaryOp::Mul,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: span(),
    }
}

fn add(lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op: buff_lang_ast::BinaryOp::Add,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: span(),
    }
}

fn call(callee: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident(callee)),
        args,
        span: span(),
    }
}

fn method_call(receiver: Expr, method: &str) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: Ident::new(method, span()),
        args: Vec::new(),
        span: span(),
    }
}

fn parse_sci(src: &str) -> Expr {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    parse_expression_with_edition(&tokens, sid(), Edition::Scientific)
        .expect("scientific-edition parser should succeed")
}

fn parse_std(src: &str) -> Result<Expr, buff_lang_error::ParseError> {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    parse_expression_with_edition(&tokens, sid(), Edition::Standard)
}

fn shape(e: &Expr) -> String {
    e.to_string()
}

// ===========================================================================
// §1 — Implicit multiplication.
// ===========================================================================

#[test]
fn implicit_mult_number_times_ident() {
    let e = parse_sci("2x");
    assert_eq!(
        shape(&e),
        shape(&mul(int(2), ident("x"))),
        "`2x` should desugar to `2 * x` in scientific edition"
    );
}

#[test]
fn implicit_mult_number_times_paren() {
    let e = parse_sci("2(x + 1)");
    let inner = add(ident("x"), int(1));
    assert_eq!(
        shape(&e),
        shape(&mul(int(2), inner)),
        "`2(x + 1)` should desugar to `2 * (x + 1)`"
    );
}

#[test]
fn implicit_mult_number_times_call() {
    let e = parse_sci("3sin(x)");
    let expected = mul(int(3), call("sin", vec![ident("x")]));
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`3sin(x)` should desugar to `3 * sin(x)`"
    );
}

#[test]
fn implicit_mult_does_not_apply_in_standard_edition() {
    // In the standard edition `2x` is two tokens with no operator between
    // them — the parser must reject it as "extra tokens after expression"
    // (the `2` parses as a complete expression; the trailing `x` is
    // unexpected). This proves the implicit-mult extension is opt-in.
    let result = parse_std("2x");
    assert!(
        result.is_err(),
        "default edition must reject `2x` as implicit multiplication"
    );
}

#[test]
fn implicit_mult_is_high_precedence() {
    // `2x + 1` should parse as `(2 * x) + 1`, NOT `2 * (x + 1)`. This
    // matches Julia's precedence (juxtaposition binds tighter than `+`).
    let e = parse_sci("2x + 1");
    let expected = add(mul(int(2), ident("x")), int(1));
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`2x + 1` should parse as `(2 * x) + 1`"
    );
}

// ===========================================================================
// §2 — Unicode comparison aliases (work in BOTH editions — pure spelling).
// ===========================================================================

#[test]
fn unicode_le_alias_works_in_standard_edition() {
    // `≤` is a pure alias for `<=` — must work in the default edition too.
    let e = parse_std("1 ≤ 2").expect("≤ must parse in standard edition");
    let expected = Expr::BinaryOp {
        op: buff_lang_ast::BinaryOp::Lte,
        lhs: Box::new(int(1)),
        rhs: Box::new(int(2)),
        span: span(),
    };
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn unicode_neq_alias_works_in_standard_edition() {
    let e = parse_std("1 ≠ 2").expect("≠ must parse in standard edition");
    let expected = Expr::BinaryOp {
        op: buff_lang_ast::BinaryOp::Neq,
        lhs: Box::new(int(1)),
        rhs: Box::new(int(2)),
        span: span(),
    };
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn unicode_ge_alias_works_in_standard_edition() {
    let e = parse_std("2 ≥ 1").expect("≥ must parse in standard edition");
    let expected = Expr::BinaryOp {
        op: buff_lang_ast::BinaryOp::Gte,
        lhs: Box::new(int(2)),
        rhs: Box::new(int(1)),
        span: span(),
    };
    assert_eq!(shape(&e), shape(&expected));
}

// ===========================================================================
// §3 — Unicode prefix operators (∑ ∏ √).
// ===========================================================================

#[test]
fn unicode_sqrt_desugars_to_function_call() {
    let e = parse_sci("√x");
    let expected = call("sqrt", vec![ident("x")]);
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`√x` should desugar to `sqrt(x)`"
    );
}

#[test]
fn unicode_sum_desugars_to_function_call() {
    let e = parse_sci("∑xs");
    let expected = call("sum", vec![ident("xs")]);
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`∑xs` should desugar to `sum(xs)`"
    );
}

#[test]
fn unicode_product_desugars_to_function_call() {
    let e = parse_sci("∏xs");
    let expected = call("product", vec![ident("xs")]);
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`∏xs` should desugar to `product(xs)`"
    );
}

#[test]
fn unicode_sqrt_high_precedence_vs_add() {
    // `√x + 1` should parse as `(sqrt(x)) + 1`.
    let e = parse_sci("√x + 1");
    let expected = add(call("sqrt", vec![ident("x")]), int(1));
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`√x + 1` should parse as `(sqrt(x)) + 1`"
    );
}

#[test]
fn unicode_sqrt_rejected_in_standard_edition() {
    let result = parse_std("√x");
    assert!(
        result.is_err(),
        "standard edition must reject `√x` (requires scientific edition)"
    );
}

// ===========================================================================
// §4 — Adjoint postfix `A'`.
// ===========================================================================

#[test]
fn adjoint_postfix_desugars_to_transpose() {
    let e = parse_sci("A'");
    let expected = method_call(ident("A"), "transpose");
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`A'` should desugar to `A.transpose()`"
    );
}

#[test]
fn adjoint_rejected_in_standard_edition() {
    // The lexer emits `Adjoint` only after an expression-ending token, so
    // `A'` lexes the same in both editions. In the standard edition the
    // parser must reject the Adjoint token with the edition opt-in message.
    let result = parse_std("A'");
    assert!(
        result.is_err(),
        "standard edition must reject `A'` (requires scientific edition)"
    );
}

#[test]
fn adjoint_after_parenthesised_expression() {
    let e = parse_sci("(A)'");
    // The parenthesised expression returns the inner `A` Ident directly
    // (Buff collapses `( x )` to `x`), so the adjoint fires on `A`.
    let expected = method_call(ident("A"), "transpose");
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`(A)'` should desugar to `A.transpose()`"
    );
}

// ===========================================================================
// §5 — Matrix literals.
// ===========================================================================

#[test]
fn matrix_row_vector() {
    // `[1 2 3]` (whitespace-separated) → nested ArrayLit
    // (`[[1, 2, 3]]` — one row of three elements).
    let e = parse_sci("[1 2 3]");
    let inner = Expr::ArrayLit {
        elements: vec![int(1), int(2), int(3)],
        span: span(),
    };
    let expected = Expr::ArrayLit {
        elements: vec![inner],
        span: span(),
    };
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`[1 2 3]` should be a 1x3 row matrix"
    );
}

#[test]
fn matrix_column_vector() {
    // `[1; 2; 3]` → nested ArrayLit with three single-element rows.
    let e = parse_sci("[1; 2; 3]");
    let row = |n| Expr::ArrayLit {
        elements: vec![int(n)],
        span: span(),
    };
    let expected = Expr::ArrayLit {
        elements: vec![row(1), row(2), row(3)],
        span: span(),
    };
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`[1; 2; 3]` should be a 3x1 column matrix"
    );
}

#[test]
fn matrix_2x2() {
    // `[1 2; 3 4]` → 2x2 nested ArrayLit.
    let e = parse_sci("[1 2; 3 4]");
    let row = |a, b| Expr::ArrayLit {
        elements: vec![int(a), int(b)],
        span: span(),
    };
    let expected = Expr::ArrayLit {
        elements: vec![row(1, 2), row(3, 4)],
        span: span(),
    };
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`[1 2; 3 4]` should be a 2x2 matrix"
    );
}

#[test]
fn matrix_3x3() {
    // `[1 2 3; 4 5 6; 7 8 9]` → 3x3 nested ArrayLit.
    let e = parse_sci("[1 2 3; 4 5 6; 7 8 9]");
    let row = |a, b, c| Expr::ArrayLit {
        elements: vec![int(a), int(b), int(c)],
        span: span(),
    };
    let expected = Expr::ArrayLit {
        elements: vec![row(1, 2, 3), row(4, 5, 6), row(7, 8, 9)],
        span: span(),
    };
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`[1 2 3; 4 5 6; 7 8 9]` should be a 3x3 matrix"
    );
}

#[test]
fn comma_separated_literal_still_works_in_scientific_edition() {
    // `[1, 2, 3]` (commas only) must remain a FLAT ArrayLit in BOTH
    // editions — this is the backward-compatibility invariant.
    let e = parse_sci("[1, 2, 3]");
    let expected = Expr::ArrayLit {
        elements: vec![int(1), int(2), int(3)],
        span: span(),
    };
    assert_eq!(
        shape(&e),
        shape(&expected),
        "`[1, 2, 3]` (commas) should remain a flat ArrayLit"
    );
}

#[test]
fn matrix_literal_rejected_in_standard_edition() {
    // `[1 2 3]` (whitespace-separated) is a parse error in the standard
    // edition — it's neither a valid comma-separated Vector nor a valid
    // anything else.
    let result = parse_std("[1 2 3]");
    assert!(
        result.is_err(),
        "standard edition must reject `[1 2 3]` (matrix literals are opt-in)"
    );
}

// ===========================================================================
// §6 — Default-edition regression: existing programs parse unchanged.
// ===========================================================================

#[test]
fn default_edition_existing_program_unchanged() {
    // A standard comma-separated Vector literal still parses as a flat
    // ArrayLit in the default edition — implicit mult and matrix syntax
    // did not change this.
    let e = parse_std("[1, 2, 3]").expect("standard Vector literal must still parse");
    let expected = Expr::ArrayLit {
        elements: vec![int(1), int(2), int(3)],
        span: span(),
    };
    assert_eq!(shape(&e), shape(&expected));
}

#[test]
fn default_edition_arithmetic_unchanged() {
    // `1 + 2 * 3` is unaffected by editions — both must agree.
    let std_e = parse_std("1 + 2 * 3").expect("standard arithmetic must parse");
    let sci_e = parse_sci("1 + 2 * 3");
    assert_eq!(
        shape(&std_e),
        shape(&sci_e),
        "default and scientific editions must agree on plain arithmetic"
    );
}
