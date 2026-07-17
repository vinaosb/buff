//! T103 integration tests — Tuple types `(String, Int)` and tuple values
//! `("A", 42)`.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-types tuples
//! ```
//!
//! These tests exercise:
//! - `TypeRef::Tuple` resolution to `Type::Tuple` via a `let` annotation.
//! - `Expr::TupleLit` inference to `Type::Tuple` via `infer_expr`.
//! - The 2+-element disambiguation: `(T)` is grouping (NOT a tuple); `(T, U)`
//!   is a tuple. (The TYPE side; the parser produces a bare `T` for `(T)`, so
//!   the type-system layer never sees a single-element `TypeRef::Tuple`.)
//! - The display form: `Type::Tuple([String, Int<64>])` → `(String, Int<64>)`.

use buff_lang_ast::{Expr, Ident, Literal, Stmt, TypeRef};
use buff_lang_error::Span;
use buff_lang_types::{Type, TypeInferencer};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn sp() -> Span {
    Span::dummy()
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: Ident::new(name, sp()),
        span: sp(),
    }
}

fn tuple_type(members: Vec<TypeRef>) -> TypeRef {
    TypeRef::Tuple(members, sp())
}

fn ident(name: &str) -> Expr {
    Expr::Ident(Ident::new(name, sp()), sp())
}

fn str_lit(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), sp())
}

fn int_lit(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), sp())
}

fn bool_lit(b: bool) -> Expr {
    Expr::Literal(Literal::Bool(b), sp())
}

fn tuple_value(members: Vec<Expr>) -> Expr {
    Expr::TupleLit(members, sp())
}

/// Build a `let value = <expr>` stmt with the given `: Type` annotation.
fn let_decl(name: &str, value: Expr, ty: TypeRef) -> Stmt {
    Stmt::LetDecl {
        name: Ident::new(name, sp()),
        value,
        mutable: false,
        ty: Some(ty),
        span: sp(),
    }
}

// ---------------------------------------------------------------------------
// Type side: `(String, Int)` resolves to `Type::Tuple([String, Int<64>])`
// ---------------------------------------------------------------------------

#[test]
fn tuples_type_two_members() {
    // `let value: (String, Int) = input`
    // with `input: (String, Int<64>)` bound in the env.
    let annotated = tuple_type(vec![named_type("String"), named_type("Int")]);
    let expected = Type::Tuple(vec![Type::string(), Type::int_default()]);

    let mut inf = TypeInferencer::new();
    inf.bind("input", expected.clone());

    let stmt = let_decl("value", ident("input"), annotated);

    assert_eq!(inf.infer_stmt(&stmt).unwrap(), expected.clone());
    assert_eq!(inf.lookup("value"), Some(&expected));
}

#[test]
fn tuples_type_three_members() {
    // `let value: (String, Int, Bool) = input`
    let annotated = tuple_type(vec![
        named_type("String"),
        named_type("Int"),
        named_type("Bool"),
    ]);
    let expected = Type::Tuple(vec![Type::string(), Type::int_default(), Type::bool()]);

    let mut inf = TypeInferencer::new();
    inf.bind("input", expected.clone());

    let stmt = let_decl("value", ident("input"), annotated);

    assert_eq!(inf.infer_stmt(&stmt).unwrap(), expected.clone());
    assert_eq!(inf.lookup("value"), Some(&expected));
}

#[test]
fn tuples_type_nested_member_resolves_recursively() {
    // `let value: (String, (Int, Bool)) = input`
    let inner = tuple_type(vec![named_type("Int"), named_type("Bool")]);
    let annotated = tuple_type(vec![named_type("String"), inner]);
    let expected = Type::Tuple(vec![
        Type::string(),
        Type::Tuple(vec![Type::int_default(), Type::bool()]),
    ]);

    let mut inf = TypeInferencer::new();
    inf.bind("input", expected.clone());

    let stmt = let_decl("value", ident("input"), annotated);

    assert_eq!(inf.infer_stmt(&stmt).unwrap(), expected.clone());
    assert_eq!(inf.lookup("value"), Some(&expected));
}

