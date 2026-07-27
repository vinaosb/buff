//! `buff-registry` — minimal Buff package registry HTTP server.
//!
//! Implements the v1.6 "Package registry" MVP surface (task T126 of
//! `buff-post-v10-tooling.md`): an [`axum`]-based HTTP server exposing
//! publish / package / download / resolve endpoints, backed by a
//! [`Storage`] trait with a pure-Rust [`InMemoryStorage`] implementation.
//!
//! ```text
//!                ┌──────────────────────────────────────┐
//!  HTTP request  │ axum::Router                         │
//!        ──────▶ │  /api/v1/publish                     │
//!                │  /api/v1/package/{name}              │
//!                │  /api/v1/download/{name}/{version}   │
//!                │  /api/v1/resolve/{name}?req=...      │
//!                └──────────────┬───────────────────────┘
//!                               │
//!                               ▼
//!                ┌──────────────────────────────────────┐
//!                │ handlers (auth, validate, cycle, ... )│
//!                └──────────────┬───────────────────────┘
//!                               │
//!                               ▼
//!                ┌──────────────────────────────────────┐
//!                │ trait Storage                        │
//!                │  └─ InMemoryStorage  (this crate)    │
//!                └──────────────────────────────────────┘
//! ```
//!
//! # Endpoints
//!
//! | Method | Path                                  | Auth | Body                | Returns                              |
//! |--------|---------------------------------------|------|--------------------|--------------------------------------|
//! | POST   | `/api/v1/publish`                     | yes  | JSON [`PublishRequest`] | `201 Created` ([`PublishResponse`]) |
//! | GET    | `/api/v1/package/{name}`              | no   | —                  | `200` ([`PackageMetadata`]) or `404` |
//! | GET    | `/api/v1/download/{name}/{version}`   | no   | —                  | `200` octet-stream or `404`          |
//! | GET    | `/api/v1/resolve/{name}?req=<semver>` | no   | —                  | `200` ([`ResolveResponse`]) or `404` |
//! | GET    | `/api/v1/packages/{name}/badges`      | no   | —                  | `200` ([`QualityBadges`]) or `404` (T70) |
//! | GET    | `/api/v1/search?q=<query>`            | no   | —                  | `200` `Vec<SearchResultRow>` (T70)   |
//!
//! # DEFERRED (post-v1.6)
//!
//! The MVP scope is deliberately small. The following are explicitly
//! DEFERRED — a real `Storage` impl drops in later without touching
//! the HTTP layer:
//!
//! - **Postgres via `diesel`** — the trait's sync surface means a
//!   `DieselStorage` impl can wrap its async DB calls in
//!   `tokio::task::spawn_blocking`, or expose a sync facade. The trait
//!   itself needs no change. The in-memory backend shipped here is the
//!   ONLY backend; it is NOT durable across restarts.
//! - **S3 / MinIO blob storage** — tarballs are currently stored
//!   in-memory alongside metadata (`Vec<u8>` per `(name, version)`).
//!   A future impl can split tarball bytes out to object storage and
//!   keep metadata in Postgres; the [`Storage::get_tarball`] /
//!   [`Storage::put_version`] signatures already encapsulate that
//!   boundary.
//! - **GitHub OAuth** — the v1.6 milestone ships a static-token
//!   [`InMemoryStorage::add_token`] surface for tests / local dev. Real
//!   token provisioning (GitHub OAuth flow with the `jsonwebtoken` +
//!   `octocrab` crates) is post-v1.6.
//! - **Deployment** — no Docker / Fly.io / Railway manifests ship here.
//!   The `buff-registry` binary binds a TCP listener whose address is
//!   configurable via `BUFF_REGISTRY_ADDR` (default `127.0.0.1:7878`),
//!   which is enough for a future deployment wrapper to wire up. Ops
//!   runbook (backup / restore) is also deferred.
//! - **Search UI, docs hosting, download stats, webhooks, teams,
//!   RustSec / CVE audit integration** — explicitly out of scope per
//!   the T126 "Must NOT do" list.
//!
//! This mirrors the workspace's established "cargo-project wiring is
//! deferred" precedent used throughout the root `Cargo.toml`.
//!
//! # Panic-free contract
//!
//! There are no `unwrap` / `expect` / `panic!` / `unimplemented!` /
//! `todo!` calls in non-test code. All fallible operations surface as
//! [`RegistryError`] (handlers) or [`StorageError`] (storage), both
//! `#[derive(thiserror::Error)]`. The HTTP layer maps every error to
//! a fixed status code via [`axum::response::IntoResponse`].
//!
//! # Testing
//!
//! Integration tests in `tests/registry_tests.rs` drive the axum
//! [`Router`] in-process via [`tower::ServiceExt::oneshot`] — no TCP
//! port allocation, no subprocess. The [`app`] function is the public
//! entry point tests build with a freshly-seeded [`InMemoryStorage`].

