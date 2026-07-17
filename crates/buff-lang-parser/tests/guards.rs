//! T73 integration tests — early-return guards (`guard ... else { ... }`).
//!
//! Coverage:
//!
//! - `guard x > 0 else { return 0 }` → `Stmt::Guard` with one `Bool` condition.
//! - `guard let Some(x) = opt else { return }` → `Stmt::Guard` with one `Let`
//!   condition (pattern reuses the shared `parse_pattern`).
//! - `guard let Some(x) = opt, x > 0 else { return }` → two conditions, in
//!   source order (let first, bool second).
//! - Malformed shapes error (no `else`, missing condition).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser guards
//! ```

#![allow(clippy::approx_constant)]

use buff_lang_ast::{Expr, GuardCondition, Pattern, Stmt};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse_statement, TokenStream};

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse a single statement. Panics on lexer or parser failure.
fn parse_stmt(src: &str) -> Stmt {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_statement(&mut stream).expect("parser should succeed")
}

/// Tokenize + parse a single statement, expecting a ParseError.
fn parse_stmt_err(src: &str) -> buff_lang_error::ParseError {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_statement(&mut stream).expect_err("parser should error")
}

/// Convenience: assert the statement is a `Guard` and hand back its
/// (`conditions`, `else_block`).
fn as_guard(stmt: &Stmt) -> (&[GuardCondition], &buff_lang_ast::Block) {
    match stmt {
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => (conditions, else_block),
        other => panic!("expected Stmt::Guard, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Single bool condition.
// ---------------------------------------------------------------------------

#[test]
fn guards_bool_condition() {
    // `guard x > 0 else { return 0 }`
    let stmt = parse_stmt("guard x > 0 else { return 0 }");
    let (conds, else_block) = as_guard(&stmt);
    assert_eq!(conds.len(), 1, "one condition: {conds:?}");
    assert!(
        matches!(conds[0], GuardCondition::Bool(_)),
        "first cond should be Bool: {:?}",
        conds[0]
    );
    // The else-block contains a single return statement.
    assert_eq!(
        else_block.stmts.len(),
        1,
        "else block should have 1 stmt: {:?}",
        else_block.stmts
    );
    assert!(
        matches!(else_block.stmts[0], Stmt::Return(Some(_), _)),
        "else[0] should be Return(Some): {:?}",
        else_block.stmts[0]
    );
}

// ---------------------------------------------------------------------------
// Single let-binding condition.
// ---------------------------------------------------------------------------

#[test]
fn guards_let_binding() {
    // `guard let Some(x) = opt else { return }`
    let stmt = parse_stmt("guard let Some(x) = opt else { return }");
    let (conds, else_block) = as_guard(&stmt);
    assert_eq!(conds.len(), 1, "one condition: {conds:?}");
    match &conds[0] {
        GuardCondition::Let { pattern, value, .. } => {
            // The pattern is Some(x).
            assert!(
                matches!(pattern, Pattern::Variant { variant, subpatterns, .. } if variant.name == "Some" && subpatterns.len() == 1),
                "Some(x) pattern: {pattern:?}"
            );
            // The value is the bare ident `opt`.
            assert!(
                matches!(value, Expr::Ident(id, _) if id.name == "opt"),
                "value = {value:?}"
            );
        }
        other => panic!("expected GuardCondition::Let, got {other:?}"),
    }
    // else-block: bare `return`.
    assert_eq!(else_block.stmts.len(), 1);
    assert!(
        matches!(else_block.stmts[0], Stmt::Return(None, _)),
        "else[0] should be Return(None): {:?}",
        else_block.stmts[0]
    );
}

// ---------------------------------------------------------------------------
// Multiple conditions (let + bool, in source order).
// ---------------------------------------------------------------------------

#[test]
fn guards_multiple_conditions() {
    // `guard let Some(x) = opt, x > 0 else { return }`
    let stmt = parse_stmt("guard let Some(x) = opt, x > 0 else { return }");
    let (conds, _) = as_guard(&stmt);
    assert_eq!(conds.len(), 2, "two conditions: {conds:?}");
    // First is the let-binding.
    assert!(
        matches!(&conds[0], GuardCondition::Let { pattern, .. } if matches!(pattern, Pattern::Variant { variant, .. } if variant.name == "Some")),
        "cond[0] should be Let(Some): {:?}",
        conds[0]
    );
    // Second is the bool condition.
    assert!(
        matches!(&conds[1], GuardCondition::Bool(_)),
        "cond[1] should be Bool: {:?}",
        conds[1]
    );
}

// ---------------------------------------------------------------------------
// Multiple bool conditions (comma-separated).
// ---------------------------------------------------------------------------

#[test]
fn guards_multiple_bool_conditions() {
    // `guard x > 0, y > 0 else { return }`
    let stmt = parse_stmt("guard x > 0, y > 0 else { return }");
    let (conds, _) = as_guard(&stmt);
    assert_eq!(conds.len(), 2, "two conditions: {conds:?}");
    assert!(matches!(conds[0], GuardCondition::Bool(_)));
    assert!(matches!(conds[1], GuardCondition::Bool(_)));
}

// ---------------------------------------------------------------------------
// Error cases.
// ---------------------------------------------------------------------------

#[test]
fn guards_missing_else_errors() {
    // `guard x > 0` (no else) → error.
    let err = parse_stmt_err("guard x > 0");
    assert!(
        err.diagnostic.message.contains("else"),
        "error should mention `else`: {}",
        err.diagnostic.message
    );
}

#[test]
fn guards_missing_condition_errors() {
    // `guard else { return }` (no condition) → error.
    let err = parse_stmt_err("guard else { return }");
    // Should be a parse error (any kind).
    assert!(
        !err.diagnostic.message.is_empty(),
        "should produce a non-empty error message"
    );
}

// ---------------------------------------------------------------------------
// Layout (indentation-based) else-block also works.
// ---------------------------------------------------------------------------

#[test]
fn guards_layout_else_block() {
    // guard x > 0 else:
    //     return 0
    let src = "guard x > 0 else:\n    return 0";
    let stmt = parse_stmt(src);
    let (conds, else_block) = as_guard(&stmt);
    assert_eq!(conds.len(), 1);
    assert!(matches!(conds[0], GuardCondition::Bool(_)));
    assert_eq!(else_block.stmts.len(), 1);
    assert!(
        matches!(else_block.stmts[0], Stmt::Return(Some(_), _)),
        "else[0] should be Return(Some): {:?}",
        else_block.stmts[0]
    );
}
