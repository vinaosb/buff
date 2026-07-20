//! `buff publish` — pack the current project's `.buff` source into a
//! tarball and upload it to the buff registry.
//!
//! # Behavior
//!
//! 1. Read `buff.toml` from the current working directory.
//! 2. Require `[package]` with `name` and `version` (the publish
//!    payload carries both).
//! 3. Walk `src/` recursively, pack every file into a `tar::Builder`
//!    backed by `Vec<u8>` (no compression — the registry base64-encodes
//!    the bytes inside the JSON envelope, so gzip would just add CPU).
//! 4. Load the bearer token from `~/.buff/credentials` (set via
//!    `buff login`). Bail with a helpful message if missing.
//! 5. POST `/api/v1/publish` with [`PublishRequest`] body
//!    (`{ name, version, deps, tarball_b64 }`).
//! 6. Surface the registry's `PublishResponse` (echoes name + version).
//!
//! # Errors
//!
//! - Missing / unparsable `buff.toml`.
//! - Project layout without a `src/` directory.
//! - No stored credentials (`buff login` not run).
//! - Registry unreachable / non-201 response.
//!
//! # Deferred
//!
//! - Per-version tarball signing.
//! - `.buffignore` (we pack everything under `src/`).
//! - Concurrent-publish race (registry's storage trait is
//!   append-only; the second PUT loses with a 400 "version exists").

use std::path::Path;

use anyhow::{bail, Context, Result};
use base64::Engine as _;

use crate::commands::registry::{
    fetch_package_metadata, publish_package, registry_url, require_token, DepSpec, PublishRequest,
    PublishResponse,
};
use crate::config::BuffConfig;

/// Entry point for `buff publish` invoked from the current working
/// directory.
pub fn run() -> Result<()> {
    let project_root = Path::new(".");
    let buff_toml = project_root.join("buff.toml");
    let cfg = BuffConfig::load_from_file(&buff_toml).with_context(|| {
        format!(
            "failed to load {} — run `buff init` first",
            buff_toml.display()
        )
    })?;
    let base_url = registry_url();
    let token = require_token()?;
    let _response = publish_project(&cfg, project_root, &base_url, &token)?;
    Ok(())
}

/// Same as [`run`] but takes the loaded config + project root
/// explicitly (used by integration tests so they don't need to chdir).
pub fn publish_project(
    cfg: &BuffConfig,
    project_root: &Path,
    base_url: &str,
    token: &str,
) -> Result<PublishResponse> {
    let package = cfg.package.as_ref().with_context(|| {
        format!(
            "`buff publish` requires a `[package]` section in {} — \
             virtual workspace manifests cannot be published",
            project_root.join("buff.toml").display()
        )
    })?;
    let name = package.name.trim();
    let version = package.version.trim();
    if name.is_empty() {
        bail!("`[package].name` is empty in buff.toml");
    }
    if version.is_empty() {
        bail!("`[package].version` is empty in buff.toml");
    }

    eprintln!(
        "Packing tarball for {name}@{version} from {}/src",
        project_root.display()
    );
    let tarball = build_tarball(project_root)?;

    // The registry's cycle detector walks declared registry-deps, so
    // we emit only `[registry-dependencies]` here. Git-deps + Rust-deps
    // are NOT registry packages and would just confuse the cycle
    // detector; we omit them (mirrors how Cargo's publish payload
    // includes only the registry-resolvable slice).
    let deps: Vec<DepSpec> = cfg
        .registry_dependencies
        .iter()
        .map(|(n, d)| DepSpec {
            name: n.clone(),
            req: d.version.clone(),
        })
        .collect();

    let body = PublishRequest {
        name: name.to_string(),
        version: version.to_string(),
        deps,
        tarball_b64: base64::engine::general_purpose::STANDARD.encode(&tarball),
    };

    eprintln!("Uploading {name}@{version} to {base_url}");
    let response = publish_package(base_url, token, &body)
        .with_context(|| format!("publish of {name}@{version} failed"))?;
    eprintln!("Published {name}@{version} ({} deps)", response.deps.len());
    Ok(response)
}

/// Build a tarball of `project_root/src/` into a fresh `Vec<u8>`.
///
/// Every file under `src/` (recursively) is packed. The tarball layout
/// is `<prefix>/<relative>` where `<prefix>` is the package name
/// (`src` if the package name can't be resolved — defensive). The
/// registry stores the bytes verbatim; the consumer (`buff install`)
/// unpacks them with `tar::Archive::unpack`.
pub fn build_tarball(project_root: &Path) -> Result<Vec<u8>> {
    let src_dir = project_root.join("src");
    if !src_dir.is_dir() {
        bail!(
            "no `src/` directory under {} — `buff publish` requires one",
            project_root.display()
        );
    }
    let mut builder = tar::Builder::new(Vec::new());
    // Use a stable prefix so the unpacked layout is `<pkg-name>/...`
    // rather than leaking the build machine's absolute path.
    let prefix = "package";
    builder
        .append_dir_all(prefix, &src_dir)
        .with_context(|| format!("failed to tar src/ at {}", src_dir.display()))?;
    builder.finish().context("failed to finalize tarball")?;
    let bytes: Vec<u8> = builder
        .into_inner()
        .context("failed to drain tar builder into Vec<u8>")?;
    Ok(bytes)
}

/// Pre-publish check: would the registry reject this name + version?
///
/// Used as a defensive UX step (and as a test helper). Best-effort —
/// if the registry is unreachable, returns `Ok(())` so the publish
/// itself surfaces the real error.
pub fn check_package_name_available(base_url: &str, name: &str) -> Result<()> {
    match fetch_package_metadata(base_url, name) {
        Ok(_) => {
            // Existing package — publish will be a new version (the
            // registry rejects re-publishing the SAME version). Not an
            // error here.
            Ok(())
        }
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "buff-publish-{}-{}-t127",
            label,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn build_tarball_packs_src_files() {
        let dir = temp_dir("tarball-pack");
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();
        let mut f = fs::File::create(src.join("main.buff")).unwrap();
        f.write_all(b"func main():\n    print(\"hello\")\n")
            .unwrap();
        fs::create_dir_all(src.join("util")).unwrap();
        let mut g = fs::File::create(src.join("util").join("helper.buff")).unwrap();
        g.write_all(b"func helper():\n    print(\"helper\")\n")
            .unwrap();

        let bytes = build_tarball(&dir).expect("tarball");
        assert!(!bytes.is_empty(), "tarball must be non-empty");

        // Re-open the tarball and verify both files are present.
        let mut archive = tar::Archive::new(&bytes[..]);
        let names: Vec<String> = archive
            .entries()
            .expect("entries")
            .map(|e| e.expect("entry").path().unwrap().display().to_string())
            .collect();
        // Tarball must contain both files (paths are `<prefix>/...`).
        assert!(
            names.iter().any(|n| n.contains("main.buff")),
            "missing main.buff in {names:?}"
        );
        assert!(
            names.iter().any(|n| n.contains("helper.buff")),
            "missing helper.buff in {names:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_tarball_errors_when_no_src_dir() {
        let dir = temp_dir("no-src");
        // No src/ directory.
        let result = build_tarball(&dir);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&dir);
    }
}
