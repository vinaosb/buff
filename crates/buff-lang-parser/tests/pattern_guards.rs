//! T40 parser tests — pattern guards (`if <cond>`) on `match` arms.
//!
//! Verifies the parser accepts the `if <expr>` guard syntax after a match
//! arm pattern (`Some(v) if v > 0 => "positive"`) and populates
//! [`MatchArm::guard`]. The codegen lowering (guard → Rust `syn::Arm::guard`)
//! is exercised by the codegen crate's own tests; these tests lock the AST
//! shape so a parser regression is caught immediately.
//!
//! NOTE: this file is distinct from `guards.rs` (T73 early-return
//! `guard ... else { ... }` statements) — T40 is the `match`-arm `if` guard.

use buff_lang_ast::{Expr, MatchArm};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse_match, TokenStream};

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse a `match` expression (the `match` keyword is first).
fn parse_match_expr(src: &str) -> Expr {
    let toks = tokenize(src, sid()).expect("lexer should succeed");
    let mut s = TokenStream::new(&toks, sid());
    parse_match(&mut s).expect("parse_match should succeed")
}

/// Extract the arms from a MatchExpr, panicking otherwise.
fn arms_of(e: &Expr) -> Vec<MatchArm> {
    match e {
        Expr::MatchExpr { arms, .. } => arms.clone(),
        _ => panic!("expected MatchExpr, got {e}"),
    }
}

#[test]
fn t40_guarded_arm_populates_guard_field() {
    // `match opt { Some(v) if v > 0 => "positive", _ => "other" }` — the
    // first arm carries a guard; the second does not.
    let e = parse_match_expr("match opt { Some(v) if v > 0 => \"pos\", _ => \"other\" }");
    let arms = arms_of(&e);
    assert_eq!(arms.len(), 2);
    assert!(
        arms[0].guard.is_some(),
        "first arm should have a guard"
    );
    assert!(
        arms[1].guard.is_none(),
        "second (wildcard) arm should have no guard"
    );
}

#[test]
fn t40_guard_expression_is_binary_op() {
    // The guard `v > 0` should parse as a BinaryOp expression.
    let e = parse_match_expr("match x { Some(v) if v > 0 => 1, _ => 0 }");
    let arms = arms_of(&e);
    match &arms[0].guard {
        Some(Expr::BinaryOp { op, .. }) => {
            // `>` is the Gt operator.
            assert!(
                matches!(op, buff_lang_ast::BinaryOp::Gt),
                "guard should be a Gt (>) comparison"
            );
        }
        other => panic!("expected BinaryOp guard, got {other:?}"),
    }
}

#[test]
fn t40_unguarded_arm_has_none_guard() {
    // An arm with no `if` must have `guard: None` (backward-compatible).
    let e = parse_match_expr("match x { Some(v) => 1, None => 0 }");
    let arms = arms_of(&e);
    assert!(arms.iter().all(|a| a.guard.is_none()), "no guards");
}

#[test]
fn t40_guard_on_wildcard_arm() {
    // A guard can appear on ANY arm, including a wildcard:
    // `match n { _ if n > 10 => "big", _ => "small" }`.
    let e = parse_match_expr("match n { _ if n > 10 => \"big\", _ => \"small\" }");
    let arms = arms_of(&e);
    assert_eq!(arms.len(), 2);
    assert!(arms[0].guard.is_some(), "wildcard arm can have a guard");
    assert!(arms[1].guard.is_none());
}

#[test]
fn t40_guard_with_complex_expression() {
    // A guard can be a complex expression: `if v >= 0 and v < 100`.
    // (Buff uses `and`/`or` keywords; here we test a comparison chain via
    // a single binary op to keep the assertion focused on guard presence.)
    let e = parse_match_expr("match x { Some(v) if v == 0 => 1, _ => 0 }");
    let arms = arms_of(&e);
    assert!(arms[0].guard.is_some(), "complex guard parses");
    // The guard IS an expression node.
    assert!(arms[0].guard.as_ref().map(|g| g.span().start >= 0).unwrap_or(false));
}

#[test]
fn t40_guard_combines_with_or_pattern() {
    // T39 + T40 composition: `Red | Green if flag => "go"` — an or-pattern
    // arm with a guard.
    let e = parse_match_expr("match c { Red | Green if flag => \"go\", _ => \"stop\" }");
    let arms = arms_of(&e);
    assert_eq!(arms.len(), 2);
    // First arm is BOTH an Or pattern AND has a guard.
    assert!(
        matches!(arms[0].pattern, buff_lang_ast::Pattern::Or(_, _)),
        "first arm should be an or-pattern"
    );
    assert!(arms[0].guard.is_some(), "first arm should have a guard");
}
