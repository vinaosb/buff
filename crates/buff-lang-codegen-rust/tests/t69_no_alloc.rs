//! T69 — `@no-alloc` lint end-to-end integration tests.
//!
//! Verifies the attribute is recognised (no codegen error), stripped from the
//! generated Rust, and that the allocation-scanning pass populates the
//! codegen's `warnings` channel when a `@no-alloc` function body contains a
//! heap-allocating construct. `print(x)` lowers to `println!(...)`, which the
//! scanner flags; a pure-arithmetic body produces no warnings.

use buff_lang_ast::{
    common::{Block, Ident},
    decl::{Attribute, FuncDecl},
    expr::{Expr, Literal},
    stmt::Stmt,
    Decl,
};
use buff_lang_codegen_rust::RustCodegen;
use buff_lang_error::Span;
use std::collections::BTreeMap;

fn no_alloc_attr() -> Attribute {
    Attribute {
        name: Ident::new("no-alloc", Span::dummy()),
        args: Vec::new(),
        named_args: BTreeMap::new(),
        span: Span::dummy(),
    }
}

fn fn_with_body(name: &str, attrs: Vec<Attribute>, body: Block) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: Ident::new(name, Span::dummy()),
        params: Vec::new(),
        return_type: None,
        body,
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: attrs,
        type_params: Vec::new(),
        span: Span::dummy(),
    })
}

fn block_of(stmts: Vec<Stmt>) -> Block {
    Block {
        stmts,
        span: Span::dummy(),
    }
}

/// `print(1)` — lowers to `println!(...)`, which the no-alloc scanner flags.
fn print_call() -> Expr {
    Expr::FuncCall {
        callee: Box::new(Expr::Ident(
            Ident::new("print", Span::dummy()),
            Span::dummy(),
        )),
        args: vec![Expr::Literal(Literal::Int(1), Span::dummy())],
        span: Span::dummy(),
    }
}

#[test]
fn no_alloc_attribute_is_recognised_and_stripped() {
    let decls = vec![fn_with_body(
        "clean",
        vec![no_alloc_attr()],
        Block::empty(Span::dummy()),
    )];
    let mut cg = RustCodegen::new();
    let file = cg.generate(&decls).expect("codegen must not error");
    let src = buff_lang_codegen_rust::format(&file);
    assert!(src.contains("fn clean"), "src: {src}");
    // The attribute is stripped — no Rust attribute leaks (rustc would reject
    // a bare #[no-alloc]).
    assert!(
        !src.contains("no-alloc") && !src.contains("no_alloc"),
        "@no-alloc must be stripped from generated source: {src}"
    );
    // Clean (empty) body → no warnings.
    let warnings = cg.take_warnings();
    assert!(
        warnings.is_empty(),
        "empty @no-alloc body must not warn: {warnings:?}"
    );
}

#[test]
fn no_alloc_violating_body_emits_warning() {
    // `print(1)` lowers to `println!(...)` — a heap-allocating macro.
    let body = block_of(vec![Stmt::ExprStmt(print_call(), Span::dummy())]);
    let decls = vec![fn_with_body("leaky", vec![no_alloc_attr()], body)];

    let mut cg = RustCodegen::new();
    let file = cg.generate(&decls).expect("codegen must not error");
    let _ = buff_lang_codegen_rust::format(&file);

    let warnings = cg.take_warnings();
    assert!(
        !warnings.is_empty(),
        "violating @no-alloc body must produce ≥1 warning"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("no-alloc") && w.message.contains("leaky")),
        "warning must name the function and the attribute: {warnings:?}"
    );
}

#[test]
fn no_alloc_underscore_spelling_also_works() {
    // The underscored `@no_alloc` spelling is accepted identically.
    let attr = Attribute {
        name: Ident::new("no_alloc", Span::dummy()),
        args: Vec::new(),
        named_args: BTreeMap::new(),
        span: Span::dummy(),
    };
    let decls = vec![fn_with_body("u", vec![attr], Block::empty(Span::dummy()))];
    let mut cg = RustCodegen::new();
    let _ = cg.generate(&decls).expect("codegen must not error");
    assert!(cg.take_warnings().is_empty(), "clean body, no warnings");
}

#[test]
fn function_without_attribute_never_warns() {
    // A violating body WITHOUT @no-alloc must NOT produce a warning — the
    // lint is opt-in.
    let body = block_of(vec![Stmt::ExprStmt(print_call(), Span::dummy())]);
    let decls = vec![fn_with_body("unmarked", Vec::new(), body)];
    let mut cg = RustCodegen::new();
    let _ = cg.generate(&decls).expect("codegen must not error");
    assert!(
        cg.take_warnings().is_empty(),
        "no @no-alloc attribute → no allocation warnings"
    );
}
