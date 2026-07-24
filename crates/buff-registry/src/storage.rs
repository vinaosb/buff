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
use crate::quality::AuditResult;

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
///
/// `Eq` is intentionally NOT derived: `SystemTime` (in
/// [`VersionInfo::published_at`]) implements `PartialEq` but not `Eq`
/// (different monotonic clocks are not totally ordered). The DTO is
/// never used as a `BTreeMap`/`HashSet` key, so dropping `Eq` has no
/// downstream impact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionInfo {
    /// The version string (canonical form from `Version::to_string`).
    pub version: String,
    /// The version's declared dependencies.
    pub deps: Vec<DepSpec>,
    /// T70: Wall-clock publish time (unix seconds since `UNIX_EPOCH`),
    /// or `None` for legacy entries published before the quality-signals
    /// extension. Surfaced for the "maintained" badge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<u64>,
    /// T70: The author (publish token) that published this version,
    /// or `None` for legacy entries. Surfaced for the "verified
    /// publisher" badge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

/// Full package metadata returned by `GET /api/v1/package/<name>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageMetadata {
    /// The package name.
    pub name: String,
    /// Every published version, sorted ascending by `semver::Version`.
    pub versions: Vec<VersionInfo>,
}

/// Internal record for one stored `(name, version)` pair.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PackageVersion {
    pub(crate) deps: Vec<DepSpec>,
    pub(crate) tarball: Vec<u8>,
    /// T70: Wall-clock publish time (unix seconds since `UNIX_EPOCH`).
    pub(crate) published_at: Option<u64>,
    /// T70: The author (publish token) that published this version.
    pub(crate) author: Option<String>,
    /// T70: Attached quality signals (coverage / doc / audit).
    pub(crate) quality: QualityAttachment,
}

/// T70: Quality signals a publisher attaches at publish time. Stored
/// alongside the tarball + deps and surfaced in badge computation.
///
/// All fields are `Option` — a publisher that attaches no coverage
/// report publishes with [`QualityAttachment::default`] (all `None`),
/// and the package's `tested` / `documented` badges are `None`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QualityAttachment {
    /// Test coverage percentage (`0.0..=100.0`) from a
    /// `cargo llvm-cov` / `cargo-tarpaulin` report. `None` when the
    /// publisher attached no report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tested_coverage: Option<f32>,
    /// Doc-comment coverage percentage (`0.0..=100.0`). `None` when
    /// unmeasured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documented_coverage: Option<f32>,
    /// Result of a T26 `buff-audit` scan. `None` until a scan is run
    /// (the registry does NOT auto-scan in the MVP).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_audit: Option<AuditResult>,
}

/// The wire-format body of `POST /api/v1/publish`.
///
/// `tarball_b64` is the raw tarball bytes base64-encoded so the entire
/// publish request is a single JSON object (no multipart parsing, no
/// binary body ordering issues). Decoded by the handler before storage.
///
/// T70: `tested_coverage` / `documented_coverage` are optional quality
/// attachments the publisher sends to populate the package's badges.
/// Omit them (or send `null`) for a package with no coverage data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishRequest {
    /// The package name (validated by the handler before storage).
    pub name: String,
    /// The version string (parsed as `semver::Version` before storage).
    pub version: String,
    /// Declared dependencies (used for cycle detection + the index API).
    pub deps: Vec<DepSpec>,
    /// Base64-encoded tarball bytes.
    pub tarball_b64: String,
    /// T70: Optional test coverage % (`0.0..=100.0`) from a coverage
    /// report. Populates [`QualityBadges::tested`]. Defaults to `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tested_coverage: Option<f32>,
    /// T70: Optional doc-comment coverage % (`0.0..=100.0`).
    /// Populates [`QualityBadges::documented`]. Defaults to `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documented_coverage: Option<f32>,
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

