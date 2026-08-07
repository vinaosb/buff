//! BUG-10 integration tests — Parser support for inferred (unannotated) params.
//!
//! Buff's README promises "Statically typed with aggressive inference — types
//! rarely written". Before this fix, `parse_params` REQUIRED every parameter
//! to carry an explicit `: Type` annotation, directly contradicting that claim.
//!
//! These tests verify that function parameters WITHOUT a `: Type` annotation
//! now parse successfully. The missing type is represented internally as a
//! placeholder `TypeRef::Named { name: "_" }` (the conventional wildcard),
//! which downstream maps to `Type::Unknown` (handled permissively by the type
//! inferencer).
//!
//! Coverage:
//! 1. Two untyped params: `func foo(x, y) -> Int: return x + y`
//! 2. Mixed typed + untyped: `func foo(x: Int, y) -> Int: return x + y`
//! 3. Single untyped param, no return type: `func foo(x): print(x)`
//! 4. Zero params (backward compat): `func foo(): print("hello")`
//! 5. Single typed param (backward compat): `func foo(x: Int): print(x)`
//! 6. `self` receiver still works (T75 bare-self special case).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser inferred_params
//! ```

use buff_lang_ast::{FuncDecl, Param, TypeRef};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse_func_decl, TokenStream};

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse a function declaration. The source must start with `func`.
fn parse_func(src: &str) -> FuncDecl {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_func_decl(&mut stream, Vec::new()).expect("parser should succeed")
}

/// Find a param by name in a func decl's param list (panics if absent).
fn param<'a>(f: &'a FuncDecl, name: &str) -> &'a Param {
    f.params
        .iter()
        .find(|p| p.name.name == name)
        .unwrap_or_else(|| panic!("no param named `{name}` in func `{}`", f.name.name))
}

/// Assert the param's type is the inferred placeholder `TypeRef::Named { "_" }`.
fn assert_inferred(p: &Param) {
    assert!(
        matches!(&p.ty, TypeRef::Named { name, .. } if name.name == "_"),
        "param `{}` should carry the inferred placeholder `TypeRef::Named {{ \"_\" }}`, \
         got {:?}",
        p.name.name,
        p.ty,
    );
}

// ---------------------------------------------------------------------------
// 1. Two untyped params: `func foo(x, y) -> Int: return x + y`
// ---------------------------------------------------------------------------

#[test]
fn inferred_params_two_untyped() {
    let f = parse_func("func foo(x, y) -> Int:\n    return x + y");
    assert_eq!(f.params.len(), 2, "expected two params");
    assert_eq!(f.params[0].name.name, "x");
    assert_eq!(f.params[1].name.name, "y");
    assert_inferred(&f.params[0]);
    assert_inferred(&f.params[1]);
    // Return type is still parsed normally.
    assert!(
        matches!(&f.return_type, Some(TypeRef::Named { name, .. }) if name.name == "Int"),
        "return type should be Int, got {:?}",
        f.return_type,
    );
}

// ---------------------------------------------------------------------------
// 2. Mixed typed + untyped: `func foo(x: Int, y) -> Int: return x + y`
// ---------------------------------------------------------------------------

#[test]
fn inferred_params_mixed_typed_and_untyped() {
    let f = parse_func("func foo(x: Int, y) -> Int:\n    return x + y");
    assert_eq!(f.params.len(), 2);
    // x is explicitly typed Int.
    let x = param(&f, "x");
    assert!(
        matches!(&x.ty, TypeRef::Named { name, .. } if name.name == "Int"),
        "`x` should carry explicit Int, got {:?}",
        x.ty,
    );
    // y is inferred.
    let y = param(&f, "y");
    assert_inferred(y);
}

// ---------------------------------------------------------------------------
// 3. Single untyped param, no return type: `func foo(x): print(x)`
// ---------------------------------------------------------------------------

#[test]
fn inferred_params_single_untyped_no_return() {
    let f = parse_func("func foo(x):\n    print(x)");
    assert_eq!(f.params.len(), 1);
    assert_inferred(&f.params[0]);
    assert!(f.return_type.is_none(), "no return type expected");
}

// ---------------------------------------------------------------------------
// 4. Zero params (backward compat): `func foo(): print("hello")`
// ---------------------------------------------------------------------------

#[test]
fn inferred_params_zero_params_backward_compat() {
    let f = parse_func("func foo():\n    print(\"hello\")");
    assert!(f.params.is_empty(), "zero-param func should parse cleanly");
}

// ---------------------------------------------------------------------------
// 5. Single typed param (backward compat): `func foo(x: Int): print(x)`
// ---------------------------------------------------------------------------

#[test]
fn inferred_params_single_typed_backward_compat() {
    let f = parse_func("func foo(x: Int):\n    print(x)");
    assert_eq!(f.params.len(), 1);
    let x = &f.params[0];
    assert!(
        matches!(&x.ty, TypeRef::Named { name, .. } if name.name == "Int"),
        "explicitly-typed `x` should carry Int, got {:?}",
        x.ty,
    );
}

// ---------------------------------------------------------------------------
// 6. `self` receiver still works (T75 bare-self special case).
//    `self` (no `: Type`) must keep its `Self` placeholder, NOT the new `_`.
// ---------------------------------------------------------------------------

#[test]
fn inferred_params_self_receiver_unchanged() {
    // The bare `self` special case (T75) must still produce `Self`, not the
    // new inferred placeholder. This guards against the fix accidentally
    // rewriting the self-receiver arm.
    let f = parse_func("func greet(self):\n    return 0");
    assert_eq!(f.params.len(), 1, "expected one param");
    let s = &f.params[0];
    assert_eq!(s.name.name, "self", "param should be named `self`");
    assert!(
        matches!(&s.ty, TypeRef::Named { name, .. } if name.name == "Self"),
        "bare `self` receiver should carry the `Self` placeholder (T75), got {:?}",
        s.ty,
    );
}

// ---------------------------------------------------------------------------
// 7. Trailing comma with untyped params still works.
// ---------------------------------------------------------------------------

#[test]
fn inferred_params_trailing_comma() {
    let f = parse_func("func foo(x, y,):\n    return 0");
    assert_eq!(f.params.len(), 2, "trailing comma should be allowed");
    assert_inferred(&f.params[0]);
    assert_inferred(&f.params[1]);
}

// ---------------------------------------------------------------------------
// 8. Untyped param WITH a default value: `func foo(x = 10)`
//    The default-value path is independent of the type-annotation path.
// ---------------------------------------------------------------------------

#[test]
fn inferred_params_untyped_with_default() {
    let f = parse_func("func foo(x = 10):\n    return 0");
    assert_eq!(f.params.len(), 1);
    let x = &f.params[0];
    assert_inferred(x);
    assert!(
        x.default_value.is_some(),
        "untyped param should still accept a default value",
    );
}
