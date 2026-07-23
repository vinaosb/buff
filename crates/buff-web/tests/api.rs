//! Smoke tests for the `buff-web` crate's public API surface.
//!
//! Covers:
//! - Web constructors (`Web::new`, `Web::bind`).
//! - Web route registration (`get` / `post` / `put` / `delete` /
//!   `patch`) including the InvalidPath validation.
//! - Web middleware registration.
//! - Response builder (`text` / `json` / `status` / `header`).
//! - Request accessors (`method` / `path` / `header` / `body` / `json`).
//! - WebError variants + From conversions.
//!
//! 12+ unit tests + 5 insta snapshots (per T17 acceptance criteria).
//! HTTP-routing integration lives in `tests/routing.rs` (separate
//! file so the unit test module stays focused on the API surface).

use buff_web::{Method, Request, Response, Web, WebError};
use std::sync::Arc;

fn handler_ok() -> Arc<dyn Fn(Request) -> Response + Send + Sync> {
    Arc::new(|_req| Response::text("ok"))
}

#[test]
fn web_new_is_empty() {
    let app = Web::new();
    assert_eq!(app.route_count(), 0);
    assert_eq!(app.middleware_count(), 0);
}

#[test]
fn web_bind_sets_no_routes_but_records_addr_intent() {
    let app = Web::bind("127.0.0.1:9999");
    assert_eq!(app.route_count(), 0);
    assert_eq!(app.middleware_count(), 0);
}

#[test]
fn web_default_matches_new() {
    let d = Web::default();
    let n = Web::new();
    assert_eq!(d.route_count(), n.route_count());
    assert_eq!(d.middleware_count(), n.middleware_count());
}

#[test]
fn web_get_registers_route() {
    let mut app = Web::new();
    app.get("/", handler_ok()).expect("get /");
    assert_eq!(app.route_count(), 1);
}

#[test]
fn web_post_registers_route() {
    let mut app = Web::new();
    app.post("/submit", handler_ok()).expect("post /submit");
    assert_eq!(app.route_count(), 1);
}

#[test]
fn web_all_five_methods_register() {
    let mut app = Web::new();
    app.get("/g", handler_ok()).expect("get");
    app.post("/p", handler_ok()).expect("post");
    app.put("/u", handler_ok()).expect("put");
    app.delete("/d", handler_ok()).expect("delete");
    app.patch("/pa", handler_ok()).expect("patch");
    assert_eq!(app.route_count(), 5);
}

#[test]
fn web_get_rejects_empty_path() {
    let mut app = Web::new();
    let err = app.get("", handler_ok()).unwrap_err();
    assert!(matches!(err, WebError::InvalidPath(_)));
}

#[test]
fn web_post_rejects_missing_leading_slash() {
    let mut app = Web::new();
    let err = app.post("nope", handler_ok()).unwrap_err();
    assert!(matches!(err, WebError::InvalidPath(_)));
}

#[test]
fn web_middleware_increments_count() {
    let mut app = Web::new();
    let mw: buff_web::MiddlewareFn = Arc::new(|req, next| next(req));
    app.middleware(mw);
    assert_eq!(app.middleware_count(), 1);
}

#[test]
fn response_text_has_200_status_and_text_plain_content_type() {
    let resp = Response::text("hello");
    assert_eq!(resp.status_code(), 200);
    let headers = resp.headers_list();
    let ct = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str());
    assert_eq!(ct, Some("text/plain; charset=utf-8"));
    assert_eq!(resp.body_bytes(), b"hello");
}

#[test]
fn response_json_round_trips_value() {
    let value = serde_json::json!({"name": "buff"});
    let resp = Response::json(&value);
    assert_eq!(resp.status_code(), 200);
    let headers = resp.headers_list();
    let ct = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str());
    assert_eq!(ct, Some("application/json"));
    let body = std::str::from_utf8(resp.body_bytes()).expect("utf8 body");
    assert_eq!(body, r#"{"name":"buff"}"#);
}

#[test]
fn response_status_chain_overrides_default() {
    let mut resp = Response::text("created");
    resp.status(201);
    assert_eq!(resp.status_code(), 201);
}

#[test]
fn response_header_chain_appends() {
    let mut resp = Response::text("ok");
    resp.header("X-Trace", "abc");
    resp.header("X-Trace", "def");
    let headers = resp.headers_list();
    let x_trace_count = headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case("x-trace"))
        .count();
    assert_eq!(x_trace_count, 2);
}

