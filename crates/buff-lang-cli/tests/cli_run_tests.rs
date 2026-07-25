//! Integration tests for the `buff run` command.
//!
//! As with the build tests, the cheaper front-end errors are exercised
//! unconditionally, while end-to-end runs (which need `rustc`) auto-skip via
//! [`rustc_available`].

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use buff_lang_cli::commands;
use buff_lang_cli::pipeline;

fn temp_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("buff-lang-cli-run-tests-{}", std::process::id()));
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
}

/// Convenience wrapper for `commands::run::run` that fills in the
/// post-T55 / T7 / T9 / T113 default values (no incremental, no
/// sccache, default linker/debuginfo/backend, native target, no race
/// detection). Keeps the per-test call sites readable.
fn run_with_defaults(file: &std::path::Path, args: &[String], release: bool) -> anyhow::Result<()> {
    commands::run::run(
        file,
        args,
        release,
        false, // incremental
        true,  // no_incremental (force legacy path)
        false, // sccache
        pipeline::LinkerChoice::default(),
        pipeline::DebugInfoChoice::default(),
        pipeline::BackendChoice::default(),
        None,  // target
        false, // detect_races
    )
}

// ---------------------------------------------------------------------------
// Front-end error tests (no rustc required)
// ---------------------------------------------------------------------------

#[test]
fn test_run_nonexistent_file_returns_clear_error() {
    let bogus = temp_root().join("no-such-file.buff");
    let err = run_with_defaults(&bogus, &[], false).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("failed to read"),
        "expected file-read error, got: {msg}"
    );
}

#[test]
fn test_run_invalid_syntax_returns_error_before_rustc() {
    // `let` at top level is illegal.
    let src = "let x = 1\n";
    let file = write_fixture("run_invalid.buff", src);
    let rs_path = file.with_extension("rs");

    let err = run_with_defaults(&file, &[], false).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("parse error"),
        "expected parse error, got: {msg}"
    );

    cleanup(&file);
    cleanup(&rs_path);
}

#[test]
fn test_run_pipeline_compile_step_covers_lex_errors() {
    let src = "func main():\n    print(\"open\n";
    let file = write_fixture("run_lex_err.buff", src);
    let err = pipeline::compile_to_rust(&file).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("lex error"), "expected lex error, got: {msg}");

    cleanup(&file);
    cleanup(&file.with_extension("rs"));
}

// ---------------------------------------------------------------------------
// End-to-end run tests (rustc required)
// ---------------------------------------------------------------------------

#[test]
fn test_run_ola_buff_end_to_end() {
    if !rustc_available() {
        eprintln!("skipping test_run_ola_buff_end_to_end: rustc not on PATH");
        return;
    }

    let src = "func main():\n    print(\"Olá, Buff!\")\n";
    let file = write_fixture("run_ola.buff", src);

    let result = run_with_defaults(&file, &[], false);

    cleanup(&file);
    // After run, the .rs file should have been cleaned up; remove if lingering.
    cleanup(&file.with_extension("rs"));

    result.expect("buff run on ola fixture should succeed end-to-end");
}

#[test]
fn test_run_cleans_up_rust_file_after_success() {
    if !rustc_available() {
        eprintln!("skipping test_run_cleans_up_rust_file_after_success: rustc not on PATH");
        return;
    }

    let src = "func main():\n    print(123)\n";
    let file = write_fixture("run_cleanup.buff", src);
    let rs_path = file.with_extension("rs");

    run_with_defaults(&file, &[], false).expect("run should succeed");

    assert!(
        !rs_path.exists(),
        "intermediate .rs file should be cleaned up after run; found at {}",
        rs_path.display()
    );

    cleanup(&file);
    cleanup(&rs_path);
}

#[test]
fn test_run_cleans_up_temp_executable_after_success() {
    if !rustc_available() {
        eprintln!("skipping test_run_cleans_up_temp_executable_after_success: rustc not on PATH");
        return;
    }

    let src = "func main():\n    print(1)\n";
    let file = write_fixture("run_exe_cleanup.buff", src);

    run_with_defaults(&file, &[], false).expect("run should succeed");

    let temp_exe_dir = std::env::temp_dir().join("buff-run");
    // The exe name is `<file-stem>` with platform extension; verify it's gone.
    let exe_name = format!(
        "run_exe_cleanup{}",
        if std::env::consts::EXE_EXTENSION.is_empty() {
            String::new()
        } else {
            format!(".{}", std::env::consts::EXE_EXTENSION)
        }
    );
    let exe_path = temp_exe_dir.join(&exe_name);
    assert!(
        !exe_path.exists(),
        "temp executable should be removed after run; found at {}",
        exe_path.display()
    );

    cleanup(&file);
    cleanup(&file.with_extension("rs"));
}

#[test]
fn test_run_args_passed_to_program() {
    if !rustc_available() {
        eprintln!("skipping test_run_args_passed_to_program: rustc not on PATH");
        return;
    }

    // A minimal program that ignores its args and exits 0. v0.1's `print`
    // mapping is the only stdlib call available, and there is no args access
    // in the Buff surface yet. This test therefore only verifies that passing
    // extra args does NOT break `buff run` (the program still exits 0).
    let src = "func main():\n    print(\"args ok\")\n";
    let file = write_fixture("run_args.buff", src);

    let result = run_with_defaults(&file, &["alpha".to_string(), "beta".to_string()], false);

    cleanup(&file);
    cleanup(&file.with_extension("rs"));

    result.expect("passing args should not break `buff run`");
}
