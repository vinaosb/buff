//! Integration tests for `buff fmt` (T54).
//!
//! Coverage:
//! - 10 insta snapshots of canonical formatter output on representative
//!   Buff programs (`fmt_snapshot_*`).
//! - Idempotency across the same 10 fixtures + the 8 v0.1/v0.5 example
//!   files (`fmt_idempotent_*`).
//! - QA: 2-space indent → 4-space (`fmt_qa_two_space_to_four_space`).
//! - Sorted imports (`fmt_imports_sorted_*`).
//! - Trailing-comma insertion in multi-line collections.
//! - No trailing whitespace.
//! - `--check` exits 0 on already-formatted, non-zero on un-formatted
//!   (`fmt_check_*`).
//! - Determinism: same source → byte-identical output across calls.

#![cfg(test)]

use std::io::Write;

use buff_lang_cli::fmt;
use insta::assert_snapshot;

// ===========================================================================
// 1. Snapshots (10 representative programs).
//
// Each snapshot covers a distinct Buff construct so the snapshot suite as
// a whole documents canonical output across the language surface.
// ===========================================================================

#[test]
fn fmt_snapshot_hello_world() {
    // Minimum viable Buff program — the v0.1 milestone.
    let src = "func main():\n    print(\"Olá, Buff!\")\n";
    let out = fmt::format_source(src).expect("format");
    assert_snapshot!(out);
}

#[test]
fn fmt_snapshot_typed_function_with_return() {
    let src = "func add(a: Int, b: Int) -> Int:\n    return a + b\n";
    let out = fmt::format_source(src).expect("format");
    assert_snapshot!(out);
}

#[test]
fn fmt_snapshot_recursive_function() {
    let src = "func fib(n: Int) -> Int:\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n\nfunc main():\n    let n = 10\n    print(fib(n))\n";
    let out = fmt::format_source(src).expect("format");
    assert_snapshot!(out);
}

#[test]
fn fmt_snapshot_imports_reorder_and_group() {
    // The RED case from the task spec: imports unsorted → reordered.
    let src = "import { zebra } from \"./z\"\nimport { mango } from \"./m\"\nimport { alpha } from \"./a\"\n\nfunc main():\n    print(\"hi\")\n";
    let out = fmt::format_source(src).expect("format");
    assert_snapshot!(out);
}

#[test]
fn fmt_snapshot_match_expression_inline() {
    // Two-arm match — the inline form.
    let src =
        "func main():\n    let x = 1\n    match x { 1 => print(\"one\"), _ => print(\"other\") }\n";
    let out = fmt::format_source(src).expect("format");
    assert_snapshot!(out);
}

#[test]
fn fmt_snapshot_closures_and_method_chains() {
    let src = "func main():\n    let doubled = [1, 2, 3, 4, 5].map({ x => x * 2 })\n    print(doubled[0])\n    let plus_one = [1, 2, 3].map({ x => x * 2 }).map({ y => y + 1 })\n    print(plus_one[0])\n";
    let out = fmt::format_source(src).expect("format");
    assert_snapshot!(out);
}

#[test]
fn fmt_snapshot_multi_line_collection_with_trailing_comma() {
    // Triggers the multi-line array-literal branch (≥4 elements).
    let src = "func main():\n    let v = [10, 20, 30, 40]\n    print(v[0])\n";
    let out = fmt::format_source(src).expect("format");
    assert_snapshot!(out);
}

#[test]
fn fmt_snapshot_enum_with_payload_variants() {
    let src = "enum Result<T, E> { Ok(T), Err(E) }\n\nfunc main():\n    print(\"defined\")\n";
    let out = fmt::format_source(src).expect("format");
    assert_snapshot!(out);
}

#[test]
fn fmt_snapshot_map_literal_and_struct_init() {
    let src = "func main():\n    let scores = {1: 10, 2: 20, 3: 30}\n    print(scores.len())\n";
    let out = fmt::format_source(src).expect("format");
    assert_snapshot!(out);
}