/// T70: A search-result row returned by `GET /api/v1/search?q=...`.
///
/// Aggregates the package name + latest version + the quality data
/// needed for badge computation (the handler calls
/// [`crate::quality::compute_badges`] on this to populate the
/// `badges` field in the JSON response).
///
/// `Eq` is intentionally NOT derived (mirrors [`VersionInfo`] — the
/// `quality` field carries `Option` types that are fine for `PartialEq`
/// but the struct is never keyed in a map).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageSummary {
    /// The package name.
    pub name: String,
    /// The latest published version (canonical semver string).
    pub latest_version: String,
    /// The author of the latest version, or `None` for legacy entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Wall-clock publish time of the latest version (unix seconds),
    /// or `None` for legacy entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_published_at: Option<u64>,
    /// Attached quality signals from the latest version.
    #[serde(default, skip_serializing_if = "QualityAttachment::is_empty")]
    pub quality: QualityAttachment,
}

impl QualityAttachment {
    /// `true` when all fields are `None` (no quality data attached).
    /// Used by serde `skip_serializing_if` to omit the `quality` field
    /// entirely for packages with no attachments.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tested_coverage.is_none()
            && self.documented_coverage.is_none()
            && self.security_audit.is_none()
    }
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
    ///
    /// T70: `author` is the publish token (resolved by the handler from
    /// the `Authorization` header); `published_at` is the wall-clock
    /// publish time (unix seconds, set by the handler); `quality`
    /// carries the publisher's optional coverage / doc / audit
    /// attachments.
    #[allow(clippy::too_many_arguments)]
    fn put_version(
        &self,
        name: &str,
        version: Version,
        deps: Vec<DepSpec>,
        tarball: Vec<u8>,
        author: Option<String>,
        published_at: Option<u64>,
        quality: QualityAttachment,
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

    /// T70: List every published package as a [`PackageSummary`] row
    /// (name + latest version + quality data). Used by the search
    /// endpoint to render results with badges.
    ///
    /// Returns an empty `Vec` when no packages are published.
    fn list_packages(&self) -> Result<Vec<PackageSummary>, StorageError>;

    /// T70: Return `true` iff `author` is in the registry's
    /// verified-author set. MVP: mock — the in-memory backend exposes
    /// [`InMemoryStorage::add_verified_author`] for tests / local dev.
    /// Future: GitHub OAuth verified-email check.
    fn is_verified_author(&self, author: &str) -> Result<bool, StorageError>;

    // --- T57: OAuth sessions + user management (default impls) ---
    //
    // These methods have default implementations that signal "not
    // supported by this backend" so [`InMemoryStorage`] keeps working
    // without code changes. [`SqliteStorage`] overrides them with real
    // table-backed persistence.

    /// T57: Create a session for `github_login`, returning the opaque
    /// session token string (a UUID v4). Used by the OAuth callback
    /// handler to mint a session after GitHub authentication succeeds.
    ///
    /// Default impl: returns an error (sessions not supported by this
    /// backend).
    fn create_session(&self, _github_login: &str, _github_id: i64) -> Result<String, StorageError> {
        Err(StorageError::Failure(
            "sessions not supported by this backend".to_string(),
        ))
    }

    /// T57: Validate a session token. Returns `Ok(Some(user))` if the
    /// session is valid, `Ok(None)` if the session is unknown / expired.
    ///
    /// Default impl: always returns `Ok(None)` (no sessions).
    fn validate_session(&self, _session_token: &str) -> Result<Option<SessionUser>, StorageError> {
        Ok(None)
    }

    /// T57: Delete a session (logout). Idempotent — deleting an unknown
    /// session is a no-op.
    ///
    /// Default impl: no-op.
    fn delete_session(&self, _session_token: &str) -> Result<(), StorageError> {
        Ok(())
    }

    /// T57: Check whether `identity` is a member of `org`. Used by the
    /// publish handler to enforce scope ownership: only org members
    /// can publish to `@org/pkg`.
    ///
    /// `identity` is the GitHub login (for OAuth-authenticated users) or
    /// the static token string (for backwards-compat token auth).
    ///
    /// Default impl: returns `false` (no orgs in this backend — scoped
    /// publishes will be rejected).
    fn is_org_member(&self, _org: &str, _identity: &str) -> Result<bool, StorageError> {
        Ok(false)
    }

    /// T57: Add `identity` as a member of `org`. Creates the org if it
    /// doesn't exist (idempotent on the membership row). Used by
    /// admin tooling / tests to provision org membership.
    ///
    /// Default impl: returns an error (orgs not supported by this backend).
    fn add_org_member(&self, _org: &str, _identity: &str) -> Result<(), StorageError> {
        Err(StorageError::Failure(
            "orgs not supported by this backend".to_string(),
        ))
    }

    /// T57: Check whether `github_login` is on the invite-only beta
    /// allowlist. Called by the OAuth callback handler BEFORE creating
    /// a session — non-allowlisted users get `403 Forbidden`.
    ///
    /// Default impl: returns `false` (allowlist not supported — all
    /// OAuth logins rejected). This is the SECURE default: production
    /// deployments MUST seed the allowlist via [`Self::add_to_allowlist`]
    /// before users can log in.
    fn is_allowlisted(&self, _github_login: &str) -> Result<bool, StorageError> {
        Ok(false)
    }

    /// T57: Add `github_login` to the invite-only beta allowlist.
    /// Idempotent. Used by admin tooling / tests.
    ///
    /// Default impl: returns an error (allowlist not supported).
    fn add_to_allowlist(&self, _github_login: &str) -> Result<(), StorageError> {
        Err(StorageError::Failure(
            "allowlist not supported by this backend".to_string(),
        ))
    }
}

