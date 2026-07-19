//! LSP server: stdio transport + main loop with debounced diagnostics.
//!
//! Built on the [`lsp_server`] crate (rust-analyzer's JSON-RPC scaffold).
//! The main loop is **single-threaded** — it uses `recv_timeout` on the
//! connection's receiver to coalesce bursts of `didChange` notifications
//! into one diagnostics publish per ~300ms idle window.
//!
//! # Debounce algorithm
//!
//! ```text
//! loop {
//!     let wait = DEBOUNCE_IDLE - last_event_time.elapsed();
//!     match connection.receiver.recv_timeout(wait) {
//!         Ok(msg) => { dispatch(msg); last_event_time = now(); }
//!         Err(Timeout) => { publish_all_pending(); last_event_time = now(); }
//!         Err(Disconnected) => break,
//!     }
//! }
//! ```
//!
//! - Each event resets `last_event_time`. While events keep arriving
//!   faster than 300ms apart, no diagnostics are published.
//! - Once 300ms elapses with no event, the loop's `recv_timeout` returns
//!   `Timeout` and we publish diagnostics for every "dirty" URI exactly
//!   once. The `dirty` set is cleared after publishing.
//! - `didOpen` marks the URI dirty and publishes immediately on the next
//!   idle tick (typically <300ms later). `didClose` publishes an empty
//!   diagnostic list for the URI and removes it from state.
//!
//! # Single-threaded trade-off
//!
//! A multi-threaded debounce (background thread + channels) would let the
//! server keep responding to requests during a slow parse, but Buff files
//! parse in microseconds (the front-end is hand-rolled + zero-alloc on
//! the hot path). Single-threaded keeps the architecture trivial and
//! avoids the shared-state borrow-checker fight.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use buff_lang_error::SourceId;
use crossbeam_channel::{RecvTimeoutError, Select};
use lsp_server::{Connection, Message, Response};
use lsp_types::{
    CompletionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, DocumentSymbolParams,
    GotoDefinitionParams, HoverParams, HoverProviderCapability, InitializeParams, OneOf,
    PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, Uri,
};
use thiserror::Error;

use crate::handlers;
use crate::state::DocumentState;

/// String-form key for the document maps. The LSP `Uri` type wraps
/// `fluent_uri::Uri<String>` which trips clippy's `mutable_key_type` lint
/// (the inner type may carry `UnsafeCell`s in some build configurations).
/// Using `String` keys (the canonical URI string) sidesteps the lint while
/// remaining semantically identical — Uri ↔ String conversion is via
/// `as_str()` / `parse()`.
type UriKey = String;

/// Convert a [`Uri`] to its canonical string form for use as a map key.
fn uri_key(uri: &Uri) -> UriKey {
    uri.as_str().to_string()
}

