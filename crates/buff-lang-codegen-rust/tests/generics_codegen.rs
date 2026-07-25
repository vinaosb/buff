//! T13 integration tests — Rust codegen for generic declarations.
//!
//! Coverage:
//!
//! - `func id<T>(x: T) -> T { return x }` →
//!   `pub fn id<T>(x: T) -> T { return x; }` (generic function).
//! - `struct Pair<T, U> { x: T, y: U }` →
//!   `pub struct Pair<T, U> { pub x: T, pub y: U }` (generic struct).
//! - `enum Result<T, E> { Ok(T), Err(E) }` →
//!   `pub enum Result<T, E> { Ok(T), Err(E) }` (generic enum — T27 baseline).
//! - Non-generic decls (empty `type_params`) emit NO `<>` (backward compat).
//! - End-to-end: generic decl + call site re-parses as valid Rust via
//!   `syn::parse_str::<syn::File>`.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test generics_codegen
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::{EnumDecl, EnumVariant, FuncDecl, StructDecl, TypeParam};
use buff_lang_ast::{Decl, Expr, Stmt, TypeRef};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::Span;

fn span() -> Span {
    Span::dummy()
}

fn ident(name: &str) -> Ident {
    Ident::new(name, span())
}

fn named_ty(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

fn type_param(name: &str) -> TypeParam {
    TypeParam {
        name: ident(name),
        bounds: Vec::new(),
        span: span(),
    }
}

/// Build a generic function `func id<T>(x: T) -> T { return x }`.
fn generic_id_fn() -> FuncDecl {
    FuncDecl {
        name: ident("id"),
        params: vec![Param {
            name: ident("x"),
            ty: named_ty("T"),
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: Some(named_ty("T")),
        body: Block {
            stmts: vec![Stmt::Return(Some(Expr::Ident(ident("x"), span())), span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: vec![type_param("T")],
        span: span(),
    }
}

/// Build a generic struct `struct Pair<T, U> { x: T, y: U }`.
fn generic_pair_struct() -> StructDecl {
    StructDecl {
        name: ident("Pair"),
        fields: vec![(ident("x"), named_ty("T")), (ident("y"), named_ty("U"))],
        traits: Vec::new(),
        type_params: vec![type_param("T"), type_param("U")],
        span: span(),
    }
}

/// Build a generic enum `enum Result<T, E> { Ok(T), Err(E) }`.
fn generic_result_enum() -> EnumDecl {
    EnumDecl {
        name: ident("Result"),
        type_params: vec![type_param("T"), type_param("E")],
        variants: vec![
            EnumVariant {
                name: ident("Ok"),
                data: Some(vec![named_ty("T")]),
                span: span(),
            },
            EnumVariant {
                name: ident("Err"),
                data: Some(vec![named_ty("E")]),
                span: span(),
            },
        ],
        span: span(),
    }
}

#[test]
fn generic_func_emits_rust_generics() {
    let decls = vec![Decl::FuncDecl(generic_id_fn())];
    let rust = generate_rust(&decls).expect("generic func codegen must succeed");
    // The generated Rust must contain `fn id<T>` (generic param on the fn).
    assert!(
        rust.contains("fn id<T>"),
        "generic func must emit `<T>` — got:\n{rust}"
    );
    // Must contain the param + return type referencing T.
    assert!(
        rust.contains("x: T"),
        "generic func param must use type param T — got:\n{rust}"
    );
    assert!(
        rust.contains("-> T"),
        "generic func return must use type param T — got:\n{rust}"
    );
}

#[test]
fn generic_struct_emits_rust_generics() {
    let decls = vec![Decl::StructDecl(generic_pair_struct())];
    let rust = generate_rust(&decls).expect("generic struct codegen must succeed");
    // The generated Rust must contain `struct Pair<T, U>`.
    assert!(
        rust.contains("struct Pair<T, U>"),
        "generic struct must emit `<T, U>` — got:\n{rust}"
    );
    // Fields must use the type params.
    assert!(
        rust.contains("pub x: T"),
        "generic struct field x must use type param T — got:\n{rust}"
    );
    assert!(
        rust.contains("pub y: U"),
        "generic struct field y must use type param U — got:\n{rust}"
    );
}

#[test]
fn generic_enum_emits_rust_generics() {
    let decls = vec![Decl::EnumDecl(generic_result_enum())];
    let rust = generate_rust(&decls).expect("generic enum codegen must succeed");
    // The generated Rust must contain `enum Result<T, E>`.
    assert!(
        rust.contains("enum Result<T, E>"),
        "generic enum must emit `<T, E>` — got:\n{rust}"
    );
    // Variants must use the type params.
    assert!(
        rust.contains("Ok(T)"),
        "generic enum variant Ok must use type param T — got:\n{rust}"
    );
    assert!(
        rust.contains("Err(E)"),
        "generic enum variant Err must use type param E — got:\n{rust}"
    );
}

#[test]
fn non_generic_func_emits_no_angle_brackets() {
    // Backward compat: a func with empty type_params must NOT emit `<>`.
    let func = FuncDecl {
        name: ident("plain"),
        params: vec![],
        return_type: None,
        body: Block::empty(span()),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let decls = vec![Decl::FuncDecl(func)];
    let rust = generate_rust(&decls).expect("plain func codegen must succeed");
    assert!(
        !rust.contains("fn plain<"),
        "non-generic func must NOT emit `<>` — got:\n{rust}"
    );
}

#[test]
fn generic_decls_parse_as_valid_rust() {
    // End-to-end: the generated Rust for a mix of generic decls must be
    // parseable by syn (i.e., it's valid Rust that rustc would accept).
    let decls = vec![
        Decl::StructDecl(generic_pair_struct()),
        Decl::FuncDecl(generic_id_fn()),
        Decl::EnumDecl(generic_result_enum()),
    ];
    let rust = generate_rust(&decls).expect("mixed generic codegen must succeed");
    syn::parse_str::<syn::File>(&rust).expect("generated Rust must parse as valid syn::File");
}
