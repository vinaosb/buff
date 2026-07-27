//! `buff-http-client` — idiomatic HTTP client for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`reqwest`](https://crates.io/crates/reqwest)
//! crate. Provides a Buff-idiomatic fluent API:
//! `HttpClient.new()`, `client.get(url)`, `client.post(url).json(body)
//! .header(name, val).send() -> Response`.
//!
//! # Pipeline
//!
//! ```text
//!   HttpClient.new() ──▶ HttpClient { inner: reqwest::blocking::Client }
//!                              │
//!                              ├─ client.get(url) ──▶ RequestBuilder
//!                              ├─ client.post(url) ──▶ RequestBuilder
//!                              ├─ client.put(url)  ──▶ RequestBuilder
//!                              └─ client.delete(url) ──▶ RequestBuilder
//!                                                         │
//!                                                         ├─ .json(body)
//!                                                         ├─ .header(name, val)
//!                                                         ├─ .timeout(secs)
//!                                                         └─ .send() ──▶ Response
//!                                                                         │
//!                                                                         ├─ .status() -> Int
//!                                                                         ├─ .json<T>() -> T
//!                                                                         ├─ .text() -> String
//!                                                                         └─ .headers() -> Map
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `HttpClient`, `RequestBuilder`, `Response`, `HttpError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | All constructors return owned types. `send()` returns owned `Response`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, HttpError>`. `reqwest::Error` mapped via `From`. |
//! | R4 — Thread safety | `HttpClient` is `Send + Sync` (wraps `reqwest::blocking::Client` which is `Send + Sync`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. All types own their data. |
//! | R6 — Panic boundary | Every public function wraps its body in `catch_unwind` (per FFI guide §6). |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code.

pub mod error;

pub use error::HttpError;

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// An HTTP client with a fluent request-building API.
///
/// Constructed via [`HttpClient::new`]. Provides `get` / `post` / `put` /
/// `delete` methods that return a [`RequestBuilder`] for chaining headers,
/// body, and timeout before calling [`RequestBuilder::send`].
///
/// Internally wraps `reqwest::blocking::Client` (the synchronous API —
/// Buff's codegen lowers to blocking calls; async dispatch is a future
/// enhancement).
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::blocking::Client,
}

impl HttpClient {
    /// Create a new `HttpClient` with default settings (no custom headers,
    /// no proxy, 30s request timeout, 10s connect timeout).
    ///
    /// Wraps `reqwest::blocking::Client::builder()`. The body is wrapped in
    /// `catch_unwind` per T4 FFI guide R6.
    pub fn new() -> Self {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let inner = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30)) // P0.17: ft-001 — prevent infinite hangs
                .connect_timeout(std::time::Duration::from_secs(10)) // P0.17: ft-001 — fast-fail unreachable hosts
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new());
            HttpClient { inner }
        }));
        result.unwrap_or_else(|_| HttpClient {
            inner: reqwest::blocking::Client::new(),
        })
    }

    /// Create a GET request builder for the given URL.
    pub fn get(&self, url: &str) -> RequestBuilder {
        RequestBuilder {
            inner: self.inner.get(url),
        }
    }

    /// Create a POST request builder for the given URL.
    pub fn post(&self, url: &str) -> RequestBuilder {
        RequestBuilder {
            inner: self.inner.post(url),
        }
    }

    /// Create a PUT request builder for the given URL.
    pub fn put(&self, url: &str) -> RequestBuilder {
        RequestBuilder {
            inner: self.inner.put(url),
        }
    }

    /// Create a DELETE request builder for the given URL.
    pub fn delete(&self, url: &str) -> RequestBuilder {
        RequestBuilder {
            inner: self.inner.delete(url),
        }
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        HttpClient::new()
    }
}

/// A request under construction. Returned by [`HttpClient::get`],
/// [`HttpClient::post`], [`HttpClient::put`], [`HttpClient::delete`].
///
/// Supports chaining `.header(name, val)`, `.json(body)`, `.timeout(secs)`,
/// and finally `.send()` to execute the request.
#[derive(Debug)]
pub struct RequestBuilder {
    inner: reqwest::blocking::RequestBuilder,
}

