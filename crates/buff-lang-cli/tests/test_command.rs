//! Integration tests for the `buff test` command (T35).
//!
//! The cheaper logic tests (discovery, pattern matching, report parsing,
//! harness generation) run unconditionally. The end-to-end tests that
//! compile + run a real Rust test binary auto-skip when `rustc` is not on
//! `PATH` (mirroring the `cli_run_tests` / `cli_build_tests` pattern).
//!
//! All file-based fixtures use UNIQUE names per test (prefixed with the
//! test name) and live under a per-process temp dir to avoid parallel-test
//! collisions (T29 lesson: never share fixture paths across tests).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use buff_lang_cli::test_runner;
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

// ---------------------------------------------------------------------------
// Test helpers (mirror cli_run_tests.rs patterns)
// ---------------------------------------------------------------------------

fn temp_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "buff-lang-cli-test-command-tests-{}",
        std::process::id()
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn write_fixture(name: &str, contents: &str) -> PathBuf {
    let path = temp_root().join(name);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, contents).unwrap_or_else(|e| panic!("failed to write fixture {path:?}: {e}"));
    path
}

fn rustc_available() -> bool {
    Command::new("rustc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn cleanup(path: &std::path::Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_dir_all(path);
    // Also clean up the `.test.rs` sibling the test runner writes.
    if path.extension().is_some_and(|e| e == "buff") {
        let test_rs = path.with_extension("test.rs");
        let _ = fs::remove_file(&test_rs);
    }
}

/// Parse a `.buff` source into a Vec<Decl>. Convenience for the logic tests.
fn parse_buff(src: &str) -> Vec<buff_lang_ast::Decl> {
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("lexer should succeed");
    parse(&tokens, sid).expect("parser should succeed")
}

// ---------------------------------------------------------------------------
// Logic tests: discovery (no rustc required)
// ---------------------------------------------------------------------------

#[test]
fn test_command_discovers_single_test_func() {
    let decls = parse_buff("@test\nfunc test_addition():\n    assert_eq(2, 2)\n");
    let names = test_runner::discover_test_names(&decls, "");
    assert_eq!(names, vec!["test_addition".to_string()]);
}

#[test]
fn test_command_discovers_multiple_tests_sorted() {
    // Define tests out of alphabetical order; discovery must SORT them.
    let decls = parse_buff(
        "@test\nfunc test_zebra():\n    assert_eq(1, 1)\n@test\nfunc test_alpha():\n    assert_eq(1, 1)\n@test\nfunc test_middle():\n    assert_eq(1, 1)\n",
    );
    let names = test_runner::discover_test_names(&decls, "");
    assert_eq!(
        names,
        vec![
            "test_alpha".to_string(),
            "test_middle".to_string(),
            "test_zebra".to_string(),
        ]
    );
}

#[test]
fn test_command_ignores_non_test_funcs() {
    let decls = parse_buff(
        "func helper():\n    print(\"helper\")\nfunc test_not_annotated():\n    print(\"no attr\")\n@test\nfunc test_real():\n    assert_eq(1, 1)\n",
    );
    let names = test_runner::discover_test_names(&decls, "");
    // Only `test_real` has `@test` — `test_not_annotated` lacks the attribute.
    assert_eq!(names, vec!["test_real".to_string()]);
}

#[test]
fn test_command_no_test_funcs_returns_empty() {
    let decls = parse_buff("func main():\n    print(\"hello\")\n");
    let names = test_runner::discover_test_names(&decls, "");
    assert!(names.is_empty(), "no @test funcs → empty discovery");
}

// ---------------------------------------------------------------------------
// Logic tests: --pattern filtering (no rustc required)
// ---------------------------------------------------------------------------

#[test]
fn test_command_pattern_filter_prefix() {
    let decls = parse_buff(
        "@test\nfunc test_foo():\n    assert_eq(1, 1)\n@test\nfunc test_bar():\n    assert_eq(1, 1)\n@test\nfunc helper_test():\n    assert_eq(1, 1)\n",
    );
    let names = test_runner::discover_test_names(&decls, "test_*");
    assert_eq!(names, vec!["test_bar".to_string(), "test_foo".to_string()],);
    // `helper_test` must NOT match `test_*`.
}

#[test]
fn test_command_pattern_filter_exact() {
    let decls = parse_buff(
        "@test\nfunc test_foo():\n    assert_eq(1, 1)\n@test\nfunc test_bar():\n    assert_eq(1, 1)\n",
    );
    let names = test_runner::discover_test_names(&decls, "test_foo");
    assert_eq!(names, vec!["test_foo".to_string()]);
}

#[test]
fn test_command_pattern_filter_no_match() {
    let decls = parse_buff("@test\nfunc test_foo():\n    assert_eq(1, 1)\n");
    let names = test_runner::discover_test_names(&decls, "nonexistent_*");
    assert!(names.is_empty());
}

// ---------------------------------------------------------------------------
// Logic tests: report parsing (no rustc required)
// ---------------------------------------------------------------------------

#[test]
fn test_command_report_all_pass() {
    let output = "test test_a ... ok\ntest test_b ... ok\n\n2 passed, 0 failed\n";
    let report = test_runner::parse_report(output, 2);
    assert_eq!(report.passed, 2);
    assert_eq!(report.failed, 0);
    assert_eq!(report.exit_code(), 0);
}

#[test]
fn test_command_report_some_fail() {
    let output = "test test_a ... ok\ntest test_b ... FAILED\n\n1 passed, 1 failed\n";
    let report = test_runner::parse_report(output, 2);
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 1);
    assert_eq!(report.failures, vec!["test_b".to_string()]);
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn test_command_report_no_tests() {
    // No tests discovered → empty report → exit 0.
    let report = test_runner::TestReport {
        total: 0,
        passed: 0,
        failed: 0,
        failures: Vec::new(),
        raw_output: String::new(),
    };
    assert_eq!(report.exit_code(), 0);
}

// ---------------------------------------------------------------------------
// Logic tests: harness generation (no rustc required — just codegen)
// ---------------------------------------------------------------------------

#[test]
fn test_command_harness_generates_runner_main() {
    let decls = parse_buff(
        "@test\nfunc test_add():\n    assert_eq(1 + 1, 2)\nfunc main():\n    print(\"hi\")\n",
    );
    let names = test_runner::discover_test_names(&decls, "");
    let rust = buff_lang_codegen_rust::generate_test_rust(&decls, &names)
        .expect("test harness codegen should succeed");

    // The runner main must be present and call the test fn.
    assert!(
        rust.contains("fn main()"),
        "harness must contain a `fn main()`; got:\n{rust}"
    );
    assert!(
        rust.contains("catch_unwind"),
        "harness must use `catch_unwind`; got:\n{rust}"
    );
    assert!(
        rust.contains("test_add"),
        "harness must reference the test fn `test_add`; got:\n{rust}"
    );
    assert!(
        rust.contains("passed") && rust.contains("failed"),
        "harness must print `<n> passed, <m> failed`; got:\n{rust}"
    );
    // The user's `main` fn body should NOT be the entry point (it's removed).
    // We check that there's exactly ONE `fn main()` (the runner's, not the
    // user's). The user's `print("hi")` may still appear if it's a different
    // fn name, but since the user fn WAS named `main`, it's removed entirely.
    let main_count = rust.matches("fn main()").count();
    assert_eq!(
        main_count, 1,
        "harness must have exactly one `fn main()` (the runner's); found {main_count}"
    );
}

#[test]
fn test_command_harness_strips_test_attr_from_fns() {
    // The codegen emits `#[test]` on `@test` fns (for `buff build`). The
    // harness generator STRIPS `#[test]` because it calls the fns directly.
    let decls = parse_buff("@test\nfunc test_x():\n    assert_eq(1, 1)\n");
    let names = test_runner::discover_test_names(&decls, "");
    let rust = buff_lang_codegen_rust::generate_test_rust(&decls, &names)
        .expect("harness codegen should succeed");
    assert!(
        !rust.contains("#[test]"),
        "harness must NOT contain `#[test]` (it's stripped); got:\n{rust}"
    );
}

// ---------------------------------------------------------------------------
// Logic tests: front-end errors (no rustc required)
// ---------------------------------------------------------------------------

#[test]
fn test_command_nonexistent_file_returns_error() {
    let bogus = temp_root().join("test_command_no_such_file.buff");
    let err = test_runner::run_tests(&bogus, "").unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("failed to read"),
        "expected file-read error, got: {msg}"
    );
}

