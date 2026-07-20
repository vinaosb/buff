//! `buff install <name>` — install a binary package from the buff
//! registry.
//!
//! For the v1.6 MVP, "install" means: resolve `<name>` to its latest
//! published version, download the tarball, and unpack it into
//! `~/.buff/install/<name>/<version>/`. This mirrors the
//! cargo-install flow's download-and-place semantics, minus the
//! post-download build step (deferred — `buff-registry` ships
//! raw `.buff` source tarballs, NOT pre-built binaries).
//!
//! # Behavior
//!
//! 1. Resolve `<name>` against `/api/v1/resolve/<name>?req=*` (latest).
//! 2. Download the tarball via `/api/v1/download/<name>/<version>`
//!    (anonymous, no auth).
//! 3. Unpack into `<buff_home>/install/<name>/<version>/`.
//! 4. Log the install path so the user can wire it into their project.
//!
//! # Errors
//!
//! - Registry unreachable.
//! - `<name>` not found.
//! - Tarball unpack I/O error.
//!
//! # Deferred
//!
//! - Building the downloaded source into a native binary (the
//!   `buff build` integration is post-v1.0 work).
//! - Adding the install path to the user's `PATH` or a symlink
//!   (cargo-install-style `~/.buff/bin/<name>`).
//! - Checksum / signature verification (deferred alongside
//!   `RegistryDependency::checksum`).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::commands::registry::{download_tarball, registry_url, resolve_version};

/// Entry point for `buff install <name>`.
///
/// Resolves `<name>` against the registry, downloads the tarball, and
/// unpacks it into `<buff_home>/install/<name>/<version>/`.
pub fn run(name: &str) -> Result<PathBuf> {
    let base_url = registry_url();
    let install_root = install_root_for(name)?;
    install_latest(name, &base_url, &install_root)
}

/// Same as [`run`] but takes the registry URL + install root
/// explicitly (used by integration tests so they can target an
/// ephemeral port + a per-test tempdir).
pub fn install_latest(name: &str, base_url: &str, install_root: &Path) -> Result<PathBuf> {
    let resolved = resolve_version(base_url, name, "*")
        .with_context(|| format!("could not resolve `{name}` on {base_url}"))?;
    let version = resolved.version.clone();
    eprintln!("Resolved {name} -> {version}");

    let tarball_bytes = download_tarball(base_url, name, &version)
        .with_context(|| format!("download of {name}@{version} failed"))?;
    eprintln!(
        "Downloaded {} bytes for {name}@{version}",
        tarball_bytes.len()
    );

    let target_dir = install_root.join(&version);
    if target_dir.exists() {
        eprintln!(
            "Reusing existing install at {} (delete to force re-install)",
            target_dir.display()
        );
        return Ok(target_dir);
    }
    fs::create_dir_all(&target_dir)
        .with_context(|| format!("failed to create {}", target_dir.display()))?;

    let mut archive = tar::Archive::new(&tarball_bytes[..]);
    archive
        .unpack(&target_dir)
        .with_context(|| format!("failed to unpack tarball into {}", target_dir.display()))?;
    eprintln!("Installed {name}@{version} -> {}", target_dir.display());
    Ok(target_dir)
}

/// Resolve the install root for `<name>`: `<buff_home>/install/<name>/`.
///
/// `<buff_home>` comes from [`crate::config::buff_home_dir`] so the
/// `BUFF_HOME` / `USERPROFILE` / `HOME` fallback chain matches the
/// rest of the CLI's user-cache discovery.
pub fn install_root_for(name: &str) -> Result<PathBuf> {
    let home = crate::config::buff_home_dir().map_err(anyhow::Error::msg)?;
    Ok(home.join("install").join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_root_for_targets_buff_install_subdir() {
        let home = std::env::temp_dir().join(format!("buff-install-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        std::env::set_var("BUFF_HOME", &home);
        let root = install_root_for("demo").expect("root");
        // Restore env (best-effort).
        std::env::remove_var("BUFF_HOME");
        assert!(root.starts_with(&home), "{}", root.display());
        assert!(root.ends_with("demo"), "{}", root.display());
        assert!(
            root.to_string_lossy().contains("install"),
            "{}",
            root.display()
        );
        let _ = fs::remove_dir_all(&home);
    }
}