mod error;
mod handlers;
mod oauth;
mod quality;
mod rate_limit;
mod storage;
mod storage_sqlite;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use axum::Router;

pub use error::{RegistryError, StorageError};
pub use oauth::OAuthConfig;
pub use quality::{compute_badges, AuditResult, Package, QualityBadges, MAINTAINED_WINDOW};
pub use rate_limit::{
    ip_rate_limit_middleware, IpRateLimiter, DEFAULT_IP_RATE_LIMIT_MAX, IP_RATE_LIMIT_MAX_ENV,
};
pub use storage::{
    DepSpec, InMemoryStorage, PackageMetadata, PackageSummary, PublishRequest, PublishResponse,
    QualityAttachment, ResolveResponse, SessionUser, Storage, StorageResult, VersionInfo,
};
pub use storage_sqlite::SqliteStorage;

/// The env-var name used to disable the invite-only beta allowlist
/// (default: enabled — the registry is invite-only). Set to `false`
/// or `0` to disable allowlist enforcement (NOT recommended for
/// production — allows open registration via OAuth).
pub const ALLOWLIST_ENABLED_ENV: &str = "BUFF_REGISTRY_ALLOWLIST_ENABLED";

/// The default bind address when `BUFF_REGISTRY_ADDR` is unset.
///
/// Loopback only — the registry does NOT listen on `0.0.0.0` by default
/// (the v1.6 milestone does not ship auth hardening, HTTPS termination,
/// or rate-limit-backed abuse protection past a per-token counter).
pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:7878";

/// The env-var name used to override the bind address.
pub const BIND_ADDR_ENV: &str = "BUFF_REGISTRY_ADDR";

/// The default per-token publish rate-limit window (5 minutes).
pub const DEFAULT_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(5 * 60);

/// The default per-token publish budget inside the rolling window.
pub const DEFAULT_RATE_LIMIT_MAX: usize = 100;

/// Shared state threaded through every handler via [`axum::extract::State`].
///
/// Cloning is cheap — `Arc<dyn Storage>` is a single atomic refcount
/// increment, and the two `Duration` / `usize` fields are `Copy`.
#[derive(Clone)]
pub struct AppState {
    /// Backend storage. Concrete type is [`InMemoryStorage`] for v1.6;
    /// [`SqliteStorage`] for T57 production; future backends implement
    /// the same trait.
    pub storage: Arc<dyn Storage>,
    /// Per-token rolling-window length. Publishes older than this are
    /// pruned before counting.
    pub rate_limit_window: Duration,
    /// Max publishes per token inside `rate_limit_window`.
    pub rate_limit_max: usize,
    /// T57: GitHub OAuth configuration. `None` when OAuth env vars are
    /// not set (login endpoints return 503; static-token auth still works).
    pub oauth_config: Option<OAuthConfig>,
    /// T57: Whether the invite-only beta allowlist is enforced. When
    /// `true` (default), OAuth logins are rejected unless the GitHub
    /// login is on the allowlist. Set to `false` via
    /// `BUFF_REGISTRY_ALLOWLIST_ENABLED=false` for open registration
    /// (NOT recommended for production).
    pub allowlist_enabled: bool,
    /// T57: Directory for filesystem tarball storage. `None` = store
    /// tarballs in the SQLite BLOB (default). When set, the new
    /// multipart upload endpoint writes tarballs to this directory.
    pub tarball_dir: Option<std::path::PathBuf>,
    /// T57: Per-IP rate limiter (applies to ALL endpoints). Shared
    /// across handler tasks.
    pub ip_rate_limiter: Arc<IpRateLimiter>,
    /// T57: Per-IP rate limit budget (requests per window).
    pub ip_rate_limit_max: usize,
    /// P0.28 (sec-hardening): Maximum request body size in bytes
    /// enforced by the router-level `DefaultBodyLimit` layer (see
    /// [`app`]). Defaults to [`handlers::MAX_BODY_BYTES`] (50 MiB);
    /// tests can override with [`Self::with_body_limit`] to exercise
    /// the 413 Payload Too Large path without allocating 50+ MiB.
    pub body_limit_bytes: usize,
}

