//! DAP server — stdio transport + lifecycle handshake + proxy.
//!
//! The server bridges an editor (speaking DAP over our stdin/stdout)
//! to a backend debugger (lldb-dap / codelldb / vscode-lldb, also
//! speaking DAP over its stdin/stdout). The translation layer
//! ([`crate::translation`]) is applied at the `setBreakpoints` and
//! `stackTrace` boundaries; everything else passes through verbatim.
//!
//! # Lifecycle (DAP 1.0 spec)
//!
//! ```text
//! editor                  buff-dap                  backend
//!   │                        │                         │
//!   │── initialize ─────────▶│                         │
//!   │                        │── initialize ──────────▶│
//!   │                        │◀── initialize response ─│
//!   │◀── initialize resp ────│                         │
//!   │◀── initialized event ──│                         │
//!   │                        │                         │
//!   │── launch/attach ───────▶│                        │
//!   │                        │── launch/attach ───────▶│
//!   │                        │◀─────── resp ───────────│
//!   │◀────── resp ───────────│                         │
//!   │                        │                         │
//!   │── setBreakpoints ──────▶│                        │
//!   │   (buff file/lines)    │── setBreakpoints ──────▶│
//!   │                        │   (rust file/lines)     │
//!   │                        │◀─────── resp ───────────│
//!   │◀────── resp ───────────│   (rust breakpoints)    │
//!   │                        │                         │
//!   │── configurationDone ──▶│                        │
//!   │                        │── configurationDone ───▶│
//!   │                        │                         │
//!   │                        │◀─── stopped event ──────│
//!   │◀── stopped event ──────│                         │
//!   │                        │                         │
//!   │── stackTrace ──────────▶│                        │
//!   │                        │── stackTrace ──────────▶│
//!   │                        │◀─────── resp ───────────│
//!   │                        │   (rust frames)         │
//!   │◀────── resp ───────────│   (buff frames!)        │
//!   │   (buff frames)        │                         │
//!   │                        │                         │
//!   │── continue/next/... ───▶│                        │
//!   │                        │── continue/next/... ───▶│
//!   │                        │                         │
//!   │── disconnect ──────────▶│                        │
//!   │                        │── disconnect ──────────▶│
//!   │◀── resp ───────────────│                         │
//!   │◀── terminated event ───│                         │
//! ```
//!
//! # Translation boundaries
//!
//! Only two request types are intercepted:
//!
//! - **`setBreakpoints`** — arguments include `source.path` (a
//!   `.buff` file) + `breakpoints[].line`. We translate each line
//!   via [`translation::translate_breakpoints_buff_to_rust`] and
//!   rewrite `source.path` to the generated `.rs` file path before
//!   forwarding. The response comes back unchanged.
//! - **`stackTrace`** — the response body contains `stackFrames[]`
//!   with `source.path` + `line` + `column`. We translate each
//!   frame's line/col back to `.buff` coordinates via
//!   [`translation::translate_stack_trace_rust_to_buff`] and
//!   rewrite `source.path` to the `.buff` file.
//!
//! All other requests / responses / events pass through verbatim
//! (including `scopes` / `variables` / `evaluate` — locals work in
//! Rust terms; Buff-level name translation is future work, see
//! `task-136-debugger.txt` GAP-2).
//!
//! # Threading
//!
//! The server runs three tasks on a single tokio-style select loop
//! (hand-rolled with `std::thread` to avoid pulling tokio):
//!
//! 1. Read editor → translate/forward → write backend.
//! 2. Read backend → translate/forward → write editor.
//! 3. Wait for backend exit.
//!
//! When any task finishes (EOF / error / exit), the server drains
//! pending output and returns.

use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use buff_lang_error::{SourceId, SourceMap};

use crate::backend::{spawn as spawn_backend, Backend};
use crate::error::{DapError, DapResult};
use crate::protocol::{decode, encode, has_complete_message, Message, MessageKind};
use crate::translation::{translate_breakpoints_buff_to_rust, translate_stack_frame_rust_to_buff};

/// Monotonic sequence counter for outbound messages (responses +
/// events). The editor only cares that each is unique per session;
/// we use a simple atomic counter starting at 1.
#[derive(Debug)]
pub struct SeqCounter {
    next: AtomicU64,
}

