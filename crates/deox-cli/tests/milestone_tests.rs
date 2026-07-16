//! v0.1 milestone acceptance tests — the canonical "did we ship?" gate.
//!
//! Each test maps 1:1 to a v0.1 exit criterion:
//!
//! | Test                              | Criterion                                     |
//! |-----------------------------------|-----------------------------------------------|
//! | `test_v01_ola`                    | `deox run examples/ola.deox` → "Olá, Deox!"   |
//! | `test_v01_fibonacci`              | `deox run examples/fibonacci.deox` → "55"     |
//! | `test_v01_calculadora`            | `deox run examples/calculadora.deox` → "5"    |
//! | `test_v01_cargo_test_workspace`   | `cargo test --workspace` exits 0              |
//! | `test_v01_clippy_clean`           | `cargo clippy --workspace -- -D warnings` 0   |
//!
//! The first three are rustc-gated subprocess invocations of the `deox` CLI
//! (spawning is the only way to capture stdout — the library API writes
//! directly to the process's stdout).
//!
//! The last two are marked `#[ignore]` because running them inside `cargo
//! test` would re-enter cargo on the same target dir (lock contention / hang).
//! They exist as on-demand CI gates: `cargo test -p deox-cli -- --ignored
//! test_v01_`.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Locate the workspace `examples/` directory by walking up from CARGO_MANIFEST_DIR.
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

/// Path to the pre-built `deox` debug binary (`target/debug/deox[.exe]`).
///
/// Computed fresh on each call (cheap). If the binary isn't on disk we fall
/// back to `cargo run -q -p deox-cli --` (see [`deox_invocation`]).
fn deox_binary() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // manifest = crates/deox-cli → workspace root = ../..  → target/ = ../../target
    let candidate = manifest
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join(if std::env::consts::EXE_EXTENSION.is_empty() {
            "deox".to_string()
        } else {
            format!("deox.{}", std::env::consts::EXE_EXTENSION)
        });
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

/// Build the `Command` that runs `deox <args...>`. Prefers the pre-built
/// binary; falls back to `cargo run` if the binary isn't on disk.
fn deox_invocation() -> Command {
    match deox_binary() {
        Some(bin) => Command::new(bin),
        None => {
            let mut c = Command::new("cargo");
            c.args(["run", "-q", "-p", "deox-cli", "--"]);
            c
        }
    }
}

/// Run `deox run <example>` and return the captured stdout. Panics on
/// non-zero exit with the stderr dumped for diagnosis.
fn deox_run_example(example: &str) -> String {
    let mut cmd = deox_invocation();
    cmd.arg("run").arg(examples_dir().join(example));
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn `deox run {example}`: {e}"));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "`deox run examples/{example}` failed with status {:?}\nstderr:\n{stderr}",
            output.status
        );
    }
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// v0.1 acceptance: each example runs and prints the expected output
// ---------------------------------------------------------------------------

#[test]
fn test_v01_ola() {
    if !rustc_available() {
        eprintln!("skipping test_v01_ola: rustc not on PATH");
        return;
    }
    let stdout = deox_run_example("ola.deox");
    assert!(
        stdout.contains("Olá, Deox!"),
        "v0.1 criterion: ola.deox must print `Olá, Deox!`; got:\n{stdout}"
    );
}

#[test]
fn test_v01_fibonacci() {
    if !rustc_available() {
        eprintln!("skipping test_v01_fibonacci: rustc not on PATH");
        return;
    }
    // fib(10) = 55.
    let stdout = deox_run_example("fibonacci.deox");
    let trimmed = stdout.trim();
    assert_eq!(
        trimmed, "55",
        "v0.1 criterion: fibonacci.deox must print `55` for fib(10); got: `{trimmed}`"
    );
}

#[test]
fn test_v01_calculadora() {
    if !rustc_available() {
        eprintln!("skipping test_v01_calculadora: rustc not on PATH");
        return;
    }
    // add(2, 3) = 5.
    let stdout = deox_run_example("calculadora.deox");
    let trimmed = stdout.trim();
    assert_eq!(
        trimmed, "5",
        "v0.1 criterion: calculadora.deox must print `5` for add(2,3); got: `{trimmed}`"
    );
}

// ---------------------------------------------------------------------------
// v0.1 meta-gates: `cargo test --workspace` and `cargo clippy` both clean.
//
// Marked `#[ignore]` because running cargo inside cargo deadlocks on the
// target-dir lock. Run them explicitly in CI:
//
//     cargo test -p deox-cli -- --ignored test_v01_cargo_test_workspace
//     cargo test -p deox-cli -- --ignored test_v01_clippy_clean
// ---------------------------------------------------------------------------

#[test]
#[ignore = "meta-gate: run with `--ignored` in CI (cargo-in-cargo deadlocks otherwise)"]
fn test_v01_cargo_test_workspace() {
    // Run from the workspace root so `--workspace` resolves correctly.
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let output = Command::new("cargo")
        .arg("test")
        .arg("--workspace")
        .current_dir(&workspace_root)
        .output()
        .expect("failed to spawn `cargo test --workspace`");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "v0.1 criterion: `cargo test --workspace` must pass.\n\
         status: {:?}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        output.status
    );
}

#[test]
#[ignore = "meta-gate: run with `--ignored` in CI (cargo-in-cargo deadlocks otherwise)"]
fn test_v01_clippy_clean() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let output = Command::new("cargo")
        .args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ])
        .current_dir(&workspace_root)
        .output()
        .expect("failed to spawn `cargo clippy`");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "v0.1 criterion: `cargo clippy --workspace -- -D warnings` must be clean.\n\
         status: {:?}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        output.status
    );
}
