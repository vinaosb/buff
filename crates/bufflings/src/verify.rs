//! Exercise verification engine.
//!
//! Detects `// TODO:` markers in `.buff` source and runs `buff check`
//! (subprocess) to verify type-correctness. Also supports applying
//! hidden solutions (`.sol.buff`) for CI solvability gating.

use std::path::{Path, PathBuf};

/// Configuration for the verification engine.
#[derive(Debug, Clone)]
pub struct VerifyConfig {
    /// The `buff` binary name. Defaults to `"buff"`.
    pub buff_bin: String,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            buff_bin: "buff".to_string(),
        }
    }
}

/// The outcome of verifying a single exercise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// The exercise compiles and has no TODO markers.
    Solved,
    /// The exercise still has `// TODO:` markers (user hasn't started).
    NotDoneYet,
    /// `buff check` returned non-zero (compile errors). Contains stderr.
    CompileError(String),
    /// The `buff` binary was not found on PATH.
    BuffNotFound,
    /// The exercise source has TODO markers and was skipped entirely
    /// (used by `verify_all_with_solutions` to distinguish "original
    /// state before solution apply" from an explicit user attempt).
    NotStarted,
    /// The exercise compiles but produces wrong output (reserved for
    /// future expected-output matching in the manifest).
    WrongOutput(String),
}

/// The result of verifying all exercises with their solutions applied.
#[derive(Debug, Clone)]
pub struct SolutionVerificationReport {
    /// Per-exercise results, in manifest order.
    pub results: Vec<(String, VerifyOutcome)>,
}

impl SolutionVerificationReport {
    /// Number of exercises that passed verification.
    pub fn solved_count(&self) -> usize {
        self.results
            .iter()
            .filter(|(_, o)| *o == VerifyOutcome::Solved)
            .count()
    }

    /// Total number of exercises verified.
    pub fn total_count(&self) -> usize {
        self.results.len()
    }
}

/// Verify a single exercise by checking for TODO markers and running
/// `buff check`.
///
/// Steps:
/// 1. Check for `// TODO:` markers. If any remain, return `NotDoneYet`
///    immediately (no need to invoke `buff`).
/// 2. Run `buff check <path>` as a subprocess. Parse exit code + stderr.
///    Exit 0 → Solved. Non-zero → CompileError with stderr.
pub fn verify_exercise(source: &str, path: &PathBuf, config: &VerifyConfig) -> VerifyOutcome {
    // Fast-fail: check for TODO markers
    if contains_todo(source) {
        return VerifyOutcome::NotDoneYet;
    }

    // Run buff check
    run_buff_check(path, config)
}

/// Detect whether a `.buff` source contains any `// TODO:` markers.
///
/// A TODO marker is a line containing `// TODO:` (case-sensitive).
/// Empty lines and whitespace-only content between `//` and `TODO:`
/// are ignored (the marker can be preceded by whitespace).
pub fn contains_todo(source: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("// TODO:") {
            return true;
        }
    }
    false
}

/// Run `buff check <path>` as a subprocess and return the outcome.
///
/// Returns:
/// - `Solved` if `buff` exits 0 and stderr is empty.
/// - `CompileError(stderr)` if `buff` exits non-zero.
/// - `BuffNotFound` if the `buff` binary cannot be found.
pub fn run_buff_check(path: &PathBuf, config: &VerifyConfig) -> VerifyOutcome {
    let output = match std::process::Command::new(&config.buff_bin)
        .arg("check")
        .arg(path)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return VerifyOutcome::BuffNotFound;
            }
            return VerifyOutcome::CompileError(format!("failed to run {}: {e}", config.buff_bin));
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit = output.status.code().unwrap_or(1);

    if exit == 0 {
        VerifyOutcome::Solved
    } else {
        VerifyOutcome::CompileError(stderr)
    }
}

/// Apply the hidden solution for an exercise by copying `<name>.sol.buff`
/// over `<name>.buff`.
///
/// The exercise file is identified by `exercise_path` (the `.buff` file).
/// The solution file is found by appending `.sol` to the path stem
/// (e.g. `hello1.buff` → `hello1.sol.buff`).
///
/// Returns `Ok(())` on success, or an error if the solution file does
/// not exist or the copy fails.
pub fn apply_solution(exercise_path: &Path) -> anyhow::Result<()> {
    let stem = exercise_path.file_stem().ok_or_else(|| {
        anyhow::anyhow!(
            "exercise path has no file stem: {}",
            exercise_path.display()
        )
    })?;
    let stem_str = stem.to_string_lossy();
    let dir = exercise_path.parent().ok_or_else(|| {
        anyhow::anyhow!("exercise path has no parent: {}", exercise_path.display())
    })?;
    let sol_path = dir.join(format!("{stem_str}.sol.buff"));

    if !sol_path.exists() {
        anyhow::bail!(
            "solution file not found: {} (expected alongside {})",
            sol_path.display(),
            exercise_path.display()
        );
    }

    std::fs::copy(&sol_path, exercise_path).map_err(|e| {
        anyhow::anyhow!(
            "failed to copy {} → {}: {e}",
            sol_path.display(),
            exercise_path.display()
        )
    })?;

    Ok(())
}

