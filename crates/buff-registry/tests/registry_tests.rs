//! Integration tests for `buff-registry`.
//!
//! Drives the axum [`buff_registry::app`] Router in-process via
//! `tower::ServiceExt::oneshot` — no TCP port allocation, no subprocess,
//! no external services. Each test builds a fresh
//! [`buff_registry::InMemoryStorage`], seeds a test token, constructs
//! the [`buff_registry::AppState`], and exercises the full HTTP path
//! (auth → validation → cycle → store → render).
//!
//! # Acceptance coverage
//!
//! Each test maps to an acceptance bullet or QA scenario from the T126
//! spec:
//!
//! - **Publish + download roundtrip** — [`publish_then_download_returns_same_bytes`]
//! - **GET /package returns all versions** — [`get_package_lists_all_published_versions`]
//! - **Semver resolution (highest compatible)** — [`resolve_returns_highest_compatible_version`]
//! - **Semver resolution (no match)** — [`resolve_returns_404_when_no_version_matches`]
//! - **Dependency cycle rejected (A→B then B→A)** — [`publish_rejects_dependency_cycle`]
//! - **Self-cycle rejected (A→A)** — [`publish_rejects_self_cycle`]
//! - **Unauthenticated publish rejected** — [`publish_without_auth_returns_401`]
//! - **Invalid token rejected** — [`publish_with_unknown_token_returns_401`]
//! - **Invalid name (path traversal)** — [`publish_with_path_traversal_name_returns_400`]
//! - **Invalid name (empty)** — [`publish_with_empty_name_returns_400`]
//! - **Invalid name (uppercase)** — [`publish_with_uppercase_name_returns_400`]
//! - **Rate limit** — [`publish_after_rate_limit_returns_429`]
//! - **Package not found** — [`get_unknown_package_returns_404`]
//! - **Download unknown version** — [`download_unknown_version_returns_404`]
//! - **Already-published version** — [`republish_same_version_returns_400`]

#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use buff_registry::{
    app, AppState, InMemoryStorage, PublishRequest, Storage, DEFAULT_RATE_LIMIT_MAX,
};
use serde_json::{json, Value};
use tower::ServiceExt;

/// Build a fresh app + storage with a test token seeded and a generous
/// rate limit so individual tests don't accidentally hit it.
fn fresh_app() -> (axum::Router, Arc<InMemoryStorage>) {
    fresh_app_with_rate_limit(Duration::from_secs(60), DEFAULT_RATE_LIMIT_MAX)
}

/// Build a fresh app + storage with the given rate-limit window/budget.
fn fresh_app_with_rate_limit(window: Duration, max: usize) -> (axum::Router, Arc<InMemoryStorage>) {
    let storage = Arc::new(InMemoryStorage::new());
    storage.add_token("test-token");
    let state = AppState::new(storage.clone() as Arc<dyn Storage>).with_rate_limit(window, max);
    (app(state), storage)
}

/// Build a JSON publish payload for the given package + version.
fn publish_payload(name: &str, version: &str, deps: &[(&str, &str)], tarball: &[u8]) -> Value {
    let deps_json: Vec<Value> = deps
        .iter()
        .map(|(n, r)| json!({ "name": n, "req": r }))
        .collect();
    json!({
        "name": name,
        "version": version,
        "deps": deps_json,
        "tarball_b64": base64::engine::general_purpose::STANDARD.encode(tarball),
    })
}

