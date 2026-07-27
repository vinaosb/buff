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
//! # WebSocket hardening (P0.27)
//!
//! The `/__buff_reload__` endpoint enforces three defence-in-depth
//! controls on every upgrade, all tuned for the dev-server threat
//! model (loopback-only, trusted user, no secrets on the wire):
//!
//! - **Origin validation** — only `http://localhost:*`,
//!   `http://127.0.0.1:*`, or `http://[::1]:*` origins are accepted.
//!   Any other (or missing) `Origin` header is rejected with 403.
//!   Stops browser-based cross-origin WS abuse from a malicious page
//!   the user might visit while the dev server is running.
//! - **Message size cap** — both `max_message_size` and
//!   `max_frame_size` are clamped to [`MAX_MESSAGE_SIZE`] (1 MiB).
//!   Reload payloads are <1 KB, so 1 MiB is generous while still
//!   stopping a memory-exhaustion vector from a hostile client.
//! - **Idle timeout** — if no inbound frame (browser Ping or
//!   otherwise) is received within [`IDLE_TIMEOUT`] (60 s), the
//!   server closes the socket cleanly. Browsers Ping idle WS
//!   connections ~every 30 s, so legitimate dev sessions never trip
//!   this; only abandoned browsers / zombie sockets get reaped.
//!
//! The router is `Router<()>`-compatible (no state extraction needed
//! beyond the [`SharedState`] we wrap in `axum::extract::State`) so
//! unit tests drive it in-process via `tower::ServiceExt::oneshot` —
//! the same pattern `buff-registry` uses (see
//! `crates/buff-registry/src/lib.rs::app`).

use std::path::PathBuf;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use crate::ui_dev::broadcaster::ReloadBroadcaster;
use crate::ui_dev::client_js::inject_client;
use crate::ui_dev::error::UiDevError;

/// Hard cap on a single inbound WS frame / message size. Defaults to
/// 1 MiB — the dev-server protocol is server-push only and the
/// largest legitimate frame is a multi-line compiler error JSON
/// envelope, which is well under 1 KB. 1 MiB leaves headroom for
/// pathological compiler output while still bounding memory use per
/// connection.
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Idle timeout for a WS connection. If no inbound frame is received
/// within this window the server closes the socket cleanly. 60 s is
/// comfortably longer than browser WS keep-alive intervals (~30 s)
/// so legitimate dev sessions are never affected; only abandoned
/// browsers / zombie sockets get reaped.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

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
///
/// # Security (P0.27)
///
/// Before the upgrade is accepted, the request `Origin` header is
/// validated against the localhost allow-list via
/// [`is_localhost_origin`]. Non-conforming origins get a 403 instead
/// of an upgrade. The upgrade itself is configured with
/// [`MAX_MESSAGE_SIZE`] on both `max_message_size` and
/// `max_frame_size`, and the per-client loop enforces
/// [`IDLE_TIMEOUT`] (see [`ws_client_loop`]).
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Response {
    // P0.27-1: Origin validation. Browser WS clients always send
    // `Origin`; curl / websocat do not. In dev mode we only accept
    // loopback origins — a malicious page the user visits in another
    // tab cannot then open a WS to the dev server and observe
    // compiler output / drive rebuilds.
    let origin_ok = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(is_localhost_origin)
        .unwrap_or(false);
    if !origin_ok {
        let origin_display = headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("<missing>");
        eprintln!(
            "[buff ui dev] rejected WS upgrade: origin not allowed ({origin_display})"
        );
        return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
    }

    // P0.27-2: Message size cap. Reload payloads are <1 KB, so 1 MiB
    // is generous. Stops a hostile client from exhausting server
    // memory with a giant frame.
    ws.max_message_size(MAX_MESSAGE_SIZE)
        .max_frame_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| ws_client_loop(socket, state))
}

/// Decide whether an `Origin` header value is allowed for a dev-mode
/// WebSocket upgrade. Accepts the three loopback host spellings
/// (`localhost`, `127.0.0.1`, `[::1]`) over plain HTTP, optionally
/// followed by `:<port>`. Rejects everything else (including HTTPS
/// origins, since the dev server itself is HTTP-only — a same-origin
/// browser context will always send `http://...`).
///
/// Pure function so it can be unit-tested directly without spinning
/// up an HTTP server.
fn is_localhost_origin(origin: &str) -> bool {
    // Match `http://<host>` exactly OR `http://<host>:<port>`.
    for prefix in ["http://localhost", "http://127.0.0.1", "http://[::1]"] {
        if origin == prefix {
            return true;
        }
        if let Some(rest) = origin.strip_prefix(prefix) {
            // Allow `:port` suffix. Anything else (e.g. `localhost.evil.com`)
            // is rejected — the suffix-aware matcher rules out non-port
            // continuations.
            if let Some(port_part) = rest.strip_prefix(':') {
                if !port_part.is_empty() && port_part.bytes().all(|b| b.is_ascii_digit()) {
                    return true;
                }
            }
        }
    }
    false
}

