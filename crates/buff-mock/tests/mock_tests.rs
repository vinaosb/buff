//! Integration tests for `buff-mock` — runtime behavior + codegen helper.
//!
//! Exercises the public API end-to-end against a sample trait. These
//! are the QA harness for the T25 acceptance scenarios:
//! "Mock a trait and verify interaction" / "Verify detects unmet
//! expectations" / "Spy records call arguments".

use buff_lang_ast::Span;
use buff_lang_ast::{Ident, MethodSig, Param, TraitDecl};
use buff_mock::{lower_mock_for_trait, ArgumentValue, Mock, MockError, ReturnValue};

trait Greeter {
    fn greet(&self, name: String) -> String;
    fn ping(&self) -> bool;
    fn add(&self, a: i64, b: i64) -> i64;
}

impl Greeter for Mock<dyn Greeter> {
    fn greet(&self, name: String) -> String {
        self.record_call("greet", vec![ArgumentValue::String(name)]);
        match self.lookup_return("greet", &[]) {
            Some(ReturnValue::String(s)) => s,
            _ => String::new(),
        }
    }
    fn ping(&self) -> bool {
        self.record_call_no_args("ping");
        match self.lookup_return_no_args("ping") {
            Some(ReturnValue::Bool(b)) => b,
            _ => false,
        }
    }
    fn add(&self, a: i64, b: i64) -> i64 {
        self.record_call("add", vec![ArgumentValue::Int(a), ArgumentValue::Int(b)]);
        match self.lookup_return("add", &[]) {
            Some(ReturnValue::Int(i)) => i,
            _ => 0,
        }
    }
}

fn mock() -> Mock<dyn Greeter> {
    Mock::<dyn Greeter>::new()
}

fn dummy_span() -> Span {
    Span::dummy()
}

fn named(s: &str) -> buff_lang_ast::TypeRef {
    buff_lang_ast::TypeRef::Named {
        name: Ident::new(s, dummy_span()),
        span: dummy_span(),
    }
}

fn mk_param(name: &str, ty: &str) -> Param {
    Param::plain(name, named(ty), dummy_span())
}

fn mk_greeter_buff_trait() -> TraitDecl {
    TraitDecl {
        name: Ident::new("Greeter", dummy_span()),
        supertraits: Vec::new(),
        associated_types: Vec::new(),
        required: vec![
            MethodSig {
                name: Ident::new("greet", dummy_span()),
                params: vec![mk_param("name", "String")],
                return_type: Some(named("String")),
                span: dummy_span(),
            },
            MethodSig {
                name: Ident::new("ping", dummy_span()),
                params: Vec::new(),
                return_type: Some(named("Bool")),
                span: dummy_span(),
            },
            MethodSig {
                name: Ident::new("add", dummy_span()),
                params: vec![mk_param("a", "Int"), mk_param("b", "Int")],
                return_type: Some(named("Int")),
                span: dummy_span(),
            },
        ],
        defaults: Vec::new(),
        span: dummy_span(),
    }
}

// === Acceptance: hello mock (expect + returning + verify) =====================

#[test]
fn acceptance_mock_returns_expected_value_when_called() {
    let m = mock();
    m.expect("greet")
        .returning(ReturnValue::String("hello world".into()));
    let result = m.greet("buff".into());
    assert_eq!(result, "hello world");
}

#[test]
fn acceptance_mock_default_when_no_expectation() {
    let m = mock();
    let result = m.greet("anyone".into());
    assert_eq!(result, "");
}

#[test]
fn acceptance_verify_passes_when_expectations_met() {
    let m = mock();
    m.expect("greet")
        .returning(ReturnValue::String("hi".into()));
    let _ = m.greet("buff".into());
    assert!(m.verify().is_ok());
}

// === Acceptance: verify detects unmet expectations ===========================

#[test]
fn acceptance_verify_fails_when_under_called() {
    let m = mock();
    m.expect("greet").times(2);
    let _ = m.greet("buff".into());
    let err = m.verify().unwrap_err();
    match err {
        MockError::VerifyFailed(msg) => {
            assert!(msg.contains("expected exactly 2 calls"));
            assert!(msg.contains("got 1"));
            assert!(msg.contains("`greet`"));
        }
        _ => unreachable!("verify must return VerifyFailed when times is unsatisfied"),
    }
}

