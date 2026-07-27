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

/// P0.28 (sec-hardening): Maximum request body size enforced by the
/// router-level [`axum::extract::DefaultBodyLimit`] layer (see
/// [`crate::app`]). 50 MiB is generous for a Buff package tarball
/// (typical `.buff` source is a few KiB; the limit catches accidental
/// or malicious multi-GB uploads that would otherwise OOM the
/// in-memory storage). When the limit is exceeded, axum returns
/// `413 Payload Too Large` BEFORE any handler runs.
pub const MAX_BODY_BYTES: usize = 50 * 1024 * 1024;

// ---------------------------------------------------------------------------
// P0.28 — Input validation helpers (handler-level, fail-fast).
// ---------------------------------------------------------------------------
//
// Three layered defenses added in P0.28:
//
// 1. [`validate_package_name`] — strict npm-style regex check on the
//    package name (or `@org/pkg` scope + pkg pair). Rejects leading
//    digits / hyphens, underscores, uppercase, Unicode, and length
//    violations (2–64 chars per segment).
//
// 2. [`validate_no_path_traversal`] — explicit `..` / null-byte /
//    absolute-path defense. Runs on EVERY user-supplied path-y input
//    (URL path params, version strings, multipart field names) as
//    defense-in-depth: even if a future code path forgets to call
//    `validate_package_name`, this guard still blocks the most common
//    filesystem-escape primitives.
//
// 3. [`validate_version_string`] — semver shape check (X.Y.Z with
//    optional -prerelease and +build). Rejects path-traversal-shaped
//    version strings BEFORE they reach [`semver::Version::parse`] (and
//    before they touch the filesystem when `tarball_dir` is set).
//
// Each helper returns `Result<(), String>` (a human-readable reason on
// failure). Handlers convert via the [`invalid_input`] adapter which
// maps to [`RegistryError::InvalidInput`] (HTTP 400 with the message
// in the JSON body).

/// P0.28: Strict npm-style package-name validation.
///
/// Complements [`crate::validate_package_name`] (which already blocks
/// `/`, `\`, `..`, uppercase, and Unicode) with the canonical Buff
/// package-name shape so the registry surface matches npm / Cargo
/// conventions:
///
/// - **Unscoped names**: must match `^[a-z][a-z0-9-]{1,63}$` — i.e.
///   2–64 chars, MUST start with a lowercase ASCII letter, only
///   lowercase letters / digits / hyphens after. No underscores, no
///   leading digit, no leading hyphen.
/// - **Scoped names** (`@org/pkg`): the org AND pkg segments must each
///   pass the same rule (so `@buff/core` is valid, `@Buff/Core` is
///   not, `@1org/pkg` is not).
///
/// Returns `Ok(())` on success or `Err(reason)` with a human-readable
/// string explaining the rule that failed. Handlers map the error to
/// [`RegistryError::InvalidInput`] (HTTP 400) via [`invalid_input`].
fn validate_package_name(name: &str) -> Result<(), String> {
    if name.starts_with('@') {
        // Scoped: @org/pkg. Strip the leading '@' and split on the
        // FIRST '/' (there must be exactly one).
        let after_at = name
            .strip_prefix('@')
            .ok_or_else(|| "invalid scoped name: missing body after '@'".to_string())?;
        let (org, pkg) = after_at.split_once('/').ok_or_else(|| {
            "invalid scoped name: missing '/' separator (expected @org/pkg)".to_string()
        })?;
        validate_name_segment(org, "scope")?;
        validate_name_segment(pkg, "package")?;
        // Reject any extra '/' lurking in the pkg half (split_once
        // already gave us the first segment; the rest may still
        // contain '/').
        if pkg.contains('/') {
            return Err("invalid scoped name: extra '/' in package part".to_string());
        }
        Ok(())
    } else {
        validate_name_segment(name, "package name")
    }
}

