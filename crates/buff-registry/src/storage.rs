//! Persistence abstraction for the Buff registry.
//!
//! [`Storage`] is the trait a backend implements to plug into the HTTP
//! handlers. The trait is **sync** (methods return `Result<T, StorageError>`
//! directly, not futures) because:
//!
//! 1. The only shipped backend ([`InMemoryStorage`]) uses
//!    `std::sync::Mutex` held for nanoseconds — calling it from async
//!    axum handlers briefly blocks the runtime, but the critical section
//!    is so short this is invisible.
//! 2. Keeping the trait sync avoids an `async_trait` dependency and the
//!    `Pin<Box<dyn Future>>` noise it introduces.
//!
//! A real database-backed impl (Postgres via `diesel`, blob storage via
//! S3/MinIO) can wrap its async operations in `tokio::task::spawn_blocking`
//! or expose a sync facade — the trait stays the same. See the crate
//! root docs for the deferred-backend note.
//!
//! # Stored shapes
//!
//! - **Packages**: `name -> {version -> PackageVersion}`. Versions are
//!   keyed by `semver::Version` so iteration order is deterministic
//!   (BTreeMap).
//! - **Tokens**: `BTreeSet<String>` of valid publish tokens. Auth is
//!   performed by [`Storage::validate_token`]; how tokens are provisioned
//!   is backend-specific (the in-memory impl exposes a
//!   [`InMemoryStorage::add_token`] helper for tests / local dev).
//! - **Rate-limit counters**: per-token `Vec<Instant>` of recent publish
//!   timestamps inside the rolling window. [`Storage::try_record_publish`]
//!   prunes + records + returns whether the publish fits the budget.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::error::StorageError;

/// A single dependency edge declared by a published package version.
///
/// `req` is a Cargo-style semver requirement string (`^1.0.0`, `>=2.1.0,
/// <3.0.0`, `*`, etc.). Parsed via [`semver::VersionReq::parse`] when
/// the resolver needs to match.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DepSpec {
    /// The depended-on package name (same `[a-z0-9_-]` charset as
    /// package names).
    pub name: String,
    /// Cargo-style semver requirement (e.g. `^1.0.0`).
    pub req: String,
}

/// Metadata for one published version of a package, returned by
/// [`Storage::get_package`] and serialized as JSON in the
/// `GET /api/v1/package/<name>` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    /// The version string (canonical form from `Version::to_string`).
    pub version: String,
    /// The version's declared dependencies.
    pub deps: Vec<DepSpec>,
}

/// Full package metadata returned by `GET /api/v1/package/<name>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageMetadata {
    /// The package name.
    pub name: String,
    /// Every published version, sorted ascending by `semver::Version`.
    pub versions: Vec<VersionInfo>,
}

/// Internal record for one stored `(name, version)` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageVersion {
    pub(crate) deps: Vec<DepSpec>,
    pub(crate) tarball: Vec<u8>,
}

/// The wire-format body of `POST /api/v1/publish`.
///
/// `tarball_b64` is the raw tarball bytes base64-encoded so the entire
/// publish request is a single JSON object (no multipart parsing, no
/// binary body ordering issues). Decoded by the handler before storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishRequest {
    /// The package name (validated by the handler before storage).
    pub name: String,
    /// The version string (parsed as `semver::Version` before storage).
    pub version: String,
    /// Declared dependencies (used for cycle detection + the index API).
    pub deps: Vec<DepSpec>,
    /// Base64-encoded tarball bytes.
    pub tarball_b64: String,
}

/// Response body for `POST /api/v1/publish` (201 Created).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishResponse {
    /// The package name.
    pub name: String,
    /// The canonical version string (post `Version::parse`).
    pub version: String,
    /// The recorded dependencies.
    pub deps: Vec<DepSpec>,
}

/// Response body for `GET /api/v1/resolve/<name>?req=...` (200 OK).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveResponse {
    /// The package name.
    pub name: String,
    /// The highest published version matching the requirement.
    pub version: String,
}

/// Persistence abstraction for the registry.
///
/// Implementations must be `Send + Sync` so they can be shared across
/// axum worker tasks via `Arc<dyn Storage>` inside [`crate::AppState`].
/// The trait is **sync** — see the module docs for the rationale.
pub trait Storage: Send + Sync {
    /// Store a new `(name, version)` pair.
    ///
    /// Returns
    /// - `Ok(())` on success,
    /// - `Err(StorageError::Failure(..))` on internal failure,
    /// - `Err(StorageError::Failure("version exists"))` if the
    ///   `(name, version)` pair is already published (the in-memory
    ///   impl uses this string-subject convention; handlers detect via
    ///   the message and map to HTTP 400 — see [`InMemoryStorage`]).
    ///
    /// Cycle detection and rate limiting are NOT done here — they live
    /// in the handler so the storage trait stays persistence-only.
    fn put_version(
        &self,
        name: &str,
        version: Version,
        deps: Vec<DepSpec>,
        tarball: Vec<u8>,
    ) -> Result<(), StorageError>;

    /// Fetch full metadata for `name`, or `Ok(None)` if the package has
    /// no published versions.
    fn get_package(&self, name: &str) -> Result<Option<PackageMetadata>, StorageError>;

    /// Fetch the raw tarball bytes for `(name, version)`, or `Ok(None)`
    /// if either the package or the version is unknown.
    fn get_tarball(&self, name: &str, version: &Version) -> Result<Option<Vec<u8>>, StorageError>;

    /// List every `(version, deps)` pair for `name`, in arbitrary order.
    /// Used by the cycle detector to walk the dep graph. Returns an
    /// empty `Vec` if the package is unknown.
    fn list_versions_with_deps(
        &self,
        name: &str,
    ) -> Result<Vec<(Version, Vec<DepSpec>)>, StorageError>;