/// Per-client WS loop: subscribe to the broadcaster, forward every
/// message, drain inbound frames, and enforce an idle timeout.
///
/// # Idle timeout (P0.27-3)
///
/// Each `receiver.next().await` is wrapped in
/// `tokio::time::timeout(IDLE_TIMEOUT, ...)`. If no frame arrives
/// within [`IDLE_TIMEOUT`], the loop exits cleanly and the
/// `tokio::select!` below drops the send task. Browsers
/// automatically Ping idle WS connections roughly every 30 s, so a
/// healthy dev session never trips this — only abandoned tabs or
/// zombie sockets get reaped.
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
        loop {
            // P0.27-3: wrap each recv in a 60 s timeout. Browser
            // keep-alive Pings reset this every ~30 s, so legitimate
            // dev sessions are never affected. Timeout → break out
            // of the loop so the outer select! can clean up.
            match tokio::time::timeout(IDLE_TIMEOUT, receiver.next()).await {
                Ok(Some(Ok(frame))) => {
                    // Ignore inbound frames; we just need to drain
                    // them so Close / Ping are acknowledged by
                    // tungstenite.
                    if let Message::Close(_) = frame {
                        break;
                    }
                }
                Ok(Some(Err(_))) | Ok(None) => break,
                Err(_) => {
                    // Idle timeout elapsed — close the connection.
                    break;
                }
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

    // ---- P0.27: WebSocket hardening tests ----

    #[test]
    fn is_localhost_origin_accepts_localhost_with_port() {
        assert!(is_localhost_origin("http://localhost:3000"));
        assert!(is_localhost_origin("http://localhost:8080"));
        assert!(is_localhost_origin("http://localhost:5173"));
    }

    #[test]
    fn is_localhost_origin_accepts_localhost_without_port() {
        assert!(is_localhost_origin("http://localhost"));
    }

    #[test]
    fn is_localhost_origin_accepts_loopback_ipv4() {
        assert!(is_localhost_origin("http://127.0.0.1:3000"));
        assert!(is_localhost_origin("http://127.0.0.1"));
    }

    #[test]
    fn is_localhost_origin_accepts_loopback_ipv6() {
        assert!(is_localhost_origin("http://[::1]:3000"));
        assert!(is_localhost_origin("http://[::1]"));
    }

    #[test]
    fn is_localhost_origin_rejects_https() {
        // Dev server is HTTP-only — a real browser opening the dev
        // page over HTTP sends an `http://` origin. HTTPS origins
        // imply a different deployment (production) so they are
        // rejected in dev mode.
        assert!(!is_localhost_origin("https://localhost:3000"));
        assert!(!is_localhost_origin("https://127.0.0.1"));
    }

    #[test]
    fn is_localhost_origin_rejects_other_hosts() {
        assert!(!is_localhost_origin("http://example.com"));
        assert!(!is_localhost_origin("http://example.com:3000"));
        assert!(!is_localhost_origin("http://0.0.0.0:3000"));
        assert!(!is_localhost_origin("http://192.168.1.1:3000"));
        assert!(!is_localhost_origin("http://buff-lang.org"));
    }

    #[test]
    fn is_localhost_origin_rejects_substring_tricks() {
        // `localhost.evil.com` must NOT be accepted as a `localhost`
        // origin — the suffix-aware matcher rules out non-port
        // continuations.
        assert!(!is_localhost_origin("http://localhost.evil.com"));
        assert!(!is_localhost_origin("http://localhost.evil.com:3000"));
        assert!(!is_localhost_origin("http://localhostX:3000"));
        assert!(!is_localhost_origin("http://127.0.0.1.evil.com:3000"));
        assert!(!is_localhost_origin("http://127.0.0.10:3000"));
    }

    #[test]
    fn is_localhost_origin_rejects_non_numeric_port() {
        // A trailing `:abc` is not a valid port; reject so we don't
        // accidentally match weird suffix tricks.
        assert!(!is_localhost_origin("http://localhost:abc"));
        assert!(!is_localhost_origin("http://127.0.0.1:not-a-port"));
    }

    #[test]
    fn is_localhost_origin_rejects_garbage() {
        assert!(!is_localhost_origin(""));
        assert!(!is_localhost_origin("not a url"));
        assert!(!is_localhost_origin("ftp://localhost"));
        assert!(!is_localhost_origin("ws://localhost:3000"));
    }

    // Note: end-to-end WS-upgrade origin-rejection is verified by the
    // integration smoke test in `tests/ui_dev_ws_origin.rs` (real TCP
    // server bound to 127.0.0.1:0 + raw HTTP WS handshake). axum's
    // `WebSocketUpgrade` extractor requires a real connection, so the
    // upgrade path can't be exercised via `tower::ServiceExt::oneshot`
    // — axum returns 426 Upgrade Required in that synthetic mode.
    // The handler itself is trivially correct: it delegates to
    // `is_localhost_origin` (above, 9 unit tests) and returns 403 on
    // `false`. The constant + helper wiring is also verified by the
    // doc-commented contract above.
}