#[test]
fn fmt_snapshot_error_propagation_and_async() {
    let src = "func half(n: Int) -> Result<Int, Error>:\n    if n < 2:\n        return Error(\"too small\")\n    return Ok(n / 2)\n\nfunc add_one(n: Int) -> Result<Int, Error>:\n    let h = half(n)?\n    return Ok(h + 1)\n";
    let out = fmt::format_source(src).expect("format");
    assert_snapshot!(out);
}

// ===========================================================================
// 2. Idempotency — `format_source(format_source(x)) == format_source(x)`.
// ===========================================================================

/// Helper: assert that formatting `src` twice yields the same string.
fn assert_idempotent(label: &str, src: &str) {
    let once =
        fmt::format_source(src).unwrap_or_else(|e| panic!("[{label}] first format failed: {e}"));
    let twice =
        fmt::format_source(&once).unwrap_or_else(|e| panic!("[{label}] second format failed: {e}"));
    assert_eq!(
        once, twice,
        "[{label}] not idempotent: second format changed the output.\n--- once ---\n{once}\n--- twice ---\n{twice}",
    );
}

#[test]
fn fmt_idempotent_hello_world() {
    assert_idempotent("hello_world", "func main():\n    print(\"hello\")\n");
}

#[test]
fn fmt_idempotent_fibonacci() {
    assert_idempotent(
        "fibonacci",
        "func fib(n: Int) -> Int:\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n\nfunc main():\n    print(fib(10))\n",
    );
}

#[test]
fn fmt_idempotent_typed_let_with_assignment() {
    assert_idempotent(
        "typed_let",
        "func main():\n    let mut x: Int = 0\n    x = x + 1\n    print(x)\n",
    );
}

#[test]
fn fmt_idempotent_imports() {
    assert_idempotent(
        "imports",
        "import { alpha } from \"./a\"\nimport { beta } from \"./b\"\nfunc main():\n    print(\"hi\")\n",
    );
}

#[test]
fn fmt_idempotent_match_inline() {
    assert_idempotent(
        "match_inline",
        "func main():\n    let x = 1\n    match x { 1 => print(\"one\"), _ => print(\"other\") }\n",
    );
}

#[test]
fn fmt_idempotent_closure_chain() {
    assert_idempotent(
        "closure_chain",
        "func main():\n    let xs = [1, 2, 3].map({ x => x + 1 }).map({ y => y * 2 })\n    print(xs[0])\n",
    );
}

#[test]
fn fmt_idempotent_error_propagation() {
    assert_idempotent(
        "error_propagation",
        "func half(n: Int) -> Result<Int, Error>:\n    if n < 2:\n        return Error(\"small\")\n    return Ok(n / 2)\n\nfunc main():\n    let h = half(10)?\n    print(h)\n",
    );
}

#[test]
fn fmt_idempotent_multi_line_collection() {
    assert_idempotent(
        "multi_line_collection",
        "func main():\n    let v = [1, 2, 3, 4, 5, 6, 7, 8]\n    print(v.len())\n",
    );
}

#[test]
fn fmt_idempotent_enum_definition() {
    assert_idempotent(
        "enum_definition",
        "enum Result<T, E> { Ok(T), Err(E) }\n\nfunc main():\n    print(\"ok\")\n",
    );
}

#[test]
fn fmt_idempotent_map_literal() {
    assert_idempotent(
        "map_literal",
        "func main():\n    let scores = {1: 10, 2: 20, 3: 30}\n    print(scores.len())\n",
    );
}

// ===========================================================================
// 3. Real example fixtures — idempotency on v0.1 + v0.5 examples.
// ===========================================================================

#[test]
fn fmt_idempotent_example_ola() {
    let src = include_str!("../../../examples/ola.buff");
    let once = fmt::format_source(src).expect("format");
    let twice = fmt::format_source(&once).expect("format again");
    assert_eq!(once, twice, "examples/ola.buff not idempotent");
}

