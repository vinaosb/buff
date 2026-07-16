//! Integration tests — run each v0.1 example end-to-end through `deox run`.
//!
//! These exercise the *whole* pipeline (lex → parse → codegen → rustc → run)
//! against the canonical example files committed under `examples/`. A failure
//! here means a regression in any phase, not just the CLI.
//!
//! All tests gate on [`rustc_available`] because the final link step needs
//! `link.exe` / the MSVC environment on Windows.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Locate the workspace `examples/` directory by walking up from CARGO_MANIFEST_DIR.
///
/// `CARGO_MANIFEST_DIR` is `.../crates/deox-cli`, so the workspace root is two
/// levels up and `examples/` sits directly under it.
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

/// Run `cargo run -p deox-cli -- run <example>` and return the captured stdout.
///
/// Spawning the CLI as a subprocess is the most faithful end-to-end check — it
/// exercises `main.rs` dispatch and the run pipeline exactly as a user would.
/// The cost is one rustc invocation per example (~1-2 s each); acceptable for
/// a milestone gate.
fn run_example(example: &str) -> String {
    let output = Command::new("cargo")
        .args(["run", "-q", "-p", "deox-cli", "--", "run"])
        .arg(examples_dir().join(example))
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn `cargo run` for {example}: {e}"));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "`deox run examples/{example}` failed with status {:?}\nstderr:\n{stderr}",
            output.status
        );
    }
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn test_example_ola() {
    if !rustc_available() {
        eprintln!("skipping test_example_ola: rustc not on PATH");
        return;
    }
    let stdout = run_example("ola.deox");
    assert!(
        stdout.contains("Olá, Deox!"),
        "ola.deox should print `Olá, Deox!`; got:\n{stdout}"
    );
}

#[test]
fn test_example_fibonacci() {
    if !rustc_available() {
        eprintln!("skipping test_example_fibonacci: rustc not on PATH");
        return;
    }
    // fib(10) = 55.
    let stdout = run_example("fibonacci.deox");
    assert!(
        stdout.trim().ends_with('5') && stdout.trim().contains("55"),
        "fibonacci.deox should print `55` for n=10; got:\n{stdout}"
    );
}

#[test]
fn test_example_calculadora() {
    if !rustc_available() {
        eprintln!("skipping test_example_calculadora: rustc not on PATH");
        return;
    }
    // add(2, 3) = 5.
    let stdout = run_example("calculadora.deox");
    let trimmed = stdout.trim();
    assert!(
        trimmed == "5",
        "calculadora.deox should print exactly `5` for add(2,3); got: `{trimmed}`"
    );
}

#[test]
fn test_examples_directory_exists_with_v01_set() {
    // Cheap existence check — no rustc needed. Catches accidental deletion of
    // an example file before the rustc-gated tests would even try to run it.
    let dir = examples_dir();
    assert!(dir.is_dir(), "examples/ dir should exist at workspace root");
    for f in &["ola.deox", "fibonacci.deox", "calculadora.deox"] {
        let p = dir.join(f);
        assert!(p.is_file(), "expected example `{}` to exist", p.display());
    }
}
