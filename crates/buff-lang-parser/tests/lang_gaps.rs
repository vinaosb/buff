//! ITER-53 language-gap regression tests (goal: fix the top-5
//! maintainability issues harvested in ITER-48).
//!
//! Gap A: bare `{}` must produce an error that suggests the `{:}` empty-map
//! marker (the form itself already parses — see the companion test).
//! Gap B: a bare `return` (no value) followed by more statements must not
//! break the enclosing block (`Stmt::Return(Option<Expr>, _)` already
//! exists in the AST).
//! Gap C: a method-chain continuation line starting with `.` at a deeper
//! indent continues the previous expression instead of breaking the block.

use buff_lang_ast::{Decl, Expr, Stmt};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

fn parse_src(src: &str) -> Result<Vec<Decl>, String> {
    let tokens = tokenize(src, SourceId(0)).map_err(|e| format!("tokenize: {e:?}"))?;
    parse(&tokens, SourceId(0)).map_err(|e| format!("parse: {e:?}"))
}

fn body_of(decls: &[Decl]) -> Vec<&Stmt> {
    match &decls[0] {
        Decl::FuncDecl(fd) => fd.body.stmts.iter().collect(),
        other => panic!("expected FuncDecl, got {other:?}"),
    }
}

/// The explicit empty-map marker `{:}` parses to an empty `Expr::MapLit`.
#[test]
fn empty_map_marker_form_parses() {
    let decls = parse_src("func f():\n    let m = {:}\n").expect("empty map marker {:} must parse");
    let stmts = body_of(&decls);
    match &stmts[0] {
        Stmt::LetDecl {
            value: Expr::MapLit { entries, .. },
            ..
        } => {
            assert!(entries.is_empty(), "expected zero entries");
        }
        other => panic!("expected let with empty MapLit, got {other:?}"),
    }
}

/// Bare `{}` fails (by design — ambiguous with code blocks), but the error
/// must point users at the `{:}` empty-map marker.
#[test]
fn bare_braces_error_suggests_empty_map_marker() {
    let err = parse_src("func f():\n    let m = {}\n").expect_err("bare {} must not parse");
    assert!(
        err.contains("{:}"),
        "error should suggest the empty-map marker {:?}: got {err:?}",
        "{:}"
    );
}

/// A bare `return` inside a nested block, followed by statements in the
/// enclosing block, must parse (ITER-48 harvest: pipeline_with_dataframe).
#[test]
fn bare_return_then_statement_parses() {
    let decls = parse_src("func f(c: Bool):\n    if c:\n        return\n    print(1)\n")
        .expect("bare return must not break the enclosing block");
    let stmts = body_of(&decls);
    assert_eq!(
        stmts.len(),
        2,
        "if + print expected, got {} stmts",
        stmts.len()
    );
    assert!(
        matches!(stmts[1], Stmt::ExprStmt(..)),
        "second stmt should be the print call"
    );
}

/// A method chain continued on a deeper-indented line starting with `.`
/// parses as one postfix chain (ITER-48 harvest: pipeline/simple).
#[test]
fn leading_dot_continuation_chain_parses() {
    let decls = parse_src("func f():\n    let x = foo()\n        .bar()\n    print(x)\n")
        .expect("leading-dot continuation line must not break the block");
    let stmts = body_of(&decls);
    assert_eq!(
        stmts.len(),
        2,
        "let + print expected, got {} stmts",
        stmts.len()
    );
    match &stmts[0] {
        Stmt::LetDecl {
            value: Expr::MethodCall { method, .. },
            ..
        } => {
            assert_eq!(method.name, "bar");
        }
        other => panic!("expected let with .bar() method call, got {other:?}"),
    }
}