#[test]
fn acceptance_verify_fails_when_over_called_exact() {
    let m = mock();
    m.expect("greet").times(1);
    let _ = m.greet("a".into());
    let _ = m.greet("b".into());
    let err = m.verify().unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("got 2"));
}

#[test]
fn acceptance_verify_passes_with_at_least_zero() {
    let m = mock();
    m.expect("greet").at_least(0);
    assert!(m.verify().is_ok());
}

#[test]
fn acceptance_verify_passes_with_at_most_zero_when_no_calls() {
    let m = mock();
    m.expect("greet").at_most(0);
    assert!(m.verify().is_ok());
}

#[test]
fn acceptance_verify_fails_when_never_violated() {
    let m = mock();
    m.expect("greet").never();
    let _ = m.greet("a".into());
    let err = m.verify().unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("never"));
}

#[test]
fn acceptance_verify_at_least_satisfied_after_enough_calls() {
    let m = mock();
    m.expect("greet").at_least(2);
    let _ = m.greet("a".into());
    assert!(m.verify().is_err());
    let _ = m.greet("b".into());
    assert!(m.verify().is_ok());
    let _ = m.greet("c".into());
    assert!(m.verify().is_ok());
}

#[test]
fn acceptance_verify_at_most_one_satisfied_by_zero_or_one_call() {
    let m = mock();
    m.expect("greet").at_most(1);
    assert!(m.verify().is_ok());
    let _ = m.greet("a".into());
    assert!(m.verify().is_ok());
    let _ = m.greet("b".into());
    assert!(m.verify().is_err());
}

// === Acceptance: spy records call arguments ==================================

#[test]
fn acceptance_spy_records_call_count() {
    let m = mock();
    let spy = m.spy("greet");
    let _ = m.greet("alice".into());
    let _ = m.greet("bob".into());
    assert_eq!(spy.call_count(), 2);
    assert!(spy.call_count() > 0);
}

#[test]
fn acceptance_spy_records_args_in_order() {
    let m = mock();
    let spy = m.spy("greet");
    let _ = m.greet("alice".into());
    let _ = m.greet("bob".into());
    let args = spy.args();
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], vec![ArgumentValue::String("alice".into())]);
    assert_eq!(args[1], vec![ArgumentValue::String("bob".into())]);
}

#[test]
fn acceptance_spy_filters_to_its_method_only() {
    let m = mock();
    let greet_spy = m.spy("greet");
    let ping_spy = m.spy("ping");
    let _ = m.greet("a".into());
    let _ = m.ping();
    let _ = m.greet("b".into());
    assert_eq!(greet_spy.call_count(), 2);
    assert_eq!(ping_spy.call_count(), 1);
}

#[test]
fn acceptance_spy_first_and_last_args_via_calls() {
    let m = mock();
    let spy = m.spy("add");
    let _ = m.add(1, 2);
    let _ = m.add(3, 4);
    let calls = spy.calls();
    let first = calls.first().expect("at least one call").args.clone();
    let last = calls.last().expect("at least one call").args.clone();
    assert_eq!(first, vec![ArgumentValue::Int(1), ArgumentValue::Int(2)]);
    assert_eq!(last, vec![ArgumentValue::Int(3), ArgumentValue::Int(4)]);
}

#[test]
fn acceptance_spy_does_not_affect_dispatch() {
    let m = mock();
    let _spy = m.spy("greet");
    m.expect("greet")
        .returning(ReturnValue::String("hello".into()));
    let r = m.greet("alice".into());
    assert_eq!(r, "hello");
}

// === Argument matching =======================================================

#[test]
fn expect_with_args_matches_specific_call() {
    let m = mock();
    m.expect("greet")
        .with_args(vec![ArgumentValue::String("alice".into())])
        .returning(ReturnValue::String("hello alice".into()));
    assert_eq!(m.greet("alice".into()), "hello alice");
    assert_eq!(m.greet("bob".into()), "");
}

#[test]
fn expect_with_args_returns_default_for_unmatched() {
    let m = mock();
    m.expect("add")
        .with_args(vec![ArgumentValue::Int(1), ArgumentValue::Int(2)])
        .returning(ReturnValue::Int(99));
    assert_eq!(m.add(1, 2), 99);
    assert_eq!(m.add(3, 4), 0);
}

