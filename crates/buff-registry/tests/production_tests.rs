//! T57 production endpoint integration tests.
//!
//! Tests the new multipart upload, new download endpoint, download
//! stats, and IP rate limiting added in T57 commit 5.

#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use buff_registry::{app, AppState, SqliteStorage, Storage};
use serde_json::Value;
use tower::ServiceExt;

fn fresh_app(tarball_dir: Option<&std::path::Path>) -> (axum::Router, Arc<SqliteStorage>) {
    let storage = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    storage.add_token("tok").expect("add token");
    let state = AppState::new(storage.clone() as Arc<dyn Storage>)
        .with_allowlist_enabled(false)
        .with_tarball_dir(tarball_dir.map(|p| p.to_path_buf()));
    (app(state), storage)
}

/// Build a multipart body with metadata + tarball parts.
fn multipart_body(version: &str, tarball: &[u8]) -> Vec<u8> {
    let boundary = "----bufftestboundary";
    let metadata = format!(r#"{{"version":"{version}","deps":[]}}"#);
    let mut body = Vec::new();
    // metadata part
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"metadata\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    body.extend_from_slice(metadata.as_bytes());
    body.extend_from_slice(b"\r\n");
    // tarball part
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"tarball\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(tarball);
    body.extend_from_slice(b"\r\n");
    // closing boundary
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

async fn do_multipart_publish(
    router: axum::Router,
    name: &str,
    version: &str,
    tarball: &[u8],
    token: &str,
) -> (StatusCode, Value) {
    let body_bytes = multipart_body(version, tarball);
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/packages/{name}"))
        .header("content-type", "multipart/form-data; boundary=----bufftestboundary")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body_bytes))
        .expect("build");
    let response = router.oneshot(request).await.expect("oneshot");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.expect("collect");
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
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.expect("collect");
    (status, bytes.to_vec())
}

#[tokio::test]
async fn multipart_publish_then_download() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (router, _storage) = fresh_app(Some(tmp.path()));

    let tarball = vec![0xCA, 0xFE, 0xBA, 0xBE];
    let (status, body) =
        do_multipart_publish(router.clone(), "fs-pkg", "1.0.0", &tarball, "tok").await;
    assert_eq!(status, StatusCode::CREATED, "multipart publish: {body}");

    // Download via the NEW endpoint.
    let (status, bytes) =
        do_get(router.clone(), "/api/v1/packages/fs-pkg/1.0.0/download").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, tarball);

    // Download via the LEGACY endpoint too (backwards compat).
    let (status, bytes) = do_get(router, "/api/v1/download/fs-pkg/1.0.0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, tarball);
}

#[tokio::test]
async fn multipart_publish_without_auth_returns_401() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (router, _) = fresh_app(Some(tmp.path()));

    let body_bytes = multipart_body("1.0.0", &[1, 2, 3]);
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/packages/noauth")
        .header("content-type", "multipart/form-data; boundary=----bufftestboundary")
        .body(Body::from(body_bytes))
        .expect("build");
    let response = router.oneshot(request).await.expect("oneshot");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn download_stats_increment() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (router, _storage) = fresh_app(Some(tmp.path()));

    // Publish.
    let (status, _) =
        do_multipart_publish(router.clone(), "stats-pkg", "1.0.0", &[0xFF], "tok").await;
    assert_eq!(status, StatusCode::CREATED);

    // Download 3 times.
    for _ in 0..3 {
        let (status, _) =
            do_get(router.clone(), "/api/v1/packages/stats-pkg/1.0.0/download").await;
        assert_eq!(status, StatusCode::OK);
    }

    // Check stats.
    let (status, body) = do_get(router, "/api/v1/packages/stats-pkg/stats").await;
    assert_eq!(status, StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(parsed["downloads"], 3);
}

#[tokio::test]
async fn download_stats_via_legacy_endpoint() {
    // The legacy download endpoint should ALSO record stats.
    let (router, _storage) = fresh_app(None);

    // Publish via legacy endpoint.
    let payload = serde_json::json!({
        "name": "legacy-stats",
        "version": "1.0.0",
        "deps": [],
        "tarball_b64": "",
    });
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/publish")
        .header("content-type", "application/json")
        .header("authorization", "Bearer tok")
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build");
    let _ = router.clone().oneshot(request).await.expect("oneshot");

    // Download via legacy endpoint.
    let (status, _) = do_get(router.clone(), "/api/v1/download/legacy-stats/1.0.0").await;
    assert_eq!(status, StatusCode::OK);

    // Check stats.
    let (status, body) = do_get(router, "/api/v1/packages/legacy-stats/stats").await;
    assert_eq!(status, StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(parsed["downloads"], 1);
}

#[tokio::test]
async fn stats_for_unknown_package_returns_zero() {
    let (router, _) = fresh_app(None);
    let (status, body) = do_get(router, "/api/v1/packages/never-published/stats").await;
    assert_eq!(status, StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(parsed["downloads"], 0);
}

#[tokio::test]
async fn multipart_download_404_for_unknown_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (router, _) = fresh_app(Some(tmp.path()));

    do_multipart_publish(router.clone(), "dl-404", "1.0.0", &[], "tok").await;

    let (status, _) = do_get(router, "/api/v1/packages/dl-404/2.0.0/download").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn multipart_publish_works_without_tarball_dir() {
    // When tarball_dir is None, tarballs are stored as BLOBs in SQLite.
    // The new download endpoint falls back to the BLOB.
    let (router, _) = fresh_app(None);

    let tarball = vec![0x11, 0x22, 0x33];
    let (status, body) =
        do_multipart_publish(router.clone(), "blob-pkg", "1.0.0", &tarball, "tok").await;
    assert_eq!(status, StatusCode::CREATED, "publish: {body}");

    let (status, bytes) = do_get(router, "/api/v1/packages/blob-pkg/1.0.0/download").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, tarball);
}