/// POST a publish request, returning the (status, json-body) tuple.
async fn do_publish(
    router: axum::Router,
    payload: &Value,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/publish")
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    let request = builder
        .body(Body::from(serde_json::to_vec(payload).expect("serialize")))
        .expect("build request");
    let response = router.oneshot(request).await.expect("oneshot");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("collect body");
    let json: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

/// Issue a GET against `uri` and return (status, body-bytes).
async fn do_get(router: axum::Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("build request");
    let response = router.oneshot(request).await.expect("oneshot");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("collect body");
    (status, bytes.to_vec())
}

// ---------------------------------------------------------------------------
// Acceptance bullet: publish endpoint accepts .buff tarball (roundtrip).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publish_then_download_returns_same_bytes() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("test-pkg", "1.0.0", &[], &[0xDE, 0xAD, 0xBE, 0xEF]);

    let (status, body) = do_publish(router.clone(), &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "publish should succeed: {body}"
    );
    assert_eq!(body["name"], "test-pkg");
    assert_eq!(body["version"], "1.0.0");

    let (status, bytes) = do_get(router, "/api/v1/download/test-pkg/1.0.0").await;
    assert_eq!(status, StatusCode::OK, "download should succeed");
    assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

// ---------------------------------------------------------------------------
// Acceptance bullet: GET /package returns all published versions.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_package_lists_all_published_versions() {
    let (router, _storage) = fresh_app();
    // Publish two versions of the same package.
    for version in ["1.0.0", "1.1.0"] {
        let payload = publish_payload("multi", version, &[], &[1, 2, 3]);
        let (status, _body) = do_publish(router.clone(), &payload, Some("test-token")).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (status, body) = do_get(router, "/api/v1/package/multi").await;
    assert_eq!(status, StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&body).expect("valid JSON");
    assert_eq!(parsed["name"], "multi");
    let versions = parsed["versions"].as_array().expect("versions array");
    assert_eq!(versions.len(), 2, "both versions present");
    let version_strings: Vec<&str> = versions
        .iter()
        .map(|v| v["version"].as_str().expect("version string"))
        .collect();
    assert!(version_strings.contains(&"1.0.0"));
    assert!(version_strings.contains(&"1.1.0"));
}

// ---------------------------------------------------------------------------
// Acceptance bullet: semver resolution (1.0.0, 1.1.0, 2.0.0 → ^1.0.0 = 1.1.0).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resolve_returns_highest_compatible_version() {
    let (router, _storage) = fresh_app();
    for version in ["1.0.0", "1.1.0", "2.0.0"] {
        let payload = publish_payload("semver-pkg", version, &[], &[]);
        let (status, _body) = do_publish(router.clone(), &payload, Some("test-token")).await;
        assert_eq!(status, StatusCode::CREATED, "publish {version}");
    }

    let (status, body) = do_get(router, "/api/v1/resolve/semver-pkg?req=%5E1.0.0").await;
    assert_eq!(status, StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&body).expect("valid JSON");
    assert_eq!(parsed["name"], "semver-pkg");
    assert_eq!(
        parsed["version"], "1.1.0",
        "^1.0.0 must resolve to 1.1.0 (NOT 2.0.0)"
    );
}

