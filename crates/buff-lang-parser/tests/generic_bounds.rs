//! T38 parser tests — generic trait bounds `<T: Bound>` on type parameters.
//!
//! Verifies the parser accepts the `: Bound (+ Bound)*` syntax after a generic
//! parameter name (on funcs, structs, and enums) and populates
//! [`TypeParam::bounds`]. The codegen lowering (bounds → Rust `<T: Clone>`)
//! is exercised by the codegen crate's own snapshot tests; these tests lock
//! the AST shape so a parser regression is caught immediately.

use buff_lang_ast::{Decl, TypeParam, TypeRef};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

/// Parse `src` and return the first decl's type_params (works for
/// FuncDecl / StructDecl / EnumDecl — all three carry `type_params`).
fn first_type_params(src: &str) -> Vec<TypeParam> {
    let sid = SourceId(7);
    let toks = tokenize(src, sid).expect("lexer should succeed");
    let decls = parse(&toks, sid).expect("parse should succeed");
    match decls.first().expect("at least one decl") {
        Decl::FuncDecl(f) => f.type_params.clone(),
        Decl::StructDecl(s) => s.type_params.clone(),
        Decl::EnumDecl(e) => e.type_params.clone(),
        other => panic!("expected FuncDecl/StructDecl/EnumDecl, got {other}"),
    }
}

/// Extract the bound names (as `&str`) from a TypeParam, asserting each bound
/// is a bare `TypeRef::Named` (the common case). Returns `Vec<&str>`.
fn bound_names(tp: &TypeParam) -> Vec<&str> {
    tp.bounds
        .iter()
        .map(|b| match b {
            TypeRef::Named { name, .. } => name.name.as_str(),
            other => panic!("expected Named bound, got {other}"),
        })
        .collect()
}

#[test]
fn t38_func_single_bound_parses() {
    // `func id<T: Clone>(x: T) -> T` — T carries a single Clone bound.
    let tps = first_type_params("func id<T: Clone>(x: T) -> T:\n    return x");
    assert_eq!(tps.len(), 1, "one type param");
    assert_eq!(tps[0].name.name, "T");
    assert_eq!(bound_names(&tps[0]), vec!["Clone"]);
}

#[test]
fn t38_func_multiple_bounds_parse() {
    // `T: Clone + Debug` — two `+`-separated bounds.
    let tps = first_type_params("func f<T: Clone + Debug>(x: T) -> T:\n    return x");
    assert_eq!(tps.len(), 1);
    assert_eq!(bound_names(&tps[0]), vec!["Clone", "Debug"]);
}

#[test]
fn t38_func_mixed_bounded_and_unbounded_params() {
    // `<T: Clone, U>` — T is bounded, U is not (bounds empty).
    let tps = first_type_params("func f<T: Clone, U>(x: T, y: U) -> T:\n    return x");
    assert_eq!(tps.len(), 2);
    assert_eq!(tps[0].name.name, "T");
    assert_eq!(bound_names(&tps[0]), vec!["Clone"]);
    assert_eq!(tps[1].name.name, "U");
    assert!(tps[1].bounds.is_empty(), "U has no bounds");
}

#[test]
fn t38_func_no_bounds_backward_compatible() {
    // `<T, U>` with no bounds — the T13 shape must still parse (bounds empty).
    let tps = first_type_params("func f<T, U>(x: T, y: U) -> T:\n    return x");
    assert_eq!(tps.len(), 2);
    assert!(tps.iter().all(|tp| tp.bounds.is_empty()));
}

#[test]
fn t38_struct_with_bounds_parses() {
    // `struct Pair<T: Clone + Default>` — struct generic params carry bounds.
    // Brace form (matches enum_match.rs test style; layout form also works
    // but brace form avoids offside-rule token subtleties in unit tests).
    let tps = first_type_params("struct Pair<T: Clone + Default> { a: T, b: T }");
    assert_eq!(tps.len(), 1);
    assert_eq!(bound_names(&tps[0]), vec!["Clone", "Default"]);
}

#[test]
fn t38_enum_with_bounds_parses() {
    // `enum Opt<T: Clone>` — enum generic params carry bounds (brace form).
    let tps = first_type_params("enum Opt<T: Clone> { Some(T), None }");
    assert_eq!(tps.len(), 1);
    assert_eq!(bound_names(&tps[0]), vec!["Clone"]);
}

#[test]
fn t38_generic_bound_parses_as_generic_typeref() {
    // `T: Container<Int>` — a bound with its own generic arguments is parsed
    // as a `TypeRef::Generic` (not a bare Named). This locks the shape so
    // the codegen's `typeref_to_trait_path` Generic arm is covered.
    //
    // NOTE: a space separates the bound's closing `>` from the param list's
    // closing `>` (`<T: Container<Int> >`). The lexer combines adjacent `>>`
    // into a single `Shr` token (the classic nested-generics limitation),
    // which is a pre-existing parser constraint unrelated to T38 bounds.
    // (Associated-type bounds like `Iterator<Item=T>` are also unsupported —
    // Buff generic args are plain types, not key=value bindings.)
    let tps = first_type_params("func f<T: Container<Int> >(x: T) -> Int:\n    return 0");
    assert_eq!(tps.len(), 1);
    assert_eq!(tps[0].bounds.len(), 1, "one bound");
    match &tps[0].bounds[0] {
        TypeRef::Generic { base, args, .. } => {
            assert!(
                matches!(base.as_ref(), TypeRef::Named { name, .. } if name.name == "Container"),
                "base should be Container"
            );
            assert_eq!(args.len(), 1, "one generic arg");
            assert!(
                matches!(&args[0], TypeRef::Named { name, .. } if name.name == "Int"),
                "arg should be Int"
            );
        }
        other => panic!("expected Generic bound, got {other}"),
    }
}
