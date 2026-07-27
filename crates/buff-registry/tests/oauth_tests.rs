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

/// Fixed CSRF state token used by all callback tests. The callback
/// handler validates `?state=<STATE>` against the `buff_oauth_state`
/// cookie (P0.25 / sec-003); both must match. Tests send the matching
/// cookie via the `STATE_COOKIE` constant below.
const STATE_COOKIE: &str = "buff_oauth_state=test-csrf-state-123";

/// Extract the `buff_session=<token>` value from a `set-cookie` header.
/// Mirrors the server-side `extract_session_from_cookie` helper. Used
/// because sec-004 removed `session_token` from the JSON response body
/// (the cookie is now the authoritative delivery channel).
fn session_from_set_cookie(headers: &axum::http::HeaderMap) -> String {
    let cookie = headers
        .get("set-cookie")
        .expect("Set-Cookie header")
        .to_str()
        .expect("ascii");
    let prefix = "buff_session=";
    let start = cookie
        .find(prefix)
        .expect("buff_session cookie") as usize
        + prefix.len();
    let rest = &cookie[start..];
    let end = rest.find(';').unwrap_or(rest.len());
    rest[..end].to_string()
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
    // P0.25 (sec-003): login MUST set a CSRF state cookie AND include
    // the matching `state=` query param in the authorize URL.
    let set_cookie = headers
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        set_cookie.contains("buff_oauth_state="),
        "login should set buff_oauth_state cookie, got: {set_cookie}"
    );
    assert!(
        location.contains("state="),
        "authorize URL should include state param, got: {location}"
    );
    // The cookie value and URL state MUST match (both come from the
    // same random draw in `login`).
    let cookie_state = set_cookie
        .split(';')
        .find_map(|p| p.trim().strip_prefix("buff_oauth_state="))
        .unwrap_or("");
    let url_state = location
        .split('&')
        .find_map(|p| p.strip_prefix("state="))
        .unwrap_or("");
    assert!(
        !cookie_state.is_empty() && cookie_state == url_state,
        "cookie state ({cookie_state:?}) must match URL state ({url_state:?})"
    );
}

#[tokio::test]
async fn callback_creates_session_via_mock_github() {
    let mock = MockServer::start();
    mock_github_endpoints(&mock, "octocat", 12345);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = oauth_app(storage, &mock);
    let router = app(state);

    // P0.25 (sec-003): callback requires `?state=` matching the
    // `buff_oauth_state` cookie. We send both via TEST_OAUTH_STATE.
    let (status, body, headers) = do_get_full(
        router,
        "/auth/github/callback?code=mock_auth_code&state=test-csrf-state-123",
        &[("cookie", STATE_COOKIE)],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "callback should succeed");

    let parsed: Value = serde_json::from_slice(&body).expect("valid JSON");
    assert_eq!(parsed["github_login"], "octocat");
    // sec-004: session_token is NOT echoed in the response body.
    assert!(
        parsed.get("session_token").is_none(),
        "session_token must not be echoed in response body (sec-004)"
    );

    let cookie = headers
        .get("set-cookie")
        .expect("Set-Cookie header")
        .to_str()
        .expect("ascii");
    assert!(cookie.contains("buff_session="));
    // sec-004: cookie MUST carry the Secure flag.
    assert!(
        cookie.contains("Secure"),
        "session cookie must set Secure flag (sec-004), got: {cookie}"
    );
    let session = session_from_set_cookie(&headers);
    assert!(!session.is_empty(), "session token from cookie");
}

#[tokio::test]
async fn callback_rejects_missing_state_csrf_defense() {
    // P0.25 (sec-003): a callback with no `state` query param MUST be
    // rejected with OAuthStateMismatch (HTTP 400), regardless of cookie.
    let mock = MockServer::start();
    mock_github_endpoints(&mock, "octocat", 1);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = oauth_app(storage, &mock);
    let router = app(state);

    let (status, _, _) = do_get_full(
        router,
        "/auth/github/callback?code=mock_auth_code",
        &[("cookie", STATE_COOKIE)],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "missing state param must be rejected (CSRF defense)"
    );
}

#[tokio::test]
async fn callback_rejects_state_cookie_mismatch() {
    // P0.25 (sec-003): a callback whose `?state=` does not match the
    // `buff_oauth_state` cookie value MUST be rejected (CSRF defense).
    let mock = MockServer::start();
    mock_github_endpoints(&mock, "octocat", 1);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = oauth_app(storage, &mock);
    let router = app(state);

    let (status, _, _) = do_get_full(
        router,
        "/auth/github/callback?code=mock_auth_code&state=attacker-forged-state",
        &[("cookie", STATE_COOKIE)],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "state mismatch must be rejected (CSRF defense)"
    );
}

#[tokio::test]
async fn callback_rejects_missing_state_cookie() {
    // P0.25 (sec-003): a callback with `?state=` but no `buff_oauth_state`
    // cookie MUST be rejected — the cookie is the proof the login flow
    // originated from us, not an attacker.
    let mock = MockServer::start();
    mock_github_endpoints(&mock, "octocat", 1);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = oauth_app(storage, &mock);
    let router = app(state);

    let (status, _, _) = do_get_full(
        router,
        "/auth/github/callback?code=mock_auth_code&state=test-csrf-state-123",
        &[],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "missing state cookie must be rejected (CSRF defense)"
    );
}

#[tokio::test]
async fn whoami_returns_user_after_login() {
    let mock = MockServer::start();
    mock_github_endpoints(&mock, "devuser", 999);

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().expect("open"));
    let state = oauth_app(storage, &mock);
    let router = app(state);

    let (_, _, headers) = do_get_full(
        router.clone(),
        "/auth/github/callback?code=code1&state=test-csrf-state-123",
        &[("cookie", STATE_COOKIE)],
    )
    .await;
    // sec-004: session token now comes from the Set-Cookie header.
    let session = session_from_set_cookie(&headers);

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

    let (_, _, headers) = do_get_full(
        router.clone(),
        "/auth/github/callback?code=c&state=test-csrf-state-123",
        &[("cookie", STATE_COOKIE)],
    )
    .await;
    let session = session_from_set_cookie(&headers);

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

    let (_, _, headers) = do_get_full(
        router.clone(),
        "/auth/github/callback?code=c&state=test-csrf-state-123",
        &[("cookie", STATE_COOKIE)],
    )
    .await;
    let session = session_from_set_cookie(&headers);

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