#[test]
fn fmt_idempotent_example_fibonacci() {
    let src = include_str!("../../../examples/fibonacci.buff");
    let once = fmt::format_source(src).expect("format");
    let twice = fmt::format_source(&once).expect("format again");
    assert_eq!(once, twice, "examples/fibonacci.buff not idempotent");
}

#[test]
fn fmt_idempotent_example_calculadora() {
    let src = include_str!("../../../examples/calculadora.buff");
    let once = fmt::format_source(src).expect("format");
    let twice = fmt::format_source(&once).expect("format again");
    assert_eq!(once, twice, "examples/calculadora.buff not idempotent");
}

#[test]
fn fmt_idempotent_example_collections() {
    let src = include_str!("../../../examples/collections.buff");
    let once = fmt::format_source(src).expect("format");
    let twice = fmt::format_source(&once).expect("format again");
    assert_eq!(once, twice, "examples/collections.buff not idempotent");
}

#[test]
fn fmt_idempotent_example_closures() {
    let src = include_str!("../../../examples/closures.buff");
    let once = fmt::format_source(src).expect("format");
    let twice = fmt::format_source(&once).expect("format again");
    assert_eq!(once, twice, "examples/closures.buff not idempotent");
}

#[test]
fn fmt_idempotent_example_pattern_matching() {
    let src = include_str!("../../../examples/pattern_matching.buff");
    let once = fmt::format_source(src).expect("format");
    let twice = fmt::format_source(&once).expect("format again");
    assert_eq!(once, twice, "examples/pattern_matching.buff not idempotent");
}

#[test]
fn fmt_idempotent_example_error_handling() {
    let src = include_str!("../../../examples/error_handling.buff");
    let once = fmt::format_source(src).expect("format");
    let twice = fmt::format_source(&once).expect("format again");
    assert_eq!(once, twice, "examples/error_handling.buff not idempotent");
}

#[test]
fn fmt_idempotent_example_prelude_demo() {
    let src = include_str!("../../../examples/prelude_demo.buff");
    let once = fmt::format_source(src).expect("format");
    let twice = fmt::format_source(&once).expect("format again");
    assert_eq!(once, twice, "examples/prelude_demo.buff not idempotent");
}

// ===========================================================================
// 4. QA — 2-space indent → 4-space (the explicit T54 acceptance case).
// ===========================================================================

#[test]
fn fmt_qa_two_space_to_four_space() {
    // Source uses 2-space indentation throughout. After formatting, the
    // output MUST use 4-space indentation (convention #2).
    let two_space_src = "func main():\n  let x = 1\n  if x > 0:\n    print(\"positive\")\n  else:\n    print(\"zero\")\n";
    let out = fmt::format_source(two_space_src).expect("format succeeded");

    // No 2-space-indented line should survive.
    for line in out.lines() {
        if line.starts_with(' ') {
            assert!(
                line.starts_with("    "),
                "line is not 4-space indented: {:?}",
                line
            );
            // Also: no line should have exactly 2 leading spaces (which
            // would be a half-step indent — bug-prone).
            assert!(
                !line.starts_with("  ") || line.starts_with("    "),
                "found 2-space (or odd) indent: {:?}",
                line
            );
        }
    }
    // And the formatted output must contain at least one 4-space indent.
    assert!(
        out.contains("\n    "),
        "no 4-space indent in output:\n{out}"
    );
    // And no tab characters.
    assert!(!out.contains('\t'), "found a tab in formatted output");
    // Idempotent.
    let twice = fmt::format_source(&out).expect("second format");
    assert_eq!(out, twice, "QA fixture not idempotent after indent fix");
}

#[test]
fn fmt_qa_no_trailing_whitespace() {
    // Source has trailing whitespace on several lines. After formatting,
    // no line should end with whitespace (convention #2).
    let trailing_ws_src = "func main():\n    let x = 1   \n    print(x)\t\n";
    let out = fmt::format_source(trailing_ws_src).expect("format");
    for (i, line) in out.lines().enumerate() {
        assert!(
            !line.ends_with([' ', '\t']),
            "line {i} has trailing whitespace: {:?}",
            line
        );
    }
}

