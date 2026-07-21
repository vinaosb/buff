//! `buffup default <version>` — set the active Buff version.
//!
//! Resolves the install dir for `<version>`, finds the `buff` (Unix)
//! or `buff.exe` (Windows) binary inside it, and points
//! `<buff_home>/bin/buff[.exe]` at that binary.
//!
//! # Platform behavior
//!
//! - **Unix** (`unix` cfg): a real symlink via
//!   `std::os::unix::fs::symlink`. No privileges required. Existing
//!   symlink/file at the target path is removed first.
//! - **Windows** (`windows` cfg): `std::os::windows::fs::symlink_file`
//!   is attempted first (requires Developer Mode on Windows 10+ or
//!   administrator privileges). On failure the command falls back to
//!   a plain file copy via `std::fs::copy`. The copy does NOT track
//!   subsequent reinstalls of the same version — running
//!   `buffup install <same-version>` again will NOT update the
//!   active binary unless the user re-runs `buffup default`.
//!
//! # Why the fallback?
//!
//! Symlink creation on Windows is gated behind privileges that the
//! overwhelming majority of users do NOT have on a fresh install.
//! Failing hard would make `buffup` unusable for the most common
//! case; the copy fallback is a small UX cost (the `buff` binary is
//! a few MB) for a working default.

use std::path::{Path, PathBuf};

use crate::error::BuffupError;
use crate::paths;

/// Entry point for `buffup default <version>`.
pub fn run(version: String) -> Result<(), BuffupError> {
    let v = semver::Version::parse(&version)?;
    let version_dir = paths::version_dir(&v)?;
    if !version_dir.exists() {
        return Err(BuffupError::VersionNotInstalled(v));
    }

    let binary = locate_binary(&version_dir)?;
    let bin_dir = paths::bin_dir()?;
    std::fs::create_dir_all(&bin_dir)?;

    let link = paths::active_link()?;

    // Remove any existing pointer (symlink OR regular file OR stub).
    // `path_exists()` is used instead of `try_exists()` because the
    // symlink target may have been deleted out-of-band — we still
    // want to unlink the dangling pointer.
    if link.exists() || is_symlink(&link) {
        remove_link(&link)?;
    }

    install_link(&binary, &link)?;

    eprintln!("buffup: default version set to v{}", v);
    eprintln!("       active binary:  {}", link.display());
    eprintln!(
        "       add this directory to your PATH:\n         {}",
        bin_dir.display()
    );
    Ok(())
}

/// Find the `buff` (Unix) or `buff.exe` (Windows) binary inside the
/// per-version install dir.
///
/// Searches the top level first, then any single nested directory
/// (GitHub tarballs sometimes add a `<owner>-<repo>-<sha>/` prefix
/// directory). Returns [`BuffupError::BinaryMissing`] if no candidate
/// exists.
fn locate_binary(version_dir: &Path) -> Result<PathBuf, BuffupError> {
    let name = paths::binary_name();

    // Top-level match (the common case).
    let direct = version_dir.join(name);
    if direct.is_file() {
        return Ok(direct);
    }

    // Single-level-nested match (GitHub tarball prefix directory).
    if let Ok(rd) = std::fs::read_dir(version_dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let candidate = path.join(name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }

    Err(BuffupError::BinaryMissing(
        version_dir.display().to_string(),
    ))
}

/// Platform-specific pointer installation.
fn install_link(target: &Path, link: &Path) -> Result<(), BuffupError> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
            .map_err(|e| BuffupError::Symlink(format!("create symlink {}: {}", link.display(), e)))
    }

    #[cfg(windows)]
    {
        // Try symlink first; fall back to plain copy.
        match std::os::windows::fs::symlink_file(target, link) {
            Ok(()) => Ok(()),
            Err(_) => {
                std::fs::copy(target, link).map_err(|e| {
                    BuffupError::Symlink(format!(
                        "copy {} -> {}: {} (Windows symlink requires Developer Mode or admin)",
                        target.display(),
                        link.display(),
                        e
                    ))
                })?;
                Ok(())
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        // Unknown platform — last-resort copy.
        std::fs::copy(target, link).map_err(|e| {
            BuffupError::Symlink(format!(
                "copy {} -> {}: {}",
                target.display(),
                link.display(),
                e
            ))
        })?;
        Ok(())
    }
}

/// Remove an existing pointer at `link` (symlink OR file).
fn remove_link(link: &Path) -> Result<(), BuffupError> {
    // `remove_file` works for both symlinks and regular files on
    // Unix AND Windows. (Removing a directory junction would need
    // `remove_dir` but we never create a junction.)
    std::fs::remove_file(link)
        .map_err(|e| BuffupError::Symlink(format!("remove existing {}: {}", link.display(), e)))
}

/// Whether `path` is a symlink (or Windows junction).
///
/// `Path::exists()` returns `false` for a symlink whose target has
/// been deleted, but we still want to unlink such a dangling pointer
/// before installing a new one — this helper lets us detect that.
fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}
