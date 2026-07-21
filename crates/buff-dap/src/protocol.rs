//! DAP wire protocol — JSON-RPC framing + message types.
//!
//! The Debug Adapter Protocol is JSON-RPC over stdio with a header-
//! prefixed framing identical to LSP:
//!
//! ```text
//! Content-Length: 1234\r\n
//! \r\n
//! {json body}
//! ```
//!
//! Each message is one of three kinds ([`MessageKind`]):
//!
//! - **Request** — `editor → adapter`. Carries `command` + `arguments`.
//! - **Response** — `adapter → editor`. Replies to a request with
//!   `success: bool` + optional `body`.
//! - **Event** — `adapter → editor` (asynchronous notification).
//!   Carries `event` + `body`.
//!
//! # Why hand-roll (not use the `dap` crate)
//!
//! DAP is small + well-documented. Hand-rolling mirrors the
//! project's "hand-rolled lexer/parser" philosophy (avoids the
//! cc-rs transitive failure class that killed chumsky/logos) and
//! matches how `buff-lsp` consumes `lsp-server` + `lsp-types`
//! (a small stdio scaffold + a thin type set). The translation
//! layer ([`crate::translation`]) — the load-bearing part — is pure
//! Rust with no dep needs; the protocol types here are the minimal
//! surface required to round-trip initialize / launch / setBreakpoints
//! / stackTrace / scopes / variables / evaluate / continue / next /
//! stepIn / stepOut / pause / disconnect.
//!
//! See <https://microsoft.github.io/debug-adapter-protocol/specification>
//! for the authoritative spec.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{DapError, DapResult};

/// The blank CRLF line separating headers from the JSON body.
const HEADER_BODY_SEP: &[u8] = b"\r\n\r\n";

/// The three DAP message kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageKind {
    /// `editor → adapter` request (carries `command`).
    Request,
    /// `adapter → editor` response (replies to a prior request).
    Response,
    /// `adapter → editor` event (asynchronous notification).
    Event,
}

impl MessageKind {
    /// The DAP `type` discriminator string.
    pub fn as_str(self) -> &'static str {
        match self {
            MessageKind::Request => "request",
            MessageKind::Response => "response",
            MessageKind::Event => "event",
        }
    }
}

/// The top-level envelope for every DAP wire message.
///
/// Every message (request / response / event) shares the `seq` +
/// `type` fields; the rest are optional depending on the kind.
/// We model the union as a single struct with optional fields so
/// the proxy can round-trip any message (including unknown /
/// forward-compat ones) without losing data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Monotonic sequence number assigned by the sender.
    pub seq: u64,
    /// `"request"` / `"response"` / `"event"`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Request only: the command name (`"initialize"`, `"launch"`, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Event only: the event name (`"initialized"`, `"stopped"`, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// Response only: the `seq` of the request this replies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_seq: Option<u64>,
    /// Response only: success indicator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    /// Response only: error message (when `success: false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Request arguments / response body / event body — opaque JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

impl Message {
    /// Build a response echoing the caller's `request_seq`.
    pub fn response(request_seq: u64, seq: u64, body: Option<Value>) -> Self {
        Self {
            seq,
            kind: MessageKind::Response.as_str().to_string(),
            command: None,
            event: None,
            request_seq: Some(request_seq),
            success: Some(true),
            message: None,
            body,
        }
    }

    /// Build an error response.
    pub fn error_response(request_seq: u64, seq: u64, message: impl Into<String>) -> Self {
        Self {
            seq,
            kind: MessageKind::Response.as_str().to_string(),
            command: None,
            event: None,
            request_seq: Some(request_seq),
            success: Some(false),
            message: Some(message.into()),
            body: None,
        }
    }

    /// Build an event message.
    pub fn event(seq: u64, event: impl Into<String>, body: Option<Value>) -> Self {
        Self {
            seq,
            kind: MessageKind::Event.as_str().to_string(),
            command: None,
            event: Some(event.into()),
            request_seq: None,
            success: None,
            message: None,
            body,
        }
    }

    /// Classify this message into a [`MessageKind`].
    ///
    /// Returns [`DapError::MalformedMessage`] when the `type` field
    /// is missing or unrecognized.
    pub fn kind(&self) -> DapResult<MessageKind> {
        match self.kind.as_str() {
            "request" => Ok(MessageKind::Request),
            "response" => Ok(MessageKind::Response),
            "event" => Ok(MessageKind::Event),
            other => Err(DapError::MalformedMessage(format!(
                "unknown message type `{other}` (seq={})",
                self.seq
            ))),
        }
    }

