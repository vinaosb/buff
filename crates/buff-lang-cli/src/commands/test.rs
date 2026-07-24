//! `buff test` — snapshot testing runner (T100).
//!
//! Discovers test files by convention (files ending in `.test.buff` or
//! files inside a `test/` directory), compiles + runs each, and compares
//! stdout against a stored `.snap` snapshot file (insta-style).
//!
//! # Snapshot format
//!
//! Snapshot files are stored alongside the test file with a `.snap`
//! extension. The format is the raw expected stdout of the test binary:
//!
//! ```text
//! ---
//! source: tests/test_foo.test.buff
//! ---
//! expected output line 1
//! expected output line 2
//! ```
//!
//! The `---` header is optional — the snapshot content is everything
//! after the second `---` line (or the entire file if no header).
//!
//! # Flags
//!
//! - `--update`: accept new/changed snapshots (writes current output as
//!   the new `.snap` file). Missing snapshots are created automatically.
//! - `--filter <PATTERN>`: only run tests whose file name matches the
//!   given glob pattern (simple `*` glob, same as `--pattern` in T35).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::pipeline;

/// Entry point for `buff test <PATH> [--filter <PATTERN>] [--update] [--detect-races]`.
pub fn run(path: &Path, filter: Option<&str>, update: bool, detect_races: bool) -> Result<()> {
    // 1. Discover test files.
    let test_files = discover_test_files(path, filter)?;

    if test_files.is_empty() {
        let filter_msg = filter
            .map(|p| format!(" matching filter `{p}`"))
            .unwrap_or_default();
        eprintln!("no test files found{filter_msg} in `{}`", path.display());
        return Ok(());
    }

    // 2. Run each test file.
    let total = test_files.len();
    let mut passed: usize = 0;
    let mut failed: usize = 0;
    let mut failures: Vec<String> = Vec::new();

    for test_file in &test_files {
        let test_name = test_file
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| test_file.to_string_lossy().to_string());

        eprint!("test {test_name} ... ");

        match run_single_test(test_file, update, detect_races) {
            Ok(true) => {
                eprintln!("ok");
                passed += 1;
            }
            Ok(false) => {
                eprintln!("FAILED");
                failed += 1;
                failures.push(test_name);
            }
            Err(e) => {
                eprintln!("ERROR: {e:#}");
                failed += 1;
                failures.push(test_name);
            }
        }
    }

    // 3. Print summary.
    eprintln!();
    eprintln!("{total} test(s): {passed} passed, {failed} failed");

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Discover test files under `path` matching the optional `filter` glob.
///
/// Convention:
/// - If `path` is a file, it's the single test file.
/// - If `path` is a directory, recursively finds files ending in
///   `.test.buff` OR files inside a `test/` subdirectory.
/// - `filter` is a simple `*` glob applied to the file name (stem).
fn discover_test_files(path: &Path, filter: Option<&str>) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }

    if !path.is_dir() {
        anyhow::bail!(
            "`{}` is neither a file nor a directory",
            path.display()
        );
    }

    let mut files: Vec<PathBuf> = Vec::new();
    collect_test_files(path, &mut files);

    // Apply filter to file stems.
    if let Some(pattern) = filter {
        let pattern = pattern.to_string();
        files.retain(|f| {
            let stem = f
                .file_stem()
                .map(|s| s.to_string_lossy())
                .unwrap_or_default();
            crate::test_runner::matches_pattern(&stem, &pattern)
        });
    }

    // Sort for deterministic order.
    files.sort();
    Ok(files)
}

/// Recursively collect test files from `dir`.
fn collect_test_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();

        if entry_path.is_dir() {
            // Recurse into subdirectories.
            collect_test_files(&entry_path, files);
        } else if entry_path.is_file() {
            let file_name = entry_path.file_name().map(|n| n.to_string_lossy());
            let Some(file_name) = file_name else {
                continue;
            };

            // Convention: files ending in `.test.buff` or inside a `test/` dir.
            let is_test_suffix = file_name.ends_with(".test.buff");
            let is_in_test_dir = entry_path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n == "test")
                .unwrap_or(false);

            if is_test_suffix || is_in_test_dir {
                files.push(entry_path);
            }
        }
    }
}