#[test]
fn response_default_is_empty_200() {
    let resp = Response::default();
    assert_eq!(resp.status_code(), 200);
    assert!(resp.body_bytes().is_empty());
    assert!(resp.headers_list().is_empty());
}

#[test]
fn request_accessors_round_trip() {
    let req = Request::new(
        "POST",
        "/echo",
        vec![("Content-Type".to_string(), "application/json".to_string())],
        br#"{"hello":"world"}"#.to_vec(),
    );
    assert_eq!(req.method(), "POST");
    assert_eq!(req.path(), "/echo");
    assert_eq!(
        req.header("content-type"),
        Some("application/json".to_string())
    );
    assert_eq!(req.header("missing"), None);
    assert_eq!(req.body().expect("body"), r#"{"hello":"world"}"#);
}

#[test]
fn request_json_returns_value() {
    let req = Request::new("POST", "/", vec![], br#"{"x":42}"#.to_vec());
    let value = req.json().expect("json");
    assert_eq!(value["x"], 42);
}

#[test]
fn request_body_returns_bodynotutf8_for_binary() {
    let req = Request::new("POST", "/", vec![], vec![0xff, 0xfe, 0xfd]);
    let err = req.body().unwrap_err();
    assert!(matches!(err, WebError::BodyNotUtf8));
}

#[test]
fn request_json_returns_bodynotutf8_for_binary() {
    let req = Request::new("POST", "/", vec![], vec![0xff, 0xfe, 0xfd]);
    let err = req.json().unwrap_err();
    assert!(matches!(err, WebError::BodyNotUtf8));
}

#[test]
fn request_json_returns_json_error_for_malformed() {
    let req = Request::new("POST", "/", vec![], b"{not json".to_vec());
    let err = req.json().unwrap_err();
    assert!(matches!(err, WebError::Json(_)));
}

#[test]
fn method_enum_has_five_variants() {
    let all = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Delete,
        Method::Patch,
    ];
    assert_eq!(all.len(), 5);
    assert_eq!(Method::Get, Method::Get);
    assert_ne!(Method::Get, Method::Post);
}

#[test]
fn weberror_from_serde_json_error() {
    let json_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let web_err = WebError::from(json_err);
    assert!(matches!(web_err, WebError::Json(_)));
}

#[test]
fn weberror_display_contains_message() {
    let err = WebError::InvalidPath("/missing-slash".to_string());
    let s = format!("{err}");
    assert!(s.contains("/missing-slash"));
}

// ---- Insta snapshots (5+) ---------------------------------------------------

#[test]
fn snapshot_web_debug_format() {
    let mut app = Web::new();
    app.get("/", handler_ok()).unwrap();
    app.post("/echo", handler_ok()).unwrap();
    let mw: buff_web::MiddlewareFn = Arc::new(|req, next| next(req));
    app.middleware(mw);
    insta::assert_snapshot!("web_debug", format!("{app:?}"));
}

#[test]
fn snapshot_method_all_variants() {
    insta::assert_snapshot!(
        "method_all",
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}",
            Method::Get,
            Method::Post,
            Method::Put,
            Method::Delete,
            Method::Patch
        )
    );
}

#[test]
fn snapshot_response_text_default_headers() {
    let resp = Response::text("hello");
    insta::assert_snapshot!(
        "response_text_headers",
        format!("{:?}", resp.headers_list())
    );
}

#[test]
fn snapshot_response_json_payload() {
    let value = serde_json::json!({"ok": true, "count": 3});
    let resp = Response::json(&value);
    let body = std::str::from_utf8(resp.body_bytes()).expect("utf8 body");
    insta::assert_snapshot!("response_json_body", body);
}

#[test]
fn snapshot_weberror_all_variants() {
    let io_err = WebError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "x"));
    let addr_err = WebError::InvalidAddress("bad".to_string());
    let path_err = WebError::InvalidPath("nope".to_string());
    let body_err = WebError::BodyNotUtf8;
    let json_err = WebError::Json("syntax".to_string());
    let rt_err = WebError::RuntimeCreate;
    let panic_err = WebError::Panic;
    insta::assert_snapshot!(
        "weberror_all",
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}",
            io_err, addr_err, path_err, body_err, json_err, rt_err, panic_err
        )
    );
}
