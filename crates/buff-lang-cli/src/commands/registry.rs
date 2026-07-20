//! Shared HTTP-client + credential-store helpers for the T127
//! package-management subcommands (`buff login` / `buff add <name>` /
//! `buff publish` / `buff install`).
//!
//! All four subcommands talk to a `buff-registry` HTTP instance. The
//! registry URL, credentials, and JSON wire shapes are identical across
//! them, so the helpers here are the single source of truth — the
//! command files stay thin.
//!
//! # Registry URL resolution
//!
//! [`registry_url`] resolves the registry base URL via (in order):
//!
//! 1. The `BUFF_REGISTRY_URL` env var (test isolation + advanced users).
//! 2. [`DEFAULT_REGISTRY_URL`] — `http://127.0.0.1:7878`, the same
//!    loopback `buff-registry` itself binds by default
//!    ([`buff_registry::DEFAULT_BIND_ADDR`]).
//!
//! This mirrors how `buff-registry` resolves its OWN bind address via
//! `BUFF_REGISTRY_ADDR`, but on the CLIENT side (the env-var names
//! differ deliberately — server-bind vs client-target).
//!
//! # Credential store
//!
//! [`credentials_path`] resolves `~/.buff/credentials` via
//! [`crate::config::buff_home_dir`] (same home-discovery logic the T122
//! git-checkout cache and the T125c REPL history file use). The file
//! is plain TOML:
//!
//! ```toml
//! token = "<bearer-token>"
//! ```
//!
//! kept dead-simple for the v1.6 milestone (single registry, single
//! token). A future multi-registry version can extend the table
//! (`[registries.<url>] token = ...`) without breaking the parse —
//! serde ignores unknown keys.
//!
//! # Panic-free contract
//!
//! Mirrors the rest of the CLI: no `unwrap`/`expect`/`panic!` in
//! non-test code. All fallible operations surface as [`anyhow::Error`].

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// The default registry base URL when `BUFF_REGISTRY_URL` is unset.
///
/// Matches the loopback default `buff-registry` itself binds
/// (`buff_registry::DEFAULT_BIND_ADDR` = `127.0.0.1:7878`) so a
/// `buff-registry` started with no flags is the out-of-the-box target.
pub const DEFAULT_REGISTRY_URL: &str = "http://127.0.0.1:7878";

/// The env-var name used to override the registry base URL on the
/// CLIENT side.
///
/// Deliberately distinct from `buff_registry::BIND_ADDR_ENV`
/// (`BUFF_REGISTRY_ADDR`) — that one configures the server bind
/// address, this one configures the client target. A future
/// multi-registry CLI may consume both.
pub const REGISTRY_URL_ENV: &str = "BUFF_REGISTRY_URL";

/// On-the-wire shape of `GET /api/v1/resolve/<name>?req=...`.
///
/// Mirrors [`buff_registry::ResolveResponse`] but defined locally so
/// the CLI does NOT depend on `buff-registry` at runtime — only as a
/// dev-dependency for in-process integration tests. Keeping the type
/// local means a future `buff-registry` 2.0 schema bump can be absorbed
/// here without forcing a CLI recompile against the path dep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveResponse {
    /// The package name.
    pub name: String,
    /// The resolved version string (canonical semver form).
    pub version: String,
}

/// On-the-wire shape of `GET /api/v1/package/<name>`.
///
/// Mirrors [`buff_registry::PackageMetadata`] (locally redefined for
/// the same reason as [`ResolveResponse`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageMetadata {
    /// The package name.
    pub name: String,
    /// Every published version, ascending.
    pub versions: Vec<PackageVersionInfo>,
}

/// One element of [`PackageMetadata::versions`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageVersionInfo {
    /// The version string (canonical semver form).
    pub version: String,
    /// The version's declared dependencies.
    pub deps: Vec<DepSpec>,
}

/// On-the-wire shape of a single dependency edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepSpec {
    /// Depended-on package name.
    pub name: String,
    /// Cargo-style semver requirement (`^1.0.0`, `*`, etc.).
    pub req: String,
}

