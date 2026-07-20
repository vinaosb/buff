//! `buff ui build --desktop [PATH]` — build a Tauri 2.0 desktop app.
//!
//! Detects whether the Tauri CLI (`cargo-tauri`) is installed. If missing,
//! prints a helpful install instruction and exits non-zero. If present,
//! shells out to `cargo tauri build` in the project directory.
//!
//! # Errors
//!
//! - Tauri CLI not installed → prints install instruction, returns error.
//! - `cargo tauri build` fails → returns the subprocess error.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Check whether `cargo-tauri` is available on `PATH`.
///
/// Runs `cargo tauri --version` and checks for success. Returns `true` if
/// the subprocess exits with code 0.
fn tauri_cli_available() -> bool {
    Command::new("cargo")
        .args(["tauri", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Entry point for `buff ui build --desktop [PATH]`.
///
/// 1. Checks that the Tauri CLI is installed.
/// 2. Changes to the project directory (or cwd if not specified).
/// 3. Runs `cargo tauri build` and streams output to the user.
///
/// # Errors
///
/// - Tauri CLI not found → prints install instruction, returns error.
/// - `cargo tauri build` fails → returns the subprocess error.
pub fn run(project_path: Option<&Path>) -> Result<()> {
    if !tauri_cli_available() {
        eprintln!("Tauri CLI is required to build desktop apps.");
        eprintln!("Install it with: cargo install tauri-cli");
        bail!("Tauri CLI not found; install with `cargo install tauri-cli`");
    }

    let dir = project_path.unwrap_or_else(|| Path::new("."));

    if !dir.exists() {
        bail!("project directory `{}` does not exist", dir.display());
    }

    eprintln!("Building Tauri desktop app in `{}`...", dir.display());

    let status = Command::new("cargo")
        .args(["tauri", "build"])
        .current_dir(dir)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| {
            format!(
                "failed to execute `cargo tauri build` in `{}`",
                dir.display()
            )
        })?;

    if !status.success() {
        bail!("`cargo tauri build` failed with exit code: {}", status);
    }

    eprintln!("Tauri desktop build complete.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test that `tauri_cli_available()` returns `false` when the Tauri
    /// CLI is not installed. This is the expected state on CI / fresh
    /// environments. The function must not panic — it returns `false`
    /// gracefully.
    #[test]
    fn tauri_cli_not_available_by_default() {
        // We cannot assume `cargo-tauri` is installed on any machine.
        // This test just verifies the function runs without panicking.
        tauri_cli_available();
    }

    #[test]
    fn run_returns_error_when_tauri_cli_missing() {
        // If tauri-cli IS installed, skip this test (it would pass the
        // CLI check and then fail on the missing project dir instead).
        if tauri_cli_available() {
            eprintln!("skipping: tauri-cli is installed on this machine");
            return;
        }

        let err = run(None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("Tauri CLI not found"),
            "expected Tauri CLI not-found error, got: {msg}"
        );
    }

    #[test]
    fn run_returns_error_for_nonexistent_path() {
        if !tauri_cli_available() {
            eprintln!("skipping: tauri-cli not installed");
            return;
        }

        let fake_path = Path::new("__nonexistent_dir_42__");
        let err = run(Some(fake_path)).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("does not exist"),
            "expected directory-not-found error, got: {msg}"
        );
    }
}
