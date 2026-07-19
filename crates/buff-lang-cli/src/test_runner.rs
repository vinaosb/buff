//! Test runner for the `buff test` command (T35).
//!
//! Discovers `@test` functions in a parsed Buff AST, generates a
//! self-contained Rust test-harness binary, compiles + runs it, and parses
//! the output into a [`TestReport`].
//!
//! ## Design choices
//!
//! - **Custom runner, not `rustc --test`**: the QA requires the output
//!   format `<n> passed, <m> failed`; Rust's built-in `--test` harness
//!   prints `1 passed; 0 failed` (different wording + semicolon). A custom
//!   runner also avoids the `#[test]`-fn-vs-user-`main` conflict.
//! - **Deterministic test order**: discovered test names are collected into
//!   a [`BTreeSet`] (sorted) so repeated `buff test` runs on the same input
//!   produce byte-identical output. (T29 lesson: never rely on HashMap
//!   iteration order.)
//! - **`--pattern` filtering**: a simple glob where `*` matches any
//!   sequence of characters. `test_*` matches `test_foo`; `*` matches
//!   everything; an empty pattern matches all tests.
//! - **Pure, unit-testable core**: [`discover_test_names`],
//!   [`matches_pattern`], and [`parse_report`] are pure functions that
//!   don't touch the filesystem or spawn processes — so they can be
//!   exercised without `rustc` on `PATH`.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use buff_lang_ast::Decl;
use buff_lang_codegen_rust::generate_test_rust;
use buff_lang_error::{SourceFile, SourceId};
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// The outcome of running `buff test` on a file.
///
/// Carries the total/pass/fail counts plus the list of failed test names
/// (sorted, deterministic) and the raw captured stdout of the test binary.
/// The [`exit_code`] is `0` when `failed == 0`, else `1`.
///
/// [`exit_code`]: TestReport::exit_code
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestReport {
    /// Total number of tests discovered (after `--pattern` filtering).
    pub total: usize,
    /// Number of tests that passed.
    pub passed: usize,
    /// Number of tests that failed (panicked).
    pub failed: usize,
    /// Sorted list of failed test names.
    pub failures: Vec<String>,
    /// The full captured stdout of the test binary (for diagnostics).
    pub raw_output: String,
}

impl TestReport {
    /// The process exit code for this report: `0` if all tests passed,
    /// `1` if any failed.
    pub fn exit_code(&self) -> i32 {
        if self.failed == 0 {
            0
        } else {
            1
        }
    }

    /// Format the report for display on stderr (after the test binary's own
    /// stdout has already been forwarded). Mirrors the `buff run` pattern of
    /// forwarding program stdout and adding a short summary.
    pub fn summary_line(&self) -> String {
        format!(
            "{} test(s): {} passed, {} failed",
            self.total, self.passed, self.failed
        )
    }
}

// ---------------------------------------------------------------------------
// Pure functions (unit-testable without rustc)
// ---------------------------------------------------------------------------

/// Scan a slice of parsed [`Decl`]s for `@test` functions whose names match
/// `pattern`, returning the matching names **sorted** (deterministic order).
///
/// `pattern` is a simple glob: `*` matches any sequence of characters, any
/// other character matches literally. An empty pattern matches ALL tests
/// (equivalent to `*`). See [`matches_pattern`].
///
/// Functions wrapped in `Decl::ExportDecl` are unwrapped: `@test export
/// func ...` is a parse error today (the dispatcher rejects attributes on
/// `export`), but defensive unwrapping keeps discovery robust if that
/// restriction is later relaxed.
pub fn discover_test_names(decls: &[Decl], pattern: &str) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for decl in decls {
        // Unwrap `export <decl>` so `@test export func ...` (if ever allowed)
        // would still be discovered. Today the parser rejects attributes on
        // `export`, so this branch is defensive.
        let inner: &Decl = match decl {
            Decl::ExportDecl(exp) => exp.inner.as_ref(),
            other => other,
        };
        if let Decl::FuncDecl(f) = inner {
            if f.attributes.iter().any(|a| a.name.name == "test") {
                let name = f.name.name.clone();
                if pattern.is_empty() || matches_pattern(&name, pattern) {
                    names.insert(name);
                }
            }
        }
    }
    names.into_iter().collect()
}

/// Simple glob match: `*` matches any sequence (including empty), all other
/// characters match literally. Case-sensitive.
///
/// Examples:
/// - `matches_pattern("test_foo", "test_*")` → `true`
/// - `matches_pattern("my_test", "test_*")` → `false`
/// - `matches_pattern("anything", "*")` → `true`
/// - `matches_pattern("test_foo", "")` → `true` (empty pattern = match all)
/// - `matches_pattern("test_foo", "test_foo")` → `true` (exact match)
pub fn matches_pattern(name: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    glob_match(name.as_bytes(), pattern.as_bytes())
}

