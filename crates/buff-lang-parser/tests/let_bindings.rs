//! T72 integration tests — if-let / for-let pattern bindings.
//!
//! Coverage:
//!
//! - `if let Some(x) = opt { ... }` → `Expr::IfLet` with a `Pattern::Variant`.
//! - `if let Some(x) = opt { ... } else { ... }` → `Expr::IfLet` with else.
//! - `for let Some(x) = iter.next() { ... }` → `Stmt::ForLet`.
//! - Regression: plain `if cond { }` STILL produces `Expr::IfExpr` (unchanged).
//! - Regression: plain `for v in iter { }` STILL produces `Stmt::ForIn`.
//! - Regression: plain `for cond { }` STILL produces `Stmt::ForWhile`.
//! - Error path: malformed `if let Some(x) opt { }` (missing `=`) → ParseError.
//! - Error path: malformed `for let Some(x) iter.next() { }` → ParseError.
//!
//! Each test feeds source strings through the lexer and then through
//! [`buff_lang_parser::parse_statement`]. The resulting AST is pattern-matched
//! to assert the expected shape.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser let_bindings
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

// ---------------------------------------------------------------------------
// if let — conditional binding.
// ---------------------------------------------------------------------------

#[test]
fn let_bindings_if_let_some() {
    // `if let Some(x) = opt { print(x) }` → Expr::IfLet (wrapped in ExprStmt)
    // with a Pattern::Variant (Some(x)).
    let stmt = parse_stmt("if let Some(x) = opt { print(x) }");
    let expr = match stmt {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("expected ExprStmt wrapping IfLet, got {other:?}"),
    };
    let (pattern, value, then_block, else_block) = match expr {
        Expr::IfLet {
            pattern,
            value,
            then_block,
            else_block,
            ..
        } => (pattern, value, then_block, else_block),
        other => panic!("expected Expr::IfLet, got {other:?}"),
    };
    // Pattern should be Variant(Some(x)).
    let (variant, subs) = match &pattern {
        Pattern::Variant {
            variant,
            subpatterns,
            ..
        } => (variant, subpatterns),
        other => panic!("expected Pattern::Variant, got {other:?}"),
    };
    assert_eq!(variant.name, "Some", "variant name");
    assert_eq!(subs.len(), 1, "Some(x) has 1 subpattern");
    assert!(
        matches!(&subs[0], Pattern::Ident(id, _) if id.name == "x"),
        "subpattern = {:?}",
        subs[0]
    );
    // Value should be the bare identifier `opt`.
    assert!(
        matches!(value.as_ref(), Expr::Ident(id, _) if id.name == "opt"),
        "value = {value:?}"
    );
    // Then-block has one statement (print(x)).
    assert_eq!(then_block.stmts.len(), 1, "then-block stmts");
    // No else.
    assert!(else_block.is_none(), "should have no else");
}

#[test]
fn let_bindings_if_let_with_else() {
    // `if let Some(x) = opt { print(x) } else { print("none") }` → IfLet
    // with else_block = Some(...).
    let stmt = parse_stmt("if let Some(x) = opt { print(x) } else { print(\"none\") }");
    let expr = match stmt {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("expected ExprStmt wrapping IfLet, got {other:?}"),
    };
    match expr {
        Expr::IfLet {
            else_block: Some(els),
            ..
        } => {
            assert!(!els.stmts.is_empty(), "else block should be non-empty");
        }
        other => panic!("expected IfLet with else, got {other:?}"),
    }
}

#[test]
fn let_bindings_if_let_none_unit_variant() {
    // `if let None = opt { print("is none") }` → IfLet with the `None`
    // parsed as Pattern::Ident("None") — bare idents in pattern position
    // are ambiguous (binding vs unit variant) and the parser conservatively
    // treats them as Ident (T27 disambiguation rule; the exhaustiveness
    // checker unifies them via variant_name_key). Codegen emits the bare
    // name which Rust resolves to the unit variant when the enum is in scope.
    let stmt = parse_stmt("if let None = opt { print(\"none\") }");
    let expr = match stmt {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("expected ExprStmt, got {other:?}"),
    };
    let pattern = match expr {
        Expr::IfLet { pattern, .. } => pattern,
        other => panic!("expected IfLet, got {other:?}"),
    };
    assert!(
        matches!(&pattern, Pattern::Ident(id, _) if id.name == "None"),
        "pattern = {pattern:?}"
    );
}