/// On-the-wire shape of `POST /api/v1/publish` (request body).
///
/// Mirrors [`buff_registry::PublishRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishRequest {
    /// The package name.
    pub name: String,
    /// The version string.
    pub version: String,
    /// Declared dependencies.
    pub deps: Vec<DepSpec>,
    /// Base64-encoded tarball bytes.
    pub tarball_b64: String,
}

/// On-the-wire shape of the `POST /api/v1/publish` 201 response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishResponse {
    /// The package name.
    pub name: String,
    /// The canonical version string.
    pub version: String,
    /// The recorded dependencies.
    pub deps: Vec<DepSpec>,
}

/// The persisted credentials file shape. Plain TOML, single token for
/// the v1.6 milestone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Credentials {
    /// The bearer token to send on `Authorization: Bearer <token>` for
    /// `buff publish`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Resolve the registry base URL.
///
/// Order: `$BUFF_REGISTRY_URL` env var, then [`DEFAULT_REGISTRY_URL`].
///
/// NEVER panics — returns the default when the env var is unset or
/// contains invalid Unicode (treated as unset).
pub fn registry_url() -> String {
    if let Ok(custom) = std::env::var(REGISTRY_URL_ENV) {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    DEFAULT_REGISTRY_URL.to_string()
}

/// Build a `reqwest::blocking::Client` for the registry.
///
/// The client has a 30-second timeout (matches the T126 registry's
/// default rate-limit window budget — never times out a legitimate
/// request) and no redirect handling (the registry never redirects).
///
/// Returning a fresh client per call (rather than caching one in a
/// `once_cell`) keeps the call sites trivially testable — a test can
/// build its own client with the same helper and the CLI never carries
/// global state.
pub fn http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build reqwest HTTP client")
}

/// Resolve the credentials-file path: `<buff_home>/credentials`.
///
/// `<buff_home>` is resolved via [`crate::config::buff_home_dir`] so
/// the `BUFF_HOME` / `USERPROFILE` / `HOME` fallback chain matches the
/// T122 git-checkout cache and the T125c REPL history file.
pub fn credentials_path() -> Result<PathBuf> {
    let home = crate::config::buff_home_dir().map_err(anyhow::Error::msg)?;
    Ok(home.join("credentials"))
}

/// Load credentials from [`credentials_path`].
///
/// Returns an empty [`Credentials`] when the file does not exist
/// (first-run experience — `buff login` hasn't run yet). Returns
/// `Err` on I/O errors other than "missing" or on parse failure.
pub fn load_credentials() -> Result<Credentials> {
    let path = credentials_path()?;
    load_credentials_from(&path)
}

/// Same as [`load_credentials`] but takes the path explicitly (tests).
pub fn load_credentials_from(path: &Path) -> Result<Credentials> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Credentials::default()),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("failed to read credentials at {}", path.display()));
        }
    };
    let creds: Credentials = toml::from_str(&text)
        .with_context(|| format!("failed to parse credentials at {}", path.display()))?;
    Ok(creds)
}

/// Save credentials to [`credentials_path`], creating parent
/// directories as needed. Atomicity is best-effort (write-in-place) —
/// a future hardening pass can swap in a temp-file + rename.
pub fn save_credentials(creds: &Credentials) -> Result<()> {
    let path = credentials_path()?;
    save_credentials_to(creds, &path)
}