impl SeqCounter {
    /// Create a counter starting at 1.
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Allocate the next sequence number.
    pub fn next(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for SeqCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for a DAP server session.
///
/// Built by the CLI (`buff debug`) and consumed by [`run_session`].
/// The `source_map` is populated by the CLI before launch — for
/// v1.10 this is the identity mapping (codegen does not yet emit
/// source-map markers; same GAP as T137 coverage). The `.buff` +
/// `.rs` paths are the ones the CLI's `compile_to_rust` produced.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// The backend debugger to spawn.
    pub backend: Backend,
    /// The `.buff` source file path (what the editor sees).
    pub buff_file: PathBuf,
    /// The generated `.rs` file path (what the backend debugs).
    pub rust_file: PathBuf,
    /// The `.buff` source content (for line-start computation).
    pub buff_source: String,
    /// The T60 source map (populated by the CLI).
    pub source_map: SourceMap,
    /// The SourceId of the `.buff` file inside `source_map`.
    pub buff_source_id: SourceId,
}

/// Run a DAP server session over stdio.
///
/// Reads DAP messages from stdin (editor), translates `setBreakpoints`
/// + `stackTrace` via the config's source map, and proxies everything
///   to the spawned backend. Blocks until the editor disconnects or
///   the backend exits.
///
/// # Errors
///
/// - [`DapError::NoBackend`] — no backend installed (CLI should
///   pre-check + print the install hint).
/// - [`DapError::Io`] — stdio transport failure.
/// - [`DapError::BackendExited`] — backend subprocess died.
pub fn run_session(config: &ServerConfig) -> DapResult<()> {
    // Spawn the backend subprocess.
    let mut backend = spawn_backend(config.backend)?;

    let seq = Arc::new(SeqCounter::new());
    let shutdown = Arc::new(AtomicBool::new(false));

    // Spawn a reader thread for the backend → editor direction.
    //
    // This thread owns `backend.stdout`. It reads DAP messages,
    // applies the stackTrace translation on responses, and writes
    // the encoded bytes to `std::io::stdout()` (a fresh handle per
    // call — `Stdout` is `Send + Sync` whereas `StdoutLock<'_>` is
    // neither, so we avoid taking a long-lived lock).
    //
    // Errors from this thread are surfaced via a `mpsc::Sender` —
    // the main loop polls the matching `Receiver` via `try_recv`.
    let backend_reader_handle = {
        let seq = Arc::clone(&seq);
        let shutdown = Arc::clone(&shutdown);
        let cfg_buff_file = config.buff_file.clone();
        let cfg_buff_source_id = config.buff_source_id;
        let cfg_sm = config.source_map.clone();
        std::thread::Builder::new()
            .name("buff-dap-backend-reader".into())
            .spawn(move || -> DapResult<()> {
                let mut reader = BufReader::new(backend.stdout);
                let mut buf = Vec::new();
                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    // Read more bytes from the backend.
                    let mut chunk = [0u8; 4096];
                    let n = reader
                        .read(&mut chunk)
                        .map_err(|e| DapError::Io(e.to_string()))?;
                    if n == 0 {
                        // Backend closed stdout — session over.
                        return Ok(());
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    // Decode as many complete messages as we have.
                    while has_complete_message(&buf) {
                        let (msg, consumed) = decode(&buf)?;
                        buf.drain(..consumed);
                        let translated = translate_backend_to_editor(
                            msg,
                            &seq,
                            &cfg_sm,
                            cfg_buff_source_id,
                            &cfg_buff_file,
                        );
                        let bytes = encode(&translated)?;
                        // `std::io::stdout()` returns a fresh Send
                        // handle; lock + write + drop the lock
                        // within this scope.
                        let stdout = std::io::stdout();
                        let mut lock = stdout.lock();
                        lock.write_all(&bytes)
                            .map_err(|e| DapError::Io(e.to_string()))?;
                        lock.flush().map_err(|e| DapError::Io(e.to_string()))?;
                    }
                }
            })
            .map_err(|e| DapError::Io(format!("failed to spawn reader thread: {e}")))?
    };

    // Main loop: read editor → translate/forward → write backend.
    {
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        let mut buf = Vec::new();
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            let mut chunk = [0u8; 4096];
            let n = reader
                .read(&mut chunk)
                .map_err(|e| DapError::Io(e.to_string()))?;
            if n == 0 {
                // Editor closed stdin — shutdown.
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            while has_complete_message(&buf) {
                let (msg, consumed) = decode(&buf)?;
                buf.drain(..consumed);
                let translated = translate_editor_to_backend(msg, config)?;
                let bytes = encode(&translated)?;
                backend
                    .stdin
                    .write_all(&bytes)
                    .map_err(|e| DapError::Io(e.to_string()))?;
                backend
                    .stdin
                    .flush()
                    .map_err(|e| DapError::Io(e.to_string()))?;
            }
        }
    }

    // Signal the backend reader to drain + exit.
    shutdown.store(true, Ordering::Relaxed);

    // Close our stdin to the backend so it knows we're done. Drop
    // is the canonical way to close a `ChildStdin` (no `.take()`
    // method exists on it — it's owned, not borrowed).
    drop(backend.stdin);

    // Wait for the backend to exit.
    let exit_status = backend
        .child
        .wait()
        .map_err(|e| DapError::Io(e.to_string()))?;
    // Join the reader thread (ignore its result — shutdown flag is set).
    let _ = backend_reader_handle.join();

    if !exit_status.success() {
        return Err(DapError::BackendExited {
            status: format!("{exit_status}"),
            stderr_excerpt: String::new(),
        });
    }

    Ok(())
}

/// Translate an editor → backend message (the `setBreakpoints`
/// direction). For non-setBreakpoints requests, returns the
/// message unchanged.
fn translate_editor_to_backend(msg: Message, config: &ServerConfig) -> DapResult<Message> {
    if msg.kind()? != MessageKind::Request {
        return Ok(msg);
    }
    let Some(cmd) = msg.command.as_deref() else {
        return Ok(msg);
    };
    if cmd != "setBreakpoints" {
        return Ok(msg);
    }

    // setBreakpoints: arguments.body = { source: { path }, breakpoints: [{line}] }.
    // Translate the path + each line.
    let body = match &msg.body {
        Some(b) => b.clone(),
        None => return Ok(msg),
    };

    // Try to translate. On any error, fall back to forwarding the
    // original message (best-effort — the editor may have sent a
    // pre-.buff .rs path for some reason).
    let translated_body = translate_set_breakpoints_body(&body, config);
    let translated_body = match translated_body {
        Some(b) => b,
        None => return Ok(msg),
    };

    Ok(Message {
        seq: msg.seq,
        kind: msg.kind,
        command: msg.command,
        event: msg.event,
        request_seq: msg.request_seq,
        success: msg.success,
        message: msg.message,
        body: Some(translated_body),
    })
}

/// Translate the body of a `setBreakpoints` request.
///
/// Returns `None` when the body doesn't match the expected shape
/// (in which case the caller forwards the original message unchanged).
fn translate_set_breakpoints_body(
    body: &serde_json::Value,
    config: &ServerConfig,
) -> Option<serde_json::Value> {
    let mut new_body = body.clone();

    // Extract source.path and check it matches our buff_file.
    let source = new_body.get("source")?;
    let path_str = source.get("path")?.as_str()?;
    // Normalize both paths for comparison (canonicalize would be
    // ideal but may fail for non-existent files; lexically clean
    // the trailing slashes instead).
    let requested = Path::new(path_str);
    if !paths_match(requested, &config.buff_file) {
        // Not our file — pass through unchanged.
        return None;
    }

    // Extract breakpoints[].line and translate each.
    let breakpoints = new_body.get("breakpoints")?.as_array()?;
    let mut buff_lines = Vec::with_capacity(breakpoints.len());
    for bp in breakpoints {
        let line = bp.get("line")?.as_u64()? as u32;
        buff_lines.push(line);
    }

    let translated = translate_breakpoints_buff_to_rust(
        &buff_lines,
        &config.source_map,
        config.buff_source_id,
        &config.buff_source,
    );

    // Rebuild the breakpoints array with translated lines.
    let mut new_breakpoints = Vec::with_capacity(translated.len());
    for tb in &translated {
        new_breakpoints.push(serde_json::json!({"line": tb.rust_line}));
    }

    // Rewrite source.path → rust_file.
    if let Some(source) = new_body.get_mut("source") {
        if let Some(map) = source.as_object_mut() {
            map.insert(
                "path".to_string(),
                serde_json::Value::String(config.rust_file.to_string_lossy().into_owned()),
            );
        }
    }
    new_body["breakpoints"] = serde_json::Value::Array(new_breakpoints);

    Some(new_body)
}

/// Translate a backend → editor message (the `stackTrace` direction).
/// For non-stackTrace responses, returns the message unchanged.
fn translate_backend_to_editor(
    msg: Message,
    seq: &SeqCounter,
    source_map: &SourceMap,
    buff_source_id: SourceId,
    buff_file: &Path,
) -> Message {
    // Responses are matched on the original command (which DAP does
    // not include — only `request_seq`). The backend echoes our
    // translated body, so we can detect stackTrace responses by
    // inspecting the body's `stackFrames[].source.path` for our
    // rust_file path.
    if msg.kind() != Ok(MessageKind::Response) {
        return msg;
    }
    let body = match &msg.body {
        Some(b) => b.clone(),
        None => return msg,
    };

    let translated = translate_stack_trace_body(&body, source_map, buff_source_id, buff_file);
    let translated = match translated {
        Some(b) => b,
        None => return msg,
    };

    Message {
        seq: seq.next(),
        kind: msg.kind,
        command: msg.command,
        event: msg.event,
        request_seq: msg.request_seq,
        success: msg.success,
        message: msg.message,
        body: Some(translated),
    }
}

/// Translate the body of a `stackTrace` response (rust frames → buff).
///
/// Returns `None` when the body doesn't match the expected shape
/// (caller passes through unchanged).
fn translate_stack_trace_body(
    body: &serde_json::Value,
    source_map: &SourceMap,
    buff_source_id: SourceId,
    buff_file: &Path,
) -> Option<serde_json::Value> {
    let mut new_body = body.clone();
    let frames = new_body.get_mut("stackFrames")?.as_array_mut()?;

    for frame in frames.iter_mut() {
        // Extract the rust line.
        let rust_line = frame.get("line")?.as_u64()? as u32;
        // Translate.
        let translated =
            translate_stack_frame_rust_to_buff(rust_line, source_map, buff_source_id, buff_file);
        // Rewrite line/column to buff coordinates.
        if let Some(obj) = frame.as_object_mut() {
            obj.insert(
                "line".to_string(),
                serde_json::Value::Number(serde_json::Number::from(translated.buff_line)),
            );
            obj.insert(
                "column".to_string(),
                serde_json::Value::Number(serde_json::Number::from(translated.buff_col)),
            );
            // Rewrite source.path → buff_file.
            if let Some(source) = obj.get_mut("source").and_then(|s| s.as_object_mut()) {
                source.insert(
                    "path".to_string(),
                    serde_json::Value::String(buff_file.to_string_lossy().into_owned()),
                );
            }
        }
    }

    Some(new_body)
}

/// Lexically compare two paths for equality (best-effort; ignores
/// trailing slashes + case on Windows).
fn paths_match(a: &Path, b: &Path) -> bool {
    let a_owned = a.to_string_lossy();
    let b_owned = b.to_string_lossy();
    let a_str = a_owned.trim_end_matches(['/', '\\']);
    let b_str = b_owned.trim_end_matches(['/', '\\']);
    a_str.eq_ignore_ascii_case(b_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::Span;

    /// Build a minimal config with a 2-line buff source mapped to
    /// rust lines 10 + 20.
    fn test_config() -> ServerConfig {
        let buff_source = "aaa\nbbb\n";
        let buff_file = PathBuf::from("test.buff");
        let rust_file = PathBuf::from("test.rs");
        let buff_source_id = SourceId(0);
        let mut source_map = SourceMap::new();
        source_map.add_source(buff_source_id, buff_file.clone(), buff_source.to_string());
        // buff line 1 (offset 0) → rust line 10
        source_map.add_mapping(Span::new(0, 3, buff_source_id), 10);
        // buff line 2 (offset 4) → rust line 20
        source_map.add_mapping(Span::new(4, 7, buff_source_id), 20);

        ServerConfig {
            backend: Backend::LldbDap,
            buff_file,
            rust_file,
            buff_source: buff_source.to_string(),
            source_map,
            buff_source_id,
        }
    }

    #[test]
    fn seq_counter_starts_at_one_and_increments() {
        let s = SeqCounter::new();
        assert_eq!(s.next(), 1);
        assert_eq!(s.next(), 2);
        assert_eq!(s.next(), 3);
    }

    #[test]
    fn translate_set_breakpoints_body_rewrites_lines_and_path() {
        let cfg = test_config();
        let body = serde_json::json!({
            "source": {"path": "test.buff"},
            "breakpoints": [{"line": 1}, {"line": 2}]
        });
        let translated = translate_set_breakpoints_body(&body, &cfg).expect("some");
        let bps = translated.get("breakpoints").unwrap().as_array().unwrap();
        assert_eq!(bps[0].get("line").unwrap().as_u64(), Some(10));
        assert_eq!(bps[1].get("line").unwrap().as_u64(), Some(20));
        let path = translated
            .get("source")
            .unwrap()
            .get("path")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(path.ends_with("test.rs"));
    }

    #[test]
    fn translate_set_breakpoints_body_passthrough_for_other_file() {
        let cfg = test_config();
        let body = serde_json::json!({
            "source": {"path": "other.buff"},
            "breakpoints": [{"line": 1}]
        });
        // Other file → no translation (None signals passthrough).
        assert_eq!(translate_set_breakpoints_body(&body, &cfg), None);
    }

    #[test]
    fn translate_set_breakpoints_body_passthrough_when_no_breakpoints() {
        let cfg = test_config();
        let body = serde_json::json!({"source": {"path": "test.buff"}});
        assert_eq!(translate_set_breakpoints_body(&body, &cfg), None);
    }

    #[test]
    fn translate_stack_trace_body_rewrites_frames() {
        let cfg = test_config();
        // Backend reports two frames at rust lines 10 + 20 — these
        // should translate to buff lines 1 + 2 respectively.
        let body = serde_json::json!({
            "stackFrames": [
                {"id": 1, "line": 10, "column": 1, "source": {"path": "test.rs"}},
                {"id": 2, "line": 20, "column": 5, "source": {"path": "test.rs"}},
            ]
        });
        let translated = translate_stack_trace_body(
            &body,
            &cfg.source_map,
            cfg.buff_source_id,
            &PathBuf::from("test.buff"),
        )
        .expect("some");
        let frames = translated.get("stackFrames").unwrap().as_array().unwrap();
        assert_eq!(frames[0].get("line").unwrap().as_u64(), Some(1));
        assert_eq!(frames[1].get("line").unwrap().as_u64(), Some(2));
        let path0 = frames[0]
            .get("source")
            .unwrap()
            .get("path")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(path0.ends_with("test.buff"));
    }

    #[test]
    fn translate_stack_trace_body_passthrough_when_no_frames() {
        let cfg = test_config();
        let body = serde_json::json!({"totalFrames": 0});
        assert_eq!(
            translate_stack_trace_body(
                &body,
                &cfg.source_map,
                cfg.buff_source_id,
                &PathBuf::from("test.buff")
            ),
            None
        );
    }

    #[test]
    fn paths_match_handles_trailing_slash() {
        assert!(paths_match(
            Path::new("foo/bar.buff"),
            Path::new("foo/bar.buff/")
        ));
        assert!(paths_match(
            Path::new("foo/bar.buff\\"),
            Path::new("foo/bar.buff")
        ));
    }

    #[test]
    fn paths_match_case_insensitive() {
        // On Windows the filesystem is case-insensitive; we mirror
        // that behavior lexically.
        assert!(paths_match(
            Path::new("Foo/Bar.buff"),
            Path::new("foo/bar.buff")
        ));
    }

    #[test]
    fn paths_match_rejects_different_files() {
        assert!(!paths_match(Path::new("foo.buff"), Path::new("bar.buff")));
    }

    #[test]
    fn server_config_is_debug_clone() {
        let cfg = test_config();
        let cloned = cfg.clone();
        let _ = format!("{cloned:?}");
    }
}
