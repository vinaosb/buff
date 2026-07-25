//! T0-B4 — `@feature(name)` conditional compilation attribute.
//!
//! Tests that decls carrying `@feature(name)` are emitted only when
//! `name` is in the resolved feature set. Mirrors Rust's
//! `#[cfg(feature = "...")]` + Go build tags.

use buff_lang_ast::{
    common::{Block, Ident},
    decl::{Attribute, FuncDecl},
    Decl,
};
use buff_lang_codegen_rust::{filter_by_features, generate_rust_with_features};
use buff_lang_error::Span;

fn feature_attr(name: &str) -> Attribute {
    Attribute {
        name: Ident::new("feature", Span::dummy()),
        args: vec![name.to_string()],
        named_args: std::collections::BTreeMap::new(),
        span: Span::dummy(),
    }
}

fn plain_fn(name: &str) -> FuncDecl {
    FuncDecl {
        name: Ident::new(name, Span::dummy()),
        params: Vec::new(),
        return_type: None,
        body: Block::empty(Span::dummy()),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: Span::dummy(),
    }
}

fn feature_fn(name: &str, feature: &str) -> FuncDecl {
    let mut f = plain_fn(name);
    f.attributes.push(feature_attr(feature));
    f
}

fn to_decl(f: FuncDecl) -> Decl {
    Decl::FuncDecl(f)
}

#[test]
fn filter_drops_feature_fn_when_feature_absent() {
    let decls = vec![
        to_decl(plain_fn("always")),
        to_decl(feature_fn("gated", "logging")),
    ];
    let filtered = filter_by_features(&decls, &[]);
    assert_eq!(filtered.len(), 1, "only the non-gated fn survives");
}

#[test]
fn filter_keeps_feature_fn_when_feature_present() {
    let decls = vec![
        to_decl(plain_fn("always")),
        to_decl(feature_fn("gated", "logging")),
    ];
    let filtered = filter_by_features(&decls, &["logging".to_string()]);
    assert_eq!(filtered.len(), 2, "both fns survive when feature enabled");
}

#[test]
fn filter_multiple_features_independent() {
    let decls = vec![
        to_decl(feature_fn("a_fn", "a")),
        to_decl(feature_fn("b_fn", "b")),
        to_decl(feature_fn("c_fn", "c")),
        to_decl(plain_fn("always")),
    ];
    let filtered = filter_by_features(&decls, &["a".to_string(), "c".to_string()]);
    assert_eq!(
        filtered.len(),
        3,
        "a_fn + c_fn + always survive; b_fn dropped"
    );
}

#[test]
fn generate_rust_with_features_emits_enabled_only() {
    let decls = vec![
        to_decl(plain_fn("always_emitted")),
        to_decl(feature_fn("only_with_logging", "logging")),
    ];
    let src =
        generate_rust_with_features(&decls, &["logging".to_string()]).expect("codegen succeeds");
    assert!(src.contains("fn always_emitted"), "src: {src}");
    assert!(src.contains("fn only_with_logging"), "src: {src}");
}

#[test]
fn generate_rust_with_features_drops_when_feature_off() {
    let decls = vec![
        to_decl(plain_fn("always_emitted")),
        to_decl(feature_fn("only_with_logging", "logging")),
    ];
    let src = generate_rust_with_features(&decls, &[]).expect("codegen succeeds");
    assert!(src.contains("fn always_emitted"), "src: {src}");
    assert!(
        !src.contains("fn only_with_logging"),
        "gated fn must be absent: {src}"
    );
}

#[test]
fn generate_rust_with_features_strips_feature_attr() {
    // Even when the fn IS emitted, the `@feature(name)` attribute itself
    // must NOT lower to a Rust attribute (Cargo handles feature gating at
    // the dep-resolution layer, not via per-fn attributes). The fn body
    // is emitted clean.
    let decls = vec![to_decl(feature_fn("enabled_fn", "logging"))];
    let src =
        generate_rust_with_features(&decls, &["logging".to_string()]).expect("codegen succeeds");
    assert!(src.contains("fn enabled_fn"), "src: {src}");
    assert!(
        !src.contains("#[feature"),
        "no Rust feature attribute emitted: {src}"
    );
}