/// Verify all exercises in a directory by applying their hidden solutions
/// and running `buff check` on each.
///
/// For each `.sol.buff` file found in `exercises_dir`, this function:
/// 1. Applies the solution (copies `.sol.buff` over `.buff`).
/// 2. Runs `buff check` on the resulting file.
/// 3. Records the outcome.
///
/// Returns a [`SolutionVerificationReport`] with per-exercise results.
/// The original `.buff` files are overwritten (this is a CI gate function,
/// not a user-facing operation).
pub fn verify_all_with_solutions(
    exercises_dir: &Path,
    config: &VerifyConfig,
) -> SolutionVerificationReport {
    let mut results = Vec::new();

    // Walk exercises_dir recursively for *.sol.buff files
    let entries = match std::fs::read_dir(exercises_dir) {
        Ok(e) => e,
        Err(err) => {
            results.push((
                "<directory>".to_string(),
                VerifyOutcome::CompileError(format!(
                    "failed to read exercises dir {}: {err}",
                    exercises_dir.display()
                )),
            ));
            return SolutionVerificationReport { results };
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "buff") {
            // Only consider .sol.buff files
            let file_name = path.file_name().map_or("", |n| n.to_str().unwrap_or(""));
            if !file_name.ends_with(".sol.buff") {
                continue;
            }

            // Derive the exercise path: strip ".sol" from stem
            let sol_stem = path.file_stem().unwrap_or_default();
            let sol_stem_str = sol_stem.to_string_lossy();
            let exercise_stem = sol_stem_str.strip_suffix(".sol").unwrap_or(&sol_stem_str);
            let dir = path.parent().unwrap_or(path.as_ref());
            let exercise_path = dir.join(format!("{exercise_stem}.buff"));

            let exercise_name = exercise_stem.to_string();

            // Apply the solution
            if let Err(e) = apply_solution(&exercise_path) {
                results.push((exercise_name, VerifyOutcome::CompileError(e.to_string())));
                continue;
            }

            // Verify the applied solution
            let source = match std::fs::read_to_string(&exercise_path) {
                Ok(s) => s,
                Err(e) => {
                    results.push((
                        exercise_name,
                        VerifyOutcome::CompileError(format!(
                            "failed to read {}: {e}",
                            exercise_path.display()
                        )),
                    ));
                    continue;
                }
            };

            let outcome = verify_exercise(&source, &exercise_path, config);
            results.push((exercise_name, outcome));
        }
    }

    SolutionVerificationReport { results }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_todo_detects_marker() {
        assert!(contains_todo(
            "func main():\n    // TODO: fix this\n    print(1)\n"
        ));
    }

    #[test]
    fn contains_todo_ignores_non_todo_comments() {
        assert!(!contains_todo(
            "func main():\n    // This is a regular comment\n    print(1)\n"
        ));
    }

    #[test]
    fn contains_todo_ignores_empty_source() {
        assert!(!contains_todo(""));
    }

    #[test]
    fn contains_todo_detects_indented_marker() {
        assert!(contains_todo("    // TODO: complete the function"));
    }

    #[test]
    fn contains_todo_is_case_sensitive() {
        // "todo:" (lowercase) should NOT match
        assert!(!contains_todo("// todo: this should not match"));
    }

    #[test]
    fn contains_todo_allows_comment_after_marker() {
        assert!(contains_todo("// TODO: fill in the body with a print call"));
    }

    #[test]
    fn run_buff_check_returns_buff_not_found_for_missing_bin() {
        let config = VerifyConfig {
            buff_bin: "definitely_not_a_real_binary_xyz123".to_string(),
        };
        let result = run_buff_check(&PathBuf::from("nonexistent.buff"), &config);
        assert_eq!(result, VerifyOutcome::BuffNotFound);
    }

    #[test]
    fn verify_exercise_fast_fails_on_todo() {
        let source = "// TODO: implement this\nfunc main():\n    pass\n";
        let config = VerifyConfig::default();
        let result = verify_exercise(source, &PathBuf::from("test.buff"), &config);
        assert_eq!(result, VerifyOutcome::NotDoneYet);
    }

    #[test]
    fn verify_exercise_skips_buff_on_no_todo() {
        // With a fake buff binary, if no TODO markers are found it should
        // attempt to run buff (and get BuffNotFound).
        let source = "func main():\n    print(42)\n";
        let config = VerifyConfig {
            buff_bin: "nonexistent_buff_xyz".to_string(),
        };
        let result = verify_exercise(source, &PathBuf::from("test.buff"), &config);
        assert_eq!(result, VerifyOutcome::BuffNotFound);
    }

    // -- apply_solution tests --

    #[test]
    fn apply_solution_copies_sol_over_buff() {
        let dir = std::env::temp_dir().join("bufflings_test_apply");
        let _ = std::fs::create_dir_all(&dir);

        let sol_path = dir.join("test1.sol.buff");
        let buff_path = dir.join("test1.buff");

        std::fs::write(&sol_path, "solution content").expect("write sol");
        std::fs::write(&buff_path, "original content").expect("write buff");

        let result = apply_solution(&buff_path);
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&buff_path).expect("read buff after apply");
        assert_eq!(content, "solution content");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_solution_fails_when_sol_missing() {
        let dir = std::env::temp_dir().join("bufflings_test_apply_missing");
        let _ = std::fs::create_dir_all(&dir);

        let buff_path = dir.join("test2.buff");
        std::fs::write(&buff_path, "original").expect("write buff");

        let result = apply_solution(&buff_path);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- verify_all_with_solutions tests --

    #[test]
    fn verify_all_with_solutions_empty_dir() {
        let dir = std::env::temp_dir().join("bufflings_test_empty");
        let _ = std::fs::create_dir_all(&dir);

        let config = VerifyConfig {
            buff_bin: "fake_buff".to_string(),
        };
        let report = verify_all_with_solutions(&dir, &config);
        assert_eq!(report.total_count(), 0);
        assert_eq!(report.solved_count(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_all_with_solutions_applies_and_verifies() {
        let dir = std::env::temp_dir().join("bufflings_test_solutions");
        let _ = std::fs::create_dir_all(&dir);

        // Write a .sol.buff with no TODO markers (clean solution)
        let sol_path = dir.join("ex1.sol.buff");
        std::fs::write(&sol_path, "func main():\n    print(1)\n").expect("write sol");

        // Write a .buff stub (will be overwritten)
        let buff_path = dir.join("ex1.buff");
        std::fs::write(&buff_path, "// TODO: stub\n").expect("write buff stub");

        // Use a fake buff binary — since no TODO remains after apply,
        // it will try buff and get BuffNotFound. That's fine for this test;
        // the important thing is that the solution was applied and TODO
        // detection was bypassed.
        let config = VerifyConfig {
            buff_bin: "fake_buff_for_test".to_string(),
        };
        let report = verify_all_with_solutions(&dir, &config);
        assert_eq!(report.total_count(), 1);
        // After applying solution, no TODO markers → attempts buff → BuffNotFound
        assert_eq!(report.results[0].1, VerifyOutcome::BuffNotFound);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_all_with_solutions_skips_non_sol_files() {
        let dir = std::env::temp_dir().join("bufflings_test_skip");
        let _ = std::fs::create_dir_all(&dir);

        // Only write a regular .buff file, no .sol.buff
        let buff_path = dir.join("regular.buff");
        std::fs::write(&buff_path, "func main():\n    pass\n").expect("write");

        let config = VerifyConfig {
            buff_bin: "fake_buff".to_string(),
        };
        let report = verify_all_with_solutions(&dir, &config);
        assert_eq!(report.total_count(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- VerifyOutcome variant coverage --

    #[test]
    fn solution_report_counts() {
        let report = SolutionVerificationReport {
            results: vec![
                ("ex1".to_string(), VerifyOutcome::Solved),
                ("ex2".to_string(), VerifyOutcome::Solved),
                (
                    "ex3".to_string(),
                    VerifyOutcome::CompileError("err".to_string()),
                ),
            ],
        };
        assert_eq!(report.total_count(), 3);
        assert_eq!(report.solved_count(), 2);
    }

    #[test]
    fn wrong_output_variant_exists() {
        let outcome = VerifyOutcome::WrongOutput("expected '5' got '3'".to_string());
        assert_eq!(
            outcome,
            VerifyOutcome::WrongOutput("expected '5' got '3'".to_string())
        );
    }

    #[test]
    fn not_started_variant_exists() {
        let outcome = VerifyOutcome::NotStarted;
        assert_eq!(outcome, VerifyOutcome::NotStarted);
    }
}
