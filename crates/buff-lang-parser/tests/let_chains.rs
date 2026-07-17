//! T74 integration tests — let-chains in `if` (comma-separated conditions).
//!
//! Coverage:
//!
//! - `if let Some(x) = a, let Some(y) = b { }` → nested IfLet → IfLet.
//! - `if let Some(x) = a, let Some(y) = b, x > 0 { }` → nested IfLet → IfLet → IfExpr.
//! - `if let Some(x) = a, x > 0 { } else { }` → else REPLICATED at both levels.
//! - QA case: `if let Some(a) = x, let Some(b) = y, a > b { }` → 3-level chain.
//! - Regression: single `if let Some(x) = opt { }` STILL → flat IfLet (no nesting).
//! - Regression: single `if cond { }` STILL → flat IfExpr (no nesting).
//! - Regression: single bool `if cond { } else { }` intact.
//! - Mixed bool-first: `if a > 0, let Some(b) = opt { }` → IfExpr → IfLet.
//! - Error path: missing block / missing condition.
//!
//! Each test feeds source strings through the lexer and then through
//! [`buff_lang_parser::parse_statement`]. The resulting AST is pattern-matched
//! to assert the expected NESTED shape (outer IfLet → inner IfLet → innermost
//! IfExpr), proving the parser desugars the comma-separated let-chain into
//! nested single-condition if-lets / ifs (T74).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser let_chains
//! ```

#![allow(clippy::approx_constant)]

use buff_lang_ast::{Expr, Pattern, Stmt};
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

/// Unwrap an `Expr::IfLet` from a statement (which wraps it in `ExprStmt`).
fn as_if_let(stmt: Stmt) -> Expr {
    match stmt {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("expected ExprStmt wrapping an if, got {other:?}"),
    }
}

/// Assert a `Pattern` is `Variant { variant: Some, subpatterns: [Ident(name)] }`.
fn assert_some_binding(pat: &Pattern, binding: &str) {
    let (variant, subs) = match pat {
        Pattern::Variant {
            variant,
            subpatterns,
            ..
        } => (variant, subpatterns),
        other => panic!("expected Pattern::Variant, got {other:?}"),
    };
    assert_eq!(variant.name, "Some", "variant name");
    assert_eq!(subs.len(), 1, "Some(_) has 1 subpattern");
    assert!(
        matches!(&subs[0], Pattern::Ident(id, _) if id.name == binding),
        "subpattern {:?} should bind {binding:?}",
        subs[0]
    );
}

// ---------------------------------------------------------------------------
// Two-let chains.
// ---------------------------------------------------------------------------

#[test]
fn let_chains_two_lets() {
    // `if let Some(x) = a, let Some(y) = b { print(x) }`
    // → outer IfLet(Some(x), a) whose then_block is a single ExprStmt wrapping
    //   inner IfLet(Some(y), b) whose then_block is the BODY.
    let stmt = parse_stmt("if let Some(x) = a, let Some(y) = b { print(x) }");
    let outer = as_if_let(stmt);
    let (outer_pat, outer_val, outer_then, outer_else) = match outer {
        Expr::IfLet {
            pattern,
            value,
            then_block,
            else_block,
            ..
        } => (pattern, value, then_block, else_block),
        other => panic!("outer should be IfLet, got {other:?}"),
    };
    assert_some_binding(&outer_pat, "x");
    assert!(
        matches!(outer_val.as_ref(), Expr::Ident(id, _) if id.name == "a"),
        "outer value = {outer_val:?}"
    );
    assert!(outer_else.is_none(), "no else in source → outer else None");
    // The outer then_block wraps the inner IfLet.
    assert_eq!(outer_then.stmts.len(), 1, "outer then wraps 1 stmt");
    let inner = match &outer_then.stmts[0] {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("outer then[0] should be ExprStmt, got {other:?}"),
    };
    let (inner_pat, inner_val, inner_then, inner_else) = match inner {
        Expr::IfLet {
            pattern,
            value,
            then_block,
            else_block,
            ..
        } => (pattern, value, then_block, else_block),
        other => panic!("inner should be IfLet, got {other:?}"),
    };
    assert_some_binding(inner_pat, "y");
    assert!(
        matches!(inner_val.as_ref(), Expr::Ident(id, _) if id.name == "b"),
        "inner value = {inner_val:?}"
    );
    assert!(inner_else.is_none(), "no else → inner else None");
    // The inner then_block is the BODY (print(x)).
    assert_eq!(inner_then.stmts.len(), 1, "inner then is the body");
}

