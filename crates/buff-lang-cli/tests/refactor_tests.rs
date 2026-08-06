//! Integration tests for `buff refactor` (T66).
//!
//! All test names contain `refactor` so
//! `cargo test -p buff-lang-cli refactor` matches them.

#![cfg(test)]

use buff_lang_cli::commands::refactor::{
    apply_extract_to_source, apply_inline_to_source, apply_rename_to_source,
};

#[test]
fn refactor_rename_simple_func_and_call() {
    let src = "func helper():\n    print(\"hi\")\n\nfunc main():\n    helper()\n";
    let out = apply_rename_to_source(src, "helper", "greet").unwrap();
    assert!(
        out.contains("func greet"),
        "expected func greet, got:\n{out}"
    );
    assert!(out.contains("greet()"), "expected greet(), got:\n{out}");
    assert!(
        !out.contains("helper"),
        "helper should be gone, got:\n{out}"
    );
}

#[test]
fn refactor_rename_preserves_other_identifiers() {
    let src = "func alpha():\n    print(\"a\")\n\nfunc beta():\n    print(\"b\")\n";
    let out = apply_rename_to_source(src, "alpha", "gamma").unwrap();
    assert!(out.contains("func gamma"));
    assert!(out.contains("func beta"), "beta must be untouched");
}

#[test]
fn refactor_rename_rejects_keyword_target() {
    let src = "func main():\n    print(\"x\")\n";
    let err = apply_rename_to_source(src, "main", "func").unwrap_err();
    assert!(
        err.to_string().contains("reserved Buff keyword"),
        "expected keyword error, got: {err}"
    );
}

#[test]
fn refactor_rename_rejects_empty_name() {
    let src = "func main():\n    print(\"x\")\n";
    let err = apply_rename_to_source(src, "main", "").unwrap_err();
    assert!(
        err.to_string().contains("empty"),
        "expected empty error, got: {err}"
    );
}

#[test]
fn refactor_rename_noop_when_name_not_found() {
    let src = "func main():\n    print(\"x\")\n";
    let out = apply_rename_to_source(src, "missing", "renamed").unwrap();
    assert_eq!(out, src, "source should be unchanged");
}

#[test]
fn refactor_rename_rejects_digit_start() {
    let src = "func main():\n    print(\"x\")\n";
    let err = apply_rename_to_source(src, "main", "1bad").unwrap_err();
    assert!(
        err.to_string().contains("does not start with a letter"),
        "expected digit-start error, got: {err}"
    );
}

#[test]
fn refactor_extract_simple_range() {
    let src = "func main():\n    let x = 1\n    print(x)\n    print(\"done\")\n";
    let out = apply_extract_to_source(src, 2, 3, "extracted").unwrap();
    assert!(
        out.contains("func extracted"),
        "expected new function, got:\n{out}"
    );
    assert!(
        out.contains("extracted()"),
        "expected call site, got:\n{out}"
    );
}

#[test]
fn refactor_extract_produces_valid_new_function() {
    let src = "func main():\n    let a = 1\n    let b = 2\n    let c = a + b\n    print(c)\n";
    let out = apply_extract_to_source(src, 2, 4, "compute_sum").unwrap();
    assert!(out.contains("func compute_sum"));
    assert!(out.contains("compute_sum()"));
    assert!(out.contains("compute_sum()"));
}

#[test]
fn refactor_extract_preserves_original_statements_in_new_fn() {
    let src = "func main():\n    let x = 10\n    print(x)\n";
    let out = apply_extract_to_source(src, 2, 3, "work").unwrap();
    assert!(out.contains("func work"));
}

#[test]
fn refactor_extract_rejects_non_contiguous_range() {
    // start > end is an invalid range — extract must reject it.
    let src = "func main():\n    let a = 1\n    let b = 2\n    let c = 3\n";
    let result = apply_extract_to_source(src, 4, 2, "f");
    assert!(
        result.is_err(),
        "extract should reject start > end range, got Ok: {:?}",
        result
    );
}

#[test]
fn refactor_extract_rejects_invalid_line_range() {
    let src = "func main():\n    print(\"x\")\n";
    let err = apply_extract_to_source(src, 100, 200, "f").unwrap_err();
    assert!(
        err.to_string().contains("no statements"),
        "expected no-statements error, got: {err}"
    );
}

#[test]
fn refactor_extract_rejects_keyword_name() {
    let src = "func main():\n    let x = 1\n    print(x)\n";
    let err = apply_extract_to_source(src, 2, 3, "let").unwrap_err();
    assert!(
        err.to_string().contains("reserved Buff keyword"),
        "expected keyword error, got: {err}"
    );
}

#[test]
fn refactor_inline_simple_int_binding() {
    let src = "func main():\n    let x = 42\n    print(x)\n";
    let out = apply_inline_to_source(src, "x").unwrap();
    assert!(
        !out.contains("let x"),
        "let x should be removed, got:\n{out}"
    );
    assert!(
        out.contains("42"),
        "initializer 42 should be inlined, got:\n{out}"
    );
}

#[test]
fn refactor_inline_replaces_multiple_uses() {
    let src = "func main():\n    let x = 5\n    print(x)\n    print(x)\n";
    let out = apply_inline_to_source(src, "x").unwrap();
    assert!(!out.contains("let x"));
    let count = out.matches("5").count();
    assert!(count >= 2, "expected at least two 5's, got:\n{out}");
}

#[test]
fn refactor_inline_removes_let_binding() {
    let src = "func main():\n    let y = 10\n    print(y)\n";
    let out = apply_inline_to_source(src, "y").unwrap();
    assert!(!out.contains("let y"), "let y must be removed, got:\n{out}");
    assert!(out.contains("10"), "10 must appear, got:\n{out}");
}

#[test]
fn refactor_inline_rejects_complex_initializer() {
    let src = "func main():\n    let x = compute()\n    print(x)\n";
    let err = apply_inline_to_source(src, "x").unwrap_err();
    assert!(
        err.to_string().contains("side-effect-free"),
        "expected side-effect-free error, got: {err}"
    );
}

#[test]
fn refactor_inline_rejects_unknown_name() {
    let src = "func main():\n    print(\"x\")\n";
    let err = apply_inline_to_source(src, "missing").unwrap_err();
    assert!(
        err.to_string().contains("no `let missing` binding found"),
        "expected not-found error, got: {err}"
    );
}

#[test]
fn refactor_inline_string_literal_initializer() {
    let src = "func main():\n    let msg = \"hello\"\n    print(msg)\n";
    let out = apply_inline_to_source(src, "msg").unwrap();
    assert!(!out.contains("let msg"));
    assert!(out.contains("\"hello\""), "string literal must be inlined");
}
