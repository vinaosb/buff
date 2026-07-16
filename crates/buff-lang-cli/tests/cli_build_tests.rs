//! Integration tests for the `buff build` command and the front-end pipeline
//! (`buff_lang_cli::pipeline::compile_to_rust`).
//!
//! Tests are split into two tiers:
//!
//! 1. **Pipeline tests** — exercise `compile_to_rust` only (no `rustc`). These
//!    are deterministic, fast, and run in every environment.
//! 2. **End-to-end build tests** — exercise the full `build::run` path including
//!    `rustc`. These auto-skip when `rustc` is not on PATH (via
//!    [`rustc_available`]), so they don't fail in environments without a Rust
//!    toolchain.
//!
//! All tests write their fixtures into [`std::env::temp_dir()`] under a unique
//! subdirectory so the user's workspace is never polluted.

use std::fs;
use std::path::{Path, PathBuf};

use buff_lang_cli::commands;
use buff_lang_cli::pipeline;

/// Helper: create a unique temp dir for this test binary's fixtures.
fn temp_root() -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("buff-lang-cli-build-tests-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Helper: write `contents` to `<temp_root>/<name>` and return the path.
fn write_fixture(name: &str, contents: &str) -> PathBuf {
    let path = temp_root().join(name);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, contents).unwrap_or_else(|e| panic!("failed to write fixture {path:?}: {e}"));
    path
}

/// Helper: detect whether `rustc` is callable on PATH.
fn rustc_available() -> bool {
    std::process::Command::new("rustc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Helper: remove a path if it exists (file or dir).
fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_dir_all(path);
}

// ---------------------------------------------------------------------------
// Tier 1: pipeline tests (no rustc required)
// ---------------------------------------------------------------------------

#[test]
fn test_compile_nonexistent_file_returns_clear_error() {
    let bogus = temp_root().join("definitely-does-not-exist.buff");
    let err = pipeline::compile_to_rust(&bogus).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("failed to read") || msg.contains("failed to read source file"),
        "expected file-read error, got: {msg}"
    );
    assert!(
        msg.contains(&bogus.display().to_string())
            || msg.contains(&bogus.to_string_lossy().to_string()),
        "expected error to mention the file path, got: {msg}"
    );
}

#[test]
fn test_compile_invalid_syntax_returns_parse_error() {
    // `let` at top level is illegal — only `func` decls are accepted (T8).
    let src = "let x = 5\n";
    let file = write_fixture("invalid_syntax.buff", src);

    let err = pipeline::compile_to_rust(&file).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("parse error"),
        "expected 'parse error' in message, got: {msg}"
    );
    // Error should reference a line number (file:line:col format).
    assert!(
        msg.contains(&file.display().to_string()),
        "expected error to reference the source file path, got: {msg}"
    );

    cleanup(&file);
    cleanup(&file.with_extension("rs"));
}

#[test]
fn test_compile_unterminated_string_returns_lex_error() {
    let src = "func main():\n    print(\"unterminated)\n";
    let file = write_fixture("unterminated.buff", src);

    let err = pipeline::compile_to_rust(&file).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("lex error"),
        "expected 'lex error' in message, got: {msg}"
    );

    cleanup(&file);
    cleanup(&file.with_extension("rs"));
}

#[test]
fn test_compile_empty_file_succeeds() {
    // An empty file should produce zero top-level decls → empty Rust file.
    let file = write_fixture("empty.buff", "");

    let out = pipeline::compile_to_rust(&file).expect("empty .buff should compile");
    assert!(
        out.rust_source.trim().is_empty(),
        "expected empty Rust output, got: {:?}",
        out.rust_source
    );
    assert_eq!(
        out.rust_file_path,
        file.with_extension("rs"),
        ".rs file should sit alongside .buff"
    );
    assert!(
        out.rust_file_path.exists(),
        ".rs file must be written to disk"
    );

    cleanup(&file);
    cleanup(&out.rust_file_path);
}

#[test]
fn test_compile_generates_fn_main_for_ola() {
    let src = "func main():\n    print(\"Olá, Buff!\")\n";
    let file = write_fixture("ola_fixture.buff", src);

    let out = pipeline::compile_to_rust(&file).expect("ola fixture must compile");
    assert!(
        out.rust_source.contains("fn main"),
        "expected 'fn main' in generated Rust, got: {:?}",
        out.rust_source
    );
    // The codegen maps `print(x)` to `println!("{}", x)`.
    assert!(
        out.rust_source.contains("println"),
        "expected 'println!' macro in generated Rust, got: {:?}",
        out.rust_source
    );
    // UTF-8 string literal preserved.
    assert!(
        out.rust_source.contains("Olá, Buff!"),
        "expected UTF-8 literal preserved, got: {:?}",
        out.rust_source
    );

    cleanup(&file);
    cleanup(&out.rust_file_path);
}

#[test]
fn test_compile_writes_rust_file_alongside_source() {
    let src = "func main():\n    print(42)\n";
    let file = write_fixture("writes_rs.buff", src);

    let expected_rs = file.with_extension("rs");
    assert!(
        !expected_rs.exists(),
        "precondition: .rs should not exist yet"
    );

    let out = pipeline::compile_to_rust(&file).expect("compile should succeed");
    assert!(expected_rs.exists(), ".rs file must exist after compile");
    assert_eq!(out.rust_file_path, expected_rs);

    cleanup(&file);
    cleanup(&expected_rs);
}

// ---------------------------------------------------------------------------
// Tier 2: end-to-end build tests (rustc required, auto-skipped otherwise)
// ---------------------------------------------------------------------------

#[test]
fn test_build_command_creates_executable_end_to_end() {
    if !rustc_available() {
        eprintln!("skipping test_build_command_creates_executable_end_to_end: rustc not on PATH");
        return;
    }

    let src = "func main():\n    print(\"build e2e\")\n";
    let file = write_fixture("build_e2e.buff", src);
    let rs_path = file.with_extension("rs");

    let result = commands::build::run(&file, None);

    let exe = {
        let mut p = file.with_extension("");
        if !std::env::consts::EXE_EXTENSION.is_empty() {
            p.set_extension(std::env::consts::EXE_EXTENSION);
        }
        p
    };

    // Assert success + existence BEFORE any cleanup.
    result.expect("build::run should succeed end-to-end with rustc available");
    assert!(exe.exists(), "executable should exist at {}", exe.display());

    cleanup(&file);
    cleanup(&rs_path);
    let _ = fs::remove_file(&exe);
}

#[test]
fn test_build_command_with_explicit_output_path() {
    if !rustc_available() {
        eprintln!("skipping test_build_command_with_explicit_output_path: rustc not on PATH");
        return;
    }

    let src = "func main():\n    print(\"explicit\")\n";
    let file = write_fixture("explicit_out.buff", src);
    let rs_path = file.with_extension("rs");

    let explicit_out = temp_root().join("custom_exe_name");
    let result = commands::build::run(&file, Some(&explicit_out));

    // rustc appends the platform exe extension.
    let actual_out = {
        let mut p = explicit_out.clone();
        if !std::env::consts::EXE_EXTENSION.is_empty() {
            p.set_extension(std::env::consts::EXE_EXTENSION);
        }
        p
    };

    // Assert success + existence BEFORE any cleanup.
    result.expect("build::run with --output should succeed");
    assert!(
        actual_out.exists(),
        "explicit output executable should exist at {}",
        actual_out.display()
    );

    cleanup(&file);
    cleanup(&rs_path);
    let _ = fs::remove_file(&actual_out);
}
