//! HTTP handlers for the Buff registry.
//!
//! Each handler is an async function that takes axum extractors and
//! returns `Result<Json<T>, RegistryError>` (axum's `IntoResponse` is
//! implemented for `RegistryError` in [`crate::error`]).
//!
//! Handler responsibilities (in order):
//!
//! 1. Authenticate (publish only).
//! 2. Validate input (name charset, version parse, body parse).
//! 3. Enforce per-token rate limit (publish only).
//! 4. Domain logic (cycle detection for publish, semver matching for
//!    resolve).
//! 5. Persist via [`crate::Storage`].
//! 6. Render response JSON.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use base64::Engine as _;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use crate::error::{RegistryError, StorageError};
use crate::quality::{compute_badges, Package, QualityBadges};
use crate::storage::{
    DepSpec, PackageSummary, PublishRequest, PublishResponse, QualityAttachment, ResolveResponse,
    VERSION_EXISTS_MARKER,
};
use crate::AppState;

/// The canonical prefix expected on publish `Authorization` headers.
const BEARER_PREFIX: &str = "Bearer ";

/// Query-string parameters for the resolve endpoint.
#[derive(Debug, Deserialize)]
pub(crate) struct ResolveParams {
    /// A Cargo-style semver requirement (`^1.0.0`, `>=2.0.0`, `*`, etc.).
    pub(crate) req: String,
}

/// Query-string parameters for the T70 search endpoint.
#[derive(Debug, Deserialize)]
pub(crate) struct SearchParams {
    /// The search query (case-insensitive substring match against
    /// package names). When empty, all packages are returned.
    pub(crate) q: Option<String>,
}

/// T70: One row of the `GET /api/v1/search` response body.
///
/// Carries the package name + latest version + the pre-computed
/// [`QualityBadges`]. The CLI renders this as:
/// `[verified] [maintained] [tested 85%] [documented 72%] <name> <ver>`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResultRow {
    /// The package name.
    pub name: String,
    /// The latest published version (canonical semver string).
    pub latest_version: String,
    /// The computed quality badges.
    pub badges: QualityBadges,
}

