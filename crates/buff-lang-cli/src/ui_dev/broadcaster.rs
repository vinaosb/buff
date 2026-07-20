//! WebSocket message protocol + broadcaster (T131).
//!
//! The dev server multicasts two kinds of messages to all connected
//! browsers:
//!
//! - `Reload` — broadcast after a successful Buff+wasm rebuild. The
//!   client-side snippet (see [`crate::ui_dev::client_js`]) calls
//!   `location.reload()` on receipt → LIVE RELOAD.
//! - `Error(<message>)` — broadcast when either the Buff front-end
//!   (`pipeline::compile_to_rust`) or the cargo+wasm-bindgen shell-out
//!   fails. The client-side snippet shows a red banner with the
//!   message.
//!
//! Wire format: JSON. The two shapes are:
//!
//! ```json
//! { "type": "reload" }
//! { "type": "error", "message": "<text>" }
//! ```
//!
//! JSON was chosen over a free-form text protocol because the
//! `error` payload carries a multi-line `message` (compiler output)
//! that would need custom escaping if inlined into a flat text frame.
//! JSON gives us the escaping for free and matches the convention of
//! Vite / trunk / cargo-leptos.
//!
//! # Broadcaster
//!
//! [`ReloadBroadcaster`] wraps a `tokio::sync::broadcast` channel —
//! each connected WS client gets its own `Receiver` and the dev server
//! holds the `Sender`. A broadcast channel (vs. `mpsc`) is the right
//! shape: every client must observe every message, and slow clients
//! are allowed to lag (with a bounded buffer of 16 messages; if a
//! client falls behind it gets a `Lagged` error on recv and the
//! per-client task just re-subscribes).

use serde::{Deserialize, Serialize};

/// Capacity of the broadcast channel. 16 is generous — the dev server
/// emits at most one message per ~200 ms (debounced file change) and
/// the client immediately reloads, so even a slow client on a laggy
/// network should keep up.
pub const BROADCAST_CAPACITY: usize = 16;

/// A message the dev server pushes to connected browsers over the
/// `/__buff_reload__` WebSocket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ReloadMessage {
    /// Instruct the client to call `location.reload()` (LIVE RELOAD —
    /// full page refresh; not state-preserving HMR).
    Reload,
    /// Show a red banner overlay with `message`. Used for both Buff
    /// compile errors and cargo/wasm-bindgen shell-out failures.
    Error {
        /// The error message to display (may be multi-line; rendered
        /// `<pre>`-style in the overlay).
        message: String,
    },
}

impl ReloadMessage {
    /// Serialise to a JSON string suitable for `WebSocket::send(Text(..))`.
    ///
    /// Falls back to a hardcoded `{"type":"error"}` envelope if
    /// serialisation fails — this is unreachable for our enum shape
    /// (serde_json handles `String` + unit variants without infallible
    /// paths), but keeps the function total so the per-client task
    /// never panics on the send path.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"type":"error","message":"serialise failed"}"#.to_string())
    }

    /// Deserialise a JSON string from `WebSocket::recv(Text(..))`.
    ///
    /// Returns `None` on parse failure so the client-task helper can
    /// just ignore malformed frames (clients should never send us
    /// frames anyway — the WS endpoint is server-push only — but we
    /// handle the case gracefully without panic).
    #[must_use]
    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}

/// Broadcaster for [`ReloadMessage`] — wraps a
/// `tokio::sync::broadcast::channel` so every connected browser gets
/// every message.
#[derive(Debug, Clone)]
pub struct ReloadBroadcaster {
    tx: tokio::sync::broadcast::Sender<ReloadMessage>,
}

