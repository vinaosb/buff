//! Filesystem path resolution for buffup.
//!
//! Every path the CLI cares about hangs off a single "buff home"
//! root:
//!
//! - `<buff_home>/versions/<ver>/` — per-version install dirs.
//! - `<buff_home>/bin/buff[.exe]` — active-version pointer (a
//!   symlink on Unix, a copy or symlink on Windows).
//!
//! The `<buff_home>` root is `$HOME/.buff` (Unix) or
//! `%USERPROFILE%\.buff` (Windows) by default, but tests override it
//! via the `BUFFUP_HOME` env var so the test suite never touches the
//! user's real `~/.buff/`.

use std::path::PathBuf;

use crate::error::BuffupError;

/// Name of the env var that overrides the `<buff_home>` root.
///
/// Tests set this to a [`tempfile::TempDir`] path so they never
/// mutate the user's real `~/.buff/`.
pub const BUFFUP_HOME_ENV: &str = "BUFFUP_HOME";

/// Resolve the `<buff_home>` root directory.
///
/// Precedence:
/// 1. `$BUFFUP_HOME` (test override / power-user override).
/// 2. `dirs::home_dir().join(".buff")`.
///
/// Returns [`BuffupError::HomeDir`] only when both fail (no env var
/// AND `dirs::home_dir()` returned `None`).
pub fn buff_home() -> Result<PathBuf, BuffupError> {
    if let Ok(p) = std::env::var(BUFFUP_HOME_ENV) {
        let path = PathBuf::from(p);
        return Ok(path);
    }
    dirs::home_dir()
        .map(|h| h.join(".buff"))
        .ok_or(BuffupError::HomeDir)
}

/// `<buff_home>/versions/` — parent of every per-version install dir.
pub fn versions_dir() -> Result<PathBuf, BuffupError> {
    Ok(buff_home()?.join("versions"))
}

/// `<buff_home>/versions/<ver>/` — install dir for a single version.
pub fn version_dir(version: &semver::Version) -> Result<PathBuf, BuffupError> {
    Ok(versions_dir()?.join(version.to_string()))
}

/// `<buff_home>/bin/` — directory the user adds to their `PATH`.
pub fn bin_dir() -> Result<PathBuf, BuffupError> {
    Ok(buff_home()?.join("bin"))
}

/// `<buff_home>/bin/buff` (Unix) or `<buff_home>/bin/buff.exe`
/// (Windows) — the active-version pointer. Symlink on Unix, symlink
/// or copy on Windows (see [`crate::commands::default_cmd`]).
pub fn active_link() -> Result<PathBuf, BuffupError> {
    let mut p = bin_dir()?.join("buff");
    if cfg!(windows) {
        p.set_extension("exe");
    }
    Ok(p)
}

/// Name of the buff binary inside an install dir.
///
/// `buff` on Unix, `buff.exe` on Windows. The install tarball is
/// expected to contain exactly one such file at its top level.
pub fn binary_name() -> &'static str {
    if cfg!(windows) {
        "buff.exe"
    } else {
        "buff"
    }
}