#[test]
fn tuples_type_unknown_member_becomes_unknown() {
    // `let value: (String, Mystery) = input` — "Mystery" is unresolvable; it
    // falls back to Unknown so the Tuple wrapper still flows through.
    let annotated = tuple_type(vec![named_type("String"), named_type("Mystery")]);
    let expected = Type::Tuple(vec![Type::string(), Type::Unknown]);

    let mut inf = TypeInferencer::new();
    inf.bind("input", expected.clone());

    let stmt = let_decl("value", ident("input"), annotated);

    assert_eq!(inf.infer_stmt(&stmt).unwrap(), expected.clone());
    assert_eq!(inf.lookup("value"), Some(&expected));
}

// ---------------------------------------------------------------------------
// Value side: `("A", 42)` infers to `Type::Tuple([String, Int<64>])`
// ---------------------------------------------------------------------------

#[test]
fn tuples_value() {
    // `("A", 42)` -> Tuple([String, Int<64>])
    let e = tuple_value(vec![str_lit("A"), int_lit(42)]);
    let expected = Type::Tuple(vec![Type::string(), Type::int_default()]);

    let mut inf = TypeInferencer::new();
    assert_eq!(inf.infer_expr(&e).unwrap(), expected);
}

#[test]
fn tuples_value_three_members() {
    // `("A", 42, true)` -> Tuple([String, Int<64>, Bool])
    let e = tuple_value(vec![str_lit("A"), int_lit(42), bool_lit(true)]);
    let expected = Type::Tuple(vec![Type::string(), Type::int_default(), Type::bool()]);

    let mut inf = TypeInferencer::new();
    assert_eq!(inf.infer_expr(&e).unwrap(), expected);
}

#[test]
fn tuples_value_nested_tuple_member() {
    // `("A", (1, true))` -> Tuple([String, Tuple([Int<64>, Bool])])
    let inner = tuple_value(vec![int_lit(1), bool_lit(true)]);
    let e = tuple_value(vec![str_lit("A"), inner]);
    let expected = Type::Tuple(vec![
        Type::string(),
        Type::Tuple(vec![Type::int_default(), Type::bool()]),
    ]);

    let mut inf = TypeInferencer::new();
    assert_eq!(inf.infer_expr(&e).unwrap(), expected);
}

// ---------------------------------------------------------------------------
// Return-type acceptance: a function returning `(String, Int)` carrying a
// tuple literal flows through (the `let` annotation path mirrors how a
// return-type annotation would pin the type).
// ---------------------------------------------------------------------------

#[test]
fn tuples_return_type() {
    // `let p: (String, Int) = ("A", 42)`
    // (Mirrors `func pair() -> (String, Int) { return ("A", 42) }` at the
    // type-system layer — the let-annotation path exercises the same
    // typeref_to_type + assignable_to machinery a return type would.)
    let annotated = tuple_type(vec![named_type("String"), named_type("Int")]);
    let value = tuple_value(vec![str_lit("A"), int_lit(42)]);
    let expected = Type::Tuple(vec![Type::string(), Type::int_default()]);

    let mut inf = TypeInferencer::new();
    let stmt = let_decl("p", value, annotated);

    assert_eq!(inf.infer_stmt(&stmt).unwrap(), expected.clone());
    assert_eq!(inf.lookup("p"), Some(&expected));
}

// ---------------------------------------------------------------------------
// Display form: `Type::Tuple([String, Int<64>])` -> "(String, Int<64>)"
// ---------------------------------------------------------------------------

#[test]
fn tuples_display_formats_with_parens_and_commas() {
    let ty = Type::Tuple(vec![Type::string(), Type::int_default()]);
    assert_eq!(format!("{ty}"), "(String, Int<64>)");
}

#[test]
fn tuples_display_three_members() {
    let ty = Type::Tuple(vec![Type::string(), Type::int_default(), Type::bool()]);
    assert_eq!(format!("{ty}"), "(String, Int<64>, Bool)");
}