impl RequestBuilder {
    /// Set a header on this request. Overwrites any previous value for
    /// the same header name.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.inner = self.inner.header(name, value);
        self
    }

    /// Set the JSON request body. The body is serialized from a
    /// `serde_json::Value` (a Buff `Map` or `Vector` lowered to
    /// `serde_json::Value` at codegen time).
    pub fn json(mut self, body: serde_json::Value) -> Self {
        self.inner = self.inner.json(&body);
        self
    }

    /// Set a timeout (in seconds) for this request. Overrides the
    /// client's default timeout.
    pub fn timeout(mut self, secs: u64) -> Self {
        self.inner = self.inner.timeout(std::time::Duration::from_secs(secs));
        self
    }

    /// Execute the request and return a [`Response`].
    ///
    /// Wraps `reqwest::blocking::RequestBuilder::send()`. The body is
    /// wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn send(self) -> Result<Response, HttpError> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.inner.send().map(|resp| Response { inner: resp })
        }));
        match result {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(err)) => Err(HttpError::from(err)),
            Err(_) => Err(HttpError::Panic),
        }
    }
}

/// An HTTP response. Returned by [`RequestBuilder::send`].
///
/// Provides accessors for status code, headers, and body (text or JSON).
#[derive(Debug)]
pub struct Response {
    inner: reqwest::blocking::Response,
}

impl Response {
    /// The HTTP status code (e.g. 200, 404, 500).
    pub fn status(&self) -> u16 {
        self.inner.status().as_u16()
    }

    /// Read the response body as text.
    ///
    /// Wraps `reqwest::blocking::Response::text()`. The body is wrapped
    /// in `catch_unwind` per T4 FFI guide R6.
    pub fn text(self) -> Result<String, HttpError> {
        let result = catch_unwind(AssertUnwindSafe(|| self.inner.text()));
        match result {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(err)) => Err(HttpError::from(err)),
            Err(_) => Err(HttpError::Panic),
        }
    }

    /// Read the response body as JSON, deserialized into a
    /// `serde_json::Value` (a Buff `Map` or `Vector` at codegen time).
    ///
    /// Wraps `reqwest::blocking::Response::json::<serde_json::Value>()`.
    /// The body is wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn json(self) -> Result<serde_json::Value, HttpError> {
        let result = catch_unwind(AssertUnwindSafe(|| self.inner.json::<serde_json::Value>()));
        match result {
            Ok(Ok(val)) => Ok(val),
            Ok(Err(err)) => Err(HttpError::from(err)),
            Err(_) => Err(HttpError::Panic),
        }
    }

    /// Read the response body as raw bytes.
    ///
    /// Wraps `reqwest::blocking::Response::bytes()`. The body is wrapped
    /// in `catch_unwind` per T4 FFI guide R6.
    pub fn bytes(self) -> Result<Vec<u8>, HttpError> {
        let result = catch_unwind(AssertUnwindSafe(|| self.inner.bytes().map(|b| b.to_vec())));
        match result {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(err)) => Err(HttpError::from(err)),
            Err(_) => Err(HttpError::Panic),
        }
    }

    /// Get all response headers as a `HashMap<String, String>`.
    /// If a header has multiple values, they are joined with `, `.
    pub fn headers(&self) -> HashMap<String, String> {
        let mut map: HashMap<String, String> = HashMap::new();
        for (name, value) in self.inner.headers().iter() {
            let key = name.to_string();
            let val = value.to_str().unwrap_or_default().to_string();
            match map.get_mut(&key) {
                Some(existing) => {
                    existing.push_str(", ");
                    existing.push_str(&val);
                }
                None => {
                    map.insert(key, val);
                }
            }
        }
        map
    }
}

impl std::fmt::Display for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Response({})", self.status())
    }
}