impl AppState {
    /// Construct an `AppState` from a storage backend, using the
    /// workspace default rate-limit settings.
    #[must_use]
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            rate_limit_window: DEFAULT_RATE_LIMIT_WINDOW,
            rate_limit_max: DEFAULT_RATE_LIMIT_MAX,
            oauth_config: OAuthConfig::from_env(),
            allowlist_enabled: parse_allowlist_enabled(),
            tarball_dir: parse_tarball_dir(),
            ip_rate_limiter: Arc::new(IpRateLimiter::new()),
            ip_rate_limit_max: parse_ip_rate_limit_max(),
            body_limit_bytes: handlers::MAX_BODY_BYTES,
        }
    }

    /// Override the rate-limit budget. Returns `self` for chaining so
    /// tests can write `AppState::new(storage).with_rate_limit(secs(1), 3)`.
    #[must_use]
    pub fn with_rate_limit(mut self, window: Duration, max: usize) -> Self {
        self.rate_limit_window = window;
        self.rate_limit_max = max;
        self
    }

    /// T57: Override the OAuth configuration (for tests that inject a
    /// mock config pointing at a local mock server).
    #[must_use]
    pub fn with_oauth_config(mut self, config: Option<OAuthConfig>) -> Self {
        self.oauth_config = config;
        self
    }

    /// T57: Override the allowlist enforcement (for tests that want
    /// to bypass the invite-only gate).
    #[must_use]
    pub fn with_allowlist_enabled(mut self, enabled: bool) -> Self {
        self.allowlist_enabled = enabled;
        self
    }

    /// T57: Override the tarball directory (for tests that want a
    /// tempdir).
    #[must_use]
    pub fn with_tarball_dir(mut self, dir: Option<impl AsRef<std::path::Path>>) -> Self {
        self.tarball_dir = dir.map(|d| d.as_ref().to_path_buf());
        self
    }

    /// P0.28: Override the maximum request body size (in bytes). Tests
    /// use a small value (e.g. `with_body_limit(1024)`) to exercise the
    /// 413 Payload Too Large path without allocating 50+ MiB.
    #[must_use]
    pub fn with_body_limit(mut self, limit_bytes: usize) -> Self {
        self.body_limit_bytes = limit_bytes;
        self
    }
}

/// Parse the `BUFF_REGISTRY_ALLOWLIST_ENABLED` env var. Returns `true`
/// (allowlist enforced) unless the env var is explicitly set to `false`
/// or `0`.
fn parse_allowlist_enabled() -> bool {
    match std::env::var(ALLOWLIST_ENABLED_ENV) {
        Ok(v) => !v.eq_ignore_ascii_case("false") && v != "0",
        Err(_) => true, // default: allowlist enforced
    }
}