impl ReloadBroadcaster {
    /// Construct a fresh broadcaster. The returned clone-able handle
    /// is what the dev server hands to the file-watcher task (which
    /// calls [`Self::broadcast`]) and to each WS client task (which
    /// calls [`Self::subscribe`]).
    #[must_use]
    pub fn new() -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(BROADCAST_CAPACITY);
        Self { tx }
    }

    /// Subscribe to the broadcast channel. Each call returns a fresh
    /// `Receiver` so the caller gets every message broadcast AFTER
    /// the subscribe call (messages broadcast before are NOT
    /// delivered — this matches the dev server's "live reload" UX
    /// where a freshly-connected browser reloads on the NEXT save,
    /// not on already-handled saves).
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ReloadMessage> {
        self.tx.subscribe()
    }

    /// Broadcast a [`ReloadMessage::Reload`] to every connected
    /// client. No-op when no clients are subscribed (broadcast channel
    /// drops the message silently — the dev server does NOT queue
    /// reloads for browsers that connect later).
    pub fn reload(&self) {
        // send returns Err when there are zero receivers — that's
        // fine; the message is just dropped.
        let _ = self.tx.send(ReloadMessage::Reload);
    }

    /// Broadcast a [`ReloadMessage::Error`] with `message` to every
    /// connected client. Same no-receiver semantics as [`Self::reload`].
    pub fn error(&self, message: impl Into<String>) {
        let _ = self.tx.send(ReloadMessage::Error {
            message: message.into(),
        });
    }
}

impl Default for ReloadBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_serialises_to_typed_json() {
        let s = ReloadMessage::Reload.to_json();
        assert_eq!(s, r#"{"type":"reload"}"#);
    }

    #[test]
    fn error_serialises_with_message_field() {
        let s = ReloadMessage::Error {
            message: "oh no".into(),
        }
        .to_json();
        assert_eq!(s, r#"{"type":"error","message":"oh no"}"#);
    }

    #[test]
    fn error_escapes_multiline_message_correctly() {
        // Compiler errors are multi-line; serde_json must escape the
        // newlines so the frame stays a single WS Text message.
        let s = ReloadMessage::Error {
            message: "line1\nline2\ttab".into(),
        }
        .to_json();
        assert!(s.contains(r"\n"), "expected escaped \\n in JSON, got: {s}");
        assert!(s.contains(r"\t"), "expected escaped \\t in JSON, got: {s}");
    }

    #[test]
    fn reload_round_trips_through_json() {
        let s = ReloadMessage::Reload.to_json();
        let parsed = ReloadMessage::from_json(&s).expect("round trip");
        assert_eq!(parsed, ReloadMessage::Reload);
    }

    #[test]
    fn error_round_trips_through_json() {
        let original = ReloadMessage::Error {
            message: "compile failed at line 3".into(),
        };
        let parsed = ReloadMessage::from_json(&original.to_json()).expect("round trip");
        assert_eq!(parsed, original);
    }

    #[test]
    fn from_json_rejects_malformed_input() {
        assert!(ReloadMessage::from_json("not json").is_none());
        assert!(ReloadMessage::from_json(r#"{"type":"unknown"}"#).is_none());
        // Missing `message` field on error variant → rejected.
        assert!(ReloadMessage::from_json(r#"{"type":"error"}"#).is_none());
    }

    #[test]
    fn broadcaster_drops_messages_with_zero_subscribers() {
        // No subscribers → broadcast is silently dropped (no panic).
        let b = ReloadBroadcaster::new();
        b.reload();
        b.error("test");
    }

    #[test]
    fn broadcaster_delivers_to_active_subscribers() {
        let b = ReloadBroadcaster::new();
        let mut rx = b.subscribe();
        b.reload();
        let msg = rx.try_recv().expect("message delivered");
        assert_eq!(msg, ReloadMessage::Reload);
    }

    #[test]
    fn broadcaster_does_not_deliver_pre_subscribe_messages() {
        let b = ReloadBroadcaster::new();
        b.reload(); // before subscribe — dropped for new subscriber
        let mut rx = b.subscribe();
        b.error("after subscribe");
        let msg = rx.try_recv().expect("post-subscribe message");
        assert!(matches!(msg, ReloadMessage::Error { .. }));
    }
}