    /// Return `true` iff `token` is known to the backend.
    ///
    /// Anonymous access (no `Authorization` header) is rejected at the
    /// handler level before reaching here, so this is only consulted
    /// for requests that DID supply a bearer token.
    fn validate_token(&self, token: &str) -> Result<bool, StorageError>;

    /// Try to record a publish event for `token` against a `(window, max)`
    /// budget.
    ///
    /// Returns
    /// - `Ok(true)` if the publish fits the budget — the timestamp is
    ///   recorded against future calls,
    /// - `Ok(false)` if the token is over budget — the timestamp is NOT
    ///   recorded (so a subsequent call once the window slides still
    ///   succeeds),
    /// - `Err(StorageError::Failure(..))` on internal failure.
    fn try_record_publish(
        &self,
        token: &str,
        window: Duration,
        max: usize,
    ) -> Result<bool, StorageError>;
}

/// Pure-Rust in-memory backend for the Buff registry.
///
/// Holds all state behind a single `std::sync::Mutex<Inner>`. Suitable
/// for:
///
/// - Local development.
/// - Integration tests (the canonical use — see `crates/buff-registry/tests/`).
/// - Single-instance production deployments that don't need durability
///   across restarts (NOT recommended for real use).
///
/// A real database-backed impl (Postgres + blob storage) is DEFERRED —
/// see the crate root docs. This impl is the ONLY backend shipped in
/// the v1.6 milestone.
#[derive(Debug, Default)]
pub struct InMemoryStorage {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// name -> { version -> PackageVersion }
    packages: BTreeMap<String, BTreeMap<Version, PackageVersion>>,
    /// valid publish tokens (added via `add_token`).
    tokens: BTreeSet<String>,
    /// token -> recent publish timestamps (rolling window).
    rate: BTreeMap<String, Vec<Instant>>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `Instant` is not Debug; elide the rate map (its values are
        // `Vec<Instant>` which can't be Debug-printed). We surface the
        // keys so debug output still shows which tokens have history.
        f.debug_struct("Inner")
            .field("packages", &self.packages)
            .field("tokens", &self.tokens)
            .field("rate_tokens", &self.rate.keys().collect::<Vec<&String>>())
            .finish()
    }
}

/// Sentinel string returned by [`InMemoryStorage::put_version`] when the
/// `(name, version)` pair is already published. Handlers match this
/// string to map to HTTP 400 `VersionExists`.
pub(crate) const VERSION_EXISTS_MARKER: &str = "version exists";

impl InMemoryStorage {
    /// Construct an empty in-memory registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a publish token. Idempotent — adding the same token twice
    /// is a no-op.
    ///
    /// Required for the publish endpoint to accept requests from this
    /// token. Real implementations would provision tokens via GitHub
    /// OAuth; that's deferred (see crate root docs).
    pub fn add_token(&self, token: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.tokens.insert(token.to_string());
        }
    }
}

impl Storage for InMemoryStorage {
    fn put_version(
        &self,
        name: &str,
        version: Version,
        deps: Vec<DepSpec>,
        tarball: Vec<u8>,
    ) -> Result<(), StorageError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::Failure(format!("mutex poisoned: {e}")))?;
        let by_version = inner.packages.entry(name.to_string()).or_default();
        if by_version.contains_key(&version) {
            return Err(StorageError::Failure(VERSION_EXISTS_MARKER.to_string()));
        }
        by_version.insert(version, PackageVersion { deps, tarball });
        Ok(())
    }

    fn get_package(&self, name: &str) -> Result<Option<PackageMetadata>, StorageError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::Failure(format!("mutex poisoned: {e}")))?;
        let Some(by_version) = inner.packages.get(name) else {
            return Ok(None);
        };
        if by_version.is_empty() {
            return Ok(None);
        }
        let versions = by_version
            .iter()
            .map(|(v, pv)| VersionInfo {
                version: v.to_string(),
                deps: pv.deps.clone(),
            })
            .collect();
        Ok(Some(PackageMetadata {
            name: name.to_string(),
            versions,
        }))
    }

    fn get_tarball(&self, name: &str, version: &Version) -> Result<Option<Vec<u8>>, StorageError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::Failure(format!("mutex poisoned: {e}")))?;
        let Some(by_version) = inner.packages.get(name) else {
            return Ok(None);
        };
        Ok(by_version.get(version).map(|pv| pv.tarball.clone()))
    }

    fn list_versions_with_deps(
        &self,
        name: &str,
    ) -> Result<Vec<(Version, Vec<DepSpec>)>, StorageError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::Failure(format!("mutex poisoned: {e}")))?;
        let Some(by_version) = inner.packages.get(name) else {
            return Ok(Vec::new());
        };
        Ok(by_version
            .iter()
            .map(|(v, pv)| (v.clone(), pv.deps.clone()))
            .collect())
    }

    fn validate_token(&self, token: &str) -> Result<bool, StorageError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::Failure(format!("mutex poisoned: {e}")))?;
        Ok(inner.tokens.contains(token))
    }

    fn try_record_publish(
        &self,
        token: &str,
        window: Duration,
        max: usize,
    ) -> Result<bool, StorageError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::Failure(format!("mutex poisoned: {e}")))?;
        let now = Instant::now();
        let timestamps = inner.rate.entry(token.to_string()).or_default();
        // Drop entries older than the rolling window.
        timestamps.retain(|&t| now.duration_since(t) < window);
        if timestamps.len() >= max {
            return Ok(false);
        }
        timestamps.push(now);
        Ok(true)
    }
}

/// A specialized [`Result`] for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;