/// Parse the `BUFF_REGISTRY_TARBALL_DIR` env var. Returns `None` when
/// unset (tarballs stored in SQLite BLOB).
fn parse_tarball_dir() -> Option<std::path::PathBuf> {
    std::env::var("BUFF_REGISTRY_TARBALL_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
}

/// Parse the `BUFF_REGISTRY_IP_RATE_LIMIT_MAX` env var. Returns the
/// default (1000) when unset.
fn parse_ip_rate_limit_max() -> usize {
    std::env::var(IP_RATE_LIMIT_MAX_ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_IP_RATE_LIMIT_MAX)
}

/// Build the axum [`Router`] for the registry.
///
/// Tests call this with a freshly-seeded [`InMemoryStorage`] and drive
/// the resulting `Router<()>` in-process via
/// [`tower::ServiceExt::oneshot`]. The binary entry ([`main`]) calls
/// this with a real storage backend and serves it via [`axum::serve`].
pub fn app(state: AppState) -> Router {
    // P0.28: capture the body limit BEFORE moving `state` into
    // `with_state` so we can install the layer with the right size.
    let body_limit = state.body_limit_bytes;
    Router::new()
        .route("/api/v1/publish", post(handlers::publish))
        .route("/api/v1/package/{name}", get(handlers::get_package))
        .route("/api/v1/download/{name}/{version}", get(handlers::download))
        .route("/api/v1/resolve/{name}", get(handlers::resolve))
        .route("/api/v1/packages/{name}/badges", get(handlers::get_badges))
        .route("/api/v1/packages/{name}/stats", get(handlers::get_stats))
        .route("/api/v1/search", get(handlers::search))
        // T57: New multipart upload + download endpoints (parallel to the
        // legacy JSON publish/download — the new endpoints use the
        // multipart tarball format and filesystem storage).
        .route("/api/v1/packages/{name}", post(handlers::multipart_publish))
        .route(
            "/api/v1/packages/{name}/{version}/download",
            get(handlers::multipart_download),
        )
        // P0.18: Health / readiness probes (BEFORE rate-limiting middleware
        // so probes are never throttled).
        .route("/health", get(handlers::health_handler))
        .route("/ready", get(handlers::ready_handler))
        // T57: GitHub OAuth login flow.
        .route("/auth/github/login", get(oauth::login))
        .route("/auth/github/callback", get(oauth::callback))
        .route("/auth/logout", post(oauth::logout))
        .route("/auth/whoami", get(oauth::whoami))
        // P0.28: 50 MiB request body limit (configurable via
        // `AppState::body_limit_bytes`). Applied AFTER routing so the
        // 413 response is consistent across all routes. Enforced by
        // axum's built-in `DefaultBodyLimit` layer — no new crate deps.
        .layer(DefaultBodyLimit::max(body_limit))
        // T57: IP-based rate limiting on ALL endpoints.
        .layer(from_fn_with_state(state.clone(), ip_rate_limit_middleware))
        .with_state(state)
}

/// Bind a TCP listener on `addr` and serve the registry.
///
/// Used by the binary entry. Tests do NOT call this — they drive the
/// `Router` directly via `oneshot`. Returns when the server stops
/// accepting connections (graceful shutdown via `axum::serve`'s default
/// signal handling is NOT wired up for v1.6 — the registry runs until
/// Ctrl-C).
///
/// # Errors
///
/// Returns [`std::io::Error`] iff the TCP bind or the serving loop fails.
pub async fn run(addr: &str, state: AppState) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app(state)).await?;
    Ok(())
}

/// Validate a Buff package name.
///
/// Rules (the registry rejects any name that fails any rule):
///
/// - Length: 1–64 characters (inclusive).
/// - Charset: ASCII lowercase letters (`a-z`), digits (`0-9`), hyphen
///   (`-`), and underscore (`_`). No uppercase, no Unicode.
/// - No path separators (`/` or `\`) — would let a malicious name
///   escape the storage layout if a future backend used the name as
///   part of a filesystem path.
/// - No `..` segment — same rationale.
/// - No leading or trailing whitespace (covered by the charset rule).
///
/// This catches the QA scenario "publish `../evil`" (path traversal)
/// and "publish empty / all-whitespace name" (squatting / typo).
pub fn validate_name(name: &str) -> Result<(), RegistryError> {
    if name.is_empty() || name.len() > 64 {
        return Err(RegistryError::InvalidName);
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(RegistryError::InvalidName);
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(RegistryError::InvalidName);
    }
    Ok(())
}

