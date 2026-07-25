//! T57 scoped package integration tests.
//!
//! Tests the full HTTP path for scoped packages (`@org/pkg`):
//! - Scoped publish succeeds when the token is an org member.
//! - Scoped publish is rejected (403) when the token is NOT an org member.
//! - Unscoped publishes still work unchanged.
//! - Scoped packages can be downloaded + resolved like unscoped ones.

#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use buff_registry::{app, AppState, InMemoryStorage, SqliteStorage, Storage};
use serde_json::{json, Value};
use tower::ServiceExt;

fn publish_payload(name: &str, version: &str, tarball: &[u8]) -> Value {
    json!({
        "name": name,
        "version": version,
        "deps": [],
        "tarball_b64": base64::engine::general_purpose::STANDARD.encode(tarball),
    })
}

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
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("collect");
    let json: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

async fn do_get(router: axum::Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("build");
    let response = router.oneshot(request).await.expect("oneshot");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("collect");
    (status, bytes.to_vec())
}

#[tokio::test]
async fn scoped_publish_succeeds_for_org_member() {
    let storage = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    storage.add_token("member-token").expect("add token");
    storage
        .add_org_member("buff", "member-token")
        .expect("add org member");
    let state = AppState::new(storage.clone() as Arc<dyn Storage>);
    let router = app(state);

    let payload = publish_payload("@buff/core", "1.0.0", &[0xAB]);
    let (status, body) = do_publish(router, &payload, Some("member-token")).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "scoped publish should succeed: {body}"
    );
    assert_eq!(body["name"], "@buff/core");
}

#[tokio::test]
async fn scoped_publish_rejected_for_non_member() {
    let storage = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    storage.add_token("outsider-token").expect("add token");
    // NOT added as org member.
    let state = AppState::new(storage.clone() as Arc<dyn Storage>);
    let router = app(state);

    let payload = publish_payload("@buff/core", "1.0.0", &[]);
    let (status, body) = do_publish(router, &payload, Some("outsider-token")).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "non-member should get 403: {body}"
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|s| s.contains("permission")),
        "error should mention permission: {body}"
    );
}

#[tokio::test]
async fn scoped_publish_rejected_for_in_memory_backend() {
    // InMemoryStorage returns false for is_org_member (default impl) —
    // scoped publishes should be rejected (no orgs exist).
    let storage = Arc::new(InMemoryStorage::new());
    storage.add_token("tok");
    let state = AppState::new(storage as Arc<dyn Storage>);
    let router = app(state);

    let payload = publish_payload("@buff/core", "1.0.0", &[]);
    let (status, _) = do_publish(router, &payload, Some("tok")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "InMemoryStorage has no orgs");
}

#[tokio::test]
async fn unscoped_publish_still_works() {
    let storage = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    storage.add_token("tok").expect("add token");
    let state = AppState::new(storage as Arc<dyn Storage>);
    let router = app(state);

    let payload = publish_payload("plain-pkg", "1.0.0", &[1, 2, 3]);
    let (status, _) = do_publish(router.clone(), &payload, Some("tok")).await;
    assert_eq!(status, StatusCode::CREATED);

    // Download should work.
    let (status, bytes) = do_get(router, "/api/v1/download/plain-pkg/1.0.0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, vec![1, 2, 3]);
}

#[tokio::test]
async fn scoped_package_download_and_resolve() {
    let storage = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    storage.add_token("tok").expect("add token");
    storage
        .add_org_member("buff", "tok")
        .expect("add org member");
    let state = AppState::new(storage as Arc<dyn Storage>);
    let router = app(state);

    // Publish two versions.
    for ver in ["1.0.0", "1.1.0"] {
        let payload = publish_payload("@buff/core", ver, &[0xFF]);
        let (status, _) = do_publish(router.clone(), &payload, Some("tok")).await;
        assert_eq!(status, StatusCode::CREATED, "publish {ver}");
    }

    // Download 1.0.0 (URL-encode the '/' in the scoped name per npm convention).
    let (status, bytes) = do_get(router.clone(), "/api/v1/download/@buff%2Fcore/1.0.0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, vec![0xFF]);

    // Resolve ^1.0.0 → 1.1.0.
    let (status, body) = do_get(router, "/api/v1/resolve/@buff%2Fcore?req=%5E1.0.0").await;
    assert_eq!(status, StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(parsed["version"], "1.1.0");
}

#[tokio::test]
async fn scoped_publish_invalid_scope_name_rejected() {
    let storage = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    storage.add_token("tok").expect("add token");
    let state = AppState::new(storage as Arc<dyn Storage>);
    let router = app(state);

    // Uppercase scope → invalid name.
    let payload = publish_payload("@BUFF/core", "1.0.0", &[]);
    let (status, _) = do_publish(router, &payload, Some("tok")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