/// Validate one segment of a package name (the whole unscoped name, or
/// one part of `@org/pkg`). Enforces `^[a-z][a-z0-9-]{1,63}$`:
/// 2–64 chars (inclusive), starts with ASCII lowercase letter, only
/// `[a-z0-9-]` after.
fn validate_name_segment(segment: &str, label: &str) -> Result<(), String> {
    // Use char count (not byte len) for Unicode-correct length — though
    // the charset check below rejects non-ASCII, this is still safer.
    let char_count = segment.chars().count();
    if !(2..=64).contains(&char_count) {
        return Err(format!(
            "invalid {label}: length must be 2–64 chars (got {char_count})"
        ));
    }
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return Err(format!("invalid {label}: empty"));
    };
    if !first.is_ascii_lowercase() {
        return Err(format!(
            "invalid {label}: must start with lowercase letter a-z (got '{first}')"
        ));
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(format!(
                "invalid {label}: illegal character '{c}' (allowed: a-z, 0-9, hyphen)"
            ));
        }
    }
    Ok(())
}

/// P0.28: Reject any input containing path-traversal / control sequences.
///
/// Catches four attack primitives that would let a malicious input
/// escape the storage layout if a future code path used it as a
/// filesystem path component:
///
/// - `..` — parent-directory reference (e.g. `../etc/passwd`).
/// - `\0` (NUL byte) — C-string truncation / `CString::new` rejection.
/// - Leading `/` — Unix absolute path.
/// - Leading `<letter>:\` or `<letter>:/` — Windows drive-letter path.
///
/// The existing [`crate::validate_package_name`] already blocks these
/// in package-name inputs (via the `[a-z0-9_-]` charset), but this
/// helper is meant to run on EVERY user-supplied path-y field as
/// defense-in-depth — including version strings, multipart field
/// names, and any future URL parameter that flows into filesystem or
/// storage-key construction.
fn validate_no_path_traversal(input: &str) -> Result<(), String> {
    if input.contains("..") {
        return Err("invalid input: contains '..' (path traversal forbidden)".to_string());
    }
    if input.contains('\0') {
        return Err("invalid input: contains NUL byte (control characters forbidden)".to_string());
    }
    if input.starts_with('/') {
        return Err("invalid input: absolute paths forbidden (leading '/')".to_string());
    }
    // Windows drive-letter: `^[A-Za-z]:[\\/]`
    let bytes = input.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return Err(
            "invalid input: Windows absolute paths forbidden (leading '<letter>:\\' or '<letter>:/')"
                .to_string(),
        );
    }
    Ok(())
}

/// P0.28: Validate a version string against the canonical semver shape.
///
/// Accepted form: `MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]` where
/// MAJOR / MINOR / PATCH are ASCII digit runs (≥1 digit each), and
/// PRERELEASE / BUILD are `[a-zA-Z0-9.-]+` runs. This mirrors the
/// regex `^\d+\.\d+\.\d+(-[a-zA-Z0-9.-]+)?(\+[a-zA-Z0-9.-]+)?$` from
/// <https://semver.org> without requiring the `regex` crate (manual
/// char-iteration parse — keeps the workspace dependency surface flat).
///
/// [`semver::Version::parse`] is strict already; this helper gives a
/// clearer 400 error message BEFORE the parse step, AND runs the
/// [`validate_no_path_traversal`] check on the version string so a
/// shaped-like-a-version attacker payload such as `1.0.0/../../etc/e`
/// is rejected at the handler door (not later when the version flows
/// into `format!("{}.tar", version)` for filesystem tarball storage).
fn validate_version_string(version: &str) -> Result<(), String> {
    // Cheap path-traversal / null-byte check first (the version may
    // flow into `format!("{}.tar", version)` when `tarball_dir` is
    // configured — see `write_tarball_to_fs`).
    validate_no_path_traversal(version)?;
    // Length sanity — typical semver is <30 chars; cap at 256 to
    // reject pathologically long inputs cheaply.
    if version.len() > 256 {
        return Err(format!(
            "invalid version: too long ({} > 256 chars)",
            version.len()
        ));
    }
    let mut chars = version.chars().peekable();
    // MAJOR.MINOR.PATCH — three dot-separated digit runs.
    if consume_digits(&mut chars) == 0 {
        return Err("invalid version: missing MAJOR (expected X.Y.Z)".to_string());
    }
    if chars.next() != Some('.') {
        return Err("invalid version: missing '.' after MAJOR".to_string());
    }
    if consume_digits(&mut chars) == 0 {
        return Err("invalid version: missing MINOR (expected X.Y.Z)".to_string());
    }
    if chars.next() != Some('.') {
        return Err("invalid version: missing '.' after MINOR".to_string());
    }
    if consume_digits(&mut chars) == 0 {
        return Err("invalid version: missing PATCH (expected X.Y.Z)".to_string());
    }
    // Optional -PRERELEASE and/or +BUILD (in that order).
    match chars.next() {
        None => return Ok(()),
        Some('-') => {
            if consume_alnum_dot_dash(&mut chars) == 0 {
                return Err("invalid version: empty prerelease after '-'".to_string());
            }
        }
        Some('+') => {
            if consume_alnum_dot_dash(&mut chars) == 0 {
                return Err("invalid version: empty build after '+'".to_string());
            }
        }
        Some(c) => {
            return Err(format!(
                "invalid version: unexpected '{c}' after PATCH (expected '-', '+', or end)"
            ));
        }
    }
    // Optional +BUILD after -PRERELEASE.
    if chars.peek() == Some(&'+') {
        chars.next();
        if consume_alnum_dot_dash(&mut chars) == 0 {
            return Err("invalid version: empty build after '+'".to_string());
        }
    }
    if chars.next().is_some() {
        return Err("invalid version: trailing characters after semver".to_string());
    }
    Ok(())
}

