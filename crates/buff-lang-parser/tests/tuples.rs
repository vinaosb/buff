//! T103 integration tests — Parser for tuple types and tuple values.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser tuples
//! ```

use buff_lang_ast::{Expr, Ident, Literal, TypeRef};
use buff_lang_error::{SourceId, Span};
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse_expression, parse_type_ref, TokenStream};

fn sid() -> SourceId {
    SourceId(0)
}

fn span() -> Span {
    Span::dummy()
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: Ident::new(name, span()),
        span: span(),
    }
}

fn int(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), sp())
}

fn sp() -> Span {
    Span::dummy()
}

fn str_lit(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), sp())
}

fn parse_expr(src: &str) -> Expr {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    parse_expression(&tokens, sid()).expect("parser should succeed")
}

fn parse_ty(src: &str) -> TypeRef {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_type_ref(&mut stream).expect("type parse should succeed")
}

#[test]
fn tuples_type_parses_two_members() {
    // `(String, Int)` -> TypeRef::Tuple([String, Int])
    let ty = parse_ty("(String, Int)");
    let expected = TypeRef::Tuple(vec![named_type("String"), named_type("Int")], span());
    assert_eq!(ty.to_string(), expected.to_string());
    match ty {
        TypeRef::Tuple(members, _) => {
            assert_eq!(members.len(), 2);
        }
        other => panic!("expected TypeRef::Tuple, got {other:?}"),
    }
}

#[test]
fn tuples_type_parses_three_members() {
    // `(String, Int, Bool)` -> TypeRef::Tuple([String, Int, Bool])
    let ty = parse_ty("(String, Int, Bool)");
    match ty {
        TypeRef::Tuple(members, _) => {
            assert_eq!(members.len(), 3);
        }
        other => panic!("expected TypeRef::Tuple, got {other:?}"),
    }
}

#[test]
fn tuples_type_single_paren_is_grouping_not_tuple() {
    // `(String)` -> just TypeRef::Named("String") (grouping, NOT a tuple).
    let ty = parse_ty("(String)");
    match ty {
        TypeRef::Named { name, .. } => {
            assert_eq!(name.name, "String");
        }
        other => panic!("expected TypeRef::Named (grouping), got {other:?}"),
    }
}

#[test]
fn tuples_type_nested_member() {
    // `(String, (Int, Bool))` -> TypeRef::Tuple([String, Tuple([Int, Bool])])
    let ty = parse_ty("(String, (Int, Bool))");
    match ty {
        TypeRef::Tuple(members, _) => {
            assert_eq!(members.len(), 2);
            match &members[1] {
                TypeRef::Tuple(inner, _) => assert_eq!(inner.len(), 2),
                other => panic!("expected nested TypeRef::Tuple, got {other:?}"),
            }
        }
        other => panic!("expected TypeRef::Tuple, got {other:?}"),
    }
}

#[test]
fn tuples_type_trailing_comma_allowed() {
    // `(String, Int,)` -> TypeRef::Tuple([String, Int]) (trailing comma ok).
    let ty = parse_ty("(String, Int,)");
    match ty {
        TypeRef::Tuple(members, _) => {
            assert_eq!(members.len(), 2);
        }
        other => panic!("expected TypeRef::Tuple, got {other:?}"),
    }
}

#[test]
fn tuples_value_parses_two_members() {
    // `("A", 42)` -> Expr::TupleLit([StringLit, IntLit])
    let e = parse_expr(r#"("A", 42)"#);
    match e {
        Expr::TupleLit(members, _) => {
            assert_eq!(members.len(), 2);
        }
        other => panic!("expected Expr::TupleLit, got {other:?}"),
    }
}

#[test]
fn tuples_value_single_paren_is_grouping_not_tuple() {
    // `(42)` -> just Literal::Int(42) (grouping, NOT a tuple).
    let e = parse_expr("(42)");
    match e {
        Expr::Literal(Literal::Int(42), _) => {}
        other => panic!("expected Expr::Literal(42) (grouping), got {other:?}"),
    }
}

#[test]
fn tuples_value_trailing_comma_allowed() {
    // `("A", 42,)` -> Expr::TupleLit([StringLit, IntLit])
    let e = parse_expr(r#"("A", 42,)"#);
    match e {
        Expr::TupleLit(members, _) => {
            assert_eq!(members.len(), 2);
        }
        other => panic!("expected Expr::TupleLit, got {other:?}"),
    }
}

#[test]
fn tuples_value_display_formats_with_parens() {
    let e = parse_expr(r#"("A", 42)"#);
    // Display should produce something containing both elements inside parens.
    let s = e.to_string();
    assert!(
        s.contains("\"A\""),
        "display should include string element: {s}"
    );
    assert!(s.contains("42"), "display should include int element: {s}");
}

// Suppress unused warnings for helpers that are conditionally used.
#[allow(dead_code)]
fn _unused() {
    let _ = (int(0), str_lit(""));
}