#[test]
fn let_bindings_if_let_ident_pattern() {
    // `if let x = opt { print(x) }` — a bare-ident pattern ALWAYS matches (it
    // just binds). This is structurally valid; the type-checker may warn.
    let stmt = parse_stmt("if let x = opt { print(x) }");
    let expr = match stmt {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("expected ExprStmt, got {other:?}"),
    };
    let pattern = match expr {
        Expr::IfLet { pattern, .. } => pattern,
        other => panic!("expected IfLet, got {other:?}"),
    };
    assert!(
        matches!(&pattern, Pattern::Ident(id, _) if id.name == "x"),
        "pattern = {pattern:?}"
    );
}

#[test]
fn let_bindings_if_let_tuple_pattern() {
    // `if let (a, b) = pair { print(a) }` → IfLet with a Tuple pattern (T71).
    let stmt = parse_stmt("if let (a, b) = pair { print(a) }");
    let expr = match stmt {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("expected ExprStmt, got {other:?}"),
    };
    let pattern = match expr {
        Expr::IfLet { pattern, .. } => pattern,
        other => panic!("expected IfLet, got {other:?}"),
    };
    let subs = match &pattern {
        Pattern::Tuple(s, _) => s,
        other => panic!("expected Pattern::Tuple, got {other:?}"),
    };
    assert_eq!(subs.len(), 2);
    assert!(matches!(&subs[0], Pattern::Ident(id, _) if id.name == "a"));
    assert!(matches!(&subs[1], Pattern::Ident(id, _) if id.name == "b"));
}

// ---------------------------------------------------------------------------
// for let — looping binding.
// ---------------------------------------------------------------------------

#[test]
fn let_bindings_for_let_some() {
    // `for let Some(x) = iter.next() { print(x) }` → Stmt::ForLet with a
    // Pattern::Variant (Some(x)).
    let stmt = parse_stmt("for let Some(x) = iter.next() { print(x) }");
    let (pattern, value, body) = match stmt {
        Stmt::ForLet {
            pattern,
            value,
            body,
            ..
        } => (pattern, value, body),
        other => panic!("expected Stmt::ForLet, got {other:?}"),
    };
    let (variant, subs) = match &pattern {
        Pattern::Variant {
            variant,
            subpatterns,
            ..
        } => (variant, subpatterns),
        other => panic!("expected Pattern::Variant, got {other:?}"),
    };
    assert_eq!(variant.name, "Some");
    assert_eq!(subs.len(), 1);
    assert!(
        matches!(&subs[0], Pattern::Ident(id, _) if id.name == "x"),
        "subpattern = {:?}",
        subs[0]
    );
    // Value should be `iter.next()` (a method call).
    assert!(
        matches!(&value, Expr::MethodCall { method, .. } if method.name == "next"),
        "value = {value:?}"
    );
    // Body has one statement.
    assert_eq!(body.stmts.len(), 1);
}

#[test]
fn let_bindings_for_let_none_unit_variant() {
    // `for let None = stream.next() { count = count + 1 }` — degenerate but
    // structurally valid (loops while the value is None). `None` parses as
    // Pattern::Ident("None") (T27 disambiguation — bare idents are Ident).
    let stmt = parse_stmt("for let None = stream.next() { count = count + 1 }");
    let pattern = match stmt {
        Stmt::ForLet { pattern, .. } => pattern,
        other => panic!("expected ForLet, got {other:?}"),
    };
    assert!(
        matches!(&pattern, Pattern::Ident(id, _) if id.name == "None"),
        "pattern = {pattern:?}"
    );
}

#[test]
fn let_bindings_for_let_ident_pattern() {
    // `for let x = stream.next() { print(x) }` — always-bind ident pattern.
    let stmt = parse_stmt("for let x = stream.next() { print(x) }");
    let pattern = match stmt {
        Stmt::ForLet { pattern, .. } => pattern,
        other => panic!("expected ForLet, got {other:?}"),
    };
    assert!(
        matches!(&pattern, Pattern::Ident(id, _) if id.name == "x"),
        "pattern = {pattern:?}"
    );
}

// ---------------------------------------------------------------------------
// Regressions: the existing plain if / for paths must be untouched.
// ---------------------------------------------------------------------------

#[test]
fn let_bindings_plain_if_still_works() {
    // `if cond { print(x) }` → Expr::IfExpr (NOT IfLet).
    let stmt = parse_stmt("if cond { print(x) }");
    let expr = match stmt {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("expected ExprStmt, got {other:?}"),
    };
    assert!(
        matches!(expr, Expr::IfExpr { .. }),
        "plain `if cond` must stay IfExpr, got {expr:?}"
    );
}

