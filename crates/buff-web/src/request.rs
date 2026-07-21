//! HTTP request type for the `buff-web` crate.
//!
//! [`Request`] is the Buff-visible wrapper around an incoming axum
//! request. It owns its method / path / headers / body so it can be
//! passed to user handler closures without exposing axum types or
//! requiring lifetime annotations (R5 — Lifetime Hiding per the
//! T4 FFI guide).

use crate::error::WebError;

/// An incoming HTTP request, owned and detached from its underlying
/// axum lifetime.
///
/// Constructed internally via [`Request::from_axum`] when the axum
/// router dispatches a route to a Buff handler. The user-facing
/// constructor surface is empty (requests are received, not built).
///
/// Buff-facing accessors:
/// - [`Request::method`] — HTTP verb as a String (`"GET"` / `"POST"` / ...).
/// - [`Request::path`] — URL path component (`"/users/42"`).
/// - [`Request::header`] — header value lookup by name (returns
///   `Option<String>`; `None` when the header is absent).
/// - [`Request::body`] — body as an owned `String` (returns
///   `Result<String, WebError>`; `Err(BodyNotUtf8)` when the bytes
///   are not valid UTF-8).
/// - [`Request::json`] — body parsed as JSON (returns
///   `Result<serde_json::Value, WebError>`).
#[derive(Debug, Clone)]
pub struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    /// Build a [`Request`] from an axum request by collecting the
    /// method, URI path, headers, and body bytes into owned data.
    ///
    /// This is the SINGLE point where axum types cross into Buff's
    /// owned-data view. After this call, the [`Request`] has no
    /// lifetime dependency on the underlying axum connection.
    ///
    /// `pub(crate)` — only the route dispatcher calls this; users
    /// never construct [`Request`] values themselves.
    pub(crate) async fn from_axum(req: axum::extract::Request) -> Self {
        const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
        let method = req.method().as_str().to_string();
        let path = req.uri().path().to_string();
        let headers = req
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        let body_bytes = axum::body::to_bytes(req.into_body(), MAX_BODY_BYTES)
            .await
            .map(|b| b.to_vec())
            .unwrap_or_default();
        Request {
            method,
            path,
            headers,
            body: body_bytes,
        }
    }

    /// Construct a [`Request`] directly from owned fields. Used by
    /// the test suite + the routing-integration tests; never called
    /// from production code (production always goes through
    /// [`Request::from_axum`]).
    #[cfg(test)]
    pub(crate) fn new(
        method: &str,
        path: &str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Self {
        Request {
            method: method.to_string(),
            path: path.to_string(),
            headers,
            body,
        }
    }

    /// The HTTP method verb as an uppercase String (`"GET"` /
    /// `"POST"` / `"PUT"` / `"DELETE"` / `"PATCH"` / ...).
    #[must_use]
    pub fn method(&self) -> String {
        self.method.clone()
    }

    /// The URL path component (`"/users/42"`). Excludes query string.
    #[must_use]
    pub fn path(&self) -> String {
        self.path.clone()
    }

    /// Look up a single header value by name (case-insensitive).
    /// Returns `None` when the header is absent. If the header is
    /// present multiple times, the FIRST occurrence wins (mirrors
    /// axum's `Headers::get` behaviour).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<String> {
        let name_lower = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(n, _)| n.to_ascii_lowercase() == name_lower)
            .map(|(_, v)| v.clone())
    }

    /// The request body as an owned UTF-8 String. Returns
    /// [`WebError::BodyNotUtf8`] when the bytes are not valid UTF-8.
    ///
    /// # Errors
    ///
    /// Returns [`WebError::BodyNotUtf8`] iff `String::from_utf8` rejects
    /// the body bytes.
    pub fn body(&self) -> Result<String, WebError> {
        String::from_utf8(self.body.clone()).map_err(|_| WebError::BodyNotUtf8)
    }

    /// The request body parsed as a JSON value. Returns
    /// [`WebError::BodyNotUtf8`] when the body bytes are not valid
    /// UTF-8, or [`WebError::Json`] when `serde_json` rejects the
    /// text.
    ///
    /// # Errors
    ///
    /// Returns [`WebError::BodyNotUtf8`] for invalid UTF-8 bodies, or
    /// [`WebError::Json`] wrapping the underlying `serde_json::Error`
    /// message for malformed JSON.
    pub fn json(&self) -> Result<serde_json::Value, WebError> {
        let text = self.body()?;
        serde_json::from_str(&text).map_err(WebError::from)
    }
}
