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
mod quality;
mod storage;
mod storage_sqlite;

use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;

pub use error::{RegistryError, StorageError};
pub use quality::{compute_badges, AuditResult, Package, QualityBadges, MAINTAINED_WINDOW};
pub use storage::{
    DepSpec, InMemoryStorage, PackageMetadata, PackageSummary, PublishRequest, PublishResponse,
    QualityAttachment, ResolveResponse, Storage, StorageResult, VersionInfo,
};
pub use storage_sqlite::SqliteStorage;

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
    /// future backends (Postgres + S3) implement the same trait.
    pub storage: Arc<dyn Storage>,
    /// Per-token rolling-window length. Publishes older than this are
    /// pruned before counting.
    pub rate_limit_window: Duration,
    /// Max publishes per token inside `rate_limit_window`.
    pub rate_limit_max: usize,
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
}

/// Build the axum [`Router`] for the registry.
///
/// Tests call this with a freshly-seeded [`InMemoryStorage`] and drive
/// the resulting `Router<()>` in-process via
/// [`tower::ServiceExt::oneshot`]. The binary entry ([`main`]) calls
/// this with a real storage backend and serves it via [`axum::serve`].
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/publish", post(handlers::publish))
        .route("/api/v1/package/{name}", get(handlers::get_package))
        .route("/api/v1/download/{name}/{version}", get(handlers::download))
        .route("/api/v1/resolve/{name}", get(handlers::resolve))
        .route(
            "/api/v1/packages/{name}/badges",
            get(handlers::get_badges),
        )
        .route("/api/v1/search", get(handlers::search))
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
}
