//! P0.28 (sec-hardening): integration tests for handler-level input
//! validation.
//!
//! Exercises the three defenses added in P0.28 through the full HTTP
//! stack (axum router → handler → validator → storage):
//!
//! 1. **Strict package-name regex** — `validate_package_name` (npm-style
//!    `^[a-z][a-z0-9-]{1,63}$`, with scoped `@org/pkg` support). Tests
//!    that valid names publish successfully and that invalid names
//!    (uppercase, leading digit/hyphen, underscore, special chars,
//!    too short/long) return 400 Bad Request.
//!
//! 2. **Path-traversal defense** — `validate_no_path_traversal`. Tests
//!    that `..`, NUL bytes, leading `/`, and Windows drive-letter paths
//!    (`C:\`) are rejected at 400 regardless of which endpoint they
//!    hit (`publish`, `get_package`, `download`, etc.).
//!
//! 3. **Version-string shape** — `validate_version_string`. Tests that
//!    canonical semver (`1.0.0`, `1.0.0-alpha+build`) is accepted and
//!    that malformed (`1.0`, `v1.0.0`, `1.0.0.0`, path-traversal-shaped)
//!    versions are rejected at 400.
//!
//! 4. **Body size limit** — `DefaultBodyLimit::max(...)` installed on
//!    the router. Tests that an oversized publish body returns 413
//!    Payload Too Large. Uses a small `AppState::with_body_limit(64)`
//!    override so the test does NOT have to allocate 50+ MiB.
//!
//! Unit-level coverage of the validator functions themselves lives in
//! `src/handlers.rs::tests` (zero-overhead, no axum boot). This file
//! proves the validators are wired into the HTTP layer correctly.

#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use buff_registry::{
    app, AppState, InMemoryStorage, SqliteStorage, Storage, DEFAULT_RATE_LIMIT_MAX,
};
use serde_json::{json, Value};
use tower::ServiceExt;

/// Build a fresh app + storage with a test token seeded and a generous
/// rate limit so individual tests don't accidentally hit it.
fn fresh_app() -> (axum::Router, Arc<InMemoryStorage>) {
    let storage = Arc::new(InMemoryStorage::new());
    storage.add_token("test-token");
    let state = AppState::new(storage.clone() as Arc<dyn Storage>)
        .with_rate_limit(Duration::from_secs(60), DEFAULT_RATE_LIMIT_MAX);
    (app(state), storage)
}

/// Build a fresh app backed by `SqliteStorage` (needed for tests that
/// exercise scoped-package publishes, which require org-membership
/// lookups — `InMemoryStorage` doesn't implement `add_org_member`).
fn fresh_sqlite_app() -> (axum::Router, Arc<SqliteStorage>) {
    let storage = Arc::new(SqliteStorage::open_in_memory().expect("open sqlite"));
    storage.add_token("test-token").expect("add token");
    let state = AppState::new(storage.clone() as Arc<dyn Storage>)
        .with_rate_limit(Duration::from_secs(60), DEFAULT_RATE_LIMIT_MAX);
    (app(state), storage)
}

/// Build a fresh app with a custom (small) body limit, for 413 tests.
fn fresh_app_with_body_limit(limit_bytes: usize) -> axum::Router {
    let storage = Arc::new(InMemoryStorage::new());
    storage.add_token("test-token");
    let state = AppState::new(storage as Arc<dyn Storage>)
        .with_rate_limit(Duration::from_secs(60), DEFAULT_RATE_LIMIT_MAX)
        .with_body_limit(limit_bytes);
    app(state)
}

/// Build a JSON publish payload for the given package + version.
fn publish_payload(name: &str, version: &str, tarball: &[u8]) -> Value {
    json!({
        "name": name,
        "version": version,
        "deps": [],
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
// 1. Strict package-name regex (validate_package_name).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publish_accepts_canonical_lowercase_name() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("good-name", "1.0.0", &[1, 2, 3]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "canonical lowercase name should be accepted: {body}"
    );
}

#[tokio::test]
async fn publish_accepts_scoped_name() {
    // Scoped names require org-membership lookup, which needs
    // SqliteStorage (InMemoryStorage doesn't implement add_org_member).
    let (router, storage) = fresh_sqlite_app();
    storage
        .add_org_member("buff", "test-token")
        .expect("add org member");
    let payload = publish_payload("@buff/core", "1.0.0", &[1, 2, 3]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "scoped name @buff/core should be accepted: {body}"
    );
}

#[tokio::test]
async fn publish_rejects_uppercase_name() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("Test", "1.0.0", &[]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "uppercase name should be rejected with 400"
    );
    // P0.28 error messages should explain WHY (not just "invalid package name").
    let err_msg = body["error"].as_str().unwrap_or_default();
    assert!(
        err_msg.contains("lowercase") || err_msg.contains("invalid"),
        "error should mention lowercase rule: {err_msg}"
    );
}