#[test]
fn fmt_qa_max_two_consecutive_blank_lines() {
    // Source has a 5-blank-line run; output must collapse to ≤2.
    let many_blanks = "func a():\n    print(1)\n\n\n\n\n\nfunc b():\n    print(2)\n";
    let out = fmt::format_source(many_blanks).expect("format");
    let mut max_run = 0;
    let mut cur_run = 0;
    for line in out.lines() {
        if line.is_empty() {
            cur_run += 1;
            if cur_run > max_run {
                max_run = cur_run;
            }
        } else {
            cur_run = 0;
        }
    }
    assert!(
        max_run <= 2,
        "found {max_run} consecutive blank lines (max allowed: 2). Output:\n{out}"
    );
}

#[test]
fn fmt_qa_trailing_comma_in_multi_line_collection() {
    // Multi-line array literal must end with a trailing comma on the last
    // element (convention #2: "Trailing comma in multi-line collections:
    // YES"). Triggered by ≥4 elements.
    let src = "func main():\n    let xs = [1, 2, 3, 4]\n    print(xs)\n";
    let out = fmt::format_source(src).expect("format");
    // The output should contain a multi-line block ending with `,` before
    // the closing `]`.
    assert!(
        out.contains(",\n    ]"),
        "expected trailing comma before closing `]`. Output:\n{out}"
    );
}

// ===========================================================================
// 5. Import sorting.
// ===========================================================================

#[test]
fn fmt_imports_sorted_alphabetically() {
    let unsorted = "import { zebra } from \"./z\"\nimport { alpha } from \"./a\"\nfunc main():\n    print(\"hi\")\n";
    let out = fmt::format_source(unsorted).expect("format");
    let alpha_pos = out.find("alpha").unwrap_or(usize::MAX);
    let zebra_pos = out.find("zebra").unwrap_or(usize::MAX);
    assert!(
        alpha_pos < zebra_pos,
        "alpha should sort before zebra. Output:\n{out}"
    );
}

#[test]
fn fmt_imports_groups_with_blank_line_before_other_decls() {
    let src = "import { alpha } from \"./a\"\nfunc main():\n    print(alpha())\n";
    let out = fmt::format_source(src).expect("format");
    // After the import line + blank line, the function begins.
    assert!(
        out.contains("from \"./a\"\n\nfunc main"),
        "expected blank line between imports and other decls. Output:\n{out}"
    );
}

#[test]
fn fmt_imports_stable_when_already_sorted() {
    let sorted = "import { alpha } from \"./a\"\nimport { beta } from \"./b\"\nfunc main():\n    print(\"hi\")\n";
    let out = fmt::format_source(sorted).expect("format");
    // Already-sorted imports should round-trip (no reshuffling).
    let alpha_pos = out.find("alpha").unwrap_or(usize::MAX);
    let beta_pos = out.find("beta").unwrap_or(usize::MAX);
    assert!(alpha_pos < beta_pos);
}

// ===========================================================================
// 6. `--check` mode — drives the CLI command end-to-end via the library.
// ===========================================================================

/// Drive the `buff fmt --check` command path on a temp file.
fn run_check(temp_path: &std::path::Path, contents: &str) -> anyhow::Result<()> {
    let mut f = std::fs::File::create(temp_path).expect("create temp file");
    f.write_all(contents.as_bytes()).expect("write temp");
    drop(f);
    let outcome = buff_lang_cli::commands::fmt::run(temp_path, /* check */ true)?;
    use buff_lang_cli::commands::fmt::FmtOutcome;
    match outcome {
        FmtOutcome::AlreadyFormatted => Ok(()),
        FmtOutcome::NeedsFormat => Err(anyhow::anyhow!(
            "file needs formatting (would exit 1 in CLI)"
        )),
        // `Formatted` shouldn't happen in check mode but treat as ok.
        FmtOutcome::Formatted => Ok(()),
    }
}