/// Recursive glob matcher on byte slices. `*` in the pattern consumes zero
/// or more bytes of the name; all other bytes must match exactly.
fn glob_match(name: &[u8], pattern: &[u8]) -> bool {
    match (name, pattern) {
        ([], []) => true,
        ([], [b'*', rest @ ..]) => glob_match(name, rest),
        ([], _) => false,
        ([_, ..], []) => false,
        ([_nc, nrest @ ..], [b'*', prest @ ..]) => {
            // `*` matches zero chars (try skipping the star) or one+ chars
            // (try advancing the name past the current char).
            glob_match(name, prest) || glob_match(nrest, pattern)
        }
        ([nc, nrest @ ..], [pc, prest @ ..]) if nc == pc => glob_match(nrest, prest),
        _ => false,
    }
}

/// Parse the stdout of the generated test binary into a [`TestReport`].
///
/// The harness prints, per test: `test <name> ... ok` or `test <name> ...
/// FAILED`, then a blank line, then `<passed> passed, <failed> failed`.
/// This function extracts the counts and the failed-test names from that
/// output. If the output doesn't match the expected shape (e.g. the binary
/// crashed before printing the summary), a best-effort report is returned
/// with the counts inferred from whatever lines ARE present.
pub fn parse_report(output: &str, total: usize) -> TestReport {
    let mut failures: Vec<String> = Vec::new();
    let mut passed: usize = 0;
    let mut failed: usize = 0;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("test ") {
            if let Some(test_name) = rest.strip_suffix(" ... ok") {
                let _ = test_name;
                passed += 1;
            } else if let Some(test_name) = rest.strip_suffix(" ... FAILED") {
                failures.push(test_name.to_string());
                failed += 1;
            }
        }
        // Also try to parse the summary line as a cross-check.
        if let Some(counts) = parse_summary_counts(trimmed) {
            passed = counts.0;
            failed = counts.1;
        }
    }

    // Fallback: if per-test lines weren't printed (binary crashed before the
    // loop) but the total is known, report all as failed.
    if passed + failed == 0 && total > 0 {
        failed = total;
    }

    TestReport {
        total,
        passed,
        failed,
        failures,
        raw_output: output.to_string(),
    }
}

/// Parse a `<n> passed, <m> failed` summary line. Returns `None` if the
/// line doesn't match the expected shape.
fn parse_summary_counts(line: &str) -> Option<(usize, usize)> {
    let mid = line.strip_suffix(" failed")?;
    let comma_idx = mid.rfind(" passed, ")?;
    let passed_str = &mid[..comma_idx];
    let failed_str = &mid[comma_idx + " passed, ".len()..];
    let passed: usize = passed_str.parse().ok()?;
    let failed: usize = failed_str.parse().ok()?;
    Some((passed, failed))
}

// ---------------------------------------------------------------------------
// Filesystem + process orchestration (requires rustc)
// ---------------------------------------------------------------------------

