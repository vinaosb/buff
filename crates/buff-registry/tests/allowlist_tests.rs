//! T57 invite-only beta allowlist integration tests.
//!
//! Tests that the OAuth callback rejects non-allowlisted GitHub users
//! with 403 Forbidden, and accepts allowlisted users.
//!
//! Uses `httpmock` to mock the GitHub API (no real GitHub calls).

#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use buff_registry::{app, AppState, OAuthConfig, SqliteStorage, Storage};
use httpmock::{Method, MockServer};
use serde_json::{json, Value};
use tower::ServiceExt;

fn oauth_app_with_allowlist(storage: Arc<dyn Storage>, mock_server: &MockServer) -> AppState {
    let config = OAuthConfig {
        client_id: "test-client-id".to_string(),
        client_secret: "test-secret".to_string(),
        redirect_uri: "http://localhost:7878/auth/github/callback".to_string(),
        authorize_url: format!("{}/login/oauth/authorize", mock_server.base_url()),
        token_url: format!("{}/login/oauth/access_token", mock_server.base_url()),
        user_url: format!("{}/user", mock_server.base_url()),
    };
    // Allowlist is ENABLED (the default — AppState::new reads the env var,
    // but we set it explicitly for clarity).
    AppState::new(storage)
        .with_oauth_config(Some(config))
        .with_allowlist_enabled(true)
}

fn mock_github(server: &MockServer, login: &str, id: i64) {
    server.mock(|when, then| {
        when.method(Method::POST).path("/login/oauth/access_token");
        then.status(200)
            .header("Content-Type", "application/json")
            .body(json!({"access_token": "gho_tok"}).to_string());
    });
    server.mock(|when, then| {
        when.method(Method::GET).path("/user");
        then.status(200)
            .header("Content-Type", "application/json")
            .body(json!({"login": login, "id": id}).to_string());
    });
}

async fn do_callback(router: axum::Router) -> (StatusCode, Value, axum::http::HeaderMap) {
    // P0.25 (sec-003): callback validates `?state=` against the
    // `buff_oauth_state` cookie. Tests send matching values.
    let request = Request::builder()
        .method("GET")
        .uri("/auth/github/callback?code=mock&state=test-csrf-state-123")
        .header("cookie", "buff_oauth_state=test-csrf-state-123")
        .body(Body::empty())
        .expect("build");
    let response = router.oneshot(request).await.expect("oneshot");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("collect");
    let json: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json, headers)
}

/// Extract `buff_session=<token>` value from a `set-cookie` header
/// (sec-004: token no longer echoed in JSON body).
fn session_from_set_cookie(headers: &axum::http::HeaderMap) -> String {
    let cookie = headers
        .get("set-cookie")
        .expect("Set-Cookie header")
        .to_str()
        .expect("ascii");
    let prefix = "buff_session=";
    let start = cookie.find(prefix).expect("buff_session cookie") + prefix.len();
    let rest = &cookie[start..];
    let end = rest.find(';').unwrap_or(rest.len());
    rest[..end].to_string()
}

#[tokio::test]
async fn allowlisted_user_can_login() {
    let mock = MockServer::start();
    mock_github(&mock, "vipuser", 1001);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    storage.add_to_allowlist("vipuser").expect("allowlist");
    let state = oauth_app_with_allowlist(storage, &mock);
    let router = app(state);

    let (status, body, _) = do_callback(router).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "allowlisted user should log in: {body}"
    );
    assert_eq!(body["github_login"], "vipuser");
}

#[tokio::test]
async fn non_allowlisted_user_gets_403() {
    let mock = MockServer::start();
    mock_github(&mock, "randomuser", 2002);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    // NOT added to allowlist.
    let state = oauth_app_with_allowlist(storage, &mock);
    let router = app(state);

    let (status, body, _) = do_callback(router).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "non-allowlisted should get 403: {body}"
    );
    let err = body["error"].as_str().unwrap_or_default();
    assert!(
        err.contains("invite-only beta"),
        "error should mention invite-only beta: {body}"
    );
}

#[tokio::test]
async fn allowlist_disabled_allows_anyone() {
    let mock = MockServer::start();
    mock_github(&mock, "anybody", 3003);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let config = OAuthConfig {
        client_id: "cid".to_string(),
        client_secret: "cs".to_string(),
        redirect_uri: "http://localhost:7878/auth/github/callback".to_string(),
        authorize_url: format!("{}/login/oauth/authorize", mock.base_url()),
        token_url: format!("{}/login/oauth/access_token", mock.base_url()),
        user_url: format!("{}/user", mock.base_url()),
    };
    let state = AppState::new(storage)
        .with_oauth_config(Some(config))
        .with_allowlist_enabled(false);
    let router = app(state);

    let (status, _, _) = do_callback(router).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "allowlist disabled = open registration"
    );
}

#[tokio::test]
async fn allowlist_is_case_sensitive() {
    let mock = MockServer::start();
    mock_github(&mock, "TestUser", 4004);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    storage.add_to_allowlist("testuser").expect("add lowercase");
    let state = oauth_app_with_allowlist(storage, &mock);
    let router = app(state);

    let (status, _, _) = do_callback(router).await;
    // GitHub logins are case-insensitive but we store them case-sensitively.
    // "TestUser" != "testuser" → 403. This is a known limitation documented
    // in the allowlist docs. Production should normalize to lowercase.
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "case mismatch = not allowlisted"
    );
}

#[tokio::test]
async fn allowlisted_user_can_publish_after_login() {
    let mock = MockServer::start();
    mock_github(&mock, "dev", 5005);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    storage.add_to_allowlist("dev").expect("allowlist");
    let state = oauth_app_with_allowlist(storage, &mock);
    let router = app(state);

    // Login. sec-004: session token now comes from Set-Cookie header.
    let (_, _, headers) = do_callback(router.clone()).await;
    let session = session_from_set_cookie(&headers);

    // Publish using the session token.
    let payload = json!({
        "name": "beta-pkg",
        "version": "1.0.0",
        "deps": [],
        "tarball_b64": "",
    });
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/publish")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {session}"))
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build");
    let response = router.oneshot(request).await.expect("oneshot");
    assert_eq!(response.status(), StatusCode::CREATED);
}