// ---------------------------------------------------------------------------
// Let + bool conditions.
// ---------------------------------------------------------------------------

#[test]
fn let_chains_let_and_bool() {
    // `if let Some(x) = opt, x > 0 { print(x) }`
    // → outer IfLet(Some(x), opt) → inner IfExpr(x > 0) → body.
    let stmt = parse_stmt("if let Some(x) = opt, x > 0 { print(x) }");
    let outer = as_if_let(stmt);
    let (outer_pat, outer_then, ..) = match outer {
        Expr::IfLet {
            pattern,
            value: _,
            then_block,
            ..
        } => (pattern, then_block),
        other => panic!("outer should be IfLet, got {other:?}"),
    };
    assert_some_binding(&outer_pat, "x");
    let inner = match &outer_then.stmts[0] {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("outer then[0] should be ExprStmt, got {other:?}"),
    };
    // The inner should be IfExpr with a BinaryOp(x > 0) cond.
    match inner {
        Expr::IfExpr {
            cond, else_block, ..
        } => {
            assert!(
                matches!(cond.as_ref(), Expr::BinaryOp { .. }),
                "inner cond should be a BinaryOp (x > 0), got {cond:?}"
            );
            assert!(else_block.is_none(), "no else → inner else None");
        }
        other => panic!("inner should be IfExpr, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Else-block replication: a failing condition at ANY level must run the else.
// ---------------------------------------------------------------------------

#[test]
fn let_chains_else_replicated_at_every_level() {
    // `if let Some(x) = a, let Some(y) = b { body } else { fallback }`
    // → BOTH the outer AND the inner IfLet get a (cloned) copy of the else.
    let stmt =
        parse_stmt("if let Some(x) = a, let Some(y) = b { print(x) } else { print(\"fb\") }");
    let outer = as_if_let(stmt);
    let (outer_then, outer_else) = match outer {
        Expr::IfLet {
            then_block,
            else_block,
            ..
        } => (then_block, else_block),
        other => panic!("outer should be IfLet, got {other:?}"),
    };
    let outer_else = outer_else.expect("outer must have else");
    assert!(!outer_else.stmts.is_empty(), "outer else non-empty");
    let inner = match &outer_then.stmts[0] {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("outer then[0] should be ExprStmt, got {other:?}"),
    };
    // Inner must ALSO carry a cloned copy of the else (replicated at every level).
    assert!(
        matches!(
            inner,
            Expr::IfLet {
                else_block: Some(_),
                ..
            }
        ),
        "inner IfLet must have an else (replicated), got {inner:?}"
    );
}

// ---------------------------------------------------------------------------
// QA case (spec): 3-level chain, outermost IfLet → IfLet → innermost IfExpr.
// ---------------------------------------------------------------------------

#[test]
fn let_chains_qa_case() {
    // `if let Some(a) = x, let Some(b) = y, a > b { }`
    // → IfLet(Some(a), x) → then: IfLet(Some(b), y) → then: IfExpr(a > b) → body.
    let stmt = parse_stmt("if let Some(a) = x, let Some(b) = y, a > b { print(a) }");
    let outer = as_if_let(stmt);
    // Level 1: outer IfLet with Some(a).
    let (outer_pat, outer_then) = match &outer {
        Expr::IfLet {
            pattern,
            then_block,
            ..
        } => (pattern, then_block),
        other => panic!("outermost should be IfLet, got {other:?}"),
    };
    assert_some_binding(outer_pat, "a");
    // Level 2: middle IfLet with Some(b).
    let mid = match &outer_then.stmts[0] {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("outer then[0] should be ExprStmt, got {other:?}"),
    };
    let (mid_pat, mid_then) = match mid {
        Expr::IfLet {
            pattern,
            then_block,
            ..
        } => (pattern, then_block),
        other => panic!("middle should be IfLet, got {other:?}"),
    };
    assert_some_binding(mid_pat, "b");
    // Level 3: innermost IfExpr with a > b.
    let inner = match &mid_then.stmts[0] {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("middle then[0] should be ExprStmt, got {other:?}"),
    };
    match inner {
        Expr::IfExpr { cond, .. } => {
            assert!(
                matches!(cond.as_ref(), Expr::BinaryOp { .. }),
                "innermost cond should be BinaryOp (a > b), got {cond:?}"
            );
        }
        other => panic!("innermost should be IfExpr, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Single-condition regressions (must NOT nest — identical to pre-T74 shape).
// ---------------------------------------------------------------------------

#[test]
fn let_chains_single_condition_unchanged() {
    // `if let Some(x) = opt { print(x) }` → flat IfLet, no nesting.
    let stmt = parse_stmt("if let Some(x) = opt { print(x) }");
    let expr = as_if_let(stmt);
    let (pattern, then_block) = match expr {
        Expr::IfLet {
            pattern,
            then_block,
            ..
        } => (pattern, then_block),
        other => panic!("should be flat IfLet, got {other:?}"),
    };
    assert_some_binding(&pattern, "x");
    // The then_block is the BODY directly (NOT a nested IfLet/IfExpr).
    assert_eq!(then_block.stmts.len(), 1, "then is the body (1 stmt)");
    match &then_block.stmts[0] {
        Stmt::ExprStmt(Expr::FuncCall { callee, .. }, _) => {
            assert!(
                matches!(callee.as_ref(), Expr::Ident(id, _) if id.name == "print"),
                "body = print(x)"
            );
        }
        other => panic!("then[0] should be print(x), got {other:?}"),
    }
}

#[test]
fn let_chains_single_bool_unchanged() {
    // `if cond { print(x) }` → flat IfExpr (NOT IfLet, NOT nested).
    let stmt = parse_stmt("if cond { print(x) }");
    let expr = as_if_let(stmt);
    assert!(
        matches!(expr, Expr::IfExpr { .. }),
        "plain `if cond` must stay flat IfExpr, got {expr:?}"
    );
}

#[test]
fn let_chains_single_bool_with_else_unchanged() {
    // `if cond { print(x) } else { print(y) }` → flat IfExpr with else.
    let stmt = parse_stmt("if cond { print(x) } else { print(y) }");
    let expr = as_if_let(stmt);
    match expr {
        Expr::IfExpr {
            else_block: Some(_),
            ..
        } => {}
        other => panic!("plain if-else must stay flat IfExpr with else, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Bool-first mixed chain (bool outer, let inner).
// ---------------------------------------------------------------------------

#[test]
fn let_chains_bool_then_let() {
    // `if a > 0, let Some(b) = opt { print(b) }`
    // → outer IfExpr(a > 0) → inner IfLet(Some(b), opt) → body.
    let stmt = parse_stmt("if a > 0, let Some(b) = opt { print(b) }");
    let outer = as_if_let(stmt);
    let (outer_cond, outer_then) = match outer {
        Expr::IfExpr {
            cond, then_block, ..
        } => (cond, then_block),
        other => panic!("outer should be IfExpr (bool-first chain), got {other:?}"),
    };
    assert!(
        matches!(outer_cond.as_ref(), Expr::BinaryOp { .. }),
        "outer cond should be BinaryOp (a > 0), got {outer_cond:?}"
    );
    let inner = match &outer_then.stmts[0] {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("outer then[0] should be ExprStmt, got {other:?}"),
    };
    match inner {
        Expr::IfLet { pattern, .. } => assert_some_binding(pattern, "b"),
        other => panic!("inner should be IfLet, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Trailing comma before the block.
// ---------------------------------------------------------------------------

#[test]
fn let_chains_trailing_comma() {
    // `if let Some(x) = a, let Some(y) = b, { print(x) }` — trailing comma OK.
    let stmt = parse_stmt("if let Some(x) = a, let Some(y) = b, { print(x) }");
    let outer = as_if_let(stmt);
    match outer {
        Expr::IfLet { pattern, .. } => assert_some_binding(&pattern, "x"),
        other => panic!("outer should be IfLet, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Error paths.
// ---------------------------------------------------------------------------

#[test]
fn let_chains_missing_value_errors() {
    // `if let Some(x) = , let Some(y) = b { }` — empty value → ParseError.
    let tokens = tokenize("if let Some(x) = , let Some(y) = b { }", sid()).expect("lexer ok");
    let mut stream = TokenStream::new(&tokens, sid());
    assert!(
        parse_statement(&mut stream).is_err(),
        "empty value in first let-condition must error, not panic"
    );
}

#[test]
fn let_chains_missing_assign_errors() {
    // `if let Some(x) opt, let Some(y) = b { }` — missing `=` → ParseError.
    let tokens = tokenize("if let Some(x) opt, let Some(y) = b { }", sid()).expect("lexer ok");
    let mut stream = TokenStream::new(&tokens, sid());
    assert!(
        parse_statement(&mut stream).is_err(),
        "missing `=` in let-condition must error, not panic"
    );
}
