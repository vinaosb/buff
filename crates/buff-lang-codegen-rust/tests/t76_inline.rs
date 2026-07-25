//! T76 — `@inline` / `@no_inline` attribute lowering tests.
//!
//! Verifies that `@inline` emits `#[inline]` and `@no_inline` emits
//! `#[inline(never)]` on the generated Rust function.

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
fn inline_emits_rust_inline_attribute() {
    let decls = vec![fn_with_attrs("hot_path", vec![attr("inline")])];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("#[inline]"), "expected #[inline], src: {src}");
    assert!(src.contains("fn hot_path"), "fn name preserved, src: {src}");
}

#[test]
fn no_inline_emits_rust_inline_never_attribute() {
    let decls = vec![fn_with_attrs("cold_path", vec![attr("no_inline")])];
    let src = generate_rust(&decls).expect("codegen");
    assert!(
        src.contains("#[inline(never)]"),
        "expected #[inline(never)], src: {src}"
    );
}

#[test]
fn inline_and_no_inline_can_coexist_with_test() {
    // Both @inline and @test should appear.
    let decls = vec![fn_with_attrs(
        "tested_hot",
        vec![attr("inline"), attr("test")],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("#[inline]"), "src: {src}");
    assert!(src.contains("#[test]"), "src: {src}");
}

#[test]
fn inline_does_not_emit_when_absent() {
    let decls = vec![fn_with_attrs("plain_fn", vec![])];
    let src = generate_rust(&decls).expect("codegen");
    assert!(
        !src.contains("#[inline"),
        "plain fn should not have #[inline], src: {src}"
    );
}
