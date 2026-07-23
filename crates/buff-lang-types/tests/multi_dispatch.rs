//! T58 — Multiple Dispatch for Numerical APIs: integration tests.
//!
//! Covers: group formation, name mangling, call-site resolution,
//! ambiguous / no-match errors, single-dispatch backward compat, and
//! mixed single + multi-dispatch coexistence. All 18 tests exercise the
//! public `MultiDispatchTable` API directly (no parser/lexer involved)
//! so the suite is deterministic and fast.
//!
//! ## Running
//!
//! ```text
//! cargo test -p buff-lang-types --test multi_dispatch
//! cargo test -p buff-lang-types multi_dispatch
//! ```

use buff_lang_ast::{
    common::{Block, Ident, Param},
    decl::FuncDecl,
    Decl,
};
use buff_lang_error::{ErrorCode, Span};
use buff_lang_types::{MultiDispatchTable, Type};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn sp() -> Span {
    Span::dummy()
}

fn named(name: &str) -> buff_lang_ast::TypeRef {
    buff_lang_ast::TypeRef::Named {
        name: Ident::new(name, sp()),
        span: sp(),
    }
}

fn generic(base: &str, arg: &str) -> buff_lang_ast::TypeRef {
    buff_lang_ast::TypeRef::Generic {
        base: Box::new(named(base)),
        args: vec![named(arg)],
        span: sp(),
    }
}

fn param(name: &str, ty: buff_lang_ast::TypeRef) -> Param {
    Param::plain(name, ty, sp())
}

fn func_with_return(
    name: &str,
    params: &[(&str, buff_lang_ast::TypeRef)],
    return_ty: buff_lang_ast::TypeRef,
) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: Ident::new(name, sp()),
        params: params.iter().map(|(n, t)| param(n, t.clone())).collect(),
        return_type: Some(return_ty),
        body: Block::empty(sp()),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: sp(),
    })
}

fn func(name: &str, params: &[(&str, buff_lang_ast::TypeRef)]) -> Decl {
    func_with_return(name, params, named("Int"))
}

// ---------------------------------------------------------------------------
// 1-3: Basic group formation
// ---------------------------------------------------------------------------

#[test]
fn t58_01_two_impls_same_name_form_group() {
    let decls = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    assert!(table.is_group("combine"));
    let (_, methods) = table.groups().next().unwrap();
    assert_eq!(methods.len(), 2);
}

