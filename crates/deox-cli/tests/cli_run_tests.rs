//! Integration tests for the `deox run` command.
//!
//! As with the build tests, the cheaper front-end errors are exercised
//! unconditionally, while end-to-end runs (which need `rustc`) auto-skip via
//! [`rustc_available`].

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use deox_cli::commands;
use deox_cli::pipeline;

fn temp_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("deox-cli-run-tests-{}", std::process::id()));
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

// ---------------------------------------------------------------------------
// Front-end error tests (no rustc required)
// ---------------------------------------------------------------------------

#[test]
fn test_run_nonexistent_file_returns_clear_error() {
    let bogus = temp_root().join("no-such-file.deox");
    let err = commands::run::run(&bogus, &[]).unwrap_err();
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
    let file = write_fixture("run_invalid.deox", src);
    let rs_path = file.with_extension("rs");

    let err = commands::run::run(&file, &[]).unwrap_err();
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
    let file = write_fixture("run_lex_err.deox", src);
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
fn test_run_ola_deox_end_to_end() {
    if !rustc_available() {
        eprintln!("skipping test_run_ola_deox_end_to_end: rustc not on PATH");
        return;
    }

    let src = "func main():\n    print(\"Olá, Deox!\")\n";
    let file = write_fixture("run_ola.deox", src);

    let result = commands::run::run(&file, &[]);

    cleanup(&file);
    // After run, the .rs file should have been cleaned up; remove if lingering.
    cleanup(&file.with_extension("rs"));

    result.expect("deox run on ola fixture should succeed end-to-end");
}

#[test]
fn test_run_cleans_up_rust_file_after_success() {
    if !rustc_available() {
        eprintln!("skipping test_run_cleans_up_rust_file_after_success: rustc not on PATH");
        return;
    }

    let src = "func main():\n    print(123)\n";
    let file = write_fixture("run_cleanup.deox", src);
    let rs_path = file.with_extension("rs");

    commands::run::run(&file, &[]).expect("run should succeed");

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
    let file = write_fixture("run_exe_cleanup.deox", src);

    commands::run::run(&file, &[]).expect("run should succeed");

    let temp_exe_dir = std::env::temp_dir().join("deox-run");
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
    // in the Deox surface yet. This test therefore only verifies that passing
    // extra args does NOT break `deox run` (the program still exits 0).
    let src = "func main():\n    print(\"args ok\")\n";
    let file = write_fixture("run_args.deox", src);

    let result = commands::run::run(&file, &["alpha".to_string(), "beta".to_string()]);

    cleanup(&file);
    cleanup(&file.with_extension("rs"));

    result.expect("passing args should not break `deox run`");
}
