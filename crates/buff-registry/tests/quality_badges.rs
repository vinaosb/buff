//! T70 integration tests — package quality signals.
//!
//! Drives the axum [`buff_registry::app`] Router in-process via
//! `tower::ServiceExt::oneshot` (same harness as `registry_tests.rs`).
//! Each test seeds 3 mock packages with distinct badge profiles and
//! exercises the two new T70 endpoints:
//!
//! - `GET /api/v1/packages/{name}/badges` — per-package badge JSON.
//! - `GET /api/v1/search?q=<query>` — search results with badges.
//!
//! # The three mock packages
//!
//! | Package | verified | maintained | tested | documented |
//! |---------|----------|-----------|--------|-----------|
//! | `buff-dataframe` | yes | yes | 85% | 72% |
//! | `buff-cli-tool` | no | yes | None | None |
//! | `buff-legacy-lib` | no | no (stale) | 40% | None |
//!
//! These three cover every badge combination the acceptance criteria
//! enumerate. They are seeded via [`seed_mock_packages`].

#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use buff_registry::{
    app, AppState, InMemoryStorage, QualityAttachment, Storage,
};
use serde_json::{json, Value};
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Build a fresh app + storage with a test token seeded.
fn fresh_app() -> (axum::Router, Arc<InMemoryStorage>) {
    let storage = Arc::new(InMemoryStorage::new());
    storage.add_token("test-token");
    let state = AppState::new(storage.clone() as Arc<dyn Storage>);
    (app(state), storage)
}

/// Current unix-seconds (wall-clock now).
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Seed the 3 mock packages (see module-level table) into `storage`.
///
/// Each package is published directly via `put_version` (bypassing the
/// HTTP publish endpoint) so the test can control the `published_at`
/// timestamp — the "maintained" badge depends on it, and `SystemTime::now()`
/// inside the handler would make the badge assertion time-sensitive.
fn seed_mock_packages(storage: &InMemoryStorage) {
    let now = now_unix();

    // --- Package 1: buff-dataframe — fully badged (verified + maintained + tested + documented)
    storage.add_verified_author("verified-author");
    storage
        .put_version(
            "buff-dataframe",
            semver::Version::parse("1.0.0").expect("version"),
            Vec::new(),
            Vec::new(),
            Some("verified-author".to_string()),
            Some(now), // recent → maintained
            QualityAttachment {
                tested_coverage: Some(85.0),
                documented_coverage: Some(72.0),
                security_audit: None,
            },
        )
        .expect("put buff-dataframe");

    // --- Package 2: buff-cli-tool — maintained but unverified, no coverage
    storage
        .put_version(
            "buff-cli-tool",
            semver::Version::parse("0.5.0").expect("version"),
            Vec::new(),
            Vec::new(),
            Some("unknown-author".to_string()),
            Some(now), // recent → maintained
            QualityAttachment::default(),
        )
        .expect("put buff-cli-tool");

    // --- Package 3: buff-legacy-lib — stale (>180 days), unverified, partial coverage
    let stale = now - 200 * 24 * 60 * 60; // 200 days ago → NOT maintained
    storage
        .put_version(
            "buff-legacy-lib",
            semver::Version::parse("0.1.0").expect("version"),
            Vec::new(),
            Vec::new(),
            Some("old-author".to_string()),
            Some(stale),
            QualityAttachment {
                tested_coverage: Some(40.0),
                documented_coverage: None,
                security_audit: None,
            },
        )
        .expect("put buff-legacy-lib");
}

