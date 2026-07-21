//! T135: integration tests for `buff ssr` CLI subcommand.
//!
//! Covers the public surface wired up in this task:
//!
//! - `commands::ssr::run` rejects missing files gracefully (file-read
//!   error path).
//! - `commands::ssr::extract_component_name` resolves the component fn
//!   name from the T133-generated source for the shipped example files
//!   (`counter.buffhtml`, `todo_list.buffhtml`).
//! - `commands::ssr::format_driver_source` splices the SSR driver main
//!   onto a codegen-buffhtml output, referencing the correct component
//!   fn name.
//! - `buff-ui-dioxus::render_to_string` is the underlying host-side
//!   renderer (its unit tests live in `crates/buff-ui-dioxus/src/lib.rs`
//!   under `tests::t135_*`; we re-assert here from the CLI's
//!   integration perspective that the helper is reachable through the
//!   wrapper crate's public API).
//!
//! Rustc + the SSR binary execution are NOT invoked here — those paths
//! are exercised by the T135 unit tests in
//! `crates/buff-ui-dioxus/src/lib.rs::tests` (real render) + by the
//! `buff ssr` USER ACTION recipe at
//! `.sisyphus/evidence/task-135-hydration-USER-ACTION.txt` (browser
//! hydration). Keeping the CLI integration test rustc-free lets it run
//! under CI's fast unit-test gate.

use std::fs;
use std::path::PathBuf;

use buff_lang_cli::commands::ssr::{extract_component_name, format_driver_source};
use buff_lang_cli::pipeline::compile_buffhtml_to_rust;

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR for tests/ssr_t135.rs is crates/buff-lang-cli —
    // the repo root is two levels up.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn examples_dir() -> PathBuf {
    repo_root().join("examples")
}

fn cleanup_rust_sibling(buffhtml_path: &std::path::Path) {
    let _ = fs::remove_file(buffhtml_path.with_extension("rs"));
}

// ---------------------------------------------------------------------------
// extract_component_name on real codegen output.
// ---------------------------------------------------------------------------

#[test]
fn t135_extract_counter_component_name_from_codegen() {
    let counter = examples_dir().join("counter.buffhtml");
    assert!(counter.exists(), "examples/counter.buffhtml must exist");
    let out = compile_buffhtml_to_rust(&counter).expect("counter compile ok");
    let name = extract_component_name(&out.rust_source)
        .expect("component name should resolve from codegen output");
    assert_eq!(
        name, "Counter",
        "Counter.buffhtml should derive component name `Counter`"
    );
    cleanup_rust_sibling(&counter);
}

#[test]
fn t135_extract_todo_list_component_name_from_codegen() {
    let todo = examples_dir().join("todo_list.buffhtml");
    assert!(todo.exists(), "examples/todo_list.buffhtml must exist");
    let out = compile_buffhtml_to_rust(&todo).expect("todo_list compile ok");
    let name = extract_component_name(&out.rust_source)
        .expect("component name should resolve from codegen output");
    assert_eq!(
        name, "TodoList",
        "todo_list.buffhtml should derive component name `TodoList` (PascalCased stem)"
    );
    cleanup_rust_sibling(&todo);
}

// ---------------------------------------------------------------------------
// format_driver_source on real codegen output.
// ---------------------------------------------------------------------------

#[test]
fn t135_format_driver_source_for_counter_codegen() {
    let counter = examples_dir().join("counter.buffhtml");
    let out = compile_buffhtml_to_rust(&counter).expect("counter compile ok");
    let driver = format_driver_source(&out.rust_source, "Counter");
    // Driver main fn is appended after the component fn declaration.
    let comp_idx = driver.find("fn Counter()").expect("component fn present");
    let main_idx = driver.find("fn main()").expect("driver main present");
    assert!(
        comp_idx < main_idx,
        "component fn must precede driver main so the symbol is in scope"
    );
    // Driver main calls the SSR helper with the correct component name.
    assert!(
        driver.contains("buff_ui_dioxus::render_to_string(Counter)"),
        "expected render_to_string(Counter) call; got:\n{driver}"
    );
    // Driver imports the wrapper crate's SSR surface.
    assert!(
        driver.contains("use buff_ui_dioxus::"),
        "expected wrapper crate import; got:\n{driver}"
    );
    cleanup_rust_sibling(&counter);
}

// ---------------------------------------------------------------------------
// Wrapper crate SSR surface reachable from CLI integration tests.
// ---------------------------------------------------------------------------

#[test]
fn t135_buff_ui_dioxus_render_to_string_reachable_from_cli() {
    // Confirm the wrapper crate exposes `render_to_string` at the path
    // the generated SSR driver references (`buff_ui_dioxus::render_to_string`).
    // We resolve the fn-item as a path value (we do NOT call it here —
    // the host-side render behavior is exercised in the buff-ui-dioxus
    // unit tests). This guards the contract that the CLI + the wrapper
    // crate share a single canonical import path.
    let _ = buff_ui_dioxus::render_to_string as fn(fn() -> buff_ui_dioxus::Element) -> String;

    // The dioxus_ssr crate must also be reachable so the underlying
    // render path stays wired in (if dioxus-ssr disappears from the
    // workspace, this reference fails compilation).
    let _ = buff_ui_dioxus::dioxus_ssr::render as fn(&buff_ui_dioxus::VirtualDom) -> String;
}

// ---------------------------------------------------------------------------
// run() rejects bad inputs gracefully.
// ---------------------------------------------------------------------------

#[test]
fn t135_run_returns_err_for_missing_file() {
    let result = buff_lang_cli::commands::ssr::run(
        std::path::Path::new("/does/not/exist/missing.buffhtml"),
        None,
        false,
    );
    assert!(
        result.is_err(),
        "run() should return Err for a non-existent file"
    );
    let err = result.expect_err("run() should return Err for a non-existent file");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("failed to read") || msg.to_lowercase().contains("no such"),
        "error should mention read failure; got: {msg}"
    );
}

#[test]
fn t135_run_returns_err_for_invalid_buffhtml_syntax() {
    // Truly malformed .buffhtml (unbalanced tag) should fail at the
    // parse stage before the driver splice even runs.
    let dir = std::env::temp_dir().join(format!(
        "buff-lang-cli-ssr-t135-tests-{}",
        std::process::id()
    ));
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("broken.buffhtml");
    fs::write(&path, "<div><span></div></span>\n").expect("write fixture");
    let result = buff_lang_cli::commands::ssr::run(&path, None, false);
    assert!(
        result.is_err(),
        "run() should return Err for malformed .buffhtml"
    );
    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(path.with_extension("rs"));
}
