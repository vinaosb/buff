//! Integration tests for range expression codegen (T68).
//!
//! Tests that `0..10` → Rust `0..10` and `0..=10` → Rust `0..=10`.

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn range_expr(start: Expr, end: Expr, inclusive: bool) -> Expr {
    Expr::Range {
        start: Box::new(start),
        end: Box::new(end),
        inclusive,
        span: span(),
    }
}

/// Build a simple function with one statement and codegen it.
fn codegen_stmt(stmt: Stmt) -> String {
    let func = Decl::FuncDecl(FuncDecl { name: ident("f"),
    params: Vec::new(),
    return_type: None,
    body: Block {
        stmts: vec![stmt],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), });
    generate_rust(&[func]).expect("codegen should succeed")
}

// ---------------------------------------------------------------------------
// Exclusive range: `0..10` → Rust `0..10`
// ---------------------------------------------------------------------------

#[test]
fn ranges_codegen_exclusive() {
    let src = codegen_stmt(Stmt::ExprStmt(
        range_expr(int_expr(0), int_expr(10), false),
        span(),
    ));
    // The generated Rust should contain `0..10` (exclusive range).
    assert!(
        src.contains("0..10"),
        "exclusive range should codegen to `0..10`, got: {src}"
    );
    // Verify it re-parses as valid Rust.
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Inclusive range: `0..=10` → Rust `0..=10`
// ---------------------------------------------------------------------------

#[test]
fn ranges_codegen_inclusive() {
    let src = codegen_stmt(Stmt::ExprStmt(
        range_expr(int_expr(0), int_expr(10), true),
        span(),
    ));
    // The generated Rust should contain `0..=10` (inclusive range).
    assert!(
        src.contains("0..=10"),
        "inclusive range should codegen to `0..=10`, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Range with ident bounds: `start..end` → Rust `start..end`
// ---------------------------------------------------------------------------

#[test]
fn ranges_codegen_ident_bounds() {
    let src = codegen_stmt(Stmt::ExprStmt(
        range_expr(ident_expr("start"), ident_expr("end"), false),
        span(),
    ));
    assert!(
        src.contains("start..end"),
        "ident range should codegen to `start..end`, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Range in for loop: `for i in 0..5`
// ---------------------------------------------------------------------------

#[test]
fn ranges_codegen_for_loop() {
    let src = codegen_stmt(Stmt::ForIn {
        var: ident("i"),
        iter: range_expr(int_expr(0), int_expr(5), false),
        body: Block::empty(span()),
        span: span(),
    });
    // The generated Rust should contain `for i in 0..5`.
    assert!(
        src.contains("for i in 0..5"),
        "for loop with range should codegen to `for i in 0..5`, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}
