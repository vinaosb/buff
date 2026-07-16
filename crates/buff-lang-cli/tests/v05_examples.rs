//! v0.5 milestone example-suite acceptance tests (T37).
//!
//! Each v0.5 example is exercised at the highest level it supports:
//!
//! | Example             | Status          | Test level                                  |
//! |---------------------|-----------------|---------------------------------------------|
//! | `closures.buff`     | runs end-to-end | `buff run` → assert exact stdout            |
//! | `collections.buff`  | runs end-to-end | `buff run` → assert exact stdout            |
//! | `pattern_matching`  | runs end-to-end | `buff run` → assert exact stdout            |
//! | `error_handling`    | runs end-to-end | `buff run` → assert exact stdout            |
//! | `async_demo.buff`   | codegen-only    | `compile_to_rust` → assert tokio markers    |
//! | `modules/*`         | codegen-only    | `parse` → assert import/export decls        |
//!
//! ## Why some examples are "codegen-only"
//!
//! The v0.1/v0.5 CLI pipeline invokes `rustc` on a SINGLE generated `.rs`
//! file with no Cargo project model, so external crates cannot be linked:
//!
//! - **async** needs `tokio` (`tokio::spawn`, `#[tokio::main]`). The codegen
//!   emits all of it (tested here + in `async_codegen.rs`); only the final
//!   `rustc` link of external crates is unwired (T32 deferral).
//! - **modules** need multi-file linking. `import` decls parse and the module
//!   graph resolves (T29, `buff-lang-types`), but codegen of a single file
//!   containing `import` is unsupported, and the CLI never concatenates
//!   imported modules. Tested here at the parse level + graph in types crate.
//!
//! Each test copies its example into a UNIQUE temp directory so that the
//! `.rs` sidecar written by `compile_to_rust` never pollutes `examples/`.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-cli --test v05_examples
//! ```

#![allow(clippy::approx_constant)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

// ---------------------------------------------------------------------------
// Helpers (mirrors crates/buff-lang-cli/tests/milestone_tests.rs)
// ---------------------------------------------------------------------------