/// Issue a GET and return `(status, parsed-json)`.
async fn do_get_json(router: axum::Router, uri: &str) -> (StatusCode, Value) {
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
    let json: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

// ---------------------------------------------------------------------------
// Acceptance bullet 1: verified_publisher true when author authenticated
// ---------------------------------------------------------------------------

#[tokio::test]
async fn badges_verified_publisher_true_when_author_in_verified_set() {
    let (router, storage) = fresh_app();
    seed_mock_packages(&storage);

    let (status, body) = do_get_json(router, "/api/v1/packages/buff-dataframe/badges").await;
    assert_eq!(status, StatusCode::OK, "badges endpoint: {body}");
    assert_eq!(
        body["verified_publisher"], true,
        "verified-author → verified_publisher badge true: {body}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 2: verified_publisher false otherwise
// ---------------------------------------------------------------------------

#[tokio::test]
async fn badges_verified_publisher_false_when_author_not_verified() {
    let (router, storage) = fresh_app();
    seed_mock_packages(&storage);

    let (status, body) = do_get_json(router, "/api/v1/packages/buff-cli-tool/badges").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["verified_publisher"], false,
        "un-verified author → badge false: {body}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 3: maintained true when recent publish
// ---------------------------------------------------------------------------

#[tokio::test]
async fn badges_maintained_true_when_recent_publish() {
    let (router, storage) = fresh_app();
    seed_mock_packages(&storage);

    let (status, body) = do_get_json(router, "/api/v1/packages/buff-cli-tool/badges").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["maintained"], true,
        "published now → maintained true: {body}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 4: maintained false when stale (>180 days)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn badges_maintained_false_when_stale() {
    let (router, storage) = fresh_app();
    seed_mock_packages(&storage);

    let (status, body) = do_get_json(router, "/api/v1/packages/buff-legacy-lib/badges").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["maintained"], false,
        "published 200 days ago → maintained false: {body}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 5: tested badge shows coverage %
// ---------------------------------------------------------------------------

#[tokio::test]
async fn badges_tested_shows_coverage_percentage() {
    let (router, storage) = fresh_app();
    seed_mock_packages(&storage);

    let (status, body) = do_get_json(router, "/api/v1/packages/buff-dataframe/badges").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["tested"], 85.0,
        "tested badge echoes the attached coverage: {body}"
    );

    // buff-cli-tool has NO coverage attached → tested is absent (skip_serializing_if None).
    let (router2, storage2) = fresh_app();
    seed_mock_packages(&storage2);
    let (_, body2) = do_get_json(router2, "/api/v1/packages/buff-cli-tool/badges").await;
    assert!(
        body2.get("tested").is_none() || body2["tested"].is_null(),
        "no coverage attached → tested null/absent: {body2}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 6: documented badge shows doc coverage %
// ---------------------------------------------------------------------------

#[tokio::test]
async fn badges_documented_shows_doc_coverage_percentage() {
    let (router, storage) = fresh_app();
    seed_mock_packages(&storage);

    let (status, body) = do_get_json(router, "/api/v1/packages/buff-dataframe/badges").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["documented"], 72.0,
        "documented badge echoes attached doc coverage: {body}"
    );

    // buff-legacy-lib has documented_coverage = None.
    let (router2, storage2) = fresh_app();
    seed_mock_packages(&storage2);
    let (_, body2) = do_get_json(router2, "/api/v1/packages/buff-legacy-lib/badges").await;
    assert!(
        body2.get("documented").is_none() || body2["documented"].is_null(),
        "no doc coverage → documented null/absent: {body2}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 7: /badges endpoint returns correct JSON
// ---------------------------------------------------------------------------

#[tokio::test]
async fn badges_endpoint_returns_404_for_unknown_package() {
    let (router, _storage) = fresh_app();

    let (status, body) = do_get_json(router, "/api/v1/packages/nonexistent/badges").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body["error"].as_str().unwrap_or_default().contains("not found"),
        "unknown package → 404 with error: {body}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 8: /search endpoint includes badges
// ---------------------------------------------------------------------------

#[tokio::test]
async fn search_endpoint_includes_badges_per_result() {
    let (router, storage) = fresh_app();
    seed_mock_packages(&storage);

    let (status, body) = do_get_json(router, "/api/v1/search").await;
    assert_eq!(status, StatusCode::OK, "search all: {body}");
    let results = body.as_array().expect("search returns array");
    assert_eq!(results.len(), 3, "all 3 mock packages returned: {body}");

    // Each row has name + latest_version + badges.
    for row in results {
        assert!(row["name"].is_string(), "name present: {row}");
        assert!(row["latest_version"].is_string(), "version present: {row}");
        assert!(row["badges"].is_object(), "badges object present: {row}");
    }

    // buff-dataframe row has verified_publisher=true + tested=85.0.
    let df = results
        .iter()
        .find(|r| r["name"] == "buff-dataframe")
        .expect("buff-dataframe in results");
    assert_eq!(df["badges"]["verified_publisher"], true);
    assert_eq!(df["badges"]["tested"], 85.0);
}

#[tokio::test]
async fn search_endpoint_filters_by_query_substring() {
    let (router, storage) = fresh_app();
    seed_mock_packages(&storage);

    // "data" matches buff-dataframe only.
    let (status, body) = do_get_json(router, "/api/v1/search?q=data").await;
    assert_eq!(status, StatusCode::OK);
    let results = body.as_array().expect("array");
    assert_eq!(results.len(), 1, "substring filter narrows to 1: {body}");
    assert_eq!(results[0]["name"], "buff-dataframe");

    // "buff" matches all 3.
    let (router2, storage2) = fresh_app();
    seed_mock_packages(&storage2);
    let (_, body2) = do_get_json(router2, "/api/v1/search?q=buff").await;
    assert_eq!(
        body2.as_array().map(Vec::len),
        Some(3),
        "common prefix matches all: {body2}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 9: all 4 badges default to false/None for a new package
// (no attachments, unverified author, but published "now" → maintained=true)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn badges_all_default_for_minimal_new_package() {
    let (router, storage) = fresh_app();

    // Publish a minimal package via the HTTP endpoint (no quality
    // attachments in the body → tested/documented default to None).
    let payload = json!({
        "name": "brand-new",
        "version": "0.1.0",
        "deps": [],
        "tarball_b64": base64::engine::general_purpose::STANDARD.encode(b""),
    });
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/publish")
        .header("content-type", "application/json")
        .header("authorization", "Bearer test-token")
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build request");
    let response = router.clone().oneshot(request).await.expect("oneshot");
    assert_eq!(response.status(), StatusCode::CREATED);
    let _ = storage; // storage is used via the router's shared Arc

    // Fetch badges — verified=false (not in verified set), maintained=true
    // (just published), tested=None, documented=None.
    let (status, body) = do_get_json(router, "/api/v1/packages/brand-new/badges").await;
    assert_eq!(status, StatusCode::OK, "badges for new pkg: {body}");
    assert_eq!(body["verified_publisher"], false);
    assert_eq!(body["maintained"], true, "just published → maintained");
    assert!(
        body.get("tested").is_none() || body["tested"].is_null(),
        "no coverage → tested null: {body}"
    );
    assert!(
        body.get("documented").is_none() || body["documented"].is_null(),
        "no doc coverage → documented null: {body}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance bullet 10: publish with quality attachments propagates to badges
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publish_with_quality_attachments_populates_badges() {
    let (router, _storage) = fresh_app();

    // Publish WITH tested_coverage + documented_coverage in the body.
    let payload = json!({
        "name": "well-tested",
        "version": "1.2.0",
        "deps": [],
        "tarball_b64": base64::engine::general_purpose::STANDARD.encode(b"tarball"),
        "tested_coverage": 95.5,
        "documented_coverage": 88.0,
    });
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/publish")
        .header("content-type", "application/json")
        .header("authorization", "Bearer test-token")
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build request");
    let response = router.clone().oneshot(request).await.expect("oneshot");
    assert_eq!(response.status(), StatusCode::CREATED);

    // Badges echo the attached values.
    let (status, body) = do_get_json(router, "/api/v1/packages/well-tested/badges").await;
    assert_eq!(status, StatusCode::OK, "badges: {body}");
    assert_eq!(body["tested"], 95.5);
    assert_eq!(body["documented"], 88.0);
    assert_eq!(body["maintained"], true, "just published → maintained");
}

// ---------------------------------------------------------------------------
// Extra: verify the full mock-package badge matrix in one assertion
// (documents the 3-package fixture for reviewers).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mock_package_matrix_matches_documented_profiles() {
    let (router, storage) = fresh_app();
    seed_mock_packages(&storage);

    // buff-dataframe: verified=true, maintained=true, tested=85, documented=72
    let (_, df) = do_get_json(router.clone(), "/api/v1/packages/buff-dataframe/badges").await;
    assert_eq!(df["verified_publisher"], true);
    assert_eq!(df["maintained"], true);
    assert_eq!(df["tested"], 85.0);
    assert_eq!(df["documented"], 72.0);

    // buff-cli-tool: verified=false, maintained=true, tested=None, documented=None
    let (_, cli) = do_get_json(router.clone(), "/api/v1/packages/buff-cli-tool/badges").await;
    assert_eq!(cli["verified_publisher"], false);
    assert_eq!(cli["maintained"], true);
    assert!(cli.get("tested").is_none() || cli["tested"].is_null());

    // buff-legacy-lib: verified=false, maintained=false, tested=40, documented=None
    let (_, leg) = do_get_json(router, "/api/v1/packages/buff-legacy-lib/badges").await;
    assert_eq!(leg["verified_publisher"], false);
    assert_eq!(leg["maintained"], false);
    assert_eq!(leg["tested"], 40.0);
    assert!(leg.get("documented").is_none() || leg["documented"].is_null());
}
