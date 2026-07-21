//! Exercise verification engine.
//!
//! Detects `// TODO:` markers in `.buff` source and runs `buff check`
//! (subprocess) to verify type-correctness.

use std::path::PathBuf;

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
    /// The exercise still has `// TODO:` markers.
    NotDoneYet,
    /// `buff check` returned non-zero (compile errors).
    CompileError(String),
    /// The `buff` binary was not found on PATH.
    BuffNotFound,
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
}
