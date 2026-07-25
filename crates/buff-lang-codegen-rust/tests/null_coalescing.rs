//! Integration tests for null-coalescing `??` operator codegen (T101).
//!
//! Tests that `opt ?? 0` → `opt.unwrap_or(0)` and
//! `name ?? "unknown"` → `name.unwrap_or("unknown")`.

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::op::BinaryOp;
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

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn null_coalesce_expr(lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op: BinaryOp::NullCoalesce,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: span(),
    }
}

/// Build a simple function with one statement and codegen it.
fn codegen_stmt(stmt: Stmt) -> String {
    let func = Decl::FuncDecl(FuncDecl {
        name: ident("f"),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts: vec![stmt],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    });
    generate_rust(&[func]).expect("codegen should succeed")
}

// ---------------------------------------------------------------------------
// `opt ?? 0` → `opt.unwrap_or(0)`
// ---------------------------------------------------------------------------

#[test]
fn null_coalescing_default_int() {
    let src = codegen_stmt(Stmt::ExprStmt(
        null_coalesce_expr(ident_expr("opt"), int_expr(0)),
        span(),
    ));
    assert!(
        src.contains("opt.unwrap_or(0)"),
        "opt ?? 0 should codegen to `opt.unwrap_or(0)`, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// `name ?? "unknown"` → `name.unwrap_or("unknown")`
// ---------------------------------------------------------------------------

#[test]
fn null_coalescing_string() {
    let src = codegen_stmt(Stmt::ExprStmt(
        null_coalesce_expr(ident_expr("name"), string_expr("unknown")),
        span(),
    ));
    assert!(
        src.contains("name.unwrap_or(\"unknown\")"),
        r#"name ?? "unknown" should codegen to `name.unwrap_or("unknown")`, got: {src}"#
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Chained: `a ?? b ?? c` → `a.unwrap_or(b.unwrap_or(c))`
// ---------------------------------------------------------------------------

#[test]
fn null_coalescing_chained() {
    let inner = null_coalesce_expr(ident_expr("b"), ident_expr("c"));
    let outer = null_coalesce_expr(ident_expr("a"), inner);
    let src = codegen_stmt(Stmt::ExprStmt(outer, span()));
    assert!(
        src.contains("a.unwrap_or(b.unwrap_or(c))"),
        "a ?? b ?? c should codegen to `a.unwrap_or(b.unwrap_or(c))`, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}
