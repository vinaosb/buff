//! Integration tests for the error-mapping infrastructure (T16).
//!
//! These exercise the public functions in `buff_lang_cli::error_mapper`:
//!
//! - [`translate_rustc_errors`] — replaces `.rs` paths with `.buff` paths in
//!   rustc's stderr output.
//! - [`translate_panic`] — replaces `.rs` paths with `.buff` paths in runtime
//!   panic messages.
//! - [`filter_backtrace`] — removes Rust-stdlib frames from a backtrace.
//!
//! Plus one end-to-end test that runs a panicking Buff program via the `buff`
//! binary and verifies the error references the `.buff` file, not the `.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use buff_lang_cli::error_mapper;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_root() -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("buff-lang-cli-errmap-tests-{}", std::process::id()));
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

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_dir_all(path);
}

fn rustc_available() -> bool {
    Command::new("rustc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

// ---------------------------------------------------------------------------
// translate_rustc_errors
// ---------------------------------------------------------------------------

#[test]
fn test_translate_rustc_errors_replaces_path() {
    let rust_file = PathBuf::from("/tmp/prog.rs");
    let buff_file = PathBuf::from("/tmp/prog.buff");
    let stderr = "error[E0384]: cannot assign twice to immutable variable `x`\n  --> /tmp/prog.rs:15:5\n   |\n15 |     x = 2;\n   |     ^^^^^";

    let result = error_mapper::translate_rustc_errors(stderr, &buff_file, &rust_file);

    assert!(
        result.contains("prog.buff:15:5"),
        "expected .buff path in translated output, got: {result}"
    );
    assert!(
        !result.contains("prog.rs:"),
        "should not contain .rs path after translation, got: {result}"
    );
}

#[test]
fn test_translate_rustc_errors_preserves_non_path_content() {
    let rust_file = PathBuf::from("build/temp.rs");
    let buff_file = PathBuf::from("src/main.buff");
    let stderr = "error: expected one of `;`, `}`\n  --> build/temp.rs:1:5\n\nerror: aborting";

    let result = error_mapper::translate_rustc_errors(stderr, &buff_file, &rust_file);

    assert!(
        result.contains("expected one of"),
        "error message preserved"
    );
    assert!(result.contains("aborting"), "trailing content preserved");
    assert!(result.contains("main.buff"), "file path translated");
}

#[test]
fn test_translate_rustc_errors_no_match_returns_unchanged() {
    let rust_file = PathBuf::from("/tmp/prog.rs");
    let buff_file = PathBuf::from("/tmp/prog.buff");
    let stderr = "some message without any file reference";

    let result = error_mapper::translate_rustc_errors(stderr, &buff_file, &rust_file);

    assert_eq!(result, stderr, "unchanged when no path present");
}

// ---------------------------------------------------------------------------
// translate_panic
// ---------------------------------------------------------------------------

#[test]
fn test_translate_panic_replaces_path() {
    let rust_file = PathBuf::from("/tmp/prog.rs");
    let buff_file = PathBuf::from("/tmp/prog.buff");
    let sm = buff_lang_error::SourceMap::new();

    let panic_msg =
        "thread 'main' panicked at 'attempt to divide by zero', /tmp/prog.rs:2:15\nnote: run";

    let result = error_mapper::translate_panic(panic_msg, &rust_file, &buff_file, &sm);

    assert!(
        result.contains("prog.buff:2:15"),
        "expected .buff path + preserved line in result, got: {result}"
    );
    assert!(
        !result.contains("prog.rs:"),
        "should not contain .rs path, got: {result}"
    );
}

#[test]
fn test_translate_panic_preserves_message_content() {
    let rust_file = PathBuf::from("out.rs");
    let buff_file = PathBuf::from("in.buff");
    let sm = buff_lang_error::SourceMap::new();

    let panic_msg = "thread 'main' panicked at 'index out of bounds: the len is 3 but the index is 5', out.rs:10:7";

    let result = error_mapper::translate_panic(panic_msg, &rust_file, &buff_file, &sm);

    assert!(
        result.contains("index out of bounds"),
        "panic message preserved: {result}"
    );
    assert!(
        result.contains("the len is 3 but the index is 5"),
        "detailed message preserved: {result}"
    );
    assert!(result.contains("in.buff"), "file translated: {result}");
}

#[test]
fn test_translate_panic_with_line_map_translates_line() {
    // Build a source map with a mapping: rust line 5 → buff span at byte 6 (line 2).
    let mut sm = buff_lang_error::SourceMap::new();
    let sid = buff_lang_error::SourceId(0);
    sm.add_source(
        sid,
        PathBuf::from("prog.buff"),
        "line1\nline2\nline3\nline4\nline5\n".to_string(),
    );
    let buff_span = buff_lang_error::Span::new(6, 11, sid);
    sm.add_mapping(buff_span, 5);

    let rust_file = PathBuf::from("prog.rs");
    let buff_file = PathBuf::from("prog.buff");
    let panic_msg = "thread 'main' panicked at 'boom', prog.rs:5:10";

    let result = error_mapper::translate_panic(panic_msg, &rust_file, &buff_file, &sm);

    // Rust line 5 should be translated to buff line 2.
    assert!(
        result.contains("prog.buff:2:"),
        "expected translated line 2 (from rust 5), got: {result}"
    );
}

// ---------------------------------------------------------------------------
// filter_backtrace
// ---------------------------------------------------------------------------

#[test]
fn test_filter_backtrace_hides_stdlib() {
    let backtrace = "\
  0: std::panicking::begin_panic
             at /rustc/abc/lib/std/panicking.rs:5
  1: prog::main
             at /home/user/prog.rs:10
  2: core::fmt::write
             at C:\\rust\\toolchains\\stable\\rustlib\\src\\core\\fmt.rs:20
  3: alloc::vec::Vec<T>::push
             at /rustc/def/lib/alloc/vec/mod.rs:1500
  4: prog::helper
             at /home/user/prog.rs:25";

    let filtered = error_mapper::filter_backtrace(backtrace);

    assert!(
        !filtered.contains("rustc"),
        "should filter rustc frames: {filtered}"
    );
    assert!(
        !filtered.contains("rustlib"),
        "should filter rustlib frames: {filtered}"
    );
    assert!(
        !filtered.contains("/std/"),
        "should filter /std/ frames: {filtered}"
    );
    assert!(
        !filtered.contains("/core/"),
        "should filter /core/ frames: {filtered}"
    );
    assert!(
        !filtered.contains("/alloc/"),
        "should filter /alloc/ frames: {filtered}"
    );
    // User frames preserved.
    assert!(
        filtered.contains("prog::main"),
        "should keep user frame prog::main: {filtered}"
    );
    assert!(
        filtered.contains("prog::helper"),
        "should keep user frame prog::helper: {filtered}"
    );
}

#[test]
fn test_filter_backtrace_preserves_user_only_frames() {
    let backtrace = "\
  0: myapp::main
             at /home/user/myapp.rs:5
  1: myapp::compute
             at /home/user/myapp.rs:12";

    let filtered = error_mapper::filter_backtrace(backtrace);

    assert_eq!(
        filtered, backtrace,
        "all-user backtrace should be unchanged"
    );
}

#[test]
fn test_filter_backtrace_empty() {
    assert_eq!(error_mapper::filter_backtrace(""), "");
}

// ---------------------------------------------------------------------------
// End-to-end: runtime panic translated to .buff
// ---------------------------------------------------------------------------

#[test]
fn test_end_to_end_runtime_error_mapped_to_buff() {
    if !rustc_available() {
        eprintln!("skipping test_end_to_end_runtime_error_mapped_to_buff: rustc not on PATH");
        return;
    }

    // CARGO_BIN_EXE_buff is set by Cargo for integration tests in this crate.
    let buff_bin = std::env::var("CARGO_BIN_EXE_buff")
        .expect("CARGO_BIN_EXE_buff must be set for integration tests");

    // Division by zero panics at runtime in the compiled Rust binary.
    let src = "func main():\n    print(1 / 0)\n";
    let file = write_fixture("runtime_panic.buff", src);

    let output = Command::new(&buff_bin)
        .arg("run")
        .arg(&file)
        .output()
        .expect("failed to spawn `buff run`");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // The translated error should reference the .buff file.
    assert!(
        stderr.contains("runtime_panic.buff"),
        "expected stderr to reference .buff file after translation, got:\n{stderr}"
    );
    // It should NOT reference the .rs file (the translation replaced it).
    assert!(
        !stderr.contains("runtime_panic.rs:"),
        "stderr should NOT reference .rs file after translation, got:\n{stderr}"
    );

    // Non-zero exit expected (the program panicked).
    assert!(
        !output.status.success(),
        "panicking program should exit non-zero"
    );

    cleanup(&file);
    cleanup(&file.with_extension("rs"));
}
