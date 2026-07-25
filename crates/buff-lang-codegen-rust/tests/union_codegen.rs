//! T76 integration tests — Rust codegen for union types `A | B`.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test union_codegen
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Stmt, TypeRef};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn named_ty(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

fn union_ty(members: Vec<TypeRef>) -> TypeRef {
    TypeRef::Union(members, span())
}

fn union_func(name: &str, param_name: &str, ty: TypeRef) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: vec![Param {
            name: ident(param_name),
            ty: ty.clone(),
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: Some(ty),
        body: Block {
            stmts: vec![Stmt::Return(Some(ident_expr(param_name)), span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    })
}

fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

#[test]
fn union_codegen_emits_wrapper_enum() {
    let decl = union_func(
        "process",
        "x",
        union_ty(vec![named_ty("String"), named_ty("Int")]),
    );

    let src = generate_rust(&[decl]).expect("union codegen must succeed");

    assert!(
        src.contains("enum StringOrInt"),
        "expected wrapper enum in:\n{src}"
    );
    assert!(
        src.contains("String(String)"),
        "expected String variant in:\n{src}"
    );
    assert!(src.contains("Int(i64)"), "expected Int variant in:\n{src}");
    assert!(
        src.contains("fn process(x: StringOrInt) -> StringOrInt"),
        "expected wrapper type in function signature:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn union_codegen_deduplicates_same_wrapper() {
    let union = union_ty(vec![named_ty("String"), named_ty("Int")]);
    let decls = vec![
        union_func("first", "x", union.clone()),
        union_func("second", "y", union),
    ];

    let src = generate_rust(&decls).expect("union codegen must succeed");

    assert_eq!(
        src.matches("enum StringOrInt").count(),
        1,
        "expected one wrapper enum:\n{src}"
    );
    assert!(src.contains("fn first(x: StringOrInt) -> StringOrInt"));
    assert!(src.contains("fn second(y: StringOrInt) -> StringOrInt"));
    must_reparse(&src);
}

#[test]
fn union_codegen_three_members_wrapper_name_is_stable() {
    let decl = union_func(
        "triple",
        "x",
        union_ty(vec![named_ty("String"), named_ty("Int"), named_ty("Bool")]),
    );

    let src = generate_rust(&[decl]).expect("union codegen must succeed");

    assert!(
        src.contains("enum StringOrIntOrBool"),
        "expected stable wrapper name:\n{src}"
    );
    assert!(src.contains("String(String)"));
    assert!(src.contains("Int(i64)"));
    assert!(src.contains("Bool(bool)"));
    must_reparse(&src);
}
