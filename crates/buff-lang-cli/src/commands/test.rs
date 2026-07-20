//! `buff test` — run tests for a `.buff` file OR a whole project (T123).
//!
//! Two modes:
//!
//! 1. **Single-file mode** (T35): `buff test <FILE> [--pattern <PATTERN>]`
//!    discovers `@test` functions in a `.buff` file via the Buff test runner
//!    (parse → discover → harness → rustc → run). See [`test_runner`].
//!
//! 2. **Project / workspace mode** (T123): `buff test` (no file argument)
//!    reads `buff.toml` from the current directory and shells out to
//!    `cargo test`. In workspace mode (`[workspace]` present) cargo fans
//!    out to all members automatically; in single-package mode cargo
//!    tests the one crate. This is a strict passthrough — Buff does NOT
//!    reinvent test discovery at the project level.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::config::BuffConfig;
use crate::test_runner;

/// Entry point for `buff test [<FILE>] [--pattern <PATTERN>]`.
///
/// - `file = Some(f)` → single-file mode (T35 behaviour).
/// - `file = None` → project / workspace mode (T123). Reads `buff.toml`
///   from cwd, shells out to `cargo test`.
///
/// Returns `Ok(())` if all tests pass (exit 0). If any test fails, the
/// process exits with code `1` directly (via [`std::process::exit`]) in
/// single-file mode so the exit code is preserved; in project mode a
/// cargo failure surfaces as an `Err` (anyhow::bail!) with the cargo
/// exit status.
///
/// # Errors
///
/// Propagates any pipeline error (file-not-found, lex/parse/codegen
/// failure, rustc invocation failure, missing `buff.toml`). A failing
/// TEST (assertion panic inside a `@test` fn in single-file mode) is NOT
/// an `Err` here — it's reflected in the [`test_runner::TestReport`]
/// counts and triggers an `exit(1)`.
pub fn run(file: Option<&Path>, pattern: Option<&str>) -> Result<()> {
    match file {
        Some(f) => run_single_file(f, pattern),
        None => run_project(),
    }
}

/// Single-file mode (T35): discover and run `@test` functions in a `.buff`
/// file via the Buff test runner.
fn run_single_file(file: &Path, pattern: Option<&str>) -> Result<()> {
    let pat = pattern.unwrap_or("");
    let report = test_runner::run_tests(file, pat)?;

    eprintln!("{}", report.summary_line());

    if report.failed > 0 {
        std::process::exit(report.exit_code());
    }
    Ok(())
}

/// Project / workspace mode (T123): shell out to `cargo test` at the
/// project root (single-package) or workspace root (workspace mode).
///
/// In workspace mode, [`commands::build`](crate::commands::build) handles
/// generating the virtual `Cargo.toml` + transpiling members on the
/// preceding `buff build`. For test-only invocations we still emit the
/// virtual `Cargo.toml` (idempotent) so cargo knows the workspace shape,
/// but we do NOT transpile here — the user is expected to have run
/// `buff build` first OR the `.rs` files are already present. (cargo will
/// error loudly if a member's `src/main.rs` is missing.)
fn run_project() -> Result<()> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    let manifest_path = cwd.join("buff.toml");
    let cfg = BuffConfig::load_from_file(&manifest_path)
        .with_context(|| format!("failed to load `{}`", manifest_path.display()))?;

    // Emit Cargo.toml (virtual workspace form OR single-package form).
    // generate_cargo_toml is idempotent so this is safe even if `buff build`
    // already wrote it.
    let cargo_toml = crate::config::generate_cargo_toml(&cfg);
    let cargo_path = cwd.join("Cargo.toml");
    std::fs::write(&cargo_path, &cargo_toml)
        .with_context(|| format!("failed to write `{}`", cargo_path.display()))?;

    // cargo test at the project / workspace root. cargo handles fan-out.
    invoke_cargo_test(&cwd)
}

/// Invoke `cargo test` at `root`, forwarding stdout/stderr and mapping
/// non-zero exit to an `Err`.
fn invoke_cargo_test(root: &PathBuf) -> Result<()> {
    let result = Command::new("cargo")
        .arg("test")
        .current_dir(root)
        .output()
        .context("failed to invoke `cargo` — is it installed and on your PATH?")?;

    // Forward cargo's stdout (test results) and stderr (progress / warnings).
    if !result.stdout.is_empty() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        print!("{stdout}");
    }
    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        eprint!("{stderr}");
    }

    if !result.status.success() {
        anyhow::bail!("cargo test exited with status {}", result.status);
    }

    eprintln!("Ran tests");
    Ok(())
}