/// Consume a run of ASCII digits from `chars`. Returns the count
/// consumed (0 if the next char isn't a digit). Used by
/// [`validate_version_string`] for MAJOR / MINOR / PATCH.
fn consume_digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> usize {
    let mut n = 0;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            chars.next();
            n += 1;
        } else {
            break;
        }
    }
    n
}

/// Consume a run of `[a-zA-Z0-9.-]` from `chars`. Returns the count.
/// Used by [`validate_version_string`] for PRERELEASE / BUILD.
fn consume_alnum_dot_dash(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> usize {
    let mut n = 0;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
            chars.next();
            n += 1;
        } else {
            break;
        }
    }
    n
}

/// P0.28: Map a validator's `Err(String)` into [`RegistryError::InvalidInput`]
/// (HTTP 400 with the message in the JSON body). Trivial adapter so
/// handler call sites stay one-lined: `validate_*(...).map_err(invalid_input)?`.
fn invalid_input(msg: String) -> RegistryError {
    RegistryError::InvalidInput(msg)
}

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
    //
    // P0.28: layer THREE validators here as defense-in-depth:
    //   (a) validate_no_path_traversal — catches `..`, `\0`, abs paths
    //       (cheapest; runs first).
    //   (b) validate_package_name — strict npm-style regex on top of
    //       crate::validate_package_name's charset check.
    //   (c) crate::validate_package_name — keeps the existing scoped
    //       + path-traversal behavior that downstream callers expect.
    // The three checks are overlapping; running all three is intentional
    // (security-layered, not perf-critical — a registry publish is a
    // once-per-version event).
    validate_no_path_traversal(&request.name).map_err(invalid_input)?;
    validate_package_name(&request.name).map_err(invalid_input)?;
    crate::validate_package_name(&request.name)?;
    // P0.28: validate version SHAPE before `Version::parse` so we get a
    // clear 400 on `1.0` / `v1.0.0` / `1.0.0.0` etc., AND so any
    // path-traversal-shaped version is rejected before it flows into
    // the filesystem tarball path.
    validate_version_string(&request.version).map_err(invalid_input)?;
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
    // P0.28: validate the URL path param BEFORE it reaches storage.
    // Defense-in-depth even though the storage layer doesn't currently
    // use the name as a filesystem path — a future SqliteStorage /
    // filesystem backend might, and a malicious URL like
    // `/api/v1/package/..%2F..%2Fetc%2Fpasswd` should fail at 400, not
    // at the storage boundary.
    validate_no_path_traversal(&name).map_err(invalid_input)?;
    validate_package_name(&name).map_err(invalid_input)?;
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
    // P0.28: validate BOTH path params (name + version) before they
    // reach `Version::parse` or storage.
    validate_no_path_traversal(&name).map_err(invalid_input)?;
    validate_no_path_traversal(&version).map_err(invalid_input)?;
    validate_version_string(&version).map_err(invalid_input)?;
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
    // P0.28: validate the URL path param (the `req` query param is
    // parsed by `VersionReq::parse` below which is already strict).
    validate_no_path_traversal(&name).map_err(invalid_input)?;
    validate_package_name(&name).map_err(invalid_input)?;
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
    // P0.28: validate the URL path param.
    validate_no_path_traversal(&name).map_err(invalid_input)?;
    validate_package_name(&name).map_err(invalid_input)?;
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
    // P0.28: same three-layer defense as the legacy publish handler.
    validate_no_path_traversal(&name).map_err(invalid_input)?;
    validate_package_name(&name).map_err(invalid_input)?;
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
    // P0.28: validate shape BEFORE `Version::parse` (clearer 400 +
    // path-traversal defense for the version that flows into the
    // filesystem tarball path).
    validate_version_string(&meta.version).map_err(invalid_input)?;
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
    // P0.28: validate BOTH path params before they reach
    // `Version::parse` or filesystem tarball read. The version flows
    // into `format!("{}.tar", version)` in `read_tarball_from_fs` —
    // path traversal here is a real risk, not theoretical.
    validate_no_path_traversal(&name).map_err(invalid_input)?;
    validate_no_path_traversal(&version_str).map_err(invalid_input)?;
    validate_version_string(&version_str).map_err(invalid_input)?;
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
    // P0.28: validate the URL path param.
    validate_no_path_traversal(&name).map_err(invalid_input)?;
    validate_package_name(&name).map_err(invalid_input)?;
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
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({"status": "ok"})),
    )
}

