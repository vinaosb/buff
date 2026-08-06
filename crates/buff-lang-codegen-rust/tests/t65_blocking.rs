//! T65 — `@blocking` attribute lowering tests.
//!
//! Verifies that `@blocking` on a function emits a `///@blocking`
//! doc-comment marker in the generated Rust source. The marker is machine-
//! readable metadata for the async runtime dispatch layer (future
//! `spawn_blocking` wrapping) and does not change the function's async
//! propagation.

use buff_lang_ast::{
    common::{Block, Ident},
    decl::{Attribute, FuncDecl},
    Decl,
};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::Span;
use std::collections::BTreeMap;

fn fn_with_attrs(name: &str, attrs: Vec<Attribute>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: Ident::new(name, Span::dummy()),
        params: Vec::new(),
        return_type: None,
        body: Block::empty(Span::dummy()),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: attrs,
        type_params: Vec::new(),
        span: Span::dummy(),
    })
}

fn attr(name: &str) -> Attribute {
    Attribute {
        name: Ident::new(name, Span::dummy()),
        args: Vec::new(),
        named_args: BTreeMap::new(),
        span: Span::dummy(),
    }
}

#[test]
fn blocking_emits_doc_marker() {
    let decls = vec![fn_with_attrs("slow_io", vec![attr("blocking")])];
    let src = generate_rust(&decls).expect("codegen");
    assert!(
        src.contains("///@blocking"),
        "expected ///@blocking doc marker, src: {src}"
    );
    assert!(src.contains("fn slow_io"), "fn name preserved, src: {src}");
}

#[test]
fn blocking_preserves_function_body() {
    let mut func = FuncDecl {
        name: Ident::new("db_query", Span::dummy()),
        params: Vec::new(),
        return_type: None,
        body: Block::empty(Span::dummy()),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: vec![attr("blocking")],
        type_params: Vec::new(),
        span: Span::dummy(),
    };
    func.body.stmts.push(buff_lang_ast::Stmt::ExprStmt(
        buff_lang_ast::Expr::Ident(Ident::new("placeholder", Span::dummy()), Span::dummy()),
        Span::dummy(),
    ));
    let src = generate_rust(&[Decl::FuncDecl(func)]).expect("codegen");
    assert!(src.contains("///@blocking"), "src: {src}");
    assert!(src.contains("placeholder"), "body preserved, src: {src}");
}

#[test]
fn blocking_does_not_break_other_attributes() {
    let decls = vec![fn_with_attrs(
        "blocked_test",
        vec![attr("blocking"), attr("test")],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("///@blocking"), "src: {src}");
    assert!(src.contains("#[test]"), "src: {src}");
}