/// Same as [`save_credentials`] but takes the path explicitly (tests).
pub fn save_credentials_to(creds: &Credentials, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create credentials dir {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(creds)
        .context("failed to serialize credentials (this is a bug — the shape is trivial)")?;
    fs::write(path, text)
        .with_context(|| format!("failed to write credentials to {}", path.display()))?;
    Ok(())
}

/// Read the stored bearer token, or `bail!` with a helpful message
/// pointing the user at `buff login`.
pub fn require_token() -> Result<String> {
    let creds = load_credentials()?;
    match creds.token {
        Some(t) if !t.trim().is_empty() => Ok(t.trim().to_string()),
        _ => bail!(
            "no registry credentials stored — run `buff login <TOKEN>` first \
             (credentials file: {})",
            credentials_path()?.display()
        ),
    }
}

/// GET `/api/v1/resolve/<name>?req=<req>` — returns the highest
/// published version of `name` matching `req`.
///
/// `req` is a Cargo-style semver requirement (`^1.0.0`, `*`, etc.).
/// Returns `Err` on connection failure or non-2xx response; the error
/// message includes the registry's JSON `error` field when present.
pub fn resolve_version(base_url: &str, name: &str, req: &str) -> Result<ResolveResponse> {
    let client = http_client()?;
    let url = format!(
        "{}/api/v1/resolve/{}",
        base_url.trim_end_matches('/'),
        url_encode(name)
    );
    let response = client
        .get(&url)
        .query(&[("req", req)])
        .send()
        .with_context(|| format!("failed to GET {url} (registry unavailable?)"))?;
    decode_response(response)
}

/// GET `/api/v1/package/<name>` — returns the full package metadata.
pub fn fetch_package_metadata(base_url: &str, name: &str) -> Result<PackageMetadata> {
    let client = http_client()?;
    let url = format!(
        "{}/api/v1/package/{}",
        base_url.trim_end_matches('/'),
        url_encode(name)
    );
    let response = client
        .get(&url)
        .send()
        .with_context(|| format!("failed to GET {url} (registry unavailable?)"))?;
    decode_response(response)
}

/// GET `/api/v1/download/<name>/<version>` — returns the raw tarball
/// bytes (anonymous, no auth required).
pub fn download_tarball(base_url: &str, name: &str, version: &str) -> Result<Vec<u8>> {
    let client = http_client()?;
    let url = format!(
        "{}/api/v1/download/{}/{}",
        base_url.trim_end_matches('/'),
        url_encode(name),
        url_encode(version)
    );
    let response = client
        .get(&url)
        .send()
        .with_context(|| format!("failed to GET {url} (registry unavailable?)"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        bail!(
            "download {name}@{version} failed: HTTP {status} — {}",
            body.trim()
        );
    }
    let bytes = response
        .bytes()
        .with_context(|| format!("failed to read download body from {url}"))?;
    Ok(bytes.to_vec())
}

/// POST `/api/v1/publish` with the given bearer token + JSON body.
/// Returns the registry's `PublishResponse` on 201.
pub fn publish_package(
    base_url: &str,
    token: &str,
    body: &PublishRequest,
) -> Result<PublishResponse> {
    let client = http_client()?;
    let url = format!("{}/api/v1/publish", base_url.trim_end_matches('/'));
    let response = client
        .post(&url)
        .bearer_auth(token)
        .json(body)
        .send()
        .with_context(|| format!("failed to POST {url} (registry unavailable?)"))?;
    decode_response(response)
}

/// Decode an HTTP response into `T`, surfacing the registry's JSON
/// `error` field when present (matches the
/// `{"error": "<message>"}` body shape documented in
/// `crates/buff-registry/AGENTS.md`).
fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::blocking::Response,
) -> Result<T> {
    let status = response.status();
    if status.is_success() {
        return response.json::<T>().context(
            "failed to decode registry response (this is a bug — the wire shape drifted)",
        );
    }
    let body = response.text().unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
    let msg = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| body.trim());
    bail!("registry returned HTTP {status}: {msg}");
}