/// Errors surfaced by the LSP server (transport or protocol failures).
#[derive(Debug, Error)]
pub enum LspError {
    /// A failure to read/write a JSON-RPC message over stdio.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// A failure to (de)serialize an LSP message.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    /// The client sent a notification we couldn't route to a handler.
    #[error("unhandled notification: {method}")]
    UnhandledNotification { method: String },
    /// Channel recv/send failure.
    #[error("channel error: {0}")]
    Channel(String),
    /// Lower-level protocol failure from the `lsp_server` crate.
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl From<lsp_server::ProtocolError> for LspError {
    fn from(value: lsp_server::ProtocolError) -> Self {
        LspError::Protocol(value.to_string())
    }
}

/// Idle duration before a pending change-set publishes diagnostics.
///
/// 300ms is the value the T117 spec calls for — long enough to absorb a
/// burst of keystrokes without thrashing the parser, short enough that the
/// user sees errors as soon as they pause.
pub const DEBOUNCE_IDLE: Duration = Duration::from_millis(300);

/// Run the LSP server on stdio until the client sends `shutdown` + `exit`.
///
/// Mirrors the [`lsp_server`] examples: handshake via
/// [`Connection::initialize`], then sit in a `recv_timeout` loop
/// dispatching each message type.
pub fn run_stdio() -> Result<(), LspError> {
    let (connection, threads) = Connection::stdio();
    let server_capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                // v1.2 only supports FULL sync (the client sends the entire
                // document on every change). Incremental is a v2.0 task.
                change: Some(TextDocumentSyncKind::FULL),
                will_save: None,
                will_save_wait_until: None,
                save: None,
            },
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(lsp_types::CompletionOptions {
            trigger_characters: Some(vec![".".to_string()]),
            ..Default::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };
    let caps_value = serde_json::to_value(&server_capabilities)?;
    let init_params_value = connection.initialize(caps_value)?;
    let init_params: InitializeParams = serde_json::from_value(init_params_value)?;
    let _ = init_params; // currently unused beyond handshake

    let mut server = ServerState::default();
    let mut source_ids: HashMap<UriKey, SourceId> = HashMap::new();
    let mut dirty: HashSet<UriKey> = HashSet::new();
    let mut last_event_time = Instant::now();

    // Build a selector over the connection's receiver. We don't actually
    // need multiple sources — `recv_timeout` alone gives us the debounce
    // behaviour — but `Select` lets us add a future cancellation channel
    // without restructuring the loop. For v1.2 we keep it simple.
    let _select = Select::new();

    loop {
        let elapsed = last_event_time.elapsed();
        let wait = DEBOUNCE_IDLE.checked_sub(elapsed).unwrap_or(Duration::ZERO);

        match connection.receiver.recv_timeout(wait) {
            Ok(msg) => {
                last_event_time = Instant::now();
                let should_exit = handle_message(
                    &msg,
                    &connection,
                    &mut server,
                    &mut source_ids,
                    &mut dirty,
                )?;
                if should_exit {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // Publish diagnostics for every dirty URI, then reset the
                // clock so the next event waits another full DEBOUNCE_IDLE.
                if !dirty.is_empty() {
                    publish_diagnostics_for(&dirty, &server, &connection)?;
                    dirty.clear();
                }
                last_event_time = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    threads
        .join()
        .map_err(|e| LspError::Io(std::io::Error::other(format!("threads join: {e:?}"))))?;
    Ok(())
}

/// Mutable server state — primarily the open documents map.
#[derive(Debug, Default)]
pub struct ServerState {
    /// Per-URI document state, keyed by the URI's canonical string form
    /// (see [`UriKey`] for why we don't use [`Uri`] directly).
    pub documents: HashMap<UriKey, DocumentState>,
}

/// Handle one incoming LSP message. Returns `true` if the server should
/// exit after this message (i.e. a `shutdown` request was processed).
fn handle_message(
    msg: &Message,
    connection: &Connection,
    server: &mut ServerState,
    source_ids: &mut HashMap<UriKey, SourceId>,
    dirty: &mut HashSet<UriKey>,
) -> Result<bool, LspError> {
    match msg {
        Message::Request(req) => {
            if connection.handle_shutdown(req)? {
                return Ok(true);
            }
            let id = req.id.clone();
            let method = req.method.clone();
            let params = req.params.clone();
            match dispatch_request(&method, params, server) {
                Ok(value) => {
                    let resp = Response::new_ok(id, value);
                    connection
                        .sender
                        .send(Message::Response(resp))
                        .map_err(|e| LspError::Channel(format!("send response: {e}")))?;
                }
                Err(e) => {
                    let err = lsp_server::ResponseError {
                        code: lsp_server::ErrorCode::InternalError as i32,
                        message: e.to_string(),
                        data: None,
                    };
                    let resp = Response::new_err(id, err.code, err.message);
                    connection
                        .sender
                        .send(Message::Response(resp))
                        .map_err(|e| LspError::Channel(format!("send error response: {e}")))?;
                }
            }
            Ok(false)
        }
        Message::Notification(notif) => {
            let method = notif.method.as_str();
            match method {
                "textDocument/didOpen" => {
                    if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(
                        notif.params.clone(),
                    ) {
                        let uri = p.text_document.uri.clone();
                        let key = uri_key(&uri);
                        let sid = fresh_source_id(source_ids, &key);
                        let st = DocumentState::new(
                            p.text_document.text,
                            sid,
                            Some(p.text_document.version),
                        );
                        server.documents.insert(key.clone(), st);
                        // Mark dirty + immediately publish on didOpen so the
                        // client sees initial diagnostics without waiting
                        // for a didChange.
                        dirty.insert(key);
                    }
                    Ok(false)
                }
                "textDocument/didChange" => {
                    if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(
                        notif.params.clone(),
                    ) {
                        let uri = p.text_document.uri.clone();
                        let key = uri_key(&uri);
                        let version = p.text_document.version;
                        // Full-sync: the last change carries the whole text.
                        let new_text = p
                            .content_changes
                            .into_iter()
                            .last()
                            .map(|c| c.text)
                            .unwrap_or_default();
                        if let Some(st) = server.documents.get_mut(&key) {
                            st.update(new_text, Some(version));
                        }
                        dirty.insert(key);
                    }
                    Ok(false)
                }
                "textDocument/didClose" => {
                    if let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(
                        notif.params.clone(),
                    ) {
                        let uri = p.text_document.uri.clone();
                        let key = uri_key(&uri);
                        server.documents.remove(&key);
                        dirty.remove(&key);
                        // Publish an empty diagnostic list to clear markers.
                        let notif_out = lsp_server::Notification::new(
                            "textDocument/publishDiagnostics".to_string(),
                            PublishDiagnosticsParams {
                                uri,
                                diagnostics: Vec::new(),
                                version: None,
                            },
                        );
                        connection
                            .sender
                            .send(Message::Notification(notif_out))
                            .map_err(|e| LspError::Channel(format!("send notif: {e}")))?;
                    }
                    Ok(false)
                }
                _ => Ok(false), // Ignore unknown notifications.
            }
        }
        Message::Response(_) => Ok(false), // We never send server->client requests.
    }
}

/// Publish diagnostics for every URI in `dirty`, reading the current
/// analysis from `server.documents`.
fn publish_diagnostics_for(
    dirty: &HashSet<UriKey>,
    server: &ServerState,
    connection: &Connection,
) -> Result<(), LspError> {
    for key in dirty {
        let (uri, diags, version): (Uri, Vec<lsp_types::Diagnostic>, Option<i32>) = match server
            .documents
            .get(key)
        {
            Some(st) => {
                // Re-parse the string key back to a Uri for the wire type.
                let uri: Uri = match key.parse() {
                    Ok(u) => u,
                    Err(e) => return Err(LspError::Channel(format!("invalid uri {key:?}: {e}"))),
                };
                (uri, handlers::diagnostics(st), st.version)
            }
            None => match key.parse() {
                Ok(u) => (u, Vec::new(), None),
                Err(e) => return Err(LspError::Channel(format!("invalid uri {key:?}: {e}"))),
            },
        };
        let notif = lsp_server::Notification::new(
            "textDocument/publishDiagnostics".to_string(),
            PublishDiagnosticsParams {
                uri,
                diagnostics: diags,
                version,
            },
        );
        connection
            .sender
            .send(Message::Notification(notif))
            .map_err(|e| LspError::Channel(format!("send notif: {e}")))?;
    }
    Ok(())
}

/// Dispatch a single LSP request to the right handler.
fn dispatch_request(
    method: &str,
    params: serde_json::Value,
    server: &mut ServerState,
) -> Result<Option<serde_json::Value>, LspError> {
    match method {
        "textDocument/hover" => {
            let p: HoverParams = serde_json::from_value(params)?;
            let uri = &p.text_document_position_params.text_document.uri;
            let pos = p.text_document_position_params.position;
            let state = match server.documents.get(&uri_key(uri)) {
                Some(s) => s,
                None => return Ok(None),
            };
            let hover = handlers::hover(state, pos);
            Ok(Some(serde_json::to_value(hover)?))
        }
        "textDocument/completion" => {
            let p: CompletionParams = serde_json::from_value(params)?;
            let uri = &p.text_document_position.text_document.uri;
            let pos = p.text_document_position.position;
            let state = match server.documents.get(&uri_key(uri)) {
                Some(s) => s,
                None => return Ok(None),
            };
            let comp = handlers::completion(state, pos);
            Ok(Some(serde_json::to_value(comp)?))
        }
        "textDocument/definition" => {
            let p: GotoDefinitionParams = serde_json::from_value(params)?;
            let uri = &p.text_document_position_params.text_document.uri;
            let pos = p.text_document_position_params.position;
            let state = match server.documents.get(&uri_key(uri)) {
                Some(s) => s,
                None => return Ok(None),
            };
            let resp = handlers::goto_definition(state, uri, pos);
            Ok(Some(serde_json::to_value(resp)?))
        }
        "textDocument/documentSymbol" => {
            let p: DocumentSymbolParams = serde_json::from_value(params)?;
            let uri = &p.text_document.uri;
            let state = match server.documents.get(&uri_key(uri)) {
                Some(s) => s,
                None => return Ok(None),
            };
            let syms = handlers::document_symbols(state);
            Ok(Some(serde_json::to_value(syms)?))
        }
        "textDocument/formatting" => {
            let p: DocumentFormattingParams = serde_json::from_value(params)?;
            let uri = &p.text_document.uri;
            let state = match server.documents.get(&uri_key(uri)) {
                Some(s) => s,
                None => return Ok(None),
            };
            let edits = handlers::formatting(state);
            Ok(Some(serde_json::to_value(edits)?))
        }
        _ => Ok(None),
    }
}

/// Allocate (or reuse) a [`SourceId`] for a URI key.
///
/// v1.2 keeps things simple: each URI gets a monotonically increasing
/// SourceId. The map is keyed by URI string so reopening the same file
/// keeps the same id.
fn fresh_source_id(map: &mut HashMap<UriKey, SourceId>, key: &UriKey) -> SourceId {
    let next = map.len() as u32;
    *map.entry(key.clone()).or_insert(SourceId(next))
}

/// Convenience wrapper exposed for tests / embedding: build a ServerState
/// with one document pre-opened.
#[doc(hidden)]
pub fn __test_server_with_doc(uri: Uri, src: &str) -> (ServerState, SourceId) {
    let mut state = ServerState::default();
    let sid = SourceId(0);
    state
        .documents
        .insert(uri_key(&uri), DocumentState::new(src.to_string(), sid, None));
    (state, sid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_state_opens_and_updates_doc() {
        let uri: Uri = "file:///t.buff".parse().unwrap();
        let (mut state, _) = __test_server_with_doc(uri.clone(), "func a():\n    print(1)\n");
        let key = uri_key(&uri);
        assert!(state.documents.contains_key(&key));
        state.documents.get_mut(&key).unwrap().update(
            "func b():\n    print(2)\n".to_string(),
            Some(2),
        );
        assert_eq!(state.documents[&key].text, "func b():\n    print(2)\n");
    }

    #[test]
    fn dispatch_hover_returns_value_for_open_doc() {
        let uri: Uri = "file:///t.buff".parse().unwrap();
        let (mut state, _) = __test_server_with_doc(
            uri.clone(),
            "func main():\n    let x = 42\n    print(x)\n",
        );
        let hover_params = serde_json::json!({
            "textDocument": {"uri": uri.as_str()},
            "position": {"line": 1, "character": 8},
        });
        let v = dispatch_request("textDocument/hover", hover_params, &mut state).expect("hover");
        assert!(v.is_some(), "expected hover value");
        let s = v.unwrap().to_string();
        assert!(s.contains("Int"), "expected Int in: {s}");
    }

    #[test]
    fn dispatch_unknown_method_returns_none() {
        let mut state = ServerState::default();
        let v = dispatch_request(
            "textDocument/doesNotExist",
            serde_json::Value::Null,
            &mut state,
        )
        .expect("dispatch");
        assert!(v.is_none());
    }

    #[test]
    fn debounce_idle_constant_is_300ms() {
        // Spec contract — must NOT drift without an explicit plan change.
        assert_eq!(DEBOUNCE_IDLE, Duration::from_millis(300));
    }
}
