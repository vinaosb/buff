//! Integration tests for T102: expression function shorthand `=>`.
//!
//! `func f(x) => EXPR` is syntactic sugar for `func f(x) { return EXPR }`.
//! These tests verify the parser desugars `=>` into a normal FuncDecl whose
//! body is a Block containing a single return statement.

use buff_lang_ast::{Decl, Expr, FuncDecl, Literal, Stmt};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse, parse_func_decl, TokenStream};

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse a function declaration. The source must start with `func`.
fn parse_func(src: &str) -> FuncDecl {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_func_decl(&mut stream, Vec::new()).expect("parser should succeed")
}

/// Assert that a FuncDecl's body is a single return statement wrapping the
/// given expression shape description (via Display).
fn assert_body_is_return(f: &FuncDecl, expected_expr_display: &str) {
    assert_eq!(
        f.body.stmts.len(),
        1,
        "expression-function body should have exactly 1 statement"
    );
    match &f.body.stmts[0] {
        Stmt::Return(Some(expr), _) => {
            let display = expr.to_string();
            assert_eq!(display, expected_expr_display, "return expression mismatch");
        }
        other => panic!("expected Return(Some(expr)), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 1. Expression function without return type
// ---------------------------------------------------------------------------

#[test]
fn expr_functions_untyped() {
    let f = parse_func("func double(x: Int) => x * 2");
    assert_eq!(f.name.name, "double");
    assert_eq!(f.params.len(), 1);
    assert_eq!(f.params[0].name.name, "x");
    assert!(f.return_type.is_none());
    assert_body_is_return(&f, "BinaryOp(*, Ident(x), Lit(Int(2)))");
}

// ---------------------------------------------------------------------------
// 2. Expression function with typed param, no return type
// ---------------------------------------------------------------------------

#[test]
fn expr_functions_typed_param() {
    let f = parse_func("func sq(x: Int) => x * x");
    assert_eq!(f.name.name, "sq");
    assert_eq!(f.params.len(), 1);
    assert_eq!(f.params[0].name.name, "x");
    assert!(f.return_type.is_none());
    assert_body_is_return(&f, "BinaryOp(*, Ident(x), Ident(x))");
}

// ---------------------------------------------------------------------------
// 3. Expression function with return type annotation
// ---------------------------------------------------------------------------

#[test]
fn expr_functions_with_return_type() {
    let f = parse_func("func sq(x: Int) -> Int => x * x");
    assert_eq!(f.name.name, "sq");
    assert_eq!(f.params.len(), 1);
    assert!(f.return_type.is_some());
    assert_body_is_return(&f, "BinaryOp(*, Ident(x), Ident(x))");
}

// ---------------------------------------------------------------------------
// 4. Expression function via top-level parse()
// ---------------------------------------------------------------------------

#[test]
fn expr_functions_via_parse_top_level() {
    let tokens = tokenize("func f(x: Int) => x + 1", sid()).expect("lex");
    let decls = parse(&tokens, sid()).expect("parse");
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::FuncDecl(f) => {
            assert_eq!(f.name.name, "f");
            assert_body_is_return(f, "BinaryOp(+, Ident(x), Lit(Int(1)))");
        }
        other => panic!("expected FuncDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 5. Expression function with multiple params and complex expression
// ---------------------------------------------------------------------------

#[test]
fn expr_functions_multi_param() {
    let f = parse_func("func add(a: Int, b: Int) => a + b");
    assert_eq!(f.name.name, "add");
    assert_eq!(f.params.len(), 2);
    assert_body_is_return(&f, "BinaryOp(+, Ident(a), Ident(b))");
}

// ---------------------------------------------------------------------------
// 6. Normal block-body functions still parse unchanged
// ---------------------------------------------------------------------------

#[test]
fn expr_functions_normal_block_still_works() {
    let f = parse_func("func foo() { return 42 }");
    assert_eq!(f.name.name, "foo");
    assert_eq!(f.body.stmts.len(), 1);
    match &f.body.stmts[0] {
        Stmt::Return(Some(expr), _) => {
            assert!(matches!(expr, Expr::Literal(Literal::Int(42), _)));
        }
        other => panic!("expected Return(Some), got {other:?}"),
    }
}