/// T57: Validate a Buff package name, supporting BOTH unscoped (`foo`)
/// and scoped (`@org/pkg`) names.
///
/// For unscoped names (no `@` prefix), this delegates to [`validate_name`].
///
/// For scoped names (`@org/pkg`), the rules are:
/// - Must start with `@`.
/// - Must contain exactly one `/` separating `@org` from `pkg`.
/// - **Org** (`@org`): 1–64 chars, ASCII lowercase letters + digits +
///   hyphens (NO underscores — matches npm's scope convention).
/// - **Pkg** (after `/`): 1–64 chars, same charset as unscoped names
///   (`[a-z0-9_-]`).
/// - No `..`, no path separators beyond the single `/`.
///
/// Returns `Ok(())` on success, `Err(RegistryError::InvalidName)` on
/// any validation failure.
///
/// # Examples
///
/// ```
/// use buff_registry::validate_package_name;
///
/// assert!(validate_package_name("foo").is_ok());          // unscoped
/// assert!(validate_package_name("@org/pkg").is_ok());     // scoped
/// assert!(validate_package_name("@buff/core").is_ok());   // scoped
/// assert!(validate_package_name("../evil").is_err());     // traversal
/// assert!(validate_package_name("@ORG/pkg").is_err());    // uppercase org
/// assert!(validate_package_name("@org/").is_err());       // empty pkg
/// ```
pub fn validate_package_name(name: &str) -> Result<(), RegistryError> {
    if name.starts_with('@') {
        validate_scoped_name(name)
    } else {
        validate_name(name)
    }
}

/// T57: Validate a scoped package name (`@org/pkg`). See
/// [`validate_package_name`] for the full rules.
fn validate_scoped_name(name: &str) -> Result<(), RegistryError> {
    // Strip the leading '@'.
    let after_at = name.strip_prefix('@').ok_or(RegistryError::InvalidName)?;
    // Split on the FIRST '/' only (pkg names can't contain '/' anyway,
    // but being explicit avoids confusion).
    let Some((org, pkg)) = after_at.split_once('/') else {
        return Err(RegistryError::InvalidName);
    };
    // Org: must start with a lowercase letter, 1–64 chars, [a-z0-9-].
    if org.is_empty() || org.len() > 64 {
        return Err(RegistryError::InvalidName);
    }
    if !org
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(RegistryError::InvalidName);
    }
    // Pkg: same rules as unscoped names, but we already consumed the '/'.
    // Re-check the pkg portion against the unscoped charset.
    if pkg.is_empty() || pkg.len() > 64 {
        return Err(RegistryError::InvalidName);
    }
    if pkg.contains('/') || pkg.contains('\\') || pkg.contains("..") {
        return Err(RegistryError::InvalidName);
    }
    if !pkg
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(RegistryError::InvalidName);
    }
    Ok(())
}