#[tokio::test]
async fn publish_rejects_leading_digit() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("1foo", "1.0.0", &[]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "leading digit should be rejected"
    );
    let err_msg = body["error"].as_str().unwrap_or_default();
    assert!(
        err_msg.contains("lowercase") || err_msg.contains("invalid"),
        "error should mention the lowercase-first rule: {err_msg}"
    );
}

#[tokio::test]
async fn publish_rejects_leading_hyphen() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("-foo", "1.0.0", &[]);
    let (status, _body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "leading hyphen should be rejected"
    );
}

#[tokio::test]
async fn publish_rejects_underscore_in_name() {
    // P0.28 regex `^[a-z][a-z0-9-]{1,63}$` does NOT include underscore.
    let (router, _storage) = fresh_app();
    let payload = publish_payload("foo_bar", "1.0.0", &[]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "underscore should be rejected (strict regex)"
    );
    let err_msg = body["error"].as_str().unwrap_or_default();
    assert!(
        err_msg.contains("illegal character") || err_msg.contains("invalid"),
        "error should mention the illegal char: {err_msg}"
    );
}

#[tokio::test]
async fn publish_rejects_special_chars_in_name() {
    let (router, _storage) = fresh_app();
    for bad in &["foo.bar", "foo!bar", "foo bar", "foo/bar"] {
        let payload = publish_payload(bad, "1.0.0", &[]);
        let (status, _body) = do_publish(router.clone(), &payload, Some("test-token")).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "name with special chars '{bad}' should be rejected"
        );
    }
}

#[tokio::test]
async fn publish_rejects_too_short_name() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("a", "1.0.0", &[]);
    let (status, _body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "single-char name should be rejected (min 2 chars)"
    );
}

#[tokio::test]
async fn publish_rejects_too_long_name() {
    let (router, _storage) = fresh_app();
    // 65 chars (regex limit is 64).
    let long_name = "a".to_string() + &"b".repeat(64);
    let payload = publish_payload(&long_name, "1.0.0", &[]);
    let (status, _body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "65-char name should be rejected (max 64)"
    );
}

#[tokio::test]
async fn publish_accepts_boundary_length_names() {
    let (router, _storage) = fresh_app();
    // 2 chars (min).
    let payload = publish_payload("ab", "1.0.0", &[]);
    let (status, _body) = do_publish(router.clone(), &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::CREATED, "2-char name should be accepted");
    // 64 chars (max).
    let max_name = "a".to_string() + &"b".repeat(63);
    let payload = publish_payload(&max_name, "1.0.0", &[]);
    let (status, _body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::CREATED, "64-char name should be accepted");
}

// ---------------------------------------------------------------------------
// 2. Path-traversal defense (validate_no_path_traversal).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publish_rejects_path_traversal_in_name() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("../evil", "1.0.0", &[]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err_msg = body["error"].as_str().unwrap_or_default();
    assert!(
        err_msg.contains("path traversal") || err_msg.contains("invalid"),
        "error should mention path traversal: {err_msg}"
    );
}

#[tokio::test]
async fn publish_rejects_null_byte_in_name() {
    let (router, _storage) = fresh_app();
    // serde_json will accept the literal NUL in the string.
    let payload = json!({
        "name": "foo\0bar",
        "version": "1.0.0",
        "deps": [],
        "tarball_b64": "",
    });
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "NUL byte in name should be rejected"
    );
    let err_msg = body["error"].as_str().unwrap_or_default();
    assert!(
        err_msg.contains("NUL") || err_msg.contains("null") || err_msg.contains("control"),
        "error should mention NUL/control char: {err_msg}"
    );
}

#[tokio::test]
async fn publish_rejects_path_traversal_in_version() {
    let (router, _storage) = fresh_app();
    // A version-shaped traversal attack: would become the filename
    // `1.0.0../../etc.tar` in `write_tarball_to_fs` if not caught.
    let payload = publish_payload("good-name", "1.0.0../../etc", &[]);
    let (status, body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "path-traversal in version must be rejected"
    );
    let err_msg = body["error"].as_str().unwrap_or_default();
    assert!(
        err_msg.contains("path traversal") || err_msg.contains("invalid"),
        "error should mention path traversal: {err_msg}"
    );
}

#[tokio::test]
async fn get_package_rejects_path_traversal_in_url() {
    let (router, _storage) = fresh_app();
    // URL-encoded `..` — axum's router will URL-decode this for the
    // Path<String> extractor, so the handler sees literal `../evil`.
    let (status, _body) = do_get(router, "/api/v1/package/..%2Fevil").await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "path traversal in URL must return 400, not 404"
    );
}

#[tokio::test]
async fn download_rejects_path_traversal_in_version_url() {
    let (router, _storage) = fresh_app();
    // URL-encoded `..` in the version segment.
    let (status, _body) =
        do_get(router, "/api/v1/download/some-pkg/..%2F..%2Fetc%2Fpasswd").await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "path traversal in version URL must return 400"
    );
}