#[tokio::test]
async fn resolve_returns_404_when_no_version_matches() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("semver-pkg", "1.0.0", &[], &[]);
    let (status, _) = do_publish(router.clone(), &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::CREATED);

    // ^2.0.0 — no published version matches.
    let (status, _body) = do_get(router, "/api/v1/resolve/semver-pkg?req=%5E2.0.0").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resolve_returns_404_for_unknown_package() {
    let (router, _storage) = fresh_app();
    let (status, _body) = do_get(router, "/api/v1/resolve/never-published?req=%5E1.0.0").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Acceptance bullet: dependency cycle rejected (A→B then B→A).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publish_rejects_dependency_cycle() {
    let (router, _storage) = fresh_app();

    // Step 1: publish A depending on B (B doesn't exist yet — allowed).
    let payload_a = publish_payload("pkg-a", "1.0.0", &[("pkg-b", "*")], &[]);
    let (status, _body) = do_publish(router.clone(), &payload_a, Some("test-token")).await;
    assert_eq!(status, StatusCode::CREATED, "step 1: A→B must succeed");

    // Step 2: try to publish B depending on A — A's published deps
    // include B (= new_name), so the cycle detector walks A→B and finds
    // the back-edge.
    let payload_b = publish_payload("pkg-b", "1.0.0", &[("pkg-a", "*")], &[]);
    let (status, body) = do_publish(router, &payload_b, Some("test-token")).await;
    assert_eq!(status, StatusCode::CONFLICT, "step 2: B→A must be rejected");
    assert!(
        body["error"].as_str().unwrap_or_default().contains("cycle"),
        "error body must mention `cycle`: {body}"
    );
}

#[tokio::test]
async fn publish_rejects_self_cycle() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("selfish", "1.0.0", &[("selfish", "*")], &[]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body["error"].as_str().unwrap_or_default().contains("cycle"),
        "self-cycle must be flagged: {body}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet: unauthenticated publish rejected, name validation.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publish_without_auth_returns_401() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("noauth", "1.0.0", &[], &[]);
    let (status, _body) = do_publish(router, &payload, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn publish_with_unknown_token_returns_401() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("noauth", "1.0.0", &[], &[]);
    let (status, _body) = do_publish(router, &payload, Some("not-a-real-token")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn publish_with_path_traversal_name_returns_400() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("../evil", "1.0.0", &[], &[]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // P0.28: validate_no_path_traversal now runs FIRST (before
    // validate_package_name), so the message is the more descriptive
    // "invalid input: contains '..' (path traversal forbidden)". The
    // legacy "invalid package name" message from validate_name is
    // still produced for non-traversal charset violations.
    let err = body["error"].as_str().unwrap_or_default();
    assert!(
        err.contains("path traversal") || err.contains("invalid package name"),
        "expected path-traversal or invalid-name error: {body}"
    );
}

#[tokio::test]
async fn publish_with_empty_name_returns_400() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("", "1.0.0", &[], &[]);
    let (status, _body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn publish_with_uppercase_name_returns_400() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("Test", "1.0.0", &[], &[]);
    let (status, _body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn publish_with_invalid_version_returns_400() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("verbad", "not-a-semver", &[], &[]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("invalid version"),
        "expected invalid-version error: {body}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet: rate limiting on publish endpoint.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publish_after_rate_limit_returns_429() {
    // Tight budget: 3 publishes per 60 seconds.
    let (router, _storage) = fresh_app_with_rate_limit(Duration::from_secs(60), 3);

    // 3 publishes must succeed.
    for i in 1..=3 {
        let payload = publish_payload(&format!("rl{i}"), "1.0.0", &[], &[]);
        let (status, body) = do_publish(router.clone(), &payload, Some("test-token")).await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "publish #{i} should succeed: {body}"
        );
    }

    // 4th publish must be rejected.
    let payload = publish_payload("rl4", "1.0.0", &[], &[]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "publish #4 must be rate-limited"
    );
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("rate limit"),
        "expected rate-limit error: {body}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet: package not found / unknown version.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_unknown_package_returns_404() {
    let (router, _storage) = fresh_app();
    let (status, _body) = do_get(router, "/api/v1/package/never-heard-of-it").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn download_unknown_version_returns_404() {
    let (router, _storage) = fresh_app();
    // Publish 1.0.0 ...
    let payload = publish_payload("dl-404", "1.0.0", &[], &[]);
    let (status, _) = do_publish(router.clone(), &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::CREATED);

    // ... ask for 2.0.0 — must 404.
    let (status, _body) = do_get(router, "/api/v1/download/dl-404/2.0.0").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn download_unknown_package_returns_404() {
    let (router, _storage) = fresh_app();
    let (status, _body) = do_get(router, "/api/v1/download/never-published/1.0.0").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Append-only contract: republishing the same (name, version) → 400.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn republish_same_version_returns_400() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("reuse", "1.0.0", &[], &[]);
    let (status, _) = do_publish(router.clone(), &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("already exists"),
        "expected version-exists error: {body}"
    );
}

// ---------------------------------------------------------------------------
// Anonymous download IS allowed (publish auth does not gate downloads).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_works_without_auth_header() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("anon-dl", "1.0.0", &[], &[1, 2, 3, 4]);
    let (status, _) = do_publish(router.clone(), &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::CREATED);

    // do_get issues NO Authorization header — must still work.
    let (status, bytes) = do_get(router, "/api/v1/download/anon-dl/1.0.0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, vec![1, 2, 3, 4]);
}

// ---------------------------------------------------------------------------
// Smoke test: the typed PublishRequest struct round-trips through serde
// the same way the JSON helpers above do. Protects against the
// subtle "the wire shape matches the struct" failure mode.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn typed_publish_request_round_trips_through_http() {
    let (router, _storage) = fresh_app();
    let typed = PublishRequest {
        name: "typed".to_string(),
        version: "1.2.3".to_string(),
        deps: vec![],
        tarball_b64: base64::engine::general_purpose::STANDARD.encode([0xAA, 0xBB]),
        tested_coverage: None,
        documented_coverage: None,
    };
    let body_bytes = serde_json::to_vec(&typed).expect("serialize");
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/publish")
        .header("content-type", "application/json")
        .header("authorization", "Bearer test-token")
        .body(Body::from(body_bytes))
        .expect("build request");
    let response = router.clone().oneshot(request).await.expect("oneshot");
    assert_eq!(response.status(), StatusCode::CREATED);

    let (status, bytes) = do_get(router, "/api/v1/download/typed/1.2.3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, vec![0xAA, 0xBB]);
}
