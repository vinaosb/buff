//! Integration tests for `buff-fuzz` — API surface + error model.
//!
//! Exercises the public constructors, validators, and runner entry
//! point end-to-end. These are the QA harness for the T27 acceptance
//! scenarios "Strategy validates input ranges" / "Runner rejects zero
//! iterations" / "Runner records failures for falsified properties".

use buff_fuzz::{run, FuzzError, FuzzSummary, Strategy};

#[test]
fn strategy_int_constructs_with_named_fields() {
    let s = Strategy::int(0, 10);
    assert!(matches!(s, Strategy::Int { min: 0, max: 10 }));
}

#[test]
fn strategy_float_constructs_with_named_fields() {
    let s = Strategy::float(-1.0, 1.0);
    assert!(matches!(
        s,
        Strategy::Float {
            min: -1.0,
            max: 1.0
        }
    ));
}

#[test]
fn strategy_bool_constructs() {
    let s = Strategy::bool();
    assert!(matches!(s, Strategy::Bool));
}

#[test]
fn strategy_string_constructs_with_max_len() {
    let s = Strategy::string(32);
    assert!(matches!(s, Strategy::String { max_len: 32 }));
}

#[test]
fn strategy_bytes_constructs_with_max_len() {
    let s = Strategy::bytes(128);
    assert!(matches!(s, Strategy::Bytes { max_len: 128 }));
}

#[test]
fn strategy_default_is_int_0_to_100() {
    let s = Strategy::default();
    assert!(matches!(s, Strategy::Int { min: 0, max: 100 }));
}

#[test]
fn strategy_validate_rejects_inverted_int_range() {
    let s = Strategy::int(100, 0);
    let err = s.validate().expect_err("inverted range should fail");
    assert!(matches!(err, FuzzError::InvalidStrategy { .. }));
}

#[test]
fn strategy_validate_rejects_zero_string_max_len() {
    let s = Strategy::string(0);
    let err = s.validate().expect_err("zero max_len should fail");
    assert!(matches!(err, FuzzError::InvalidStrategy { .. }));
}

#[test]
fn strategy_validate_rejects_zero_bytes_max_len() {
    let s = Strategy::bytes(0);
    let err = s.validate().expect_err("zero max_len should fail");
    assert!(matches!(err, FuzzError::InvalidStrategy { .. }));
}

#[test]
fn strategy_validate_accepts_valid_int() {
    let s = Strategy::int(0, 10);
    s.validate().expect("valid range should pass");
}

#[test]
fn strategy_display_includes_name_and_args() {
    let s = Strategy::int(0, 10);
    let displayed = format!("{s}");
    assert_eq!(displayed, "Strategy.int(0, 10)");
}

#[test]
fn run_rejects_zero_iterations() {
    let s = Strategy::int(0, 10);
    let err = run(&s, 0, |_| true).expect_err("zero iterations should fail");
    assert!(matches!(err, FuzzError::InvalidIterations { count: 0 }));
}

#[test]
fn run_rejects_invalid_strategy() {
    let s = Strategy::int(100, 0);
    let err = run(&s, 16, |_| true).expect_err("invalid strategy should fail");
    assert!(matches!(err, FuzzError::InvalidStrategy { .. }));
}

#[test]
fn run_records_iteration_count_for_passing_property() {
    let s = Strategy::int(0, 100);
    let summary = run(&s, 64, |n| n >= 0 && n <= 100).expect("valid run");
    assert_eq!(summary.iterations, 64);
}

#[test]
fn run_summary_passed_is_true_for_passing_property() {
    let s = Strategy::int(0, 100);
    let summary = run(&s, 64, |n| n >= 0 && n <= 100).expect("valid run");
    assert!(summary.passed());
}

#[test]
fn run_summary_failed_count_is_zero_for_passing_property() {
    let s = Strategy::int(0, 100);
    let summary = run(&s, 64, |n| n >= 0 && n <= 100).expect("valid run");
    assert_eq!(summary.failed_count(), 0);
}

#[test]
fn fuzz_summary_passed_helper_works() {
    let passing = FuzzSummary {
        iterations: 10,
        failures: vec![],
    };
    assert!(passing.passed());
    assert_eq!(passing.failed_count(), 0);

    let failing = FuzzSummary {
        iterations: 10,
        failures: vec![42],
    };
    assert!(!failing.passed());
    assert_eq!(failing.failed_count(), 1);
}
