//! T103 integration tests — Rust codegen for tuple types and tuple values.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test tuples
//! ```
//!
//! Verifies `func pair() -> (String, Int) { return ("A", 42) }` lowers to a
//! Rust function whose return type is a real Rust tuple `(String, i64)` and
//! whose body returns a tuple literal `("A", 42)`.

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

fn named_ty(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

fn tuple_ty(members: Vec<TypeRef>) -> TypeRef {
    TypeRef::Tuple(members, span())
}

fn str_lit(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn int_lit(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn tuple_value(members: Vec<Expr>) -> Expr {
    Expr::TupleLit(members, span())
}

/// `func pair() -> (String, Int) { return ("A", 42) }`
fn pair_func() -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident("pair"),
        params: Vec::new(),
        return_type: Some(tuple_ty(vec![named_ty("String"), named_ty("Int")])),
        body: Block {
            stmts: vec![Stmt::Return(
                Some(tuple_value(vec![str_lit("A"), int_lit(42)])),
                span(),
            )],
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
fn tuples_codegen_pair_function() {
    let src = generate_rust(&[pair_func()]).expect("tuple codegen must succeed");

    // Signature: `fn pair() -> (String, i64)` — a real Rust tuple return type.
    assert!(
        src.contains("fn pair() -> (String, i64)"),
        "expected tuple return type in:\n{src}"
    );
    // Body: returns a tuple literal `("A", 42)`. prettyplease emits the form
    // `("A", 42)` (or with String::from wrappers — just check both elements
    // are present inside parens after the return).
    assert!(
        src.contains("return ("),
        "expected tuple literal return in:\n{src}"
    );
    assert!(
        src.contains("\"A\""),
        "expected string element in tuple literal:\n{src}"
    );
    assert!(
        src.contains("42"),
        "expected integer element in tuple literal:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn tuples_codegen_three_member_return() {
    // `func triple() -> (String, Int, Bool) { return ("X", 7, true) }`
    let decl = Decl::FuncDecl(FuncDecl {
        name: ident("triple"),
        params: Vec::new(),
        return_type: Some(tuple_ty(vec![
            named_ty("String"),
            named_ty("Int"),
            named_ty("Bool"),
        ])),
        body: Block {
            stmts: vec![Stmt::Return(
                Some(tuple_value(vec![
                    str_lit("X"),
                    int_lit(7),
                    Expr::Literal(Literal::Bool(true), span()),
                ])),
                span(),
            )],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    });

    let src = generate_rust(&[decl]).expect("tuple codegen must succeed");

    assert!(
        src.contains("fn triple() -> (String, i64, bool)"),
        "expected 3-element tuple return type in:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn tuples_codegen_tuple_as_param_type() {
    // `func take(t: (Int, Int)) -> Int { return 0 }`
    let decl = Decl::FuncDecl(FuncDecl {
        name: ident("take"),
        params: vec![Param {
            name: ident("t"),
            ty: tuple_ty(vec![named_ty("Int"), named_ty("Int")]),
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: Some(named_ty("Int")),
        body: Block {
            stmts: vec![Stmt::Return(Some(int_lit(0)), span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    });

    let src = generate_rust(&[decl]).expect("tuple codegen must succeed");

    assert!(
        src.contains("fn take(t: (i64, i64)) -> i64"),
        "expected tuple param type in:\n{src}"
    );
    must_reparse(&src);
}