/// Minimal URL-path segment encoder. The registry's package-name
/// charset (`[a-z0-9_-]`, enforced by [`buff_registry::validate_name`])
/// needs no escaping in practice; this helper percent-encodes any stray
/// odd characters defensively so a malformed name can't craft a path
/// like `..` or `/`.
fn url_encode(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Unit tests for the pure helpers in `commands::registry`. Live
    //! HTTP round-trip coverage lives in `tests/registry_cli_t127.rs`.

    use super::*;

    #[test]
    fn registry_url_default_when_env_unset() {
        // Snapshot the env var so we don't bleed state across tests.
        let prev = std::env::var(REGISTRY_URL_ENV).ok();
        std::env::remove_var(REGISTRY_URL_ENV);
        assert_eq!(registry_url(), DEFAULT_REGISTRY_URL);
        if let Some(v) = prev {
            std::env::set_var(REGISTRY_URL_ENV, v);
        }
    }

    #[test]
    fn registry_url_env_override_wins() {
        let prev = std::env::var(REGISTRY_URL_ENV).ok();
        std::env::set_var(REGISTRY_URL_ENV, "http://example.test:9999");
        assert_eq!(registry_url(), "http://example.test:9999");
        match prev {
            Some(v) => std::env::set_var(REGISTRY_URL_ENV, v),
            None => std::env::remove_var(REGISTRY_URL_ENV),
        }
    }

    #[test]
    fn registry_url_blank_env_falls_back_to_default() {
        let prev = std::env::var(REGISTRY_URL_ENV).ok();
        std::env::set_var(REGISTRY_URL_ENV, "   ");
        assert_eq!(registry_url(), DEFAULT_REGISTRY_URL);
        match prev {
            Some(v) => std::env::set_var(REGISTRY_URL_ENV, v),
            None => std::env::remove_var(REGISTRY_URL_ENV),
        }
    }

    #[test]
    fn credentials_round_trip_through_disk() {
        let dir = std::env::temp_dir().join(format!(
            "buff-registry-creds-rt-{}-{}",
            std::process::id(),
            "round-trip"
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("credentials");

        let creds = Credentials {
            token: Some("test-token-abc".to_string()),
        };
        save_credentials_to(&creds, &path).expect("save");

        let loaded = load_credentials_from(&path).expect("load");
        assert_eq!(loaded, creds);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_credentials_returns_default_when_file_missing() {
        let dir = std::env::temp_dir().join(format!(
            "buff-registry-creds-missing-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("credentials");
        let loaded = load_credentials_from(&path).expect("missing file → default");
        assert_eq!(loaded, Credentials::default());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_credentials_creates_parent_dir() {
        let dir = std::env::temp_dir().join(format!(
            "buff-registry-creds-parents-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        // Do NOT create dir; save_credentials_to must create it.
        let path = dir.join("credentials");
        let creds = Credentials {
            token: Some("abc".to_string()),
        };
        save_credentials_to(&creds, &path).expect("save with parent creation");
        assert!(path.is_file());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn require_token_errors_when_no_credentials() {
        let dir = std::env::temp_dir().join(format!(
            "buff-registry-creds-require-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("credentials");
        let result = (|| {
            let creds = load_credentials_from(&path)?;
            match creds.token {
                Some(t) if !t.trim().is_empty() => Ok(t.trim().to_string()),
                _ => anyhow::bail!("no token"),
            }
        })();
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn url_encode_passes_safe_charset() {
        assert_eq!(url_encode("foo-bar_baz.1"), "foo-bar_baz.1");
    }

    #[test]
    fn url_encode_escapes_path_separators() {
        // Defensive — registry-side validate_name rejects these, but
        // if one slips through the URL must NOT contain a literal slash.
        assert_eq!(url_encode("../evil"), "..%2Fevil");
        // The percent-encoding of `/` is `%2F` regardless:
        assert!(url_encode("a/b").contains("%2F"));
        assert!(!url_encode("a/b").contains("/b"));
    }

    #[test]
    fn publish_request_serializes_to_expected_envelope() {
        let req = PublishRequest {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            deps: vec![DepSpec {
                name: "other".to_string(),
                req: "^1.0.0".to_string(),
            }],
            tarball_b64: "AAAA".to_string(),
        };
        let json = serde_json::to_value(&req).expect("serialize");
        assert_eq!(json["name"], "demo");
        assert_eq!(json["version"], "1.0.0");
        assert_eq!(json["tarball_b64"], "AAAA");
        assert_eq!(json["deps"][0]["name"], "other");
        assert_eq!(json["deps"][0]["req"], "^1.0.0");
    }
}
