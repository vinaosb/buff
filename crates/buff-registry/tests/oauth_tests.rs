//! T57 OAuth flow integration tests.
//!
//! Drives the axum [`buff_registry::app`] Router in-process via
//! `tower::ServiceExt::oneshot`, with a mock GitHub API stood up via
//! `httpmock` (a real TCP listener on an ephemeral port — no real
//! GitHub calls).
//!
//! # Test coverage
//!
//! - `login` returns 503 when OAuth is not configured.
//! - `login` returns 302 redirect when OAuth IS configured.
//! - `callback` exchanges code → token → user → session via the mock.
//! - `whoami` returns the session user after login.
//! - `whoami` returns 401 without a session.
//! - `logout` clears the session.
//! - `publish` accepts session tokens (OAuth auth → publish).

#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use buff_registry::{app, AppState, OAuthConfig, SqliteStorage, Storage};
use httpmock::{Method, MockServer};
use serde_json::{json, Value};
use tower::ServiceExt;

/// Build an app backed by SQLite with OAuth pointing at `mock_server`.
/// Allowlist is disabled (tested separately in allowlist_tests.rs).
fn oauth_app(storage: Arc<dyn Storage>, mock_server: &MockServer) -> AppState {
    let config = OAuthConfig {
        client_id: "test-client-id".to_string(),
        client_secret: "test-secret".to_string(),
        redirect_uri: "http://localhost:7878/auth/github/callback".to_string(),
        authorize_url: format!("{}/login/oauth/authorize", mock_server.base_url()),
        token_url: format!("{}/login/oauth/access_token", mock_server.base_url()),
        user_url: format!("{}/user", mock_server.base_url()),
    };
    AppState::new(storage)
        .with_oauth_config(Some(config))
        .with_allowlist_enabled(false)
}

/// Issue a GET and return (status, body-bytes, headers).
async fn do_get_full(
    router: axum::Router,
    uri: &str,
    extra_headers: &[(&str, &str)],
) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    let mut builder = Request::builder().method("GET").uri(uri);
    for (k, v) in extra_headers {
        builder = builder.header(*k, *v);
    }
    let request = builder.body(Body::empty()).expect("build request");
    let response = router.oneshot(request).await.expect("oneshot");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("collect body");
    (status, bytes.to_vec(), headers)
}

/// Set up the two mock GitHub endpoints (token exchange + user info).
fn mock_github_endpoints(server: &MockServer, login: &str, id: i64) {
    server.mock(|when, then| {
        when.method(Method::POST).path("/login/oauth/access_token");
        then.status(200)
            .header("Content-Type", "application/json")
            .body(json!({"access_token": "gho_mock_token"}).to_string());
    });
    server.mock(|when, then| {
        when.method(Method::GET).path("/user");
        then.status(200)
            .header("Content-Type", "application/json")
            .body(json!({"login": login, "id": id}).to_string());
    });
}

#[tokio::test]
async fn login_returns_503_when_oauth_not_configured() {
    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = AppState::new(storage).with_oauth_config(None);
    let router = app(state);

    let (status, _, _) = do_get_full(router, "/auth/github/login", &[]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn login_returns_302_redirect_when_configured() {
    let mock = MockServer::start();
    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = oauth_app(storage, &mock);
    let router = app(state);

    let (status, _, headers) = do_get_full(router, "/auth/github/login", &[]).await;
    assert_eq!(status, StatusCode::FOUND);
    let location = headers
        .get("location")
        .expect("Location header")
        .to_str()
        .expect("ascii");
    assert!(location.contains("client_id=test-client-id"));
    assert!(location.contains("scope=read"));
}

#[tokio::test]
async fn callback_creates_session_via_mock_github() {
    let mock = MockServer::start();
    mock_github_endpoints(&mock, "octocat", 12345);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = oauth_app(storage, &mock);
    let router = app(state);

    let (status, body, headers) =
        do_get_full(router, "/auth/github/callback?code=mock_auth_code", &[]).await;
    assert_eq!(status, StatusCode::OK, "callback should succeed");

    let parsed: Value = serde_json::from_slice(&body).expect("valid JSON");
    assert_eq!(parsed["github_login"], "octocat");
    assert!(parsed["session_token"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));

    let cookie = headers
        .get("set-cookie")
        .expect("Set-Cookie header")
        .to_str()
        .expect("ascii");
    assert!(cookie.contains("buff_session="));
}

#[tokio::test]
async fn whoami_returns_user_after_login() {
    let mock = MockServer::start();
    mock_github_endpoints(&mock, "devuser", 999);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = oauth_app(storage, &mock);
    let router = app(state);

    let (_, body, _) = do_get_full(router.clone(), "/auth/github/callback?code=code1", &[]).await;
    let parsed: Value = serde_json::from_slice(&body).expect("JSON");
    let session = parsed["session_token"]
        .as_str()
        .expect("session token")
        .to_string();

    let auth_header = format!("Bearer {session}");
    let (status, body, _) =
        do_get_full(router, "/auth/whoami", &[("authorization", &auth_header)]).await;
    assert_eq!(status, StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(parsed["github_login"], "devuser");
    assert_eq!(parsed["github_id"], 999);
}

#[tokio::test]
async fn whoami_returns_401_without_session() {
    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = AppState::new(storage).with_oauth_config(None);
    let router = app(state);

    let (status, _, _) = do_get_full(router, "/auth/whoami", &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_clears_session() {
    let mock = MockServer::start();
    mock_github_endpoints(&mock, "logoutuser", 42);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = oauth_app(storage, &mock);
    let router = app(state);

    let (_, body, _) = do_get_full(router.clone(), "/auth/github/callback?code=c", &[]).await;
    let parsed: Value = serde_json::from_slice(&body).expect("JSON");
    let session = parsed["session_token"]
        .as_str()
        .expect("session")
        .to_string();

    let auth_header = format!("Bearer {session}");
    let request = Request::builder()
        .method("POST")
        .uri("/auth/logout")
        .header("authorization", &auth_header)
        .body(Body::empty())
        .expect("build");
    let response = router.clone().oneshot(request).await.expect("oneshot");
    assert_eq!(response.status(), StatusCode::OK);

    let (status, _, _) =
        do_get_full(router, "/auth/whoami", &[("authorization", &auth_header)]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn publish_accepts_session_token() {
    let mock = MockServer::start();
    mock_github_endpoints(&mock, "publisher", 777);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = oauth_app(storage, &mock);
    let router = app(state);

    let (_, body, _) = do_get_full(router.clone(), "/auth/github/callback?code=c", &[]).await;
    let parsed: Value = serde_json::from_slice(&body).expect("JSON");
    let session = parsed["session_token"]
        .as_str()
        .expect("session")
        .to_string();

    let payload = json!({
        "name": "oauth-pkg",
        "version": "1.0.0",
        "deps": [],
        "tarball_b64": "",
    });
    let auth_header = format!("Bearer {session}");
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/publish")
        .header("content-type", "application/json")
        .header("authorization", &auth_header)
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build");
    let response = router.oneshot(request).await.expect("oneshot");
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "publish with session token should succeed"
    );
}