/// `GET /ready` — readiness probe.
///
/// Returns `200 OK` with `{"status":"ready"}`. The in-memory storage
/// backend is always ready — there is no database connection to wait
/// for. A future backend (e.g. Postgres) would check connectivity here
/// and return `503 Service Unavailable` on failure.
pub(crate) async fn ready_handler() -> impl IntoResponse {
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({"status": "ready"})),
    )
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

    // --- P0.28 unit tests for handler-level validators ---

    #[test]
    fn validate_package_name_accepts_canonical_unscoped() {
        assert!(validate_package_name("foo").is_ok());
        assert!(validate_package_name("foo-bar").is_ok());
        assert!(validate_package_name("foo123").is_ok());
        assert!(validate_package_name("ab").is_ok()); // boundary: 2 chars
                                                      // Boundary: 64 chars (regex max).
        let max_name = "a".to_string() + &"b".repeat(63);
        assert_eq!(max_name.len(), 64);
        assert!(validate_package_name(&max_name).is_ok());
    }

    #[test]
    fn validate_package_name_accepts_scoped() {
        assert!(validate_package_name("@buff/core").is_ok());
        assert!(validate_package_name("@my-org/my-pkg").is_ok());
        assert!(validate_package_name("@org123/pkg456").is_ok());
    }

    #[test]
    fn validate_package_name_rejects_leading_digit_or_hyphen() {
        // The strict regex requires first char to be a lowercase letter.
        assert!(validate_package_name("1foo").is_err());
        assert!(validate_package_name("-foo").is_err());
        assert!(validate_package_name("@1org/pkg").is_err());
        assert!(validate_package_name("@org/-pkg").is_err());
    }

    #[test]
    fn validate_package_name_rejects_uppercase() {
        assert!(validate_package_name("Foo").is_err());
        assert!(validate_package_name("fooBar").is_err());
        assert!(validate_package_name("@ORG/pkg").is_err());
        assert!(validate_package_name("@org/Pkg").is_err());
    }

    #[test]
    fn validate_package_name_rejects_underscore_and_special() {
        // Task regex `^[a-z][a-z0-9-]{1,63}$` does NOT include `_`.
        assert!(validate_package_name("foo_bar").is_err());
        assert!(validate_package_name("foo.bar").is_err());
        assert!(validate_package_name("foo bar").is_err());
        assert!(validate_package_name("foo!bar").is_err());
    }

    #[test]
    fn validate_package_name_rejects_too_short_or_long() {
        assert!(validate_package_name("a").is_err()); // 1 char
        let too_long = "a".to_string() + &"b".repeat(64); // 65 chars
        assert!(validate_package_name(&too_long).is_err());
    }

    #[test]
    fn validate_no_path_traversal_rejects_dot_dot() {
        assert!(validate_no_path_traversal("../evil").is_err());
        assert!(validate_no_path_traversal("foo/../bar").is_err());
        assert!(validate_no_path_traversal("..").is_err());
        assert!(validate_no_path_traversal("a..b").is_err());
    }

    #[test]
    fn validate_no_path_traversal_rejects_null_byte() {
        assert!(validate_no_path_traversal("foo\0bar").is_err());
        assert!(validate_no_path_traversal("\0").is_err());
    }

    #[test]
    fn validate_no_path_traversal_rejects_absolute_paths() {
        // Unix leading '/'.
        assert!(validate_no_path_traversal("/etc/passwd").is_err());
        assert!(validate_no_path_traversal("/foo").is_err());
        // Windows drive-letter forms.
        assert!(validate_no_path_traversal("C:\\Windows\\evil").is_err());
        assert!(validate_no_path_traversal("C:/Users").is_err());
        assert!(validate_no_path_traversal("D:\\buff").is_err());
        assert!(validate_no_path_traversal("z:/foo").is_err());
    }

    #[test]
    fn validate_no_path_traversal_accepts_clean_inputs() {
        assert!(validate_no_path_traversal("foo").is_ok());
        assert!(validate_no_path_traversal("foo-bar").is_ok());
        assert!(validate_no_path_traversal("1.0.0").is_ok());
        assert!(validate_no_path_traversal("@buff/core").is_ok());
        assert!(validate_no_path_traversal("").is_ok()); // empty isn't a traversal
    }

    #[test]
    fn validate_version_string_accepts_canonical_semver() {
        assert!(validate_version_string("1.0.0").is_ok());
        assert!(validate_version_string("0.0.0").is_ok());
        assert!(validate_version_string("0.0.1").is_ok());
        assert!(validate_version_string("10.20.30").is_ok());
        assert!(validate_version_string("1.0.0-alpha").is_ok());
        assert!(validate_version_string("1.0.0-alpha.1").is_ok());
        assert!(validate_version_string("1.0.0+build.1").is_ok());
        assert!(validate_version_string("1.0.0-alpha+build.1").is_ok());
        assert!(validate_version_string("1.0.0-x.7.z.92").is_ok());
    }

    #[test]
    fn validate_version_string_rejects_malformed() {
        // Missing parts.
        assert!(validate_version_string("1.0").is_err());
        assert!(validate_version_string("1").is_err());
        assert!(validate_version_string("").is_err());
        // Wrong separators.
        assert!(validate_version_string("1.0.0.0").is_err());
        assert!(validate_version_string("1-0-0").is_err());
        // Leading 'v' (common typo).
        assert!(validate_version_string("v1.0.0").is_err());
        // Path-traversal-shaped (the critical security case).
        assert!(validate_version_string("1.0.0/../../etc/passwd").is_err());
        assert!(validate_version_string("..1.0.0").is_err());
        assert!(validate_version_string("1.0.0\0").is_err());
        // Empty prerelease.
        assert!(validate_version_string("1.0.0-").is_err());
        // Empty build.
        assert!(validate_version_string("1.0.0+").is_err());
        // Trailing junk.
        assert!(validate_version_string("1.0.0junk").is_err());
    }

    #[test]
    fn validate_version_string_rejects_overlong() {
        // 256-byte cap: boundary at 256 (allowed), 257 (rejected).
        // Prefix "1.0.0-" is 6 bytes, so add 250 / 251 filler chars.
        let big = "1.0.0-".to_string() + &"a".repeat(250);
        assert_eq!(big.len(), 256, "boundary: 256 chars still OK");
        assert!(validate_version_string(&big).is_ok());
        let too_big = "1.0.0-".to_string() + &"a".repeat(251);
        assert_eq!(too_big.len(), 257, "over the cap");
        assert!(validate_version_string(&too_big).is_err());
    }
}
