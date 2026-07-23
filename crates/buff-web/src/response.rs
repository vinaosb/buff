//! HTTP response builder for the `buff-web` crate.
//!
//! [`Response`] is the Buff-visible builder for outgoing HTTP
//! responses. Each static constructor returns a fully-formed
//! [`Response`] value; chainable mutators (`status`, `header`)
//! return `&mut Self` so the user can write
//! `Response::text("ok").status(201).header("X-Trace", "abc")` (the
//! Buff codegen lowers chained method calls exactly this way).

use crate::error::WebError;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::IntoResponse;

/// An outgoing HTTP response.
///
/// Constructed via [`Response::text`] (200 `text/plain; charset=utf-8`)
/// or [`Response::json`] (200 `application/json`). The chainable
/// mutators [`Response::status`] and [`Response::header`] modify the
/// response in place after construction.
///
/// Internally carries an HTTP status code, a header list (Vec of
/// owned `(name, value)` pairs), and a body byte Vec. The conversion
/// to an axum response happens in [`Response::into_axum_response`],
/// which is `pub(crate)` — the Buff user never sees axum types.
#[derive(Debug, Clone)]
pub struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    /// Build a 200 `text/plain; charset=utf-8` response carrying
    /// `body` as its UTF-8 encoded payload. The default content-type
    /// header can be overridden by chaining `.header("Content-Type",
    /// "text/html")` after construction.
    #[must_use]
    pub fn text(body: &str) -> Self {
        Response {
            status: 200,
            headers: vec![(
                "Content-Type".to_string(),
                "text/plain; charset=utf-8".to_string(),
            )],
            body: body.as_bytes().to_vec(),
        }
    }

    /// Build a 200 `application/json` response carrying `value`
    /// serialised to canonical JSON as its payload. The serialisation
    /// is infallible at the surface — a `serde_json::Error` (which
    /// can only happen for non-string map keys) collapses to an
    /// empty JSON object body via `unwrap_or_default()` (matches
    /// Buff's "no panicking generated code" rule).
    #[must_use]
    pub fn json(value: &serde_json::Value) -> Self {
        let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
        Response {
            status: 200,
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: body.into_bytes(),
        }
    }

    /// Build an empty response carrying only a status code (used for
    /// `204 No Content`, `404 Not Found`, etc.). Body is empty; no
    /// Content-Type header is added.
    #[must_use]
    pub fn status_only(code: u16) -> Self {
        Response {
            status: code,
            headers: vec![],
            body: vec![],
        }
    }

    /// Override the status code. Chainable. Returns `&mut Self` so
    /// `Response::text("created").status(201)` works at the call
    /// site (Buff codegen lowers this to a sequential method call
    /// chain in Rust).
    pub fn status(&mut self, code: u16) -> &mut Self {
        self.status = code;
        self
    }

    /// Append a header to the response. Chainable. Returns `&mut Self`.
    /// Duplicate header names are allowed (axum will render both as
    /// separate lines in the on-wire response).
    pub fn header(&mut self, name: &str, value: &str) -> &mut Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// The HTTP status code as a `u16` (200, 404, ...).
    #[must_use]
    pub fn status_code(&self) -> u16 {
        self.status
    }

    /// Read-only access to the response headers as an owned Vec of
    /// `(name, value)` pairs. Used by the test suite.
    #[must_use]
    pub fn headers_list(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }

    /// Read-only access to the response body bytes. Used by the test
    /// suite + the IntoResponse lowering.
    #[must_use]
    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    /// Consume self and produce an axum-compatible response value.
    /// `pub(crate)` — only the route dispatcher calls this; the Buff
    /// user never sees axum types.
    ///
    /// # Errors
    ///
    /// Returns [`WebError`] iff a header name or value fails to parse
    /// (the user supplied an invalid header like `"Bad Header"` with
    /// a space). The dispatcher maps the error to a 500 response.
    pub(crate) fn into_axum_response(self) -> Result<axum::response::Response, WebError> {
        let status = StatusCode::from_u16(self.status).map_err(|e| {
            WebError::InvalidPath(format!("invalid status code {}: {e}", self.status))
        })?;
        let mut header_map = HeaderMap::new();
        for (name, value) in self.headers {
            header_map.insert(HeaderName::try_from(name)?, HeaderValue::try_from(value)?);
        }
        let body = Body::from(self.body);
        Ok((status, header_map, body).into_response())
    }
}

impl Default for Response {
    fn default() -> Self {
        Response {
            status: 200,
            headers: vec![],
            body: vec![],
        }
    }
}