/// `POST /api/v1/publish`.
///
/// Auth required (`Authorization: Bearer <token>`). Body is a JSON
/// envelope of shape [`PublishRequest`]. On success: `201 Created` with
/// a JSON echo of the recorded metadata.
///
/// Validation order: auth → body parse → tarball base64 decode → name
/// → version → rate limit → cycle → store.
pub(crate) async fn publish(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, RegistryError> {
    // --- 1. Auth -----------------------------------------------------------
    // T57: accept BOTH static tokens (backwards compat from T126) AND
    // OAuth session tokens (from the /auth/github/callback flow).
    let token = extract_bearer(&headers).ok_or(RegistryError::Unauthorized)?;
    let session_user = state.storage.validate_session(token)?;
    let is_valid = state.storage.validate_token(token)? || session_user.is_some();
    if !is_valid {
        return Err(RegistryError::Unauthorized);
    }

    // --- 2. Parse body -----------------------------------------------------
    let request: PublishRequest =
        serde_json::from_slice(&body).map_err(|e| RegistryError::InvalidBody(e.to_string()))?;

    // --- 3. Decode tarball -------------------------------------------------
    let tarball = base64::engine::general_purpose::STANDARD
        .decode(request.tarball_b64.as_bytes())
        .map_err(|e| RegistryError::InvalidTarball(e.to_string()))?;

    // --- 4. Validate name + version ----------------------------------------
    // T57: validate_package_name accepts BOTH unscoped (foo) and scoped
    // (@org/pkg) names. validate_name (legacy) rejected '/' so scoped
    // names would fail — we use the new validator.
    crate::validate_package_name(&request.name)?;
    let version = Version::parse(&request.version)
        .map_err(|e| RegistryError::InvalidVersion(e.to_string()))?;

    // --- 4b. T57: Scope ownership check -----------------------------------
    // For scoped packages (@org/pkg), require the authenticated identity
    // to be a member of the org. Unscoped packages skip this check.
    if let Some(org) = crate::scope_of(&request.name) {
        let identity = session_user
            .as_ref()
            .map(|u| u.github_login.as_str())
            .unwrap_or(token);
        if !state.storage.is_org_member(org, identity)? {
            return Err(RegistryError::ScopeForbidden);
        }
    }

    // --- 5. Rate limit -----------------------------------------------------
    if !state
        .storage
        .try_record_publish(token, state.rate_limit_window, state.rate_limit_max)?
    {
        return Err(RegistryError::RateLimited);
    }

    // --- 6. Cycle detection ------------------------------------------------
    if has_cycle(state.storage.as_ref(), &request.name, &request.deps)? {
        return Err(RegistryError::CycleDetected);
    }

    // --- 7. Build T70 quality attachment + publish timestamp ----------------
    // The publish timestamp is unix-seconds (NOT Instant — we need
    // wall-clock time for the "maintained" badge, which compares across
    // server restarts). `duration_since(UNIX_EPOCH)` fails only on
    // pre-epoch clocks (never in practice); the fallback is `None`,
    // which means "maintained" defaults to false for that entry.
    let published_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .ok();
    let quality = QualityAttachment {
        tested_coverage: request.tested_coverage,
        documented_coverage: request.documented_coverage,
        security_audit: None,
    };
    // The author is the GitHub login (if OAuth session) or the bearer
    // token (for static-token backwards compat). The verified-publisher
    // badge checks this against the verified-author set.
    let author = session_user
        .as_ref()
        .map(|u| u.github_login.clone())
        .or_else(|| Some(token.to_string()));

    // --- 8. Store ----------------------------------------------------------
    let deps = request.deps.clone();
    let put_result = state.storage.put_version(
        &request.name,
        version.clone(),
        request.deps,
        tarball,
        author,
        published_at,
        quality,
    );
    match put_result {
        Ok(()) => (),
        Err(StorageError::Failure(msg)) if msg == VERSION_EXISTS_MARKER => {
            return Err(RegistryError::VersionExists {
                name: request.name,
                version: request.version,
            });
        }
        Err(e) => return Err(e.into()),
    }

    // --- 8. Render response ------------------------------------------------
    let response = PublishResponse {
        name: request.name,
        version: version.to_string(),
        deps,
    };
    Ok((axum::http::StatusCode::CREATED, Json(response)))
}

/// `GET /api/v1/package/{name}`.
///
/// Returns the full metadata for `name` (every published version) or
/// `404 Not Found` if the package has no published versions.
pub(crate) async fn get_package(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, RegistryError> {
    let metadata = state.storage.get_package(&name)?;
    match metadata {
        Some(m) => Ok(Json(m)),
        None => Err(RegistryError::NotFound),
    }
}

/// `GET /api/v1/download/{name}/{version}`.
///
/// Returns the raw tarball bytes (`Content-Type: application/octet-stream`)
/// for `(name, version)`. Anonymous (no auth). `404 Not Found` if either
/// the package or the version is unknown.
pub(crate) async fn download(
    State(state): State<AppState>,
    Path((name, version)): Path<(String, String)>,
) -> Result<impl IntoResponse, RegistryError> {
    let version =
        Version::parse(&version).map_err(|e| RegistryError::InvalidVersion(e.to_string()))?;
    let tarball = state.storage.get_tarball(&name, &version)?;
    match tarball {
        Some(bytes) => {
            // T57: record the download for stats tracking.
            let _ = state.storage.record_download(&name, &version);
            Ok((
                [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
                bytes,
            ))
        }
        None => Err(RegistryError::VersionNotFound),
    }
}

/// `GET /api/v1/resolve/{name}?req=<semver-req>`.
///
/// Returns the highest published version of `name` matching the
/// requirement (`semver::VersionReq::matches`), or `404 Not Found` if
/// no version satisfies it.
pub(crate) async fn resolve(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<ResolveParams>,
) -> Result<impl IntoResponse, RegistryError> {
    let req = VersionReq::parse(&params.req)
        .map_err(|e| RegistryError::InvalidBody(format!("invalid `req` query: {e}")))?;
    let versions_with_deps = state.storage.list_versions_with_deps(&name)?;
    if versions_with_deps.is_empty() {
        return Err(RegistryError::NotFound);
    }
    // Pick the highest matching version. `list_versions_with_deps`
    // returns in BTreeMap order (ascending), but we use `max_by_key`
    // for clarity — input is small (typical package has <100 versions).
    let highest_version = versions_with_deps
        .into_iter()
        .map(|(v, _)| v)
        .filter(|v| req.matches(v))
        .max_by(|a, b| a.cmp(b));
    let Some(version) = highest_version else {
        return Err(RegistryError::NoMatchingVersion);
    };
    Ok(Json(ResolveResponse {
        name,
        version: version.to_string(),
    }))
}

/// T70: `GET /api/v1/packages/{name}/badges`.
///
/// Returns the computed [`QualityBadges`] for `name`. Anonymous (no
/// auth). `404 Not Found` if the package has no published versions.
///
/// Badge computation reads the LATEST published version's metadata
/// (author, publish timestamp, attached coverage / doc / audit data)
/// and resolves `verified_publisher` against the storage's
/// verified-author set.
pub(crate) async fn get_badges(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, RegistryError> {
    let summary = latest_summary(&state, &name)?.ok_or(RegistryError::NotFound)?;
    let badges = compute_from_summary(&state, &summary)?;
    Ok(Json(badges))
}

/// T70: `GET /api/v1/search?q=<query>`.
///
/// Returns every published package whose name contains `q`
/// (case-insensitive substring). When `q` is empty or absent, all
/// packages are returned. Each row carries the computed
/// [`QualityBadges`] so the CLI can render badges inline.
pub(crate) async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, RegistryError> {
    let q = params.q.as_deref().unwrap_or("").to_ascii_lowercase();
    let summaries = state.storage.list_packages()?;
    let mut rows: Vec<SearchResultRow> = Vec::with_capacity(summaries.len());
    for summary in summaries {
        if !q.is_empty() && !summary.name.to_ascii_lowercase().contains(&q) {
            continue;
        }
        let badges = compute_from_summary(&state, &summary)?;
        rows.push(SearchResultRow {
            name: summary.name,
            latest_version: summary.latest_version,
            badges,
        });
    }
    Ok(Json(rows))
}

/// Build a [`Package`] view from a [`PackageSummary`] + resolve
/// `verified_publisher` against the storage's verified-author set,
/// then compute badges relative to `SystemTime::now()`.
///
/// Extracted so both [`get_badges`] and [`search`] share the same
/// computation path (no drift between the two endpoints).
fn compute_from_summary(
    state: &AppState,
    summary: &PackageSummary,
) -> Result<QualityBadges, RegistryError> {
    let verified_publisher = match &summary.author {
        Some(author) => state.storage.is_verified_author(author)?,
        None => false,
    };
    let last_published_at = summary
        .last_published_at
        .and_then(|secs| SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(secs)));
    let package = Package {
        name: summary.name.clone(),
        verified_publisher,
        last_published_at,
        tested_coverage: summary.quality.tested_coverage,
        documented_coverage: summary.quality.documented_coverage,
        security_audit: summary.quality.security_audit.clone(),
    };
    Ok(compute_badges(&package, SystemTime::now()))
}

/// Fetch the [`PackageSummary`] for `name` (latest version only).
///
/// Returns `Ok(None)` when the package has no published versions.
fn latest_summary(state: &AppState, name: &str) -> Result<Option<PackageSummary>, RegistryError> {
    let summaries = state.storage.list_packages()?;
    Ok(summaries.into_iter().find(|s| s.name == name))
}

/// Extract the bearer token from an `Authorization` header.
///
/// Returns `Some(token)` iff the header is present and starts with
/// `Bearer `. Returns `None` otherwise (the caller maps that to
/// `401 Unauthorized`). We do NOT accept any other auth scheme in
/// v1.6 — see crate root docs (GitHub OAuth is deferred).
fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    value.strip_prefix(BEARER_PREFIX).map(str::trim)
}

/// Decide whether publishing `new_name` with `deps` would create a
/// dependency cycle.
///
/// Walks the dep graph starting at each dep's package name. If any
/// traversal reaches `new_name`, returns `true` (cycle).
///
/// Uses the LATEST-PUBLISHED version of each package as the source of
/// edges — since the registry is append-only and never yanks for v1.6,
/// this is the conservative choice (any older version's deps are a
/// subset of the union we'd walk otherwise; using only the latest is
/// simpler and matches what a real resolver would do).
///
/// If a dep package doesn't exist yet (forward-publishing like
/// "A depends on B" before B exists), the traversal through it stops
/// — there's no published edge to follow. This means step 1 of the
/// cycle scenario (`A→B`, B doesn't exist yet) succeeds, and step 2
/// (`B→A`, A exists with `B` as a dep) is detected as a cycle because
/// walking A's published deps leads back to `B == new_name`.
fn has_cycle(
    storage: &dyn crate::Storage,
    new_name: &str,
    deps: &[DepSpec],
) -> Result<bool, StorageError> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = deps.iter().map(|d| d.name.clone()).collect();
    while let Some(name) = stack.pop() {
        if name == new_name {
            return Ok(true);
        }
        if !visited.insert(name.clone()) {
            continue;
        }
        let versions_with_deps = storage.list_versions_with_deps(&name)?;
        for (_, dep_list) in versions_with_deps {
            for d in dep_list {
                if !visited.contains(&d.name) {
                    stack.push(d.name);
                }
            }
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// T57 Production endpoints: multipart upload, new download, stats
// ---------------------------------------------------------------------------

/// Metadata carried in the `metadata` part of the multipart upload.
#[derive(Debug, Deserialize)]
struct MultipartMetadata {
    version: String,
    #[serde(default)]
    deps: Vec<DepSpec>,
}

/// T57: `POST /api/v1/packages/{name}` — multipart tarball upload.
///
/// Accepts a multipart/form-data body with two parts:
/// - `metadata`: JSON `{ "version": "1.0.0", "deps": [...] }`.
/// - `tarball`: binary tarball bytes.
///
/// Auth required (Bearer token OR OAuth session — same as legacy publish).
/// Validates name + version + scope ownership, then stores the tarball
/// to the filesystem (if `tarball_dir` is configured) AND to the storage
/// backend (for metadata + backwards-compat download).
pub(crate) async fn multipart_publish(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, RegistryError> {
    // --- 1. Auth (same logic as legacy publish) ---
    let token = extract_bearer(&headers).ok_or(RegistryError::Unauthorized)?;
    let session_user = state.storage.validate_session(token)?;
    let is_valid = state.storage.validate_token(token)? || session_user.is_some();
    if !is_valid {
        return Err(RegistryError::Unauthorized);
    }

    // --- 2. Validate name ---
    crate::validate_package_name(&name)?;

    // --- 3. Parse multipart body ---
    let mut metadata: Option<MultipartMetadata> = None;
    let mut tarball: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| RegistryError::InvalidBody(format!("multipart parse: {e}")))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "metadata" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| RegistryError::InvalidBody(format!("metadata read: {e}")))?;
                metadata = Some(
                    serde_json::from_slice(&bytes)
                        .map_err(|e| RegistryError::InvalidBody(format!("metadata JSON: {e}")))?,
                );
            }
            "tarball" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| RegistryError::InvalidTarball(format!("tarball read: {e}")))?;
                tarball = Some(bytes.to_vec());
            }
            _ => {
                // Unknown field — ignore (forward-compat).
            }
        }
    }
    let meta = metadata
        .ok_or_else(|| RegistryError::InvalidBody("missing 'metadata' part".to_string()))?;
    let tarball_bytes = tarball
        .ok_or_else(|| RegistryError::InvalidTarball("missing 'tarball' part".to_string()))?;

    // --- 4. Validate version ---
    let version =
        Version::parse(&meta.version).map_err(|e| RegistryError::InvalidVersion(e.to_string()))?;

    // --- 5. Scope ownership (same as legacy publish) ---
    if let Some(org) = crate::scope_of(&name) {
        let identity = session_user
            .as_ref()
            .map(|u| u.github_login.as_str())
            .unwrap_or(token);
        if !state.storage.is_org_member(org, identity)? {
            return Err(RegistryError::ScopeForbidden);
        }
    }

    // --- 6. Store tarball to filesystem (if configured) ---
    if let Some(dir) = &state.tarball_dir {
        write_tarball_to_fs(dir, &name, &version, &tarball_bytes)?;
    }

    // --- 7. Store metadata + tarball to storage backend ---
    let author = session_user
        .as_ref()
        .map(|u| u.github_login.clone())
        .or_else(|| Some(token.to_string()));
    let published_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .ok();
    let deps = meta.deps.clone();
    state.storage.put_version(
        &name,
        version.clone(),
        meta.deps,
        tarball_bytes,
        author,
        published_at,
        QualityAttachment::default(),
    )?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({
            "name": name,
            "version": version.to_string(),
            "deps": deps,
        })),
    ))
}