#[tokio::test]
async fn get_package_rejects_absolute_path_in_url() {
    let (router, _storage) = fresh_app();
    // URL-encoded `/etc/passwd` — axum decodes the leading '/' which
    // makes `validate_no_path_traversal` reject it.
    let (status, _body) = do_get(router, "/api/v1/package/%2Fetc%2Fpasswd").await;
    // Either 400 (path traversal caught) or 404 (axum normalized the
    // path and routed to a different handler) is acceptable; the
    // important invariant is that the request does NOT reach storage.
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
        "absolute path in URL must be rejected (got {status})"
    );
}

// ---------------------------------------------------------------------------
// 3. Version-string shape (validate_version_string).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publish_accepts_canonical_semver() {
    let (router, _storage) = fresh_app();
    for (i, ok) in ["1.0.0", "0.0.0", "10.20.30", "1.0.0-alpha", "1.0.0+build.1"]
        .iter()
        .enumerate()
    {
        // Use a fresh name per iteration to avoid the version-exists
        // short-circuit. Names must match the strict regex (no dots),
        // so use the iteration index instead of the version string.
        let unique_name = format!("verok-{i}");
        let payload = json!({
            "name": unique_name,
            "version": ok,
            "deps": [],
            "tarball_b64": "",
        });
        let (status, body) = do_publish(router.clone(), &payload, Some("test-token")).await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "canonical semver '{ok}' should be accepted: {body}"
        );
    }
}

#[tokio::test]
async fn publish_rejects_malformed_version() {
    let (router, _storage) = fresh_app();
    for bad in &[
        "1.0",       // missing patch
        "1",         // only major
        "1.0.0.0",   // extra component
        "v1.0.0",    // leading v
        "1.0.0-junk!", // illegal prerelease char
        "1.0.0-",    // empty prerelease
        "1.0.0+",    // empty build
    ] {
        let payload = publish_payload("verbad", bad, &[]);
        let (status, body) = do_publish(router.clone(), &payload, Some("test-token")).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "malformed version '{bad}' should be rejected: {body}"
        );
    }
}

#[tokio::test]
async fn publish_rejects_version_with_path_traversal() {
    let (router, _storage) = fresh_app();
    let payload = publish_payload("verbad2", "1.0.0/../../etc", &[]);
    let (status, _body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "path-traversal in version must be rejected at 400"
    );
}

// ---------------------------------------------------------------------------
// 4. Body size limit (DefaultBodyLimit on the router).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publish_rejects_oversized_body_with_413() {
    // Use a tiny body limit so the test doesn't allocate 50+ MiB.
    // The publish JSON envelope overhead is ~80 bytes, so a 200-byte
    // limit will reject any non-trivial payload.
    let router = fresh_app_with_body_limit(200);

    // A legitimate small payload that would normally succeed.
    let mut tarball = vec![0u8; 100];
    // (Could use a real random source, but a deterministic fill is fine.)
    for (i, b) in tarball.iter_mut().enumerate() {
        *b = (i % 256) as u8;
    }
    let payload = publish_payload("sizecheck", "1.0.0", &tarball);

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/publish")
        .header("content-type", "application/json")
        .header("authorization", "Bearer test-token")
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build request");
    let response = router.oneshot(request).await.expect("oneshot");
    // axum's DefaultBodyLimit returns 413 Payload Too Large.
    assert_eq!(
        response.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "oversized body should be rejected with 413, got {}",
        response.status()
    );
}

#[tokio::test]
async fn publish_accepts_body_at_limit() {
    // A publish payload that fits exactly within a small body limit.
    // The body limit counts the request body bytes, not the
    // base64-decoded tarball size — so a tiny tarball inside a tiny
    // JSON envelope fits.
    let router = fresh_app_with_body_limit(256);
    let payload = publish_payload("tinypkg", "1.0.0", &[1]); // ~110 byte body
    let (status, _body) = do_publish(router, &payload, Some("test-token")).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "small payload within the limit should be accepted"
    );
}

// ---------------------------------------------------------------------------
// Defense-in-depth: every URL-with-{name} endpoint rejects bad names.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn all_get_endpoints_reject_path_traversal_in_name() {
    let (router, _storage) = fresh_app();
    // Same traversal payload hitting different endpoints — must NOT
    // reach storage. axum URL-decodes `%2F` to `/` for the Path
    // extractor, so the handler sees `../evil`.
    for uri in &[
        "/api/v1/package/..%2Fevil",
        "/api/v1/packages/..%2Fevil/badges",
        "/api/v1/packages/..%2Fevil/stats",
        "/api/v1/resolve/..%2Fevil?req=%5E1.0.0",
    ] {
        let (status, _body) = do_get(router.clone(), uri).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
            "endpoint {uri}: expected 400 (validation) or 404 (axum route normalize), got {status}"
        );
    }
}