#[test]
fn test_command_parse_error_propagates() {
    // `let` at top level is illegal.
    let src = "let x = 1\n";
    let file = write_fixture("test_command_parse_err.buff", src);
    let err = test_runner::run_tests(&file, "").unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("parse error"),
        "expected parse error, got: {msg}"
    );
    cleanup(&file);
}

#[test]
fn test_command_unknown_attribute_errors() {
    // `@bogus` on a `@test` func: discovery finds it (has `@test`), then
    // codegen runs and errors on the unrecognised `@bogus` attribute.
    let src = "@test\n@bogus\nfunc test_x():\n    assert_eq(1, 1)\n";
    let file = write_fixture("test_command_unknown_attr.buff", src);
    let err = test_runner::run_tests(&file, "").unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("unrecognised attribute") || msg.contains("codegen error"),
        "expected attribute/codegen error, got: {msg}"
    );
    cleanup(&file);
}

// ---------------------------------------------------------------------------
// End-to-end tests (rustc required)
// ---------------------------------------------------------------------------

#[test]
fn test_command_e2e_all_pass() {
    if !rustc_available() {
        eprintln!("skipping test_command_e2e_all_pass: rustc not on PATH");
        return;
    }

    let src = "func add(a: Int, b: Int) -> Int:\n    return a + b\n@test\nfunc test_add():\n    assert_eq(add(2, 3), 5)\n";
    let file = write_fixture("test_command_e2e_pass.buff", src);

    let report = test_runner::run_tests(&file, "").expect("test run should succeed");
    assert_eq!(report.total, 1, "one test discovered");
    assert_eq!(report.passed, 1, "one test passed");
    assert_eq!(report.failed, 0, "zero failures");
    assert_eq!(report.exit_code(), 0, "exit 0 when all pass");

    cleanup(&file);
}

