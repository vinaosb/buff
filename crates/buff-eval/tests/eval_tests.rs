//! Acceptance tests for `buff-eval`.
//!
//! These exercise the public end-to-end pipeline: classify → compose →
//! lex → parse → codegen → rustc → spawn-and-capture. They invoke
//! `rustc` on real generated Rust source, so they need the host's
//! MSVC env (`LIB` / `INCLUDE`) to be set up — see the workspace root
//! AGENTS.md "COMMANDS" section for the canonical vcvars invocation.
//!
//! Scenarios covered (per task T125-prep §2 EXPECTED OUTCOME):
//!
//! 1. `eval("2 + 3")` → value `5`               — [`eval_expression`]
//! 2. `eval_line` state persists across calls    — [`eval_line_accumulates_state`]
//! 3. `type_of("x")` returns Int after the above — [`type_of_uses_accumulated_env`]
//! 4. `print` writes to a capturable stdout
//!    buffer, NOT process stdout                 — [`print_output_is_captured`]
//! 5. Errors surface as a diagnostic, no panic   — [`error_returns_diagnostic`]
//!
//! All tests are independent (each constructs a fresh `Evaluator`).
//!
//! NOTE: rustc is invoked per test (a couple of seconds each on a
//! warm MSVC toolchain). The full suite is <10s on a dev laptop.

#![allow(clippy::needless_pass_by_value)]

use buff_eval::{EvalResult, Evaluator};

