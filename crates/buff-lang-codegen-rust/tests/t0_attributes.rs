//! T0 (B2+B3+F2+G3) — New attribute lowering tests.
//!
//! Verifies the codegen lowers `@deprecated`, `@should_panic`, `@ignore`,
//! `@bench` to the corresponding Rust attributes; strips `@internal` and
//! `@property` (convention-only for v1.13).

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

fn deprecated_attr(since: Option<&str>, replacement: Option<&str>) -> Attribute {
    let mut named = BTreeMap::new();
    if let Some(s) = since {
        named.insert("since".to_string(), s.to_string());
    }
    if let Some(r) = replacement {
        named.insert("replacement".to_string(), r.to_string());
    }
    Attribute {
        name: Ident::new("deprecated", Span::dummy()),
        args: Vec::new(),
        named_args: named,
        span: Span::dummy(),
    }
}

#[test]
fn should_panic_lowers_to_rust_attribute() {
    let decls = vec![fn_with_attrs("explodes", vec![attr("should_panic")])];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("#[should_panic]"), "src: {src}");
    assert!(src.contains("fn explodes"), "src: {src}");
}

#[test]
fn ignore_lowers_to_rust_attribute() {
    let decls = vec![fn_with_attrs("slow_test", vec![attr("ignore")])];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("#[ignore]"), "src: {src}");
}

#[test]
fn bench_lowers_to_rust_attribute() {
    let decls = vec![fn_with_attrs("bench_sort", vec![attr("bench")])];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("#[bench]"), "src: {src}");
}

#[test]
fn internal_is_stripped_silently() {
    let decls = vec![fn_with_attrs("helper", vec![attr("internal")])];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("fn helper"), "src: {src}");
    assert!(
        !src.contains("#[internal"),
        "no Rust attribute emitted for @internal: {src}"
    );
}

#[test]
fn property_is_stripped_silently() {
    let decls = vec![fn_with_attrs("prop_test", vec![attr("property")])];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("fn prop_test"), "src: {src}");
    assert!(
        !src.contains("#[property"),
        "no Rust attribute emitted for @property: {src}"
    );
}

#[test]
fn deprecated_with_since_and_replacement_lowers() {
    let decls = vec![fn_with_attrs(
        "old_fn",
        vec![deprecated_attr(Some("2.0"), Some("new_fn"))],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("#[deprecated"), "src: {src}");
    assert!(src.contains("since = \"2.0\""), "src: {src}");
    // The replacement is rendered as the note: `note = "use 'new_fn'"`.
    assert!(src.contains("use 'new_fn'"), "src: {src}");
}

#[test]
fn deprecated_with_only_since_lowers() {
    let decls = vec![fn_with_attrs(
        "old_fn",
        vec![deprecated_attr(Some("1.5"), None)],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("since = \"1.5\""), "src: {src}");
    // No replacement → no note.
    assert!(
        !src.contains("note ="),
        "no note when replacement absent: {src}"
    );
}

#[test]
fn deprecated_with_no_args_still_emits_attribute() {
    let decls = vec![fn_with_attrs("old_fn", vec![deprecated_attr(None, None)])];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("#[deprecated"), "src: {src}");
}

#[test]
fn multiple_attributes_on_one_fn() {
    let decls = vec![fn_with_attrs(
        "skipped_test",
        vec![attr("test"), attr("ignore"), attr("should_panic")],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("#[test"), "src: {src}");
    assert!(src.contains("#[ignore]"), "src: {src}");
    assert!(src.contains("#[should_panic]"), "src: {src}");
}

#[test]
fn unknown_attribute_surfaces_codegen_error() {
    let decls = vec![fn_with_attrs("x", vec![attr("totally_made_up")])];
    let err = generate_rust(&decls).expect_err("unknown attribute must error");
    let msg = format!("{err}");
    assert!(msg.contains("@totally_made_up"), "err: {msg}");
}
