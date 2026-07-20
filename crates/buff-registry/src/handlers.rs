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

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use base64::Engine as _;
use semver::{Version, VersionReq};
use serde::Deserialize;

use crate::error::{RegistryError, StorageError};
use crate::storage::{
    DepSpec, PublishRequest, PublishResponse, ResolveResponse, VERSION_EXISTS_MARKER,
};
use crate::{validate_name, AppState};

/// The canonical prefix expected on publish `Authorization` headers.
const BEARER_PREFIX: &str = "Bearer ";

/// Query-string parameters for the resolve endpoint.
#[derive(Debug, Deserialize)]
pub(crate) struct ResolveParams {
    /// A Cargo-style semver requirement (`^1.0.0`, `>=2.0.0`, `*`, etc.).
    pub(crate) req: String,
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
    let token = extract_bearer(&headers).ok_or(RegistryError::Unauthorized)?;
    if !state.storage.validate_token(token)? {
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
    validate_name(&request.name)?;
    let version = Version::parse(&request.version)
        .map_err(|e| RegistryError::InvalidVersion(e.to_string()))?;

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

    // --- 7. Store ----------------------------------------------------------
    let deps = request.deps.clone();
    let put_result =
        state
            .storage
            .put_version(&request.name, version.clone(), request.deps, tarball);
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
        Some(bytes) => Ok((
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::DepSpec;
    use crate::storage::Storage;
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