/// T57: User identity resolved from a valid session token.
///
/// Returned by [`Storage::validate_session`] when a session token is
/// valid. Carries the GitHub login + numeric ID so handlers can
/// attribute publishes + enforce org membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionUser {
    /// The GitHub username (e.g. `octocat`).
    pub github_login: String,
    /// The numeric GitHub user ID (stable across username changes).
    pub github_id: i64,
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
    /// T70: verified-author set (mock — populated via `add_verified_author`).
    verified_authors: BTreeSet<String>,
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
            .field("verified_authors", &self.verified_authors)
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

    /// T70: Register a verified author (mock — real verification via
    /// GitHub OAuth verified-email is deferred). Idempotent.
    ///
    /// Once an author is in this set, every package whose latest
    /// version was published by that author (token) gets the
    /// `verified_publisher` badge. The check is consulted via
    /// [`Storage::is_verified_author`].
    pub fn add_verified_author(&self, author: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.verified_authors.insert(author.to_string());
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
        author: Option<String>,
        published_at: Option<u64>,
        quality: QualityAttachment,
    ) -> Result<(), StorageError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::Failure(format!("mutex poisoned: {e}")))?;
        let by_version = inner.packages.entry(name.to_string()).or_default();
        if by_version.contains_key(&version) {
            return Err(StorageError::Failure(VERSION_EXISTS_MARKER.to_string()));
        }
        by_version.insert(
            version,
            PackageVersion {
                deps,
                tarball,
                published_at,
                author,
                quality,
            },
        );
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
                published_at: pv.published_at,
                author: pv.author.clone(),
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

    fn list_packages(&self) -> Result<Vec<PackageSummary>, StorageError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::Failure(format!("mutex poisoned: {e}")))?;
        Ok(inner
            .packages
            .iter()
            .filter_map(|(name, by_version)| {
                // Pick the latest version (BTreeMap ascending → last entry).
                let (latest_v, latest_pv) = by_version.iter().next_back()?;
                Some(PackageSummary {
                    name: name.clone(),
                    latest_version: latest_v.to_string(),
                    author: latest_pv.author.clone(),
                    last_published_at: latest_pv.published_at,
                    quality: latest_pv.quality.clone(),
                })
            })
            .collect())
    }

    fn is_verified_author(&self, author: &str) -> Result<bool, StorageError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::Failure(format!("mutex poisoned: {e}")))?;
        Ok(inner.verified_authors.contains(author))
    }
}

/// A specialized [`Result`] for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;
