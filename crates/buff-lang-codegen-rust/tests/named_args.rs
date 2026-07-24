//! T105 integration tests — Codegen for named call arguments.
//!
//! Verifies that the Rust codegen:
//! - REORDERS named args to match the callee's declared param order.
//! - Extracts values from named args when reorder isn't possible.
//! - Leaves pure-positional calls unchanged (no regression).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust named_args
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn str_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn named_expr(name: &str, value: Expr) -> Expr {
    Expr::NamedArg {
        name: ident(name),
        value: Box::new(value),
        span: span(),
    }
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

/// Build a free-function decl `func <name>(<params...>) { <body> }`.
fn func_decl(name: &str, params: &[(&str, &str)], body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl { name: ident(name),
    params: params
        .iter()
        .map(|(n, t)| Param {
            name: ident(n),
            ty: named_type(t),
            default_value: None,
            is_comptime: false,
            span: span(),
        })
        .collect(),
    return_type: None,
    body: Block {
        stmts: body_stmts,
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), })
}

fn expr_stmt(e: Expr) -> Stmt {
    Stmt::ExprStmt(e, span())
}

// ---------------------------------------------------------------------------
// greet(name: "Alice", greeting: "Hi") → greet("Alice", "Hi")
// (declaration order: name, greeting → no reorder needed, but values are
// extracted from NamedArg nodes)
// ---------------------------------------------------------------------------

#[test]
fn named_args_codegen_declaration_order() {
    let greet = func_decl(
        "greet",
        &[("name", "String"), ("greeting", "String")],
        Vec::new(),
    );
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(Expr::Ident(ident("greet"), span())),
            args: vec![
                named_expr("name", str_expr("Alice")),
                named_expr("greeting", str_expr("Hi")),
            ],
            span: span(),
        })],
    );
    let src = generate_rust(&[greet, main]).expect("codegen should succeed");
    // The generated Rust should contain `greet("Alice", "Hi")` — values
    // extracted from NamedArg nodes, in source order.
    assert!(
        src.contains(r#"greet("Alice", "Hi")"#),
        "expected `greet(\"Alice\", \"Hi\")` in generated Rust, got: {src}"
    );
    // Verify it re-parses as valid Rust.
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// greet(greeting: "Hi", name: "Alice") → greet("Alice", "Hi")
// (REORDERED to match the callee's declared param order: name, greeting)
// ---------------------------------------------------------------------------

#[test]
fn named_args_codegen_reordered() {
    let greet = func_decl(
        "greet",
        &[("name", "String"), ("greeting", "String")],
        Vec::new(),
    );
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(Expr::Ident(ident("greet"), span())),
            args: vec![
                named_expr("greeting", str_expr("Hi")),
                named_expr("name", str_expr("Alice")),
            ],
            span: span(),
        })],
    );
    let src = generate_rust(&[greet, main]).expect("codegen should succeed");
    // The generated Rust should reorder to declaration order:
    // `greet("Alice", "Hi")` even though source wrote `greeting` first.
    assert!(
        src.contains(r#"greet("Alice", "Hi")"#),
        "expected reordered `greet(\"Alice\", \"Hi\")` in generated Rust, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// create(host: "x", port: 80) → create("x", 80)
// (spec example — host before port; reordered iff declaration matches)
// ---------------------------------------------------------------------------

#[test]
fn named_args_codegen_create_example() {
    let create = func_decl("create", &[("host", "String"), ("port", "Int")], Vec::new());
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(Expr::Ident(ident("create"), span())),
            args: vec![
                named_expr("host", str_expr("x")),
                named_expr("port", int_expr(80)),
            ],
            span: span(),
        })],
    );
    let src = generate_rust(&[create, main]).expect("codegen should succeed");
    assert!(
        src.contains(r#"create("x", 80)"#),
        "expected `create(\"x\", 80)` in generated Rust, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// create(port: 80, host: "x") → create("x", 80) (reversed source order)
// ---------------------------------------------------------------------------

#[test]
fn named_args_codegen_create_reordered() {
    let create = func_decl("create", &[("host", "String"), ("port", "Int")], Vec::new());
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(Expr::Ident(ident("create"), span())),
            args: vec![
                named_expr("port", int_expr(80)),
                named_expr("host", str_expr("x")),
            ],
            span: span(),
        })],
    );
    let src = generate_rust(&[create, main]).expect("codegen should succeed");
    assert!(
        src.contains(r#"create("x", 80)"#),
        "expected reordered `create(\"x\", 80)` in generated Rust, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Pure-positional regression: greet("Alice", "Hi") → greet("Alice", "Hi")
// ---------------------------------------------------------------------------

#[test]
fn named_args_codegen_positional_unchanged() {
    let greet = func_decl(
        "greet",
        &[("name", "String"), ("greeting", "String")],
        Vec::new(),
    );
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(Expr::Ident(ident("greet"), span())),
            args: vec![str_expr("Alice"), str_expr("Hi")],
            span: span(),
        })],
    );
    let src = generate_rust(&[greet, main]).expect("codegen should succeed");
    assert!(
        src.contains(r#"greet("Alice", "Hi")"#),
        "positional call should codegen unchanged: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Mixed positional + named: greet("Alice", greeting: "Hi") → greet("Alice", "Hi")
// ---------------------------------------------------------------------------

#[test]
fn named_args_codegen_mixed_positional_and_named() {
    let greet = func_decl(
        "greet",
        &[("name", "String"), ("greeting", "String")],
        Vec::new(),
    );
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(Expr::Ident(ident("greet"), span())),
            args: vec![str_expr("Alice"), named_expr("greeting", str_expr("Hi"))],
            span: span(),
        })],
    );
    let src = generate_rust(&[greet, main]).expect("codegen should succeed");
    assert!(
        src.contains(r#"greet("Alice", "Hi")"#),
        "mixed positional+named should codegen to `greet(\"Alice\", \"Hi\")`: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Unknown callee (no fn decl in compilation unit): values extracted, no reorder.
// `other(greeting: "Hi", name: "Alice")` → `other("Hi", "Alice")` (source order)
// ---------------------------------------------------------------------------

#[test]
fn named_args_codegen_unknown_callee_extracts_values() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(Expr::Ident(ident("other"), span())),
            args: vec![
                named_expr("greeting", str_expr("Hi")),
                named_expr("name", str_expr("Alice")),
            ],
            span: span(),
        })],
    );
    let src = generate_rust(&[main]).expect("codegen should succeed");
    // No matching fn decl → extract values in source order (no reorder).
    assert!(
        src.contains(r#"other("Hi", "Alice")"#),
        "unknown callee should extract values in source order: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}