/// Run a single test file: compile, execute, compare stdout to snapshot.
///
/// Returns `Ok(true)` if the output matches the snapshot (or if `--update`
/// wrote a new snapshot). Returns `Ok(false)` if the output differs.
fn run_single_test(test_file: &Path, update: bool, detect_races: bool) -> Result<bool> {
    // 1. Compile the test file to a temporary executable.
    let source = std::fs::read_to_string(test_file)
        .with_context(|| format!("failed to read `{}`", test_file.display()))?;

    let source_id = buff_lang_error::SourceId(0);
    let source_file = buff_lang_error::SourceFile::new(test_file.to_path_buf(), source.clone());
    let tokens = buff_lang_lexer::tokenize(&source, source_id).map_err(|e| {
        pipeline::format_diagnostic_error("lex", &e.inner.diagnostic, &source_file, test_file)
    })?;
    let decls = buff_lang_parser::parse(&tokens, source_id).map_err(|e| {
        pipeline::format_diagnostic_error("parse", &e.diagnostic, &source_file, test_file)
    })?;

    let rust_source = buff_lang_codegen_rust::generate_rust(&decls, source_id).map_err(|e| {
        pipeline::format_diagnostic_error("codegen", &e.diagnostic, &source_file, test_file)
    })?;

    // Write generated Rust to a temp dir.
    let temp_dir = std::env::temp_dir().join("buff-test-snap");
    std::fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create temp dir `{}`", temp_dir.display()))?;

    let stem = test_file
        .file_stem()
        .map(|s| s.to_owned())
        .unwrap_or_else(|| std::ffi::OsString::from("buff_test"));
    let rust_file = temp_dir.join(format!("{}_test.rs", stem.to_string_lossy()));
    std::fs::write(&rust_file, &rust_source)
        .with_context(|| format!("failed to write `{}`", rust_file.display()))?;

    // Compile with rustc (debug mode for fast iteration).
    let exe_stem = pipeline::with_exe_extension(
        &temp_dir.join(format!("{}_test", stem.to_string_lossy())),
    );
    let exe_path = pipeline::compile_rust_to_exe_with_races(
        &rust_file,
        &exe_stem,
        test_file,
        pipeline::BuildMode::Debug,
        detect_races,
    )?;

    // 2. Execute, capturing stdout.
    let output = Command::new(&exe_path)
        .output()
        .with_context(|| format!("failed to execute `{}`", exe_path.display()))?;

    let actual_stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    // 3. Clean up temp files (best-effort).
    let _ = std::fs::remove_file(&exe_path);
    let _ = std::fs::remove_file(&rust_file);

    // 4. Determine the snapshot path.
    let snap_path = test_file.with_extension("snap");

    // 5. Compare or update.
    if update || !snap_path.exists() {
        let is_new = !snap_path.exists();

        // Write (or overwrite) the snapshot file.
        let snap_content = format!(
            "---\nsource: {}\n---\n{}",
            test_file.display(),
            actual_stdout
        );
        std::fs::write(&snap_path, &snap_content)
            .with_context(|| format!("failed to write snapshot `{}`", snap_path.display()))?;

        if is_new {
            eprintln!("(new snapshot written)");
        } else {
            eprintln!("(snapshot updated)");
        }
        return Ok(true);
    }

    // 6. Read the existing snapshot.
    let snap_content = std::fs::read_to_string(&snap_path)
        .with_context(|| format!("failed to read snapshot `{}`", snap_path.display()))?;

    // Extract the expected output (everything after the second `---` line).
    let expected = extract_snapshot_body(&snap_content);

    // 7. Compare.
    if actual_stdout == expected {
        Ok(true)
    } else {
        // Print diff.
        print_diff(&expected, &actual_stdout, test_file);
        Ok(false)
    }
}

/// Extract the snapshot body from a `.snap` file.
///
/// The format is:
/// ```text
/// ---
/// source: <path>
/// ---
/// <body>
/// ```
///
/// Returns everything after the second `---` line. If there's no header
/// (no `---` lines), returns the entire content.
fn extract_snapshot_body(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();

    // Find the second `---` line.
    let mut dash_count = 0;
    let mut body_start = 0;
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "---" {
            dash_count += 1;
            if dash_count == 2 {
                body_start = i + 1;
                break;
            }
        }
    }

    // If we found the second `---`, take everything after it.
    // Otherwise, the entire content is the body.
    if dash_count >= 2 {
        lines[body_start..].join("\n")
    } else {
        content.to_string()
    }
}

/// Print a simple diff between expected and actual output.
fn print_diff(expected: &str, actual: &str, test_file: &Path) {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    eprintln!(
        "snapshot mismatch in `{}`:",
        test_file.file_name().unwrap_or_default().to_string_lossy()
    );

    // Simple line-by-line diff.
    let max_lines = expected_lines.len().max(actual_lines.len());
    for i in 0..max_lines {
        let exp = expected_lines.get(i).copied().unwrap_or("");
        let act = actual_lines.get(i).copied().unwrap_or("");
        if exp != act {
            if exp.is_empty() {
                eprintln!("  +{act}");
            } else if act.is_empty() {
                eprintln!("  -{exp}");
            } else {
                eprintln!("  -{exp}");
                eprintln!("  +{act}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_snapshot_body_with_header() {
        let content = "---\nsource: tests/foo.test.buff\n---\nhello\nworld\n";
        assert_eq!(extract_snapshot_body(content), "hello\nworld\n");
    }

    #[test]
    fn extract_snapshot_body_no_header() {
        let content = "hello\nworld\n";
        assert_eq!(extract_snapshot_body(content), "hello\nworld\n");
    }

    #[test]
    fn extract_snapshot_body_empty() {
        assert_eq!(extract_snapshot_body(""), "");
    }

    #[test]
    fn extract_snapshot_body_only_header() {
        let content = "---\nsource: foo\n---\n";
        assert_eq!(extract_snapshot_body(content), "");
    }
}
