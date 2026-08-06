//! T64 — `@prefer(cpu)` / `@prefer(gpu)` / `@force(gpu)` dispatch overrides.
//!
//! Verifies the codegen lowers the dispatch-hint attributes to machine-readable
//! `///@...` doc-comment markers (mirrors the T65 `@blocking` and T66 `@workgroup`
//! pattern). The runtime dispatch layer (`buff_lang_runtime::hints`) reads these
//! markers from the generated source metadata; codegen itself emits no Rust
//! semantic effect — the generated function always compiles unchanged.

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

fn attr_with_arg(name: &str, arg: &str) -> Attribute {
    Attribute {
        name: Ident::new(name, Span::dummy()),
        args: vec![arg.to_string()],
        named_args: BTreeMap::new(),
        span: Span::dummy(),
    }
}

#[test]
fn prefer_cpu_emits_doc_marker() {
    let decls = vec![fn_with_attrs(
        "scalar_work",
        vec![attr_with_arg("prefer", "cpu")],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("fn scalar_work"), "src: {src}");
    assert!(
        src.contains("///@prefer(cpu)"),
        "@prefer(cpu) must emit a /// doc marker: {src}"
    );
    // No semantic Rust attribute — pure metadata.
    assert!(
        !src.contains("#[prefer"),
        "no bare #[prefer attribute (rustc would reject it): {src}"
    );
}

#[test]
fn prefer_gpu_emits_doc_marker() {
    let decls = vec![fn_with_attrs(
        "vec_work",
        vec![attr_with_arg("prefer", "gpu")],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(
        src.contains("///@prefer(gpu)"),
        "@prefer(gpu) must emit a /// doc marker: {src}"
    );
}

#[test]
fn prefer_npu_emits_doc_marker() {
    let decls = vec![fn_with_attrs(
        "accel_work",
        vec![attr_with_arg("prefer", "npu")],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(
        src.contains("///@prefer(npu)"),
        "@prefer(npu) must emit a /// doc marker: {src}"
    );
}

#[test]
fn force_gpu_emits_doc_marker() {
    let decls = vec![fn_with_attrs(
        "heavy_gpu",
        vec![attr_with_arg("force", "gpu")],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("fn heavy_gpu"), "src: {src}");
    assert!(
        src.contains("///@force(gpu)"),
        "@force(gpu) must emit a /// doc marker: {src}"
    );
    assert!(
        !src.contains("#[force"),
        "no bare #[force attribute (rustc would reject it): {src}"
    );
}

#[test]
fn prefer_and_force_coexist_with_other_attrs() {
    // A function can carry both a dispatch override and a regular attribute.
    let decls = vec![fn_with_attrs(
        "mixed",
        vec![
            attr_with_arg("prefer", "cpu"),
            Attribute {
                name: Ident::new("inline", Span::dummy()),
                args: Vec::new(),
                named_args: BTreeMap::new(),
                span: Span::dummy(),
            },
        ],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(src.contains("///@prefer(cpu)"), "src: {src}");
    assert!(src.contains("#[inline]"), "src: {src}");
}

#[test]
fn prefer_without_arg_defaults_to_cpu() {
    // Argument-less `@prefer` is tolerated (defaults to "cpu") rather than
    // erroring — defensive, matches the @workgroup(64) default-arg precedent.
    let decls = vec![fn_with_attrs(
        "defaulted",
        vec![Attribute {
            name: Ident::new("prefer", Span::dummy()),
            args: Vec::new(),
            named_args: BTreeMap::new(),
            span: Span::dummy(),
        }],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(
        src.contains("///@prefer(cpu)"),
        "argument-less @prefer defaults to cpu: {src}"
    );
}

#[test]
fn force_without_arg_defaults_to_gpu() {
    let decls = vec![fn_with_attrs(
        "defaulted",
        vec![Attribute {
            name: Ident::new("force", Span::dummy()),
            args: Vec::new(),
            named_args: BTreeMap::new(),
            span: Span::dummy(),
        }],
    )];
    let src = generate_rust(&decls).expect("codegen");
    assert!(
        src.contains("///@force(gpu)"),
        "argument-less @force defaults to gpu: {src}"
    );
}