#[test]
fn multiple_expectations_on_same_method_match_in_order() {
    let m = mock();
    m.expect("greet")
        .with_args(vec![ArgumentValue::String("alice".into())])
        .returning(ReturnValue::String("hi alice".into()));
    m.expect("greet")
        .with_args(vec![ArgumentValue::String("bob".into())])
        .returning(ReturnValue::String("hey bob".into()));
    assert_eq!(m.greet("alice".into()), "hi alice");
    assert_eq!(m.greet("bob".into()), "hey bob");
}

// === Times constraints =======================================================

#[test]
fn times_zero_satisfied_by_no_calls() {
    let m = mock();
    m.expect("greet").times(0);
    assert!(m.verify().is_ok());
}

#[test]
fn times_zero_violated_by_one_call() {
    let m = mock();
    m.expect("greet").times(0);
    let _ = m.greet("a".into());
    assert!(m.verify().is_err());
}

#[test]
fn returning_with_separate_times_constraint() {
    let m = mock();
    m.expect("greet")
        .returning(ReturnValue::String("hi".into()));
    m.expect("greet").times(1);
    let _ = m.greet("a".into());
    assert!(m.verify().is_ok());
}

// === Codegen lowering ========================================================

#[test]
fn lower_mock_for_trait_emits_impl_for_greeter() {
    let t = mk_greeter_buff_trait();
    let item = lower_mock_for_trait(&t).expect("lowering should succeed");
    let file = syn::File {
        attrs: Vec::new(),
        items: vec![item],
        shebang: None,
    };
    let source = prettyplease::unparse(&file);
    assert!(source.contains("impl Greeter for buff_mock::Mock<Greeter>"));
    assert!(source.contains("fn greet(&self, name: String) -> String"));
    assert!(source.contains("fn ping(&self) -> bool"));
    assert!(source.contains("fn add(&self, a: i64, b: i64) -> i64"));
    assert!(source.contains("buff_mock::ArgumentValue::String(name)"));
    assert!(source.contains("buff_mock::ArgumentValue::Int(a)"));
}

#[test]
fn lower_mock_for_trait_rejects_supertraits() {
    let mut t = mk_greeter_buff_trait();
    t.supertraits.push(named("Other"));
    let err = lower_mock_for_trait(&t).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("supertraits"));
}

#[test]
fn lower_mock_for_trait_rejects_unsupported_param() {
    let mut t = mk_greeter_buff_trait();
    t.required[0].params[0].ty =
        buff_lang_ast::TypeRef::Option(Box::new(named("Int")), dummy_span());
    let err = lower_mock_for_trait(&t).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("parameter type"));
}

// === State inspection ========================================================

#[test]
fn mock_clone_shares_state() {
    let m = mock();
    let clone = m.clone();
    m.record_call_no_args("ping");
    assert_eq!(m.call_count_for("ping"), 1);
    assert_eq!(clone.call_count_for("ping"), 1);
}

#[test]
fn calls_snapshot_returns_all_records() {
    let m = mock();
    let _ = m.greet("a".into());
    let _ = m.ping();
    let _ = m.add(1, 2);
    let calls = m.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].method, "greet");
    assert_eq!(calls[1].method, "ping");
    assert_eq!(calls[2].method, "add");
}

#[test]
fn call_count_for_filters_by_method() {
    let m = mock();
    let _ = m.greet("a".into());
    let _ = m.greet("b".into());
    let _ = m.ping();
    assert_eq!(m.call_count_for("greet"), 2);
    assert_eq!(m.call_count_for("ping"), 1);
    assert_eq!(m.call_count_for("add"), 0);
}

#[test]
fn clear_resets_mock_to_clean_slate() {
    let m = mock();
    m.expect("greet")
        .returning(ReturnValue::String("hi".into()));
    let _ = m.greet("a".into());
    assert_eq!(m.calls().len(), 1);
    m.clear();
    assert_eq!(m.calls().len(), 0);
    assert!(m.lookup_return_no_args("greet").is_none());
}

#[test]
fn debug_rendering_includes_trait_name() {
    let m = mock();
    let s = format!("{m:?}");
    assert!(s.contains("Greeter"));
}

#[test]
fn times_constraint_renders_human_readable_in_error() {
    let m = mock();
    m.expect("greet").times(3);
    let _ = m.greet("a".into());
    let err = m.verify().unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("exactly 3 calls"));
    assert!(msg.contains("got 1"));
}