/// T57: `GET /api/v1/packages/{name}/{version}/download` — serve tarball.
///
/// Reads the tarball from the filesystem (if `tarball_dir` is configured
/// and the file exists) or falls back to the storage backend (BLOB).
/// Records a download event for stats tracking.
pub(crate) async fn multipart_download(
    State(state): State<AppState>,
    Path((name, version_str)): Path<(String, String)>,
) -> Result<impl IntoResponse, RegistryError> {
    let version =
        Version::parse(&version_str).map_err(|e| RegistryError::InvalidVersion(e.to_string()))?;

    // Record the download for stats.
    let _ = state.storage.record_download(&name, &version);

    // Try filesystem first (if configured).
    if let Some(dir) = &state.tarball_dir {
        if let Some(bytes) = read_tarball_from_fs(dir, &name, &version) {
            return Ok((
                [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
                bytes,
            )
                .into_response());
        }
    }

    // Fall back to storage backend (BLOB).
    let tarball = state.storage.get_tarball(&name, &version)?;
    match tarball {
        Some(bytes) => Ok((
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )
            .into_response()),
        None => Err(RegistryError::VersionNotFound),
    }
}

/// T57: `GET /api/v1/packages/{name}/stats` — download statistics.
///
/// Returns the total download count for `name` across all versions.
pub(crate) async fn get_stats(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, RegistryError> {
    let count = state.storage.download_count(&name)?;
    Ok(Json(serde_json::json!({
        "name": name,
        "downloads": count,
    })))
}

/// `GET /health` — liveness probe.
///
/// Returns `200 OK` with `{"status":"ok"}`. Always succeeds — the
/// registry has no external dependencies that would make it unhealthy
/// while the process is running.
pub(crate) async fn health_handler() -> impl IntoResponse {
    (axum::http::StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

/// `GET /ready` — readiness probe.
///
/// Returns `200 OK` with `{"status":"ready"}`. The in-memory storage
/// backend is always ready — there is no database connection to wait
/// for. A future backend (e.g. Postgres) would check connectivity here
/// and return `503 Service Unavailable` on failure.
pub(crate) async fn ready_handler() -> impl IntoResponse {
    (axum::http::StatusCode::OK, Json(serde_json::json!({"status": "ready"})))
}

// --- Filesystem tarball helpers ---

/// Sanitize a package name for use as a filesystem path component.
/// Replaces `/` with `__` so `@org/pkg` becomes `@org__pkg` (no
/// subdirectory traversal).
fn sanitize_name_for_fs(name: &str) -> String {
    name.replace('/', "__")
}

/// Write a tarball to the filesystem under `tarball_dir`.
fn write_tarball_to_fs(
    tarball_dir: &std::path::Path,
    name: &str,
    version: &Version,
    bytes: &[u8],
) -> Result<(), RegistryError> {
    let sanitized = sanitize_name_for_fs(name);
    let dir = tarball_dir.join(&sanitized);
    std::fs::create_dir_all(&dir)
        .map_err(|e| RegistryError::Storage(format!("create tarball dir: {e}")))?;
    let path = dir.join(format!("{}.tar", version));
    std::fs::write(&path, bytes)
        .map_err(|e| RegistryError::Storage(format!("write tarball: {e}")))?;
    Ok(())
}

/// Read a tarball from the filesystem. Returns `None` if the file
/// doesn't exist.
fn read_tarball_from_fs(
    tarball_dir: &std::path::Path,
    name: &str,
    version: &Version,
) -> Option<Vec<u8>> {
    let sanitized = sanitize_name_for_fs(name);
    let path = tarball_dir.join(sanitized).join(format!("{}.tar", version));
    std::fs::read(&path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::DepSpec;
    use crate::storage::{QualityAttachment, Storage};
    use crate::InMemoryStorage;

    fn dep(name: &str) -> DepSpec {
        DepSpec {
            name: name.to_string(),
            req: "*".to_string(),
        }
    }

    #[test]
    fn has_cycle_false_for_no_deps() {
        let storage = InMemoryStorage::new();
        let cyclic = has_cycle(&storage, "a", &[]).expect("storage ok");
        assert!(!cyclic);
    }

    #[test]
    fn has_cycle_false_for_self_dep_on_unpublished_other() {
        // A→B, B doesn't exist. No cycle (yet).
        let storage = InMemoryStorage::new();
        let cyclic = has_cycle(&storage, "a", &[dep("b")]).expect("storage ok");
        assert!(!cyclic);
    }

    #[test]
    fn has_cycle_true_for_self_dep() {
        // A→A directly.
        let storage = InMemoryStorage::new();
        let cyclic = has_cycle(&storage, "a", &[dep("a")]).expect("storage ok");
        assert!(cyclic);
    }

    #[test]
    fn has_cycle_true_when_back_edge_exists() {
        // Step 1: publish A→B (B doesn't exist yet).
        // Step 2: try to publish B→A. Walking A's deps reaches B (=new_name).
        let storage = InMemoryStorage::new();
        storage
            .put_version(
                "a",
                Version::parse("1.0.0").expect("version"),
                vec![dep("b")],
                Vec::new(),
                None,
                None,
                QualityAttachment::default(),
            )
            .expect("put a");
        let cyclic = has_cycle(&storage, "b", &[dep("a")]).expect("storage ok");
        assert!(cyclic);
    }
    #[test]
    fn extract_bearer_returns_token_for_valid_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer abc123".parse().expect("header value"),
        );
        assert_eq!(extract_bearer(&headers), Some("abc123"));
    }

    #[test]
    fn extract_bearer_none_when_missing() {
        let headers = HeaderMap::new();
        assert_eq!(extract_bearer(&headers), None);
    }

    #[test]
    fn extract_bearer_none_when_wrong_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Basic abc123".parse().expect("header value"),
        );
        assert_eq!(extract_bearer(&headers), None);
    }
}
