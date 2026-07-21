//! `buffup list` — enumerate installed versions.
//!
//! Scans `<buff_home>/versions/`, parses each entry name as a semver
//! `Version`, sorts ascending, and prints one per line. The active
//! version (the one `<buff_home>/bin/buff[.exe]` points at) is
//! marked with `* (active)`.
//!
//! # Output shape
//!
//! ```text
//! v1.0.0
//! v1.1.0 * (active)
//! v1.2.0
//! ```
//!
//! Non-semver directory names are silently skipped — this keeps the
//! output stable even if the user manually drops an `old/` or
//! `staging/` directory into `versions/`.

use crate::error::BuffupError;
use crate::paths;

/// A single entry in the `list` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    pub version: semver::Version,
    pub active: bool,
}

/// Collect the list of installed versions (sorted ascending).
///
/// Returns `Ok(vec![])` when the `versions/` dir does not exist yet
/// (a fresh install). The CLI's [`run`] helper prints a friendly
/// "no versions installed" message in that case.
pub fn collect() -> Result<Vec<ListEntry>, BuffupError> {
    let dir = paths::versions_dir()?;
    let active = read_active_version();

    let mut entries: Vec<ListEntry> = Vec::new();

    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            // Bind the OsString to a local so the &str outlives the
            // match arm (otherwise the temporary is dropped at the
            // statement boundary — E0716).
            let file_name = entry.file_name();
            let name = match file_name.to_str() {
                Some(n) => n,
                None => continue,
            };
            let v = match semver::Version::parse(name) {
                Ok(v) => v,
                Err(_) => continue,
            };
            entries.push(ListEntry {
                version: v.clone(),
                active: active.as_ref() == Some(&v),
            });
        }
    }

    entries.sort_by(|a, b| a.version.cmp(&b.version));
    Ok(entries)
}

/// Read the currently-active version by resolving the
/// `<buff_home>/bin/buff` symlink.
///
/// Returns `None` when:
/// - the symlink doesn't exist (no version marked active yet).
/// - the symlink target is not under `versions/` (corrupted state).
/// - the target's parent dir name is not a valid semver.
fn read_active_version() -> Option<semver::Version> {
    let link = paths::active_link().ok()?;
    let target = std::fs::canonicalize(&link).ok()?;
    let parent = target.parent()?;
    let name = parent.file_name()?.to_str()?;
    semver::Version::parse(name).ok()
}

/// Entry point for `buffup list`. Prints to stdout.
pub fn run() -> Result<(), BuffupError> {
    let entries = collect()?;
    if entries.is_empty() {
        println!("No versions installed.");
        println!("Run `buffup install <version>` to install one.");
        return Ok(());
    }
    for entry in &entries {
        let marker = if entry.active { " * (active)" } else { "" };
        println!("v{}{}", entry.version, marker);
    }
    Ok(())
}