/// Locate the workspace `examples/` directory from CARGO_MANIFEST_DIR.
fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn rustc_available() -> bool {
    Command::new("rustc")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Path to the pre-built `buff` debug binary, if present.
fn buff_binary() -> Option<PathBuf> {
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join(if std::env::consts::EXE_EXTENSION.is_empty() {
            "buff".to_string()
        } else {
            format!("buff.{}", std::env::consts::EXE_EXTENSION)
        });
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

/// Build a `Command` running `buff <args...>`. Prefers the pre-built binary;
/// falls back to `cargo run` otherwise.
fn buff_invocation() -> Command {
    match buff_binary() {
        Some(bin) => Command::new(bin),
        None => {
            let mut c = Command::new("cargo");
            c.args(["run", "-q", "-p", "buff-lang-cli", "--"]);
            c
        }
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Copy `examples/<name>` into a fresh, unique temp dir and return the copy's
/// path. Unique-per-call so parallel test runs never collide, and so the `.rs`
/// sidecar written by `compile_to_rust` never lands in `examples/`.
fn copy_to_unique_temp(name: &str) -> PathBuf {
    let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir()
        .join("buff-v05-examples")
        .join(format!("{name}-{pid}-{id}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    let src = examples_dir().join(name);
    let dst = dir.join(name);
    let content = fs::read(&src)
        .unwrap_or_else(|e| panic!("failed to read example `{name}`: {e}"));
    fs::write(&dst, content).expect("failed to write temp copy");
    dst
}

/// Run `buff run <path>` and return captured stdout. Panics on non-zero exit
/// with stderr dumped for diagnosis.
fn buff_run(path: &Path) -> String {
    let mut cmd = buff_invocation();
    cmd.arg("run").arg(path);
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn `buff run`: {e}"));
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "`buff run {}` failed with status {:?}\nstdout:\n{}\nstderr:\n{stderr}",
            path.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout)
        );
    }
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn sid() -> SourceId {
    SourceId(0)
}

// ---------------------------------------------------------------------------
// End-to-end examples: run via `buff run` and assert exact stdout.
//
// These four exercise the features that compile to STD-ONLY Rust (Vector,
// .map closures, Option/Result, the `?` operator, match, the builtin Error
// type) and therefore link cleanly under the single-file `rustc` pipeline.
// ---------------------------------------------------------------------------

#[test]
fn test_v05_closures_runs_e2e() {
    if !rustc_available() {
        eprintln!("skipping: rustc not on PATH");
        return;
    }
    let path = copy_to_unique_temp("closures.buff");
    let stdout = buff_run(&path);
    // doubled[0]=2, doubled[4]=10, squared[2]=9, plus_one[0]=3, plus_one[4]=11,
    // echo[1]=40, count=5.
    assert_eq!(
        stdout.trim(),
        "2\n10\n9\n3\n11\n40\n5",
        "closures.buff e2e output mismatch"
    );
}

#[test]
fn test_v05_collections_runs_e2e() {
    if !rustc_available() {
        eprintln!("skipping: rustc not on PATH");
        return;
    }
    let path = copy_to_unique_temp("collections.buff");
    let stdout = buff_run(&path);
    // v[0]=10, v[3]=40, v.len()=4, stack.push(4)->4, drawer.pop()->9,
    // scaled[2]=30, scores.len()=3.
    assert_eq!(
        stdout.trim(),
        "10\n40\n4\n4\n9\n30\n3",
        "collections.buff e2e output mismatch"
    );
}

#[test]
fn test_v05_pattern_matching_runs_e2e() {
    if !rustc_available() {
        eprintln!("skipping: rustc not on PATH");
        return;
    }
    let path = copy_to_unique_temp("pattern_matching.buff");
    let stdout = buff_run(&path);
    // classify 0=100, 2=300, 5=999; drawer.pop()->33; empty pop->0;
    // lookup(1)->111; lookup(99)->0.
    assert_eq!(
        stdout.trim(),
        "100\n300\n999\n33\n0\n111\n0",
        "pattern_matching.buff e2e output mismatch"
    );
}

#[test]
fn test_v05_error_handling_runs_e2e() {
    if !rustc_available() {
        eprintln!("skipping: rustc not on PATH");
        return;
    }
    let path = copy_to_unique_temp("error_handling.buff");
    let stdout = buff_run(&path);
    // add_one(10)=6 (half=5+1); add_one(1) errors via `?`->0;
    // half(8)=4; half(0) errors->0.
    assert_eq!(
        stdout.trim(),
        "6\n0\n4\n0",
        "error_handling.buff e2e output mismatch"
    );
}

// ---------------------------------------------------------------------------
// Codegen-only example: async_demo.buff.
//
// `compile_to_rust` succeeds (the front-end is complete); the generated Rust
// references the external `tokio` crate, which the single-file `rustc`
// pipeline cannot link. We assert the codegen emitted the tokio markers
// rather than asserting execution.
// ---------------------------------------------------------------------------

#[test]
fn test_v05_async_demo_transpiles_with_tokio_markers() {
    // compile_to_rust does NOT require rustc (it stops after codegen), so no
    // rustc gate here.
    let path = copy_to_unique_temp("async_demo.buff");
    let out = buff_lang_cli::pipeline::compile_to_rust(&path)
        .expect("async_demo.buff must transpile (codegen-only; rustc link is the gap)");
    let src = out.rust_source;
    // `async fn main` + `#[tokio::main]` (main joined the async set).
    assert!(
        src.contains("#[tokio::main]"),
        "expected `#[tokio::main]` on async main in: {src}"
    );
    assert!(
        src.contains("async fn main"),
        "expected `async fn main` in: {src}"
    );
    // Call-graph propagation: `pipeline` calls an async fn, so it becomes
    // `async fn pipeline` even though the Buff source has no `async` keyword.
    assert!(
        src.contains("async fn pipeline"),
        "expected `async fn pipeline` (auto-propagated) in: {src}"
    );
    // Auto-inserted `.await` at the async call site inside `pipeline`.
    assert!(
        src.contains("fetch_value().await"),
        "expected auto-inserted `.await` in: {src}"
    );
    // `spawn fetch_value()` -> `tokio::spawn(async move { ... })`.
    assert!(
        src.contains("tokio::spawn(async move"),
        "expected `tokio::spawn(async move ...)` in: {src}"
    );
    // `task.result()` -> `task.await`.
    assert!(
        src.contains(".await"),
        "expected `.await` (from task.result()) in: {src}"
    );
    // Re-parse the generated source to guarantee it is valid Rust syntax.
    syn::parse_str::<syn::File>(&src)
        .unwrap_or_else(|e| panic!("generated async source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// Codegen-only example: modules/*.buff.
//
// `buff run` cannot transpile a file containing `import` (codegen rejects it)
// and cannot link multiple files. We assert the module examples PARSE into
// the expected import/export decls — the level at which T29 module support is
// implemented and tested (graph resolution lives in buff-lang-types).
// ---------------------------------------------------------------------------

#[test]
fn test_v05_modules_main_parses_import_decl() {
    let main_src = fs::read_to_string(examples_dir().join("modules").join("main.buff"))
        .expect("modules/main.buff must exist");
    let tokens = tokenize(&main_src, sid()).expect("modules/main.buff must lex");
    let decls = parse(&tokens, sid()).expect("modules/main.buff must parse");
    // At least one ImportDecl naming `greet` from "./greet.buff".
    let has_import = decls.iter().any(|d| {
        matches!(
            d,
            buff_lang_ast::Decl::ImportDecl(_)
        )
    });
    assert!(has_import, "modules/main.buff must contain an import decl");
    // And it must still have a main function.
    let has_main = decls.iter().any(|d| {
        matches!(
            d,
            buff_lang_ast::Decl::FuncDecl(f) if f.name.name == "main"
        )
    });
    assert!(has_main, "modules/main.buff must contain a `func main`");
}

#[test]
fn test_v05_modules_greet_parses_export_decls() {
    let greet_src = fs::read_to_string(examples_dir().join("modules").join("greet.buff"))
        .expect("modules/greet.buff must exist");
    let tokens = tokenize(&greet_src, sid()).expect("modules/greet.buff must lex");
    let decls = parse(&tokens, sid()).expect("modules/greet.buff must parse");
    // greet.buff exports `greet` and `greeting_for` (two ExportDecl wrappings).
    let export_count = decls
        .iter()
        .filter(|d| matches!(d, buff_lang_ast::Decl::ExportDecl(_)))
        .count();
    assert_eq!(
        export_count, 2,
        "modules/greet.buff must export two functions (greet, greeting_for)"
    );
}

#[test]
fn test_v05_modules_all_examples_are_nonempty() {
    // Smoke check: every example file we ship is present and non-empty.
    for name in [
        "closures.buff",
        "collections.buff",
        "pattern_matching.buff",
        "error_handling.buff",
        "async_demo.buff",
    ] {
        let p = examples_dir().join(name);
        let len = fs::metadata(&p)
            .map(|m| m.len())
            .unwrap_or_else(|e| panic!("example `{name}` missing: {e}"));
        assert!(len > 0, "example `{name}` is empty");
    }
    for name in ["main.buff", "greet.buff"] {
        let p = examples_dir().join("modules").join(name);
        let len = fs::metadata(&p)
            .map(|m| m.len())
            .unwrap_or_else(|e| panic!("modules/{name} missing: {e}"));
        assert!(len > 0, "modules/{name} is empty");
    }
}