#[test]
fn let_bindings_plain_if_else_still_works() {
    // `if cond { print(x) } else { print(y) }` → IfExpr with else.
    let stmt = parse_stmt("if cond { print(x) } else { print(y) }");
    let expr = match stmt {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("expected ExprStmt, got {other:?}"),
    };
    match expr {
        Expr::IfExpr {
            else_block: Some(_),
            ..
        } => {}
        other => panic!("plain `if cond ... else ...` must stay IfExpr, got {other:?}"),
    }
}

#[test]
fn let_bindings_plain_for_in_still_works() {
    // `for v in iter { print(v) }` → Stmt::ForIn (NOT ForLet).
    let stmt = parse_stmt("for v in iter { print(v) }");
    match stmt {
        Stmt::ForIn { var, .. } => assert_eq!(var.name, "v"),
        other => panic!("plain `for v in iter` must stay ForIn, got {other:?}"),
    }
}

#[test]
fn let_bindings_plain_for_while_still_works() {
    // `for cond { print(x) }` → Stmt::ForWhile (NOT ForLet).
    let stmt = parse_stmt("for cond { print(x) }");
    match stmt {
        Stmt::ForWhile { .. } => {}
        other => panic!("plain `for cond` must stay ForWhile, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Error paths: malformed if-let / for-let must produce a ParseError, not panic.
// ---------------------------------------------------------------------------

#[test]
fn let_bindings_if_let_missing_assign_errors() {
    // `if let Some(x) opt { ... }` — missing `=` between pattern and value.
    let tokens = tokenize("if let Some(x) opt { print(x) }", sid()).expect("lexer ok");
    let mut stream = TokenStream::new(&tokens, sid());
    assert!(
        parse_statement(&mut stream).is_err(),
        "missing `=` in if-let must error, not panic"
    );
}

#[test]
fn let_bindings_for_let_missing_assign_errors() {
    // `for let Some(x) iter.next() { ... }` — missing `=`.
    let tokens = tokenize("for let Some(x) iter.next() { print(x) }", sid()).expect("lexer ok");
    let mut stream = TokenStream::new(&tokens, sid());
    assert!(
        parse_statement(&mut stream).is_err(),
        "missing `=` in for-let must error, not panic"
    );
}

#[test]
fn let_bindings_if_let_missing_value_errors() {
    // `if let Some(x) = { ... }` — value expression is missing (the `{`
    // opens the then-block immediately, so the value is empty/invalid).
    let tokens = tokenize("if let Some(x) = { print(x) }", sid()).expect("lexer ok");
    let mut stream = TokenStream::new(&tokens, sid());
    assert!(
        parse_statement(&mut stream).is_err(),
        "missing value in if-let must error, not panic"
    );
}

// ---------------------------------------------------------------------------
// Nested / chained forms.
// ---------------------------------------------------------------------------

#[test]
fn let_bindings_else_if_let_chain() {
    // `if let Some(x) = a { ... } else if let Some(y) = b { ... }` — the
    // else-branch is a nested IfLet wrapped in a single-stmt block.
    let stmt = parse_stmt("if let Some(x) = a { print(x) } else if let Some(y) = b { print(y) }");
    let expr = match stmt {
        Stmt::ExprStmt(e, _) => e,
        other => panic!("expected ExprStmt, got {other:?}"),
    };
    let else_block = match expr {
        Expr::IfLet {
            else_block: Some(eb),
            ..
        } => eb,
        other => panic!("expected IfLet with else, got {other:?}"),
    };
    // The else-block wraps a single ExprStmt containing the nested IfLet.
    assert_eq!(else_block.stmts.len(), 1, "else-block has 1 stmt");
    match &else_block.stmts[0] {
        Stmt::ExprStmt(Expr::IfLet { pattern, .. }, _) => {
            // The nested pattern binds `y`.
            let subs = match pattern {
                Pattern::Variant {
                    variant,
                    subpatterns,
                    ..
                } => {
                    assert_eq!(variant.name, "Some");
                    subpatterns
                }
                other => panic!("nested pattern = {other:?}"),
            };
            assert_eq!(subs.len(), 1);
            assert!(
                matches!(&subs[0], Pattern::Ident(id, _) if id.name == "y"),
                "nested binding = {:?}",
                subs[0]
            );
        }
        other => panic!("else-block stmt = {other:?}"),
    }
}
