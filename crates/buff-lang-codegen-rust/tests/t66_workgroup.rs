//! T66 — `@workgroup(N)` attribute tests.
//!
//! Verifies:
//! 1. The parser accepts `@workgroup(64)` with an integer literal arg.
//! 2. The codegen emits a `#[doc = "@workgroup(64)"]` marker.
//! 3. Argument-less `@workgroup` defaults to 64.

use buff_lang_ast::{
    common::{Block, Ident},
    decl::{Attribute, FuncDecl},
    Decl,
};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::{SourceId, Span};
use buff_lang_parser::parse;
use std::collections::BTreeMap;

fn fn_with_workgroup(n: Option<&str>) -> Decl {
    let attrs = vec![Attribute {
        name: Ident::new("workgroup", Span::dummy()),
        args: n.map(|s| vec![s.to_string()]).unwrap_or_default(),
        named_args: BTreeMap::new(),
        span: Span::dummy(),
    }];
    Decl::FuncDecl(FuncDecl {
        name: Ident::new("gpu_kernel", Span::dummy()),
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

#[test]
fn workgroup_with_explicit_size_emits_marker() {
    let decls = vec![fn_with_workgroup(Some("64"))];
    let src = generate_rust(&decls).expect("codegen");
    assert!(
        src.contains("#[doc = \"@workgroup(64)\"]"),
        "expected workgroup marker, src: {src}"
    );
    assert!(src.contains("fn gpu_kernel"), "fn name preserved, src: {src}");
}

#[test]
fn workgroup_without_arg_defaults_to_64() {
    let decls = vec![fn_with_workgroup(None)];
    let src = generate_rust(&decls).expect("codegen");
    assert!(
        src.contains("#[doc = \"@workgroup(64)\"]"),
        "default workgroup size 64, src: {src}"
    );
}

#[test]
fn parser_accepts_integer_workgroup_arg() {
    let src = "@workgroup(128)\nfunc kernel():\n    return\n";
    let tokens = buff_lang_lexer::tokenize(src, SourceId(0)).expect("lex");
    let decls = parse(&tokens, SourceId(0)).expect("parse");
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::FuncDecl(f) => {
            assert_eq!(f.attributes.len(), 1);
            assert_eq!(f.attributes[0].name.name, "workgroup");
            assert_eq!(f.attributes[0].args, vec!["128".to_string()]);
        }
        other => panic!("expected FuncDecl, got {other:?}"),
    }
}
