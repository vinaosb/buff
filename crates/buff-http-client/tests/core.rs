//! Integration tests for the `buff-http-client` crate.
//!
//! Covers all public API surface:
//! - HttpClient::new, get, post, put, delete
//! - RequestBuilder::header, json, timeout, send
//! - Response::status, text, json, bytes, headers
//! - HttpError variants
//!
//! Tests use httpmock for hermetic HTTP mocking (no real network).

use buff_http_client::{HttpClient, HttpError};

fn make_mock_server() -> httpmock::MockServer {
    httpmock::MockServer::start()
}

#[test]
fn http_client_new_creates_default_client() {
    let client = HttpClient::new();
    // Default client should be usable (no panic on construction)
    let _ = client.get("http://example.com");
}

#[test]
fn http_client_default_trait() {
    let client: HttpClient = Default::default();
    let _ = client.get("http://example.com");
}

#[test]
fn http_client_get_returns_request_builder() {
    let client = HttpClient::new();
    let builder = client.get("http://example.com");
    // RequestBuilder is opaque but Debug-printable
    let _debug = format!("{builder:?}");
}

#[test]
fn http_client_post_returns_request_builder() {
    let client = HttpClient::new();
    let _builder = client.post("http://example.com");
}

#[test]
fn http_client_put_returns_request_builder() {
    let client = HttpClient::new();
    let _builder = client.put("http://example.com");
}

#[test]
fn http_client_delete_returns_request_builder() {
    let client = HttpClient::new();
    let _builder = client.delete("http://example.com");
}

#[test]
fn request_builder_header_chains() {
    let client = HttpClient::new();
    let builder = client
        .get("http://example.com")
        .header("Authorization", "Bearer test")
        .header("Accept", "application/json");
    let _debug = format!("{builder:?}");
}

#[test]
fn request_builder_json_chains() {
    let client = HttpClient::new();
    let builder = client
        .post("http://example.com")
        .json(serde_json::json!({"key": "value"}));
    let _debug = format!("{builder:?}");
}

#[test]
fn request_builder_timeout_chains() {
    let client = HttpClient::new();
    let builder = client.get("http://example.com").timeout(30);
    let _debug = format!("{builder:?}");
}

#[test]
fn response_status_from_mock() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/status-test");
        then.status(201);
    });

    let client = HttpClient::new();
    let resp = client
        .get(&server.url("/status-test"))
        .send()
        .expect("GET should succeed");
    assert_eq!(resp.status(), 201);
    mock.assert();
}

#[test]
fn response_text_from_mock() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/text-test");
        then.status(200).body("hello world");
    });

    let client = HttpClient::new();
    let resp = client
        .get(&server.url("/text-test"))
        .send()
        .expect("GET should succeed");
    assert_eq!(resp.text().expect("text() should succeed"), "hello world");
    mock.assert();
}

#[test]
fn response_json_from_mock() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/json-test");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"name":"Buff","version":1}"#);
    });

    let client = HttpClient::new();
    let resp = client
        .get(&server.url("/json-test"))
        .send()
        .expect("GET should succeed");
    let val = resp.json().expect("json() should succeed");
    assert_eq!(val["name"], "Buff");
    assert_eq!(val["version"], 1);
    mock.assert();
}

#[test]
fn response_bytes_from_mock() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/bytes-test");
        then.status(200).body(b"binary data" as &[u8]);
    });

    let client = HttpClient::new();
    let resp = client
        .get(&server.url("/bytes-test"))
        .send()
        .expect("GET should succeed");
    let bytes = resp.bytes().expect("bytes() should succeed");
    assert_eq!(bytes, b"binary data");
    mock.assert();
}

#[test]
fn response_headers_from_mock() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/headers-test");
        then.status(200)
            .header("content-type", "application/json")
            .header("x-custom", "buff");
    });

    let client = HttpClient::new();
    let resp = client
        .get(&server.url("/headers-test"))
        .send()
        .expect("GET should succeed");
    let headers = resp.headers();
    assert_eq!(
        headers.get("content-type").map(|s| s.as_str()),
        Some("application/json")
    );
    assert_eq!(headers.get("x-custom").map(|s| s.as_str()), Some("buff"));
    mock.assert();
}

#[test]
fn post_with_json_body() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/post-test")
            .header("content-type", "application/json")
            .json_body(serde_json::json!({"key": "value"}));
        then.status(200).body("created");
    });

    let client = HttpClient::new();
    let resp = client
        .post(&server.url("/post-test"))
        .json(serde_json::json!({"key": "value"}))
        .send()
        .expect("POST should succeed");
    assert_eq!(resp.status(), 200);
    mock.assert();
}

#[test]
fn put_with_json_body() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::PUT)
            .path("/put-test")
            .json_body(serde_json::json!({"updated": true}));
        then.status(200).body("ok");
    });

    let client = HttpClient::new();
    let resp = client
        .put(&server.url("/put-test"))
        .json(serde_json::json!({"updated": true}))
        .send()
        .expect("PUT should succeed");
    assert_eq!(resp.status(), 200);
    mock.assert();
}

#[test]
fn delete_request() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::DELETE).path("/delete-test");
        then.status(204);
    });

    let client = HttpClient::new();
    let resp = client
        .delete(&server.url("/delete-test"))
        .send()
        .expect("DELETE should succeed");
    assert_eq!(resp.status(), 204);
    mock.assert();
}

#[test]
fn request_with_custom_header() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/header-test")
            .header("x-api-key", "secret-123");
        then.status(200);
    });

    let client = HttpClient::new();
    let resp = client
        .get(&server.url("/header-test"))
        .header("x-api-key", "secret-123")
        .send()
        .expect("GET with header should succeed");
    assert_eq!(resp.status(), 200);
    mock.assert();
}

#[test]
fn request_with_timeout() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/timeout-test");
        then.status(200);
    });

    let client = HttpClient::new();
    let resp = client
        .get(&server.url("/timeout-test"))
        .timeout(30)
        .send()
        .expect("GET with timeout should succeed");
    assert_eq!(resp.status(), 200);
    mock.assert();
}

#[test]
fn http_error_display() {
    let err = HttpError::Panic;
    let msg = format!("{err}");
    assert!(msg.contains("internal error"));
}

#[test]
fn response_display() {
    let server = make_mock_server();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/display-test");
        then.status(200);
    });

    let client = HttpClient::new();
    let resp = client
        .get(&server.url("/display-test"))
        .send()
        .expect("GET should succeed");
    let display = format!("{resp}");
    assert!(display.contains("200"));
    mock.assert();
}

// ---- Insta snapshots ---------------------------------------------------

#[test]
fn snapshot_http_error_debug() {
    let err1 = HttpError::Panic;
    let err2 = HttpError::Request("connection refused".to_string());
    insta::assert_snapshot!("http_error_debug", format!("{err1}\n{err2}"));
}