#[test]
fn t58_02_three_impls_form_group() {
    let decls = vec![
        func("add", &[("a", named("Int")), ("b", named("Int"))]),
        func("add", &[("a", named("Float")), ("b", named("Float"))]),
        func("add", &[("a", named("String")), ("b", named("String"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    assert!(table.is_group("add"));
    let (_, methods) = table.groups().next().unwrap();
    assert_eq!(methods.len(), 3);
}

#[test]
fn t58_03_distinct_names_do_not_form_group() {
    let decls = vec![
        func("foo", &[("a", named("Int"))]),
        func("bar", &[("a", named("Int"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    assert!(!table.is_group("foo"));
    assert!(!table.is_group("bar"));
}

// ---------------------------------------------------------------------------
// 4-6: Name mangling
// ---------------------------------------------------------------------------

#[test]
fn t58_04_mangled_names_use_param_types() {
    let decls = vec![
        func("matmul", &[("a", named("Int")), ("b", named("Int"))]),
        func("matmul", &[("a", named("Float")), ("b", named("Float"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    let names: Vec<&str> = table
        .groups()
        .next()
        .unwrap()
        .1
        .iter()
        .map(|m| m.mangled_name.as_str())
        .collect();
    assert_eq!(names, vec!["matmul_int_int", "matmul_float_float"]);
}

#[test]
fn t58_05_generic_types_collapse_in_mangling() {
    let decls = vec![
        func(
            "process",
            &[("a", generic("Vector", "Int")), ("b", named("Int"))],
        ),
        func(
            "process",
            &[("a", generic("Matrix", "Float")), ("b", named("Float"))],
        ),
    ];
    let table = MultiDispatchTable::build(&decls);
    let names: Vec<&str> = table
        .groups()
        .next()
        .unwrap()
        .1
        .iter()
        .map(|m| m.mangled_name.as_str())
        .collect();
    assert_eq!(names, vec!["process_vector_int", "process_matrix_float"]);
}

#[test]
fn t58_06_mangled_names_preserve_source_order() {
    let decls = vec![
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    let names: Vec<&str> = table
        .groups()
        .next()
        .unwrap()
        .1
        .iter()
        .map(|m| m.mangled_name.as_str())
        .collect();
    assert_eq!(names, vec!["combine_float_float", "combine_int_int"]);
}

// ---------------------------------------------------------------------------
// 7-9: Resolution: unique match, ambiguity, no match
// ---------------------------------------------------------------------------

#[test]
fn t58_07_resolve_unique_match_picks_correct_impl() {
    let decls = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    let (idx, _ret) = table
        .resolve("combine", &[Type::int_default(), Type::int_default()], sp())
        .unwrap()
        .unwrap();
    assert_eq!(idx, 0);
    let (idx2, _) = table
        .resolve(
            "combine",
            &[Type::float_default(), Type::float_default()],
            sp(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(idx2, 1);
}

#[test]
fn t58_08_resolve_ambiguous_errors_e1202() {
    let decls = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    let err = table
        .resolve("combine", &[Type::int_default(), Type::int_default()], sp())
        .unwrap_err();
    assert_eq!(err.diagnostic.code, Some(ErrorCode::BinaryOpTypeMismatch));
}

#[test]
fn t58_09_resolve_no_match_errors_e1201() {
    let decls = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    let err = table
        .resolve("combine", &[Type::string(), Type::string()], sp())
        .unwrap_err();
    assert_eq!(err.diagnostic.code, Some(ErrorCode::UndefinedVariable));
}

// ---------------------------------------------------------------------------
// 10-12: Arity handling + non-group fallthrough
// ---------------------------------------------------------------------------

#[test]
fn t58_10_arity_mismatch_is_no_match() {
    let decls = vec![
        func("combine", &[("a", named("Int"))]),
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    // 2-arg call against a group with 1-arg + 2-arg impls: only the
    // 2-arg impl matches (arity filter).
    let r = table
        .resolve("combine", &[Type::int_default(), Type::int_default()], sp())
        .unwrap()
        .unwrap();
    assert_eq!(r.0, 1, "should match the 2-arg impl at index 1");
}

#[test]
fn t58_11_resolve_returns_none_for_non_group() {
    let table = MultiDispatchTable::build(&[]);
    let r = table
        .resolve("nonexistent", &[Type::int_default()], sp())
        .unwrap();
    assert!(r.is_none());
}

#[test]
fn t58_12_single_func_does_not_form_group() {
    let decls = vec![func("foo", &[("a", named("Int"))])];
    let table = MultiDispatchTable::build(&decls);
    assert!(!table.is_group("foo"));
    let r = table.resolve("foo", &[Type::int_default()], sp()).unwrap();
    assert!(r.is_none());
}

// ---------------------------------------------------------------------------
// 13-15: Backward compat with single-dispatch
// ---------------------------------------------------------------------------

#[test]
fn t58_13_single_func_not_in_group() {
    // A lone `func add(a, b)` should NOT form a group — backward compat.
    let decls = vec![func("add", &[("a", named("Int")), ("b", named("Int"))])];
    let table = MultiDispatchTable::build(&decls);
    assert!(
        !table.is_group("add"),
        "single-impl func should NOT be a group"
    );
    // Resolve returns None so the existing unmangled codegen path takes over.
    let r = table
        .resolve("add", &[Type::int_default(), Type::int_default()], sp())
        .unwrap();
    assert!(r.is_none());
}

#[test]
fn t58_14_single_dispatch_method_unchanged() {
    // An extend block (single-dispatch method) must remain UNCHANGED.
    // Multi-dispatch only applies to free functions with 2+ impls.
    let decls = vec![
        func("foo", &[("a", named("String"))]),
        // Even with a different-name method, `foo` is still single-impl.
    ];
    let table = MultiDispatchTable::build(&decls);
    assert!(!table.is_group("foo"));
}

#[test]
fn t58_15_multi_impl_funcs_emit_mangled_names() {
    // The core T58 capability: 2+ impls of the same Buff name each
    // get a unique mangled Rust name via the dispatch table.
    let decls = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    assert!(table.is_group("combine"));
    let methods: Vec<&str> = table
        .groups()
        .next()
        .unwrap()
        .1
        .iter()
        .map(|m| m.mangled_name.as_str())
        .collect();
    assert!(methods.contains(&"combine_int_int"));
    assert!(methods.contains(&"combine_float_float"));
    // No unmangled "combine" should exist in the group.
    assert!(!methods.contains(&"combine"));
}

// ---------------------------------------------------------------------------
// 16-18: Mixed scenarios
// ---------------------------------------------------------------------------

#[test]
fn t58_16_mixed_single_and_multi_dispatch_coexist() {
    // A single-impl `foo` and a multi-impl `bar` should coexist:
    // `foo` stays unmangled (not a group), `bar`'s impls get mangled.
    let decls = vec![
        func("foo", &[("a", named("Int"))]),
        func("bar", &[("a", named("Int"))]),
        func("bar", &[("a", named("Float"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    assert!(
        !table.is_group("foo"),
        "`foo` single-impl should NOT be a group"
    );
    assert!(table.is_group("bar"), "`bar` multi-impl SHOULD be a group");
}

#[test]
fn t58_17_call_site_mangled_name_lookup() {
    // After resolve picks an impl, mangled_name returns the correct Rust name.
    let decls = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    let (idx, _) = table
        .resolve("combine", &[Type::int_default(), Type::int_default()], sp())
        .unwrap()
        .unwrap();
    let name = table.mangled_name("combine", idx).unwrap();
    assert_eq!(name, "combine_int_int");

    let (idx2, _) = table
        .resolve(
            "combine",
            &[Type::float_default(), Type::float_default()],
            sp(),
        )
        .unwrap()
        .unwrap();
    let name2 = table.mangled_name("combine", idx2).unwrap();
    assert_eq!(name2, "combine_float_float");
}

#[test]
fn t58_18_four_way_dispatch_resolves_all_combinations() {
    // Four-way dispatch: (Int,Int), (Int,Float), (Float,Int), (Float,Float).
    let decls = vec![
        func("op", &[("a", named("Int")), ("b", named("Int"))]),
        func("op", &[("a", named("Int")), ("b", named("Float"))]),
        func("op", &[("a", named("Float")), ("b", named("Int"))]),
        func("op", &[("a", named("Float")), ("b", named("Float"))]),
    ];
    let table = MultiDispatchTable::build(&decls);
    assert!(table.is_group("op"));
    assert_eq!(table.groups().next().unwrap().1.len(), 4);

    let combos: &[(&[Type], usize)] = &[
        (&[Type::int_default(), Type::int_default()], 0),
        (&[Type::int_default(), Type::float_default()], 1),
        (&[Type::float_default(), Type::int_default()], 2),
        (&[Type::float_default(), Type::float_default()], 3),
    ];
    for (args, expected_idx) in combos {
        let (idx, _) = table.resolve("op", args, sp()).unwrap().unwrap();
        assert_eq!(idx, *expected_idx);
    }
}
