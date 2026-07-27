//! Integration tests for `buff-fuzz` — property-based testing behavior.
//!
//! Drives real property assertions through the runner to verify the
//! proptest-backed loop produces correct pass/fail summaries. These
//! are the QA harness for the T27 acceptance scenarios "Passing
//! property holds across many iterations" / "Failing property records
//! counter-examples" / "Lowering helper produces valid syn::Item".

use buff_fuzz::{lower_fuzz_harness, run, FuzzError, Strategy};
use buff_lang_ast::{Block, FuncDecl, Ident, Param, Span, TypeRef};

fn dummy_span() -> Span {
    Span::dummy()
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: Ident::new(name, dummy_span()),
        span: dummy_span(),
    }
}

fn mk_param(name: &str, ty: &str) -> Param {
    Param::plain(name, named_type(ty), dummy_span())
}

fn mk_fuzz_func(name: &str, param_ty: &str, param_count: usize) -> FuncDecl {
    let params: Vec<Param> = (0..param_count)
        .map(|i| mk_param(&format!("p{i}"), param_ty))
        .collect();
    FuncDecl {
        name: Ident::new(name, dummy_span()),
        params,
        return_type: Some(named_type("Bool")),
        body: Block {
            stmts: Vec::new(),
            span: dummy_span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: dummy_span(),
    }
}

#[test]
fn passing_property_holds_for_int_range_invariant() {
    let s = Strategy::int(0, 100);
    let summary = run(&s, 256, |n| n >= 0 && n <= 100).expect("valid run");
    assert!(summary.passed());
    assert_eq!(summary.iterations, 256);
}

#[test]
fn passing_property_holds_for_square_non_negative() {
    let s = Strategy::int(0, 1000);
    let summary = run(&s, 256, |n| n * n >= 0).expect("valid run");
    assert!(summary.passed());
}

#[test]
fn failing_property_records_counter_examples() {
    let s = Strategy::int(0, 100);
    let summary = run(&s, 256, |n| n < 50).expect("valid run");
    assert!(!summary.passed());
    assert!(summary.failed_count() > 0);
    for value in &summary.failures {
        assert!(*value >= 50);
    }
}

#[test]
fn failing_property_caps_recorded_failures_at_sixteen() {
    let s = Strategy::int(0, 100);
    let summary = run(&s, 512, |n| n < 50).expect("valid run");
    assert!(summary.failed_count() <= 16);
}

#[test]
fn bool_strategy_generates_zero_or_one() {
    let s = Strategy::bool();
    let summary = run(&s, 64, |n| n == 0 || n == 1).expect("valid run");
    assert!(summary.passed());
}

#[test]
fn string_strategy_generates_lengths_in_range() {
    let s = Strategy::string(64);
    let summary = run(&s, 64, |len| len >= 0 && len <= 64).expect("valid run");
    assert!(summary.passed());
}

#[test]
fn bytes_strategy_generates_lengths_in_range() {
    let s = Strategy::bytes(128);
    let summary = run(&s, 64, |len| len >= 0 && len <= 128).expect("valid run");
    assert!(summary.passed());
}

#[test]
fn lower_fuzz_harness_emits_valid_fn_for_int_param() {
    let func = mk_fuzz_func("parse_property", "Int", 1);
    let item = lower_fuzz_harness(&func).expect("lowering should succeed");
    let file = syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![item],
    };
    let source = prettyplease::unparse(&file);
    assert!(source.contains("fn parse_property"));
    assert!(source.contains("buff_fuzz::Strategy::int"));
    assert!(source.contains("buff_fuzz::run"));
    assert!(source.contains("let strategy"));
    assert!(source.contains("let summary"));
}

#[test]
fn lower_fuzz_harness_includes_param_name_in_closure() {
    let func = mk_fuzz_func("my_target", "Int", 1);
    let item = lower_fuzz_harness(&func).expect("lowering should succeed");
    let file = syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![item],
    };
    let source = prettyplease::unparse(&file);
    assert!(source.contains("p0"));
}

#[test]
fn lower_fuzz_harness_rejects_zero_params() {
    let func = mk_fuzz_func("no_params", "Int", 0);
    let err = lower_fuzz_harness(&func).expect_err("zero params should fail");
    assert!(matches!(err, FuzzError::LoweringFailed { .. }));
}

#[test]
fn lower_fuzz_harness_rejects_two_params() {
    let func = mk_fuzz_func("two_params", "Int", 2);
    let err = lower_fuzz_harness(&func).expect_err("two params should fail");
    assert!(matches!(err, FuzzError::LoweringFailed { .. }));
}

#[test]
fn lower_fuzz_harness_rejects_float_param() {
    let func = mk_fuzz_func("float_target", "Float", 1);
    let err = lower_fuzz_harness(&func).expect_err("Float param should fail");
    assert!(matches!(err, FuzzError::LoweringFailed { .. }));
}

#[test]
fn lower_fuzz_harness_rejects_string_param() {
    let func = mk_fuzz_func("string_target", "String", 1);
    let err = lower_fuzz_harness(&func).expect_err("String param should fail");
    assert!(matches!(err, FuzzError::LoweringFailed { .. }));
}

#[test]
fn lower_fuzz_harness_snapshot_for_canonical_input() {
    let func = mk_fuzz_func("canonical_target", "Int", 1);
    let item = lower_fuzz_harness(&func).expect("lowering should succeed");
    let file = syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![item],
    };
    let source = prettyplease::unparse(&file);
    insta::assert_snapshot!(source);
}