    /// The command name on a request (None on response/event).
    pub fn command(&self) -> Option<&str> {
        self.command.as_deref()
    }

    /// The event name on an event (None on request/response).
    pub fn event_name(&self) -> Option<&str> {
        self.event.as_deref()
    }
}

// -----------------------------------------------------------------
// Framing — read/write `Content-Length: N\r\n\r\n{body}` chunks.
// -----------------------------------------------------------------

/// Encode a [`Message`] into the DAP wire framing.
///
/// Produces `Content-Length: <n>\r\n\r\n<json>` — ready to write
/// to a transport sink.
pub fn encode(message: &Message) -> DapResult<Vec<u8>> {
    let json = serde_json::to_vec(message).map_err(|e| DapError::Json(e.to_string()))?;
    let header = format!("Content-Length: {}\r\n\r\n", json.len());
    let mut out = Vec::with_capacity(header.len() + json.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&json);
    Ok(out)
}

/// Parse a single DAP message from a byte buffer that begins with
/// the `Content-Length: N\r\n\r\n{json}` framing.
///
/// Returns the parsed [`Message`] AND the number of bytes consumed
/// (so the caller can drain a buffered stream). Returns
/// [`DapError::MalformedMessage`] when the framing is incomplete /
/// malformed (used by the transport to decide whether to wait for
/// more bytes or close the connection).
pub fn decode(buf: &[u8]) -> DapResult<(Message, usize)> {
    // Find the header/body separator (\r\n\r\n).
    let sep_pos = buf
        .windows(HEADER_BODY_SEP.len())
        .position(|w| w == HEADER_BODY_SEP)
        .ok_or_else(|| DapError::MalformedMessage("missing header/body separator".into()))?;

    let header_bytes = &buf[..sep_pos];
    let body_start = sep_pos + HEADER_BODY_SEP.len();

    // Parse the Content-Length header. The DAP spec also permits
    // other headers (e.g. `Content-Type: utf-8`); we ignore them —
    // only `Content-Length` is load-bearing for framing.
    let header_str = std::str::from_utf8(header_bytes)
        .map_err(|e| DapError::MalformedMessage(format!("header not utf8: {e}")))?;
    let content_length = parse_content_length(header_str)?;

    // Verify the body is fully present.
    let body_end = body_start + content_length;
    if buf.len() < body_end {
        return Err(DapError::MalformedMessage(format!(
            "body truncated: have {} bytes, need {body_end}",
            buf.len()
        )));
    }

    let body_bytes = &buf[body_start..body_end];
    let message: Message =
        serde_json::from_slice(body_bytes).map_err(|e| DapError::Json(e.to_string()))?;
    Ok((message, body_end))
}

/// Extract the `Content-Length` value from a header block.
fn parse_content_length(header: &str) -> DapResult<usize> {
    for line in header.split("\r\n") {
        // Case-insensitive match on the header name; the value is
        // a decimal byte count.
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            let n: usize = rest
                .trim()
                .parse()
                .map_err(|e| DapError::MalformedMessage(format!("bad content-length: {e}")))?;
            return Ok(n);
        }
    }
    Err(DapError::MalformedMessage(
        "no Content-Length header found".into(),
    ))
}

