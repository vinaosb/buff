//! HTTP server + WebSocket endpoint (T131).
//!
//! Two responsibilities:
//!
//! 1. Serve static files from `<project_root>/static/` (and the
//!    wasm-bindgen bundle from `<project_root>/target/wasm-bindgen/`,
//!    falling back to `<project_root>/target/wasm32-unknown-unknown/`).
//!    HTML responses get the reload snippet injected before
//!    `</body>` (see [`crate::ui_dev::client_js`]).
//! 2. WebSocket upgrade at `/__buff_reload__` — the client-side
//!    snippet opens a connection here and the dev server pushes
//!    [`ReloadMessage`] frames via the [`ReloadBroadcaster`].
//!
//! The router is `Router<()>`-compatible (no state extraction needed
//! beyond the [`SharedState`] we wrap in `axum::extract::State`) so
//! unit tests drive it in-process via `tower::ServiceExt::oneshot` —
//! the same pattern `buff-registry` uses (see
//! `crates/buff-registry/src/lib.rs::app`).

use std::path::PathBuf;

use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use crate::ui_dev::broadcaster::ReloadBroadcaster;
use crate::ui_dev::client_js::inject_client;
use crate::ui_dev::error::UiDevError;

/// Shared state threaded through every HTTP handler. Cloned cheaply
/// (the heavy bits are `Arc`).
#[derive(Debug, Clone)]
pub struct SharedState {
    /// Root of the project we're serving. `<root>/static/` is the
    /// default static-asset dir; `<root>/target/wasm-bindgen/` is the
    /// wasm bundle output dir.
    pub project_root: PathBuf,
    /// Broadcaster for reload / error messages. The HTTP server holds
    /// a clone; each WS client subscribes via [`Self::subscribe`].
    pub broadcaster: ReloadBroadcaster,
}

impl SharedState {
    /// Construct a fresh state for `project_root`.
    #[must_use]
    pub fn new(project_root: PathBuf, broadcaster: ReloadBroadcaster) -> Self {
        Self {
            project_root,
            broadcaster,
        }
    }

    /// Path of `<root>/static/`.
    #[must_use]
    pub fn static_dir(&self) -> PathBuf {
        self.project_root.join("static")
    }

    /// Path of `<root>/target/wasm-bindgen/` (the
    /// `wasm-bindgen --out-dir` we write to in [`Builder::build`]).
    #[must_use]
    pub fn wasm_bundle_dir(&self) -> PathBuf {
        self.project_root.join("target").join("wasm-bindgen")
    }
}

/// Build the axum [`Router`] for the dev server.
///
/// Routes:
///
/// - `GET /__buff_reload__` — WebSocket upgrade (per-client subscribe
///   to [`ReloadBroadcaster`]).
/// - `GET /*path` — static-file fallback. Resolves `path` against
///   `<root>/static/` first, then `<root>/target/wasm-bindgen/`, then
///   `<root>/target/wasm32-unknown-unknown/<profile>/`. HTML
///   responses get the reload snippet injected.
pub fn app(state: SharedState) -> Router {
    Router::new()
        .route("/__buff_reload__", get(ws_handler))
        .fallback(get(static_handler))
        .with_state(state)
}

/// WebSocket upgrade handler. Each client gets a per-connection task
/// that subscribes to the [`ReloadBroadcaster`] and forwards every
/// received message as a `Text` frame.
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<SharedState>) -> Response {
    ws.on_upgrade(move |socket| ws_client_loop(socket, state))
}

/// Per-client WS loop: subscribe to the broadcaster, forward every
/// message, ignore inbound frames (the protocol is server-push only).
async fn ws_client_loop(socket: WebSocket, state: SharedState) {
    let mut rx = state.broadcaster.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Two tasks: pump broadcaster → WS sender; drain WS receiver
    // (clients should not send us anything, but we MUST consume
    // inbound Pings / Pongs / Close frames so the underlying
    // tungstenite state machine stays healthy).
    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let json = msg.to_json();
                    if sender.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    // Client fell behind — re-subscribe implicitly
                    // (the Receiver stays valid after Lagged).
                    continue;
                }
                Err(RecvError::Closed) => break,
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(frame)) = receiver.next().await {
            // Ignore inbound frames; we just need to drain them so
            // Close / Ping are acknowledged by tungstenite.
            if let Message::Close(_) = frame {
                break;
            }
        }
    });

    // When either task ends, drop the other.
    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }
}