#[test]
fn test_command_e2e_failing_test_exit_one() {
    if !rustc_available() {
        eprintln!("skipping test_command_e2e_failing_test_exit_one: rustc not on PATH");
        return;
    }

    let src = "@test\nfunc test_fail():\n    assert_eq(2, 3)\n";
    let file = write_fixture("test_command_e2e_fail.buff", src);

    let report = test_runner::run_tests(&file, "").expect("test run should succeed");
    assert_eq!(report.total, 1);
    assert_eq!(report.failed, 1, "the test must fail");
    assert_eq!(report.passed, 0);
    assert_eq!(report.exit_code(), 1, "exit 1 when any test fails");

    cleanup(&file);
}

#[test]
fn test_command_e2e_pattern_filter() {
    if !rustc_available() {
        eprintln!("skipping test_command_e2e_pattern_filter: rustc not on PATH");
        return;
    }

    // Two tests: one passes, one fails. Use --pattern to run only the passing one.
    let src = "@test\nfunc test_good():\n    assert_eq(1, 1)\n@test\nfunc test_bad():\n    assert_eq(1, 2)\n";
    let file = write_fixture("test_command_e2e_pattern.buff", src);

    let report = test_runner::run_tests(&file, "test_good").expect("test run should succeed");
    assert_eq!(report.total, 1, "only one test matches the pattern");
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 0);

    cleanup(&file);
}

#[test]
fn test_command_e2e_no_tests_graceful() {
    if !rustc_available() {
        eprintln!("skipping test_command_e2e_no_tests_graceful: rustc not on PATH");
        return;
    }

    let src = "func main():\n    print(\"no tests here\")\n";
    let file = write_fixture("test_command_e2e_no_tests.buff", src);

    let report = test_runner::run_tests(&file, "").expect("should succeed with empty report");
    assert_eq!(report.total, 0);
    assert_eq!(report.exit_code(), 0);

    cleanup(&file);
}