/// Check whether `buf` contains at least one complete DAP message
/// (header + body). Used by the transport to decide whether to
/// block on more input or dispatch immediately.
pub fn has_complete_message(buf: &[u8]) -> bool {
    match decode(buf) {
        Ok(_) => true,
        Err(DapError::MalformedMessage(_)) => false,
        // Other errors (JSON parse, etc.) mean we have a full
        // message but it's malformed — let the caller surface the
        // error rather than block forever.
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip_request() {
        let msg = Message {
            seq: 1,
            kind: "request".into(),
            command: Some("initialize".into()),
            event: None,
            request_seq: None,
            success: None,
            message: None,
            body: Some(serde_json::json!({"clientID": "vscode"})),
        };
        let bytes = encode(&msg).expect("encode");
        let (decoded, consumed) = decode(&bytes).expect("decode");
        assert_eq!(consumed, bytes.len());
        assert_eq!(decoded.seq, 1);
        assert_eq!(decoded.kind, "request");
        assert_eq!(decoded.command.as_deref(), Some("initialize"));
        assert_eq!(decoded.body, msg.body);
    }

    #[test]
    fn encode_decode_roundtrip_event() {
        let msg = Message::event(42, "initialized", None);
        let bytes = encode(&msg).expect("encode");
        let (decoded, _) = decode(&bytes).expect("decode");
        assert_eq!(decoded.event.as_deref(), Some("initialized"));
        assert_eq!(decoded.seq, 42);
    }

    #[test]
    fn encode_includes_content_length_header() {
        let msg = Message::event(1, "terminated", None);
        let bytes = encode(&msg).expect("encode");
        let header_end = bytes
            .windows(4)
            .position(|w| w == HEADER_BODY_SEP)
            .expect("separator present");
        let header = std::str::from_utf8(&bytes[..header_end]).expect("utf8");
        assert!(header.starts_with("Content-Length: "));
        // The header value should equal the body length.
        let body_len = bytes.len() - header_end - HEADER_BODY_SEP.len();
        let declared: usize = header
            .strip_prefix("Content-Length: ")
            .expect("prefix")
            .parse()
            .expect("usize");
        assert_eq!(declared, body_len);
    }

    #[test]
    fn decode_rejects_missing_separator() {
        let buf = b"Content-Length: 10\r\n"; // no separator + no body
        let err = decode(buf).unwrap_err();
        assert!(matches!(err, DapError::MalformedMessage(_)));
    }

    #[test]
    fn decode_rejects_truncated_body() {
        let header = b"Content-Length: 100\r\n\r\n";
        let body = b"{ \"short\" }"; // declared 100 but only 11 bytes
        let mut buf = Vec::new();
        buf.extend_from_slice(header);
        buf.extend_from_slice(body);
        let err = decode(&buf).unwrap_err();
        assert!(matches!(err, DapError::MalformedMessage(_)));
    }

    #[test]
    fn decode_returns_bytes_consumed() {
        // Build two messages back-to-back; decode should return the
        // length of the first, leaving the second for the next call.
        let m1 = Message::event(1, "stopped", None);
        let m2 = Message::event(2, "terminated", None);
        let b1 = encode(&m1).expect("encode");
        let b2 = encode(&m2).expect("encode");
        let mut buf = Vec::with_capacity(b1.len() + b2.len());
        buf.extend_from_slice(&b1);
        buf.extend_from_slice(&b2);
        let (_, consumed) = decode(&buf).expect("decode");
        assert_eq!(consumed, b1.len());
        // The remainder should decode as the second message.
        let (m, _) = decode(&buf[consumed..]).expect("decode second");
        assert_eq!(m.seq, 2);
    }

    #[test]
    fn parse_content_length_case_insensitive() {
        let n = parse_content_length("content-length: 42\r\n").expect("ok");
        assert_eq!(n, 42);
    }

    #[test]
    fn parse_content_length_missing_header_errors() {
        let err = parse_content_length("some-other: 1\r\n").unwrap_err();
        assert!(matches!(err, DapError::MalformedMessage(_)));
    }

    #[test]
    fn message_kind_classifies_strings() {
        let m = Message {
            seq: 0,
            kind: "request".into(),
            command: None,
            event: None,
            request_seq: None,
            success: None,
            message: None,
            body: None,
        };
        assert_eq!(m.kind().unwrap(), MessageKind::Request);
    }

    #[test]
    fn message_kind_rejects_unknown() {
        let m = Message {
            seq: 0,
            kind: "bogus".into(),
            command: None,
            event: None,
            request_seq: None,
            success: None,
            message: None,
            body: None,
        };
        assert!(m.kind().is_err());
    }

    #[test]
    fn has_complete_message_true_for_full() {
        let bytes = encode(&Message::event(1, "initialized", None)).expect("encode");
        assert!(has_complete_message(&bytes));
    }

    #[test]
    fn has_complete_message_false_for_partial() {
        let bytes = encode(&Message::event(1, "initialized", None)).expect("encode");
        // Truncate to half — no separator present.
        let half = &bytes[..bytes.len() / 2];
        assert!(!has_complete_message(half));
    }

    #[test]
    fn response_builder_marks_success() {
        let r = Message::response(10, 20, None);
        assert_eq!(r.request_seq, Some(10));
        assert_eq!(r.seq, 20);
        assert_eq!(r.success, Some(true));
    }

    #[test]
    fn error_response_builder_marks_failure() {
        let r = Message::error_response(7, 8, "boom");
        assert_eq!(r.success, Some(false));
        assert_eq!(r.message.as_deref(), Some("boom"));
    }
}