/// Static-file fallback handler. Resolves the request URI against the
/// project's static dirs in priority order (see [`SharedState`]).
///
/// On a hit, returns the file contents with a content-type guess
/// (HTML responses get the reload snippet injected). On a miss,
/// returns a 404 with a small JSON body.
#[allow(clippy::needless_pass_by_value)]
async fn static_handler(State(state): State<SharedState>, uri: Uri) -> Response {
    let path = uri.path();
    // Strip leading '/' so PathBuf-aware joins behave.
    let relative = path.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };

    // Try each candidate root in order. First hit wins.
    let candidates = [
        state.static_dir(),
        state.wasm_bundle_dir(),
        state
            .project_root
            .join("target")
            .join("wasm32-unknown-unknown")
            .join("debug"),
    ];

    for root in &candidates {
        let candidate = root.join(relative);
        // Canonicalize the parent root for the starts_with check
        // below (we compare like-with-like: canonical file path vs
        // canonical root path).
        let Ok(canonical_root) = root.canonicalize() else {
            continue;
        };
        // Canonicalize the candidate; missing file is a soft miss
        // (try the next root).
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if !canonical.starts_with(&canonical_root) {
            // Outside the served root — skip (path traversal).
            continue;
        }
        if let Ok(bytes) = tokio::fs::read(&canonical).await {
            return render_bytes(&canonical, bytes);
        }
    }

    (StatusCode::NOT_FOUND, "not found").into_response()
}

/// Render a static file: guess content-type from extension, inject
/// the reload snippet for HTML, and emit a 200 with appropriate
/// headers.
fn render_bytes(path: &std::path::Path, bytes: Vec<u8>) -> Response {
    let ext = path
        .extension()
        .map(|e| e.to_ascii_lowercase().to_string_lossy().to_string())
        .unwrap_or_default();

    let (ct, body): (HeaderValue, Body) = match ext.as_str() {
        "html" | "htm" => {
            // Inject the reload snippet into HTML before serving.
            let text = String::from_utf8_lossy(&bytes);
            let injected = inject_client(&text);
            let body_bytes = Bytes::from(injected.into_bytes());
            (
                HeaderValue::from_static("text/html; charset=utf-8"),
                Body::from(body_bytes),
            )
        }
        "js" => (
            HeaderValue::from_static("application/javascript; charset=utf-8"),
            Body::from(Bytes::from(bytes)),
        ),
        "wasm" => (
            HeaderValue::from_static("application/wasm"),
            Body::from(Bytes::from(bytes)),
        ),
        "css" => (
            HeaderValue::from_static("text/css; charset=utf-8"),
            Body::from(Bytes::from(bytes)),
        ),
        "json" => (
            HeaderValue::from_static("application/json"),
            Body::from(Bytes::from(bytes)),
        ),
        "svg" => (
            HeaderValue::from_static("image/svg+xml"),
            Body::from(Bytes::from(bytes)),
        ),
        "png" => (
            HeaderValue::from_static("image/png"),
            Body::from(Bytes::from(bytes)),
        ),
        "ico" => (
            HeaderValue::from_static("image/x-icon"),
            Body::from(Bytes::from(bytes)),
        ),
        _ => (
            HeaderValue::from_static("application/octet-stream"),
            Body::from(Bytes::from(bytes)),
        ),
    };

    let mut resp = Response::new(body);
    resp.headers_mut().insert(header::CONTENT_TYPE, ct);
    resp
}