#[test]
fn fmt_check_already_formatted_returns_ok() {
    let dir = std::env::temp_dir().join("buff-fmt-tests");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("already_formatted.buff");
    let canonical = "func main():\n    print(\"hello\")\n";
    run_check(&path, canonical).expect("check should pass on already-formatted file");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fmt_check_unformatted_returns_needs_format() {
    let dir = std::env::temp_dir().join("buff-fmt-tests");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("unformatted.buff");
    // 2-space indent — the formatter would change this.
    let unformatted = "func main():\n  print(\"hello\")\n";
    let result = run_check(&path, unformatted);
    assert!(
        result.is_err(),
        "expected check to signal NeedsFormat (Err) on un-formatted file"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fmt_check_unformatted_outcome_enum_value() {
    // Stronger variant: assert the exact outcome enum value (the CLI
    // translates this to exit 1).
    let dir = std::env::temp_dir().join("buff-fmt-tests");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("outcome_enum.buff");
    std::fs::write(&path, "func main():\n  print(\"hello\")\n").expect("write temp");
    let outcome = buff_lang_cli::commands::fmt::run(&path, /* check */ true).expect("ok");
    use buff_lang_cli::commands::fmt::FmtOutcome;
    assert_eq!(
        outcome,
        FmtOutcome::NeedsFormat,
        "expected NeedsFormat for un-formatted file"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fmt_write_mode_round_trip() {
    // Write mode: format an unformatted temp file in place, then verify
    // the file's contents are the canonical form.
    let dir = std::env::temp_dir().join("buff-fmt-tests");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("write_mode.buff");
    let unformatted = "func main():\n  print(\"hello\")\n";
    std::fs::write(&path, unformatted).expect("write temp");

    let outcome = buff_lang_cli::commands::fmt::run(&path, /* check */ false).expect("format");
    use buff_lang_cli::commands::fmt::FmtOutcome;
    assert_eq!(outcome, FmtOutcome::Formatted);

    let after = std::fs::read_to_string(&path).expect("read after format");
    let expected = fmt::format_source(unformatted).expect("canonical");
    assert_eq!(after, expected, "file content after write-mode format");
    let _ = std::fs::remove_file(&path);
}

// ===========================================================================
// 7. Determinism — same input → byte-identical output across calls.
// ===========================================================================

#[test]
fn fmt_deterministic_across_calls() {
    let src = "func main():\n    let x = 1\n    print(x)\n";
    let o1 = fmt::format_source(src).expect("format");
    let o2 = fmt::format_source(src).expect("format");
    let o3 = fmt::format_source(src).expect("format");
    assert_eq!(o1, o2);
    assert_eq!(o2, o3);
}

// ===========================================================================
// 8. Error reporting — malformed input surfaces a FormatError.
// ===========================================================================

#[test]
fn fmt_lexer_error_propagates() {
    // Unterminated string → LexerError.
    let bad = "func main():\n    print(\"oops)\n";
    let result = fmt::format_source(bad);
    assert!(result.is_err(), "expected an error on unterminated string");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.to_lowercase().contains("string")
            || err_msg.to_lowercase().contains("unterminated"),
        "expected unterminated-string message, got: {err_msg}"
    );
}

#[test]
fn fmt_parse_error_propagates() {
    // Top-level statement (not a function) → ParseError.
    let bad = "let x = 1\n";
    let result = fmt::format_source(bad);
    assert!(result.is_err(), "expected an error on top-level let");
}

// ===========================================================================
// 9. is_already_formatted helper.
// ===========================================================================

#[test]
fn fmt_is_already_formatted_true_on_canonical() {
    let canonical = "func main():\n    print(\"hi\")\n";
    assert!(fmt::is_already_formatted(canonical).expect("check"));
}

#[test]
fn fmt_is_already_formatted_false_on_unformatted() {
    let unformatted = "func main():\n  print(\"hi\")\n";
    assert!(!fmt::is_already_formatted(unformatted).expect("check"));
}