/// Helper: assert the result has no diagnostic and report a useful
/// failure message including stderr when it does.
fn assert_ok(result: &EvalResult) {
    assert!(
        result.diagnostic.is_none(),
        "expected success but got diagnostic: {:?}\nstderr: {}",
        result.diagnostic,
        result.stderr
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 1: `eval("2 + 3")` returns value `5`.
// ---------------------------------------------------------------------------

#[test]
fn eval_expression() {
    let mut ev = Evaluator::new();
    let result = ev.eval("2 + 3");
    assert_ok(&result);
    assert_eq!(
        result.value.as_deref(),
        Some("5"),
        "value should be 5, got {:?}; stdout={:?}",
        result.value,
        result.stdout
    );
    // The wrapped `print(2 + 3)` writes the value to stdout too.
    assert!(
        result.stdout.trim().contains('5'),
        "stdout should contain '5', got {:?}",
        result.stdout
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 2: `eval_line("let x = 42")` then `eval_line("x + 8")`
// returns `50` (state persists).
// ---------------------------------------------------------------------------

#[test]
fn eval_line_accumulates_state() {
    let mut ev = Evaluator::new();

    // First line: a `let` statement. No value, no stdout.
    let r1 = ev.eval_line("let x = 42");
    assert_ok(&r1);
    assert!(
        r1.value.is_none(),
        "let-statement should not produce a value, got {:?}",
        r1.value
    );

    // Second line: a bare expression using the previously-declared `x`.
    // The composed program is `func main(): let x = 42\n    print(x + 8)\n`.
    let r2 = ev.eval_line("x + 8");
    assert_ok(&r2);
    assert_eq!(
        r2.value.as_deref(),
        Some("50"),
        "value should be 50 after state accumulation, got {:?}; stdout={:?}",
        r2.value,
        r2.stdout
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 3: `type_of("x")` returns Int after the above.
// ---------------------------------------------------------------------------

#[test]
fn type_of_uses_accumulated_env() {
    let mut ev = Evaluator::new();
    // Seed state the same way the previous test does.
    let r1 = ev.eval_line("let x = 42");
    assert_ok(&r1);

    let ty = ev
        .type_of("x")
        .expect("type_of(x) should resolve after `let x = 42`");

    // The Buff prelude widens integer literals to `Int<64>` by default
    // (the inference picks the smallest width that fits; 42 fits in i8,
    // but the default `Int` literal type is `i64`). The exact width is
    // an implementation detail of the inferencer; we assert it's a
    // signed integer of some width.
    match ty {
        buff_eval::ResolvedType::Int { .. } => {}
        other => panic!("expected Int, got {other:?}"),
    }

    // The Display form of `Type::Int { width: W64 }` is `Int<64>`.
    // Asserting on the Display string gives us a stable human-readable
    // contract for downstream consumers (REPL/Jupyter) without coupling
    // to the `IntWidth` enum.
    assert!(
        format!("{ty}").starts_with("Int<"),
        "type display should start with `Int<`, got `{ty}`"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 4: print output is captured into EvalResult.stdout,
// NOT written to process stdout.
// ---------------------------------------------------------------------------

#[test]
fn print_output_is_captured() {
    let mut ev = Evaluator::new();
    // A bare expression that's already a `print(...)` call. The
    // classifier detects the print callee and does NOT double-wrap.
    let result = ev.eval("print(\"hello-eval\")");
    assert_ok(&result);
    assert_eq!(
        result.stdout.trim(),
        "hello-eval",
        "stdout should contain the printed text, got {:?}",
        result.stdout
    );
    // For a print call, there is no return value (`print` returns Void).
    assert!(
        result.value.is_none(),
        "print() should not produce a value, got {:?}",
        result.value
    );
}

/// A standalone smoke test verifying that captured stdout accumulates
/// across multiple `print(...)` calls in a single program. The
/// Jupyter/Bufflings consumers depend on the FULL stdout buffer being
/// preserved verbatim (not just the last line).
#[test]
fn print_output_is_captured_multiline() {
    let mut ev = Evaluator::new();
    let result = ev.eval(
        "func main():\n    print(\"line-one\")\n    print(\"line-two\")\n    print(\"line-three\")",
    );
    assert_ok(&result);
    let lines: Vec<&str> = result.stdout.lines().collect();
    assert_eq!(
        lines,
        ["line-one", "line-two", "line-three"],
        "all printed lines should appear in stdout, got {:?}",
        result.stdout
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 5: errors return a diagnostic in EvalResult; do NOT
// panic.
// ---------------------------------------------------------------------------

#[test]
fn error_returns_diagnostic() {
    let mut ev = Evaluator::new();

    // Parse error: `let` without a value. This should surface as a
    // diagnostic, NOT a panic.
    let result = ev.eval_line("let x =");
    assert!(
        result.diagnostic.is_some(),
        "expected a diagnostic for `let x =`, got {:?}",
        result
    );

    // Type/codegen error: bare expression with unknown identifier.
    // rustc will fail to compile (cannot find `unknown_ident`). The
    // diagnostic field should be populated; stderr should carry the
    // rustc message.
    let mut ev2 = Evaluator::new();
    let r2 = ev2.eval_line("undefined_identifier_xyz");
    assert!(
        r2.diagnostic.is_some(),
        "expected a diagnostic for unknown identifier, got {:?}",
        r2
    );
}

/// The first QA scenario from the task description, run end-to-end with
/// all assertions in one test for a single-shot trace. Kept in addition
/// to the per-bullet tests above so a CI failure points straight at the
/// failing scenario.
#[test]
fn full_qa_scenario_smoke() {
    let mut ev = Evaluator::new();

    // 1. eval("2 + 3") returns value 5.
    let r = ev.eval("2 + 3");
    assert_ok(&r);
    assert_eq!(r.value.as_deref(), Some("5"));

    // 2. eval_line accumulates: let x = 42, then x + 8 → 50.
    let r1 = ev.eval_line("let x = 42");
    assert_ok(&r1);
    let r2 = ev.eval_line("x + 8");
    assert_ok(&r2);
    assert_eq!(r2.value.as_deref(), Some("50"));

    // 3. type_of("x") returns Int (something starting with `Int<`).
    let ty = ev.type_of("x").expect("type_of(x) should resolve");
    let display = format!("{ty}");
    assert!(
        display.starts_with("Int<"),
        "type_of(x) should be Int, got `{display}`"
    );

    // 4. print is captured.
    let r3 = ev.eval("print(\"captured\")");
    assert_ok(&r3);
    assert_eq!(r3.stdout.trim(), "captured");

    // 5. errors return a diagnostic, no panic.
    let r4 = ev.eval("let broken =");
    assert!(r4.diagnostic.is_some());
}