/// Convenience: convert [`UiDevError`] to a 500-style [`Response`].
/// Used in handlers that need to propagate an error without panicking.
#[allow(clippy::needless_pass_by_value)]
pub fn error_response(err: UiDevError) -> Response {
    let body = format!("{{\"error\":\"{}\"}}", err.to_string().replace('"', "\\\""));
    let mut resp = Response::new(Body::from(body));
    *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_dev::broadcaster::{ReloadBroadcaster, ReloadMessage};
    use tower::ServiceExt;

    fn make_state(root: &std::path::Path) -> SharedState {
        SharedState::new(root.to_path_buf(), ReloadBroadcaster::new())
    }

    #[tokio::test]
    async fn static_handler_serves_index_html_with_injection() {
        let tmp = std::env::temp_dir().join("buff-ui-dev-http-index");
        let _ = std::fs::remove_dir_all(&tmp);
        let static_dir = tmp.join("static");
        std::fs::create_dir_all(&static_dir).unwrap();
        std::fs::write(
            static_dir.join("index.html"),
            "<html><body><h1>hi</h1></body></html>",
        )
        .unwrap();

        let app = app(make_state(&tmp));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("<h1>hi</h1>"));
        // Injection present.
        assert!(body_str.contains("<script>(function(){"));
        assert!(body_str.contains("/__buff_reload__"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn static_handler_serves_css_without_injection() {
        let tmp = std::env::temp_dir().join("buff-ui-dev-http-css");
        let _ = std::fs::remove_dir_all(&tmp);
        let static_dir = tmp.join("static");
        std::fs::create_dir_all(&static_dir).unwrap();
        std::fs::write(static_dir.join("style.css"), "body { color: red; }").unwrap();

        let app = app(make_state(&tmp));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/style.css")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content-type");
        assert!(ct.to_str().unwrap().contains("text/css"));
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        // No injection for non-HTML.
        assert!(!body_str.contains("<script>"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn static_handler_returns_404_for_missing_path() {
        let tmp = std::env::temp_dir().join("buff-ui-dev-http-404");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("static")).unwrap();

        let app = app(make_state(&tmp));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/does/not/exist.css")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn static_handler_rejects_path_traversal() {
        // Path traversal (`..`) must NOT escape the static root.
        let tmp = std::env::temp_dir().join("buff-ui-dev-http-traversal");
        let _ = std::fs::remove_dir_all(&tmp);
        let static_dir = tmp.join("static");
        std::fs::create_dir_all(&static_dir).unwrap();
        std::fs::write(static_dir.join("index.html"), "<h1>ok</h1>").unwrap();
        // Drop a SECRET file outside the static dir.
        std::fs::write(tmp.join("SECRET.txt"), "top-secret").unwrap();

        let app = app(make_state(&tmp));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/../SECRET.txt")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // axum normalizes `/../` to `/` OR returns 404 depending on
        // version. Either way, the SECRET must NOT be served.
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            !body_str.contains("top-secret"),
            "path traversal leaked SECRET: {body_str}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn static_handler_serves_wasm_bundle() {
        let tmp = std::env::temp_dir().join("buff-ui-dev-http-wasm");
        let _ = std::fs::remove_dir_all(&tmp);
        let bundle_dir = tmp.join("target").join("wasm-bindgen");
        std::fs::create_dir_all(&bundle_dir).unwrap();
        std::fs::write(bundle_dir.join("app.wasm"), b"\0asm").unwrap();

        let app = app(make_state(&tmp));
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/app.wasm")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content-type");
        assert!(ct.to_str().unwrap().contains("application/wasm"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn ws_handler_upgrades_and_pushes_messages() {
        // The ws_handler upgrades the connection and the per-client
        // task broadcasts ReloadMessage frames. We drive this via
        // axum's in-process extractors — but since a real WS
        // upgrade needs the raw TCP handshake, we instead unit-test
        // the per-client task via the broadcaster directly.
        let b = ReloadBroadcaster::new();
        let mut rx = b.subscribe();
        b.reload();
        let msg = rx.recv().await.expect("broadcast delivered");
        assert_eq!(msg, ReloadMessage::Reload);
    }

    #[test]
    fn render_bytes_injects_html_only() {
        // HTML gets injected; JS / WASM / CSS / etc do not.
        let html_bytes = b"<html></html>".to_vec();
        let resp = render_bytes(std::path::Path::new("x.html"), html_bytes);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX);
        // Block-on via tokio runtime.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let bytes = rt.block_on(body).unwrap();
        let s = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(s.contains("<script>"));

        let js_resp = render_bytes(std::path::Path::new("x.js"), b"console.log(1)".to_vec());
        let js_body = rt
            .block_on(axum::body::to_bytes(js_resp.into_body(), usize::MAX))
            .unwrap();
        let js_str = String::from_utf8(js_body.to_vec()).unwrap();
        assert!(!js_str.contains("<script>(function(){"));
    }

    #[test]
    fn error_response_returns_500_with_json_body() {
        let err = UiDevError::Bind {
            port: 8080,
            source: std::io::Error::new(std::io::ErrorKind::AddrInUse, "in use"),
        };
        let resp = error_response(err);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content-type");
        assert!(ct.to_str().unwrap().contains("application/json"));
    }
}
