//! `buff update` — update all dependencies (wraps `cargo update`).
//!
//! Shells out to `cargo update` in the current working directory. This
//! regenerates `Cargo.lock` with the latest compatible versions matching
//! the version requirements in `buff.toml`.
//!
//! # Errors
//!
//! - Fails if `cargo` cannot be invoked (not installed / not in `PATH`).
//! - Fails if `cargo update` exits with a non-zero status.

use std::process::Command;

use anyhow::{Context, Result};

/// Entry point for `buff update`.
///
/// Invokes `cargo update` in the current working directory. Prints a
/// confirmation to stderr on success.
pub fn run() -> Result<()> {
    let result = Command::new("cargo")
        .arg("update")
        .output()
        .context("failed to invoke `cargo` — is it installed and on your PATH?")?;

    // Forward cargo's stderr (progress / dependency info).
    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        eprint!("{stderr}");
    }

    if !result.status.success() {
        anyhow::bail!("cargo update exited with status {}", result.status);
    }

    eprintln!("Updated dependencies");
    Ok(())
}