/// Run the full `buff test` pipeline on `file` with an optional `--pattern`.
///
/// Steps:
/// 1. Read + lex + parse the `.buff` file.
/// 2. Discover `@test` functions matching `pattern`.
/// 3. If no tests → return an empty report (exit 0).
/// 4. Generate the Rust test harness via [`generate_test_rust`].
/// 5. Write the harness to `<file>.test.rs`.
/// 6. Compile with `rustc` (normal mode).
/// 7. Execute the test binary, capturing stdout.
/// 8. Parse the output into a [`TestReport`].
/// 9. Clean up the binary + `.test.rs` file.
///
/// # Errors
///
/// Returns an error if any pipeline step fails (file-not-found, lex/parse
/// failure, codegen failure, rustc invocation failure, or the binary fails
/// to spawn). A failing TEST (assertion panic) is NOT an error — it's
/// reflected in the [`TestReport`] counts.
pub fn run_tests(file: &Path, pattern: &str) -> Result<TestReport> {
    // 1. Read + lex + parse.
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read source file `{}`", file.display()))?;
    let source_id = SourceId(0);
    let source_file = SourceFile::new(file.to_path_buf(), source.clone());
    let tokens = tokenize(&source, source_id).map_err(|e| {
        crate::pipeline::format_diagnostic_error("lex", &e.inner.diagnostic, &source_file, file)
    })?;
    let decls = parse(&tokens, source_id).map_err(|e| {
        crate::pipeline::format_diagnostic_error("parse", &e.diagnostic, &source_file, file)
    })?;

    // 2. Discover + filter tests.
    let test_names = discover_test_names(&decls, pattern);

    // 3. No tests → graceful empty report.
    if test_names.is_empty() {
        let pat_msg = if pattern.is_empty() {
            String::new()
        } else {
            format!(" matching pattern `{pattern}`")
        };
        eprintln!("no `@test` functions found{pat_msg} in {}", file.display());
        return Ok(TestReport {
            total: 0,
            passed: 0,
            failed: 0,
            failures: Vec::new(),
            raw_output: String::new(),
        });
    }

    // 4. Generate the Rust test harness.
    let rust_source = generate_test_rust(&decls, &test_names).map_err(|e| {
        crate::pipeline::format_diagnostic_error("codegen", &e.diagnostic, &source_file, file)
    })?;

    // 5. Write the harness .rs file to a TEMP dir (not alongside the source)
    //    so the crate name rustc infers is clean (no dots). Writing next to
    //    the source as `<file>.test.rs` would give rustc the crate name
    //    `<stem>.test` which is invalid (dots aren't allowed in crate names).
    let temp_dir = std::env::temp_dir().join("buff-test");
    std::fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create temp dir `{}`", temp_dir.display()))?;
    let stem = file
        .file_stem()
        .map(|s| s.to_owned())
        .unwrap_or_else(|| std::ffi::OsString::from("buff_test"));
    let rust_file = temp_dir.join(format!("{}_test.rs", stem.to_string_lossy()));
    std::fs::write(&rust_file, &rust_source)
        .with_context(|| format!("failed to write `{}`", rust_file.display()))?;

    // 6. Compile with rustc (reuse the existing pipeline helper so rustc
    //    diagnostics are translated `.rs`→`.buff`). Test compilation stays
    //    in BuildMode::Debug for fast iteration — release-grade LTO would
    //    slow the test loop without changing which tests pass.
    let exe_stem = crate::pipeline::with_exe_extension(
        &temp_dir.join(format!("{}_test", stem.to_string_lossy())),
    );
    let exe_path = crate::pipeline::compile_rust_to_exe(
        &rust_file,
        &exe_stem,
        file,
        crate::pipeline::BuildMode::Debug,
    )?;

    // 7. Execute, capturing stdout.
    let output = Command::new(&exe_path)
        .output()
        .with_context(|| format!("failed to execute `{}`", exe_path.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    // Forward the test binary's stdout + stderr.
    use std::io::Write;
    if !output.stdout.is_empty() {
        let _ = std::io::stdout().write_all(&output.stdout);
        let _ = std::io::stdout().flush();
    }
    if !output.stderr.is_empty() {
        let _ = std::io::stderr().write_all(&output.stderr);
        let _ = std::io::stderr().flush();
    }

    // 8. Parse the output.
    let report = parse_report(&stdout, test_names.len());

    // 9. Clean up (best-effort, mirroring `buff run`).
    let _ = std::fs::remove_file(&exe_path);
    let _ = std::fs::remove_file(&rust_file);

    Ok(report)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- matches_pattern -----------------------------------------------

    #[test]
    fn glob_star_prefix() {
        assert!(matches_pattern("test_foo", "test_*"));
        assert!(matches_pattern("test_", "test_*"));
        assert!(matches_pattern("test_bar_baz", "test_*"));
    }

    #[test]
    fn glob_star_does_not_match_unprefixed() {
        assert!(!matches_pattern("my_test", "test_*"));
        assert!(!matches_pattern("other", "test_*"));
    }

    #[test]
    fn glob_star_only_matches_all() {
        assert!(matches_pattern("anything", "*"));
        assert!(matches_pattern("", "*"));
    }

    #[test]
    fn glob_exact_match() {
        assert!(matches_pattern("test_foo", "test_foo"));
        assert!(!matches_pattern("test_foo", "test_bar"));
    }

    #[test]
    fn glob_empty_pattern_matches_all() {
        assert!(matches_pattern("test_foo", ""));
        assert!(matches_pattern("anything", ""));
    }

    #[test]
    fn glob_star_middle() {
        assert!(matches_pattern("test_foo_bar", "test_*_bar"));
        assert!(!matches_pattern("test_foo_baz", "test_*_bar"));
    }

    // --- parse_report --------------------------------------------------

    #[test]
    fn parse_all_pass() {
        let out = "test test_a ... ok\ntest test_b ... ok\n\n2 passed, 0 failed\n";
        let r = parse_report(out, 2);
        assert_eq!(r.total, 2);
        assert_eq!(r.passed, 2);
        assert_eq!(r.failed, 0);
        assert!(r.failures.is_empty());
    }

    #[test]
    fn parse_one_fail() {
        let out = "test test_a ... ok\ntest test_b ... FAILED\n\n1 passed, 1 failed\n";
        let r = parse_report(out, 2);
        assert_eq!(r.passed, 1);
        assert_eq!(r.failed, 1);
        assert_eq!(r.failures, vec!["test_b".to_string()]);
    }

    #[test]
    fn parse_crash_no_summary() {
        // Binary crashed before printing summary lines — only total is known.
        let r = parse_report("", 3);
        assert_eq!(r.total, 3);
        assert_eq!(r.passed, 0);
        assert_eq!(r.failed, 3);
    }

    #[test]
    fn exit_code_logic() {
        let pass = TestReport {
            total: 2,
            passed: 2,
            failed: 0,
            failures: Vec::new(),
            raw_output: String::new(),
        };
        assert_eq!(pass.exit_code(), 0);
        let fail = TestReport {
            total: 2,
            passed: 1,
            failed: 1,
            failures: vec!["x".into()],
            raw_output: String::new(),
        };
        assert_eq!(fail.exit_code(), 1);
    }

    // --- parse_summary_counts ------------------------------------------

    #[test]
    fn summary_counts_parsed() {
        assert_eq!(parse_summary_counts("3 passed, 1 failed"), Some((3, 1)));
        assert_eq!(parse_summary_counts("0 passed, 0 failed"), Some((0, 0)));
        assert_eq!(parse_summary_counts("not a summary"), None);
    }
}
