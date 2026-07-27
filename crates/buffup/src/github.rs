//! GitHub Releases API client for buffup.
//!
//! Buff's release artifacts are published to
//! [`github.com/buff-lang/buff/releases`](https://github.com/buff-lang/buff/releases).
//! This module wraps the small slice of the GitHub REST API we need:
//!
//! - `GET /repos/buff-lang/buff/releases/tags/v<version>` — fetch
//!   the release metadata (JSON envelope with `tag_name`,
//!   `tarball_url`, `zipball_url`, and any uploaded `assets`).
//!
//! The tarball URL embedded in that JSON is then fetched separately
//! by [`crate::commands::install`] — we don't stream it through this
//! module to keep the API surface minimal.
//!
//! # Test override
//!
//! Tests redirect the API base URL via the [`GITHUB_API_BASE_ENV`]
//! env var (set to a `httpmock::MockServer::base_url()`). Production
//! callers leave the env var unset and [`api_base`] returns the
//! [`GITHUB_API_BASE`] constant.

use serde::Deserialize;

use crate::error::BuffupError;

/// Hex-encoded SHA-256 digest string (64 lowercase hex chars).
pub type Sha256Hex = String;

/// Default base URL for the GitHub Releases API.
///
/// Matches the canonical `api.github.com/repos/{owner}/{repo}` shape.
pub const GITHUB_API_BASE: &str = "https://api.github.com/repos/buff-lang/buff";

/// Name of the env var that overrides [`GITHUB_API_BASE`] (tests
/// point this at a `httpmock::MockServer`).
pub const GITHUB_API_BASE_ENV: &str = "BUFFUP_GITHUB_API";

/// User-Agent header sent on every request.
///
/// GitHub's REST API mandates a `User-Agent` (requests without one
/// get `403`). The value is informational — `buffup/<version>` would
/// be ideal but hardcoding `buffup` keeps the bin self-contained
/// without needing to track `env!("CARGO_PKG_VERSION")` everywhere.
pub const USER_AGENT: &str = "buffup";

/// Resolve the API base URL, honoring [`GITHUB_API_BASE_ENV`] when set.
pub fn api_base() -> String {
    std::env::var(GITHUB_API_BASE_ENV).unwrap_or_else(|_| GITHUB_API_BASE.to_string())
}

/// Subset of the GitHub Release JSON envelope we care about.
///
/// Field names match the API response 1:1 — `serde` rename rules are
/// NOT applied so a missing or renamed field surfaces as a clear
/// `BuffupError::Json` rather than a silent `None`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    /// Git tag the release points at (e.g. `v1.0.0`).
    pub tag_name: String,
    /// Absolute URL to the gzip tarball download (api.github.com
    /// shortlink that redirects to codeload.github.com).
    pub tarball_url: String,
    /// Absolute URL to the zip archive download (unused on Unix;
    /// available for a future Windows-preferred path).
    pub zipball_url: String,
    /// Release assets (uploaded binaries). Empty for source-only
    /// releases. Kept for forward compatibility — buffup's install
    /// flow currently consumes `tarball_url` only.
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// A single asset attached to a GitHub Release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    /// Asset filename (e.g. `buff-x86_64-unknown-linux-gnu.tar.gz`).
    pub name: String,
    /// Direct download URL for the asset blob.
    pub browser_download_url: String,
}

/// Fetch the release JSON for `version` (without the leading `v`).
///
/// Constructs `GET <api_base>/releases/tags/v<version>`, sends it
/// with a `User-Agent: buffup` header (mandated by GitHub's API),
/// and deserializes the body into a [`Release`].
///
/// Non-2xx responses surface as [`BuffupError::HttpStatus`] carrying
/// the raw status code. The most common case — `404` for an
/// unpublished version — is special-cased in
/// [`crate::commands::install`] with a clearer "GitHub Releases don't
/// exist yet" message before re-returning the error.
pub async fn fetch_release(
    client: &reqwest::Client,
    version: &str,
) -> Result<Release, BuffupError> {
    let url = format!("{}/releases/tags/v{}", api_base(), version);
    let resp = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(BuffupError::HttpStatus(status));
    }
    let release: Release = resp.json().await?;
    Ok(release)
}

/// Fetch the `.sha256` sidecar for a tarball and return the expected
/// hex digest.
///
/// The sidecar URL is `<tarball_url>.sha256`. GitHub Releases
/// published by the release workflow include a `.sha256` sidecar for
/// every archive. The sidecar content is either `<hex_hash>  <filename>`
/// or just `<hex_hash>` — we parse the first whitespace-delimited token.
pub async fn fetch_checksum_sidecar(
    client: &reqwest::Client,
    tarball_url: &str,
) -> Result<Sha256Hex, BuffupError> {
    let sidecar_url = format!("{tarball_url}.sha256");
    let resp = client
        .get(&sidecar_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(BuffupError::HttpStatus(status));
    }
    let body = resp.text().await?;
    // Parse first whitespace-delimited token (handles both
    // "<hex>  <filename>" and bare "<hex>" formats).
    let hex = body
        .split_whitespace()
        .next()
        .ok_or_else(|| BuffupError::Extract(format!("empty .sha256 sidecar at {sidecar_url}")))?;
    Ok(hex.to_string())
}

/// Compute the SHA-256 hex digest of `data`.
pub fn sha256_digest(data: &[u8]) -> Sha256Hex {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(data);
    hash.iter().map(|b| format!("{b:02x}")).collect()
}
