//! HTTP routing integration tests for the `buff-web` crate.
//!
//! Drives the `axum::Router` produced by [`buff_web::Web::build_router`]
//! in-process via `tower::ServiceExt::oneshot` — NO TCP port
//! allocation, NO subprocess (mirrors the buff-registry T126 test
//! pattern). Each test builds a fresh `Web`, registers routes +
//! middleware, and exercises the full HTTP path:
//!
//! ```text
//!   axum::Request ─▶ Web.build_router() ─▶ Request::from_axum()
//!                                              │
//!                                              ▼
//!                                       middleware chain
//!                                              │
//!                                              ▼
//!                                          user handler
//!                                              │
//!                                              ▼
//!                                       Response.into_axum_response()
//!                                              │
//!                                              ▼
//!                                          axum::Response
//! ```

use buff_web::{Request, Response, Web};
use std::sync::Arc;
use tower::ServiceExt;

fn make_request(method: &str, uri: &str, body: &str) -> axum::extract::Request {
    axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .body(axum::body::Body::from(body.to_string()))
        .expect("build test request")
}

async fn send(app: Web, req: axum::extract::Request) -> axum::response::Response {
    app.build_router()
        .oneshot(req)
        .await
        .expect("router oneshot should not error")
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 16 * 1024 * 1024)
        .await
        .expect("read body bytes");
    String::from_utf8(bytes.to_vec()).expect("utf8 body")
}

#[tokio::test]
async fn get_text_route_returns_body() {
    let mut app = Web::new();
    app.get("/", Arc::new(|_req| Response::text("hello")))
        .expect("get /");
    let resp = send(app, make_request("GET", "/", "")).await;
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = body_string(resp).await;
    assert_eq!(body, "hello");
}

#[tokio::test]
async fn post_json_route_round_trips_payload() {
    let mut app = Web::new();
    app.post(
        "/echo",
        Arc::new(|req: Request| {
            let value = req
                .json()
                .unwrap_or_else(|_| serde_json::json!({"error": "bad json"}));
            Response::json(&value)
        }),
    )
    .expect("post /echo");
    let resp = send(app, make_request("POST", "/echo", r#"{"name":"buff"}"#)).await;
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = body_string(resp).await;
    assert_eq!(body, r#"{"name":"buff"}"#);
}

#[tokio::test]
async fn get_path_param_route_dispatches() {
    let mut app = Web::new();
    app.get(
        "/users/{id}",
        Arc::new(|req: Request| {
            let path = req.path();
            let id = path.trim_start_matches("/users/");
            Response::text(&format!("user-{id}"))
        }),
    )
    .expect("get /users/{id}");
    let resp = send(app, make_request("GET", "/users/42", "")).await;
    let body = body_string(resp).await;
    assert_eq!(body, "user-42");
}

#[tokio::test]
async fn put_route_returns_200_with_payload() {
    let mut app = Web::new();
    app.put(
        "/items/{id}",
        Arc::new(|req: Request| {
            let body = req.body().unwrap_or_default();
            Response::text(&body)
        }),
    )
    .expect("put /items/{id}");
    let resp = send(app, make_request("PUT", "/items/7", "updated")).await;
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = body_string(resp).await;
    assert_eq!(body, "updated");
}

#[tokio::test]
async fn delete_route_returns_text() {
    let mut app = Web::new();
    app.delete("/items/{id}", Arc::new(|_req| Response::text("deleted")))
        .expect("delete /items/{id}");
    let resp = send(app, make_request("DELETE", "/items/3", "")).await;
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = body_string(resp).await;
    assert_eq!(body, "deleted");
}

#[tokio::test]
async fn patch_route_dispatches() {
    let mut app = Web::new();
    app.patch(
        "/items/{id}",
        Arc::new(|_req| {
            let mut r = Response::text("patched");
            r.status(200);
            r
        }),
    )
    .expect("patch /items/{id}");
    let resp = send(app, make_request("PATCH", "/items/9", "")).await;
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = body_string(resp).await;
    assert_eq!(body, "patched");
}

#[tokio::test]
async fn method_dispatch_rejects_wrong_method() {
    let mut app = Web::new();
    app.get("/only-get", Arc::new(|_req| Response::text("ok")))
        .expect("get /only-get");
    let resp = send(app, make_request("POST", "/only-get", "")).await;
    assert_eq!(resp.status(), axum::http::StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn middleware_short_circuits_with_its_own_response() {
    let mut app = Web::new();
    let mw: buff_web::MiddlewareFn = Arc::new(|_req, _next| {
        let mut r = Response::text("blocked");
        r.status(403);
        r
    });
    app.middleware(mw);
    app.get("/open", Arc::new(|_req| Response::text("never reached")))
        .expect("get /open");
    let resp = send(app, make_request("GET", "/open", "")).await;
    assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
    let body = body_string(resp).await;
    assert_eq!(body, "blocked");
}

#[tokio::test]
async fn middleware_delegates_to_handler_when_next_called() {
    let mut app = Web::new();
    let mw: buff_web::MiddlewareFn = Arc::new(|req, next| next(req));
    app.middleware(mw);
    app.get(
        "/passthrough",
        Arc::new(|_req| Response::text("from-handler")),
    )
    .expect("get /passthrough");
    let resp = send(app, make_request("GET", "/passthrough", "")).await;
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = body_string(resp).await;
    assert_eq!(body, "from-handler");
}

#[tokio::test]
async fn middleware_chain_executes_in_registration_order() {
    let mut app = Web::new();
    let mw1: buff_web::MiddlewareFn = Arc::new(|req, next| {
        let resp = next(req);
        let body = std::str::from_utf8(resp.body_bytes())
            .unwrap_or("")
            .to_string();
        let mut r = Response::text(&format!("[mw1:{body}]"));
        r.status(resp.status_code());
        r
    });
    let mw2: buff_web::MiddlewareFn = Arc::new(|req, next| {
        let resp = next(req);
        let body = std::str::from_utf8(resp.body_bytes())
            .unwrap_or("")
            .to_string();
        let mut r = Response::text(&format!("[mw2:{body}]"));
        r.status(resp.status_code());
        r
    });
    app.middleware(mw1);
    app.middleware(mw2);
    app.get("/chain", Arc::new(|_req| Response::text("handler")))
        .expect("get /chain");
    let resp = send(app, make_request("GET", "/chain", "")).await;
    let body = body_string(resp).await;
    assert_eq!(body, "[mw1:[mw2:handler]]");
}

#[tokio::test]
async fn request_header_passed_through_to_handler() {
    let mut app = Web::new();
    app.get(
        "/auth",
        Arc::new(|req: Request| match req.header("authorization") {
            Some(token) if token == "Bearer secret" => Response::text("ok"),
            _ => {
                let mut r = Response::text("unauthorized");
                r.status(401);
                r
            }
        }),
    )
    .expect("get /auth");
    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/auth")
        .header("authorization", "Bearer secret")
        .body(axum::body::Body::empty())
        .expect("build authed request");
    let resp = send(app, req).await;
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = body_string(resp).await;
    assert_eq!(body, "ok");
}

#[tokio::test]
async fn response_json_handler_returns_application_json_content_type() {
    let mut app = Web::new();
    app.get(
        "/api/status",
        Arc::new(|_req| Response::json(&serde_json::json!({"status": "ok"}))),
    )
    .expect("get /api/status");
    let resp = send(app, make_request("GET", "/api/status", "")).await;
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.starts_with("application/json"));
    let body = body_string(resp).await;
    assert_eq!(body, r#"{"status":"ok"}"#);
}
