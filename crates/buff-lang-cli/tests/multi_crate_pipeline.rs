//! Integration tests for the T8 multi-crate pipeline entry point
//! [`buff_lang_cli::pipeline::compile_to_rust_multi`].
//!
//! These tests exercise the file-walking + module-graph-BFS + multi-file
//! `.rs` emission pipeline. They write small multi-module `.buff`
//! fixtures into temp dirs and assert that the generated root + sibling
//! `.rs` files have the expected `mod` + `use` wiring.
//!
//! End-to-end `buff run examples/modules/main.buff` (which additionally
//! invokes `rustc`) is covered by `cli_run_tests.rs` (rustc-gated).

use std::fs;
use std::path::PathBuf;

use buff_lang_cli::pipeline;

fn temp_root() -> PathBuf {
    // Per-thread + per-process subdir to avoid parallel-test collisions.
    let thread_id_str = format!("{:?}", std::thread::current().id());
    let thread_id_sanitised: String = thread_id_str
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let dir = std::env::temp_dir().join(format!(
        "buff-lang-cli-multi-crate-tests-{}-{}",
        std::process::id(),
        thread_id_sanitised,
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn write_fixture(root: &PathBuf, name: &str, contents: &str) -> PathBuf {
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, contents).unwrap_or_else(|e| panic!("failed to write fixture {path:?}: {e}"));
    path
}

fn cleanup(path: &std::path::Path) {
    let _ = fs::remove_file(path);
}

fn cleanup_dir(path: &std::path::Path) {
    let _ = fs::remove_dir_all(path);
}

// ---------------------------------------------------------------------------
// Single-file fallback (no imports → existing codegen path).
// ---------------------------------------------------------------------------

#[test]
fn single_file_program_falls_back_to_compile_to_rust() {
    // A program with NO imports should fall back to the single-file path
    // and produce exactly ONE .rs file (no siblings).
    let root = temp_root().join("single_file");
    let _ = fs::create_dir_all(&root);
    let src = "func main():\n    print(\"hello\")\n";
    let file = write_fixture(&root, "main.buff", src);

    let out = pipeline::compile_to_rust_multi(&file, &root).expect("single-file fallback ok");

    // Root .rs path is in the requested out_dir.
    assert_eq!(
        out.root_rust_path.parent(),
        Some(root.as_path()),
        "root should live in out_dir"
    );
    // No sibling module files (single-file path).
    assert!(
        out.module_rust_paths.is_empty(),
        "single-file program should produce zero sibling modules"
    );
    // Root source contains the user's fn main.
    assert!(
        out.root_source.contains("fn main"),
        "root source should contain fn main; got:\n{}",
        out.root_source
    );

    cleanup_dir(&root);
}

// ---------------------------------------------------------------------------
// Two-module program: root + one imported module.
// ---------------------------------------------------------------------------

#[test]
fn two_module_program_emits_root_and_sibling() {
    let root = temp_root().join("two_module");
    let _ = fs::create_dir_all(&root);
    let main_src = "\
import { greet } from \"./greet.buff\"

func main():
    print(greet(\"Buff\"))
";
    let greet_src = "\
export func greet(name: String) -> String:
    return name
";
    let main_file = write_fixture(&root, "main.buff", main_src);
    let _greet_file = write_fixture(&root, "greet.buff", greet_src);

    let out = pipeline::compile_to_rust_multi(&main_file, &root).expect("multi-crate ok");

    // Root .rs file written.
    assert!(
        out.root_rust_path.exists(),
        "root .rs should exist at {}",
        out.root_rust_path.display()
    );
    // Exactly one sibling module.
    assert_eq!(
        out.module_rust_paths.len(),
        1,
        "expected 1 sibling module, got {}",
        out.module_rust_paths.len()
    );
    let greet_rs = &out.module_rust_paths[0];
    assert!(
        greet_rs.exists(),
        "greet.rs should exist at {}",
        greet_rs.display()
    );
    // The sibling's filename is the sanitised module ident.
    assert!(
        greet_rs
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "greet.rs")
            .unwrap_or(false),
        "sibling should be greet.rs, got {}",
        greet_rs.display()
    );

    // Root source has `mod greet;` + `use greet::greet;` + `fn main`.
    let root_src = fs::read_to_string(&out.root_rust_path).unwrap_or_default();
    assert!(
        root_src.contains("mod greet"),
        "root should declare `mod greet;`; got:\n{}",
        root_src
    );
    assert!(
        root_src.contains("use greet::greet"),
        "root should bring greet into scope via `use greet::greet;`; got:\n{}",
        root_src
    );
    assert!(
        root_src.contains("fn main"),
        "root should contain `fn main`; got:\n{}",
        root_src
    );

    // greet.rs contains `fn greet` (the module's body).
    let greet_src_out = fs::read_to_string(greet_rs).unwrap_or_default();
    assert!(
        greet_src_out.contains("fn greet"),
        "greet.rs should contain `fn greet`; got:\n{}",
        greet_src_out
    );

    cleanup(&main_file);
    for p in &out.module_rust_paths {
        cleanup(p);
    }
    cleanup(&out.root_rust_path);
    cleanup_dir(&root);
}

// ---------------------------------------------------------------------------
// Missing imported file surfaces a clear error.
// ---------------------------------------------------------------------------

#[test]
fn missing_import_file_surfaces_clear_error() {
    let root = temp_root().join("missing_import");
    let _ = fs::create_dir_all(&root);
    let main_src = "\
import { greet } from \"./missing.buff\"

func main():
    print(\"hi\")
";
    let main_file = write_fixture(&root, "main.buff", main_src);
    // Note: NO missing.buff is written.

    let result = pipeline::compile_to_rust_multi(&main_file, &root);
    assert!(
        result.is_err(),
        "expected error when imported file is missing"
    );
    let msg = format!("{:#}", result.unwrap_err());
    assert!(
        msg.contains("missing.buff") || msg.contains("cannot read"),
        "error should reference the missing file; got: {msg}"
    );

    cleanup(&main_file);
    cleanup_dir(&root);
}

// ---------------------------------------------------------------------------
// Three-module program: root + three direct imports.
// ---------------------------------------------------------------------------

#[test]
fn three_module_program_emits_all_three_siblings() {
    let root = temp_root().join("three_module");
    let _ = fs::create_dir_all(&root);
    let main_src = "\
import { a_fn } from \"./a.buff\"
import { b_fn } from \"./b.buff\"
import { c_fn } from \"./c.buff\"

func main():
    print(\"done\")
";
    write_fixture(&root, "a.buff", "export func a_fn():\n    return\n");
    write_fixture(&root, "b.buff", "export func b_fn():\n    return\n");
    write_fixture(&root, "c.buff", "export func c_fn():\n    return\n");
    let main_file = write_fixture(&root, "main.buff", main_src);

    let out = pipeline::compile_to_rust_multi(&main_file, &root).expect("multi-crate ok");

    assert_eq!(out.module_rust_paths.len(), 3, "expected 3 sibling modules");
    let root_src = fs::read_to_string(&out.root_rust_path).unwrap_or_default();
    for name in &["a", "b", "c"] {
        assert!(
            root_src.contains(&format!("mod {name}")),
            "root should declare mod {name}; got:\n{}",
            root_src
        );
    }

    cleanup(&main_file);
    for p in &out.module_rust_paths {
        cleanup(p);
    }
    cleanup(&out.root_rust_path);
    cleanup_dir(&root);
}