/// T57: Extract the org name from a scoped package name.
///
/// Returns `Some("org")` for `@org/pkg`, `None` for unscoped names.
#[must_use]
pub fn scope_of(name: &str) -> Option<&str> {
    if name.starts_with('@') {
        name.strip_prefix('@')
            .and_then(|s| s.split_once('/'))
            .map(|(org, _)| org)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_accepts_canonical_form() {
        assert!(validate_name("foo").is_ok());
        assert!(validate_name("foo-bar").is_ok());
        assert!(validate_name("foo_bar").is_ok());
        assert!(validate_name("foo123").is_ok());
        assert!(validate_name("a").is_ok());
        // Boundary: 64-char name is OK.
        assert!(validate_name(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn validate_name_rejects_empty_and_long() {
        assert!(matches!(validate_name(""), Err(RegistryError::InvalidName)));
        assert!(matches!(
            validate_name(&"a".repeat(65)),
            Err(RegistryError::InvalidName)
        ));
    }

    #[test]
    fn validate_name_rejects_path_traversal() {
        assert!(matches!(
            validate_name("../evil"),
            Err(RegistryError::InvalidName)
        ));
        assert!(matches!(
            validate_name("foo/bar"),
            Err(RegistryError::InvalidName)
        ));
        assert!(matches!(
            validate_name("foo\\bar"),
            Err(RegistryError::InvalidName)
        ));
        assert!(matches!(
            validate_name(".."),
            Err(RegistryError::InvalidName)
        ));
    }

    #[test]
    fn validate_name_rejects_uppercase_and_unicode() {
        assert!(matches!(
            validate_name("Foo"),
            Err(RegistryError::InvalidName)
        ));
        assert!(matches!(
            validate_name("café"),
            Err(RegistryError::InvalidName)
        ));
        assert!(matches!(
            validate_name("foo.bar"),
            Err(RegistryError::InvalidName)
        ));
        assert!(matches!(
            validate_name("foo bar"),
            Err(RegistryError::InvalidName)
        ));
    }

    #[test]
    fn appstate_new_uses_default_rate_limit() {
        let storage: Arc<dyn Storage> = Arc::new(InMemoryStorage::new());
        let state = AppState::new(storage);
        assert_eq!(state.rate_limit_window, DEFAULT_RATE_LIMIT_WINDOW);
        assert_eq!(state.rate_limit_max, DEFAULT_RATE_LIMIT_MAX);
    }

    #[test]
    fn appstate_with_rate_limit_overrides() {
        let storage: Arc<dyn Storage> = Arc::new(InMemoryStorage::new());
        let state = AppState::new(storage).with_rate_limit(Duration::from_secs(1), 3);
        assert_eq!(state.rate_limit_window, Duration::from_secs(1));
        assert_eq!(state.rate_limit_max, 3);
    }

    // --- T57 scoped-name validation tests ---

    #[test]
    fn validate_package_name_accepts_unscoped() {
        assert!(validate_package_name("foo").is_ok());
        assert!(validate_package_name("foo-bar").is_ok());
        assert!(validate_package_name("foo_bar").is_ok());
    }

    #[test]
    fn validate_package_name_accepts_scoped() {
        assert!(validate_package_name("@org/pkg").is_ok());
        assert!(validate_package_name("@buff/core").is_ok());
        assert!(validate_package_name("@my-org/my-pkg").is_ok());
        assert!(validate_package_name("@org123/pkg").is_ok());
    }

    #[test]
    fn validate_package_name_rejects_uppercase_scope() {
        assert!(matches!(
            validate_package_name("@ORG/pkg"),
            Err(RegistryError::InvalidName)
        ));
        assert!(matches!(
            validate_package_name("@org/Pkg"),
            Err(RegistryError::InvalidName)
        ));
    }

    #[test]
    fn validate_package_name_rejects_underscore_in_scope() {
        // npm convention: scopes use hyphens, not underscores.
        assert!(matches!(
            validate_package_name("@my_org/pkg"),
            Err(RegistryError::InvalidName)
        ));
    }

    #[test]
    fn validate_package_name_rejects_empty_scope_or_pkg() {
        assert!(matches!(
            validate_package_name("@/pkg"),
            Err(RegistryError::InvalidName)
        ));
        assert!(matches!(
            validate_package_name("@org/"),
            Err(RegistryError::InvalidName)
        ));
        assert!(matches!(
            validate_package_name("@"),
            Err(RegistryError::InvalidName)
        ));
    }

    #[test]
    fn validate_package_name_rejects_no_slash_in_scoped() {
        assert!(matches!(
            validate_package_name("@orgpkg"),
            Err(RegistryError::InvalidName)
        ));
    }

    #[test]
    fn scope_of_extracts_org() {
        assert_eq!(scope_of("@org/pkg"), Some("org"));
        assert_eq!(scope_of("@buff/core"), Some("buff"));
        assert_eq!(scope_of("unscoped"), None);
        assert_eq!(scope_of(""), None);
    }
}
