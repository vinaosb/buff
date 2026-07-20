//! `buff clean` — remove the `target/` build directory (wraps `cargo clean`).
//!
//! Shells out to `cargo clean` in the current working directory. This is the
//! simplest possible wrapper — no flags, no options. Equivalent to running
//! `cargo clean` in the project root.
//!
//! # Errors
//!
//! - Fails if `cargo` cannot be invoked (not installed / not in `PATH`).
//! - Fails if `cargo clean` exits with a non-zero status.

use std::process::Command;

use anyhow::{Context, Result};

/// Entry point for `buff clean`.
///
/// Invokes `cargo clean` in the current working directory. Prints a
/// confirmation to stderr on success.
pub fn run() -> Result<()> {
    let result = Command::new("cargo")
        .arg("clean")
        .output()
        .context("failed to invoke `cargo` — is it installed and on your PATH?")?;

    // Forward cargo's stderr (progress / warnings).
    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        eprint!("{stderr}");
    }

    if !result.status.success() {
        anyhow::bail!("cargo clean exited with status {}", result.status);
    }

    eprintln!("Cleaned build artifacts");
    Ok(())
}
