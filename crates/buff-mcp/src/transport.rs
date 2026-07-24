//! Stdio framed-JSON transport for MCP (T62).
//!
//! MCP messages are exchanged as newline-delimited JSON-RPC 2.0 over
//! stdio. One `serde_json::Value` per line on stdin (requests +
//! notifications from the client) and stdout (responses from us). All
//! log / diagnostic output goes to stderr so the JSON stream on stdout
//! is never corrupted — mirrors `buff-lsp::server` + `buff-jupyter`.
//!
//! [`read_message`] reads one line (blocking) and parses it as JSON.
//! [`write_message`] serializes one [`serde_json::Value`] to stdout
//! followed by a single `\n` and flushes — atomic per message.
//!
//! [`run_stdio`] is the main loop: read -> dispatch -> write, until
//! stdin closes (EOF) or a fatal I/O error occurs.

use std::io::{self, BufRead, Write};

use serde_json::Value;

use crate::protocol::{handle_request, McpResponse};

/// Read one newline-delimited JSON message from stdin (blocking).
///
/// Returns `Ok(None)` on EOF (stdin closed — the standard shutdown
/// signal for an MCP stdio server). Returns `Err` only on I/O failure
/// of the underlying stdin handle.
///
/// Blank lines + lines that fail to parse as JSON are skipped (the
/// MCP spec is silent on framing noise from clients; we treat it as
/// benign and continue rather than tearing down the connection —
/// matches how `buff-lsp::server` handles malformed JSON-RPC frames
/// from the `lsp-server` crate).
///
/// # Errors
///
/// Propagates `std::io::Error` from the underlying `BufReader`.
pub fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        // Recurse past blank lines — read_message returns the next
        // non-blank line (or EOF).
        return read_message(reader);
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(v) => Ok(Some(v)),
        Err(_) => {
            // Malformed JSON — skip per the framing-noise policy
            // documented above. We do NOT return an error here
            // because tearing down the server on a single bad frame
            // would be catastrophic for the AI assistant's session.
            read_message(reader)
        }
    }
}

/// Write one JSON message to `writer` followed by a single `\n`,
/// then flush. Atomic per message (no inter-message buffering) so a
/// client reading line-by-line sees each response as soon as it's
/// produced.
///
/// # Errors
///
/// Propagates `std::io::Error` from the underlying writer (write or
/// flush failure).
pub fn write_message<W: Write>(writer: &mut W, message: &Value) -> io::Result<()> {
    // serde_json::to_string produces compact, single-line JSON (no
    // embedded newlines) — safe to append our own `\n` framing.
    let serialized = serde_json::to_string(message).unwrap_or_else(|_| {
        "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"internal serialization error\"}}"
            .to_string()
    });
    writer.write_all(serialized.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Run the MCP stdio main loop until stdin closes (EOF) or a fatal
/// I/O error occurs.
///
/// One iteration: read a message from stdin -> dispatch via
/// [`handle_request`] -> write the response (when present —
/// notifications produce no response per JSON-RPC 2.0) to stdout.
/// All log / debug output goes to stderr (via [`eprintln!`] — never
/// `println!`, which would corrupt the stdout JSON stream).
///
/// # Errors
///
/// Returns `Err` on stdin / stdout I/O failure (broken pipe, disk
/// full, etc.). The caller (`main.rs`) surfaces the error on stderr
/// and exits non-zero — mirrors `buff-lsp::server::run_stdio`.
pub fn run_stdio() -> Result<(), io::Error> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = io::BufWriter::new(stdout.lock());

    loop {
        let request = match read_message(&mut reader)? {
            Some(msg) => msg,
            None => {
                // EOF — clean shutdown.
                eprintln!("[buff-mcp] stdin closed, shutting down");
                return Ok(());
            }
        };

        match handle_request(&request) {
            Ok(Some(response)) => {
                let value = match response {
                    McpResponse::Success(value) => value,
                    McpResponse::Error(value) => value,
                };
                if let Err(e) = write_message(&mut writer, &value) {
                    eprintln!("[buff-mcp] stdout write failed: {e}");
                    return Err(e);
                }
            }
            Ok(None) => {
                // Notification (no `id` field) — no response needed
                // per JSON-RPC 2.0. Continue the loop.
            }
            Err(e) => {
                // Protocol-level error — the dispatcher already
                // produced a JSON-RPC error response. Surface it.
                eprintln!("[buff-mcp] protocol error: {e}");
                if let Err(write_err) = write_message(&mut writer, &e.to_json()) {
                    eprintln!("[buff-mcp] stdout write failed: {write_err}");
                    return Err(write_err);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_message_parses_single_json_object() {
        let mut input =
            Cursor::new(b"{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"id\":1}\n".to_vec());
        let msg = read_message(&mut input)
            .expect("read")
            .expect("some message");
        assert_eq!(msg["method"], "ping");
        assert_eq!(msg["id"], 1);
    }

    #[test]
    fn read_message_returns_none_on_eof() {
        let mut input = Cursor::new(Vec::<u8>::new());
        let msg = read_message(&mut input).expect("read");
        assert!(msg.is_none(), "expected None on EOF");
    }

    #[test]
    fn read_message_skips_blank_lines() {
        let mut input = Cursor::new(b"\n\n  \n{\"ok\":true}\n".to_vec());
        let msg = read_message(&mut input)
            .expect("read")
            .expect("some message");
        assert_eq!(msg["ok"], true);
    }

    #[test]
    fn read_message_skips_malformed_json() {
        let mut input = Cursor::new(b"this is not json\n{\"ok\":true}\n".to_vec());
        let msg = read_message(&mut input)
            .expect("read")
            .expect("some message");
        assert_eq!(msg["ok"], true);
    }

    #[test]
    fn write_message_emits_compact_json_plus_newline() {
        let mut output: Vec<u8> = Vec::new();
        let value = serde_json::json!({"hello": "world", "n": 42});
        write_message(&mut output, &value).expect("write");
        let s = String::from_utf8(output).expect("utf8");
        assert!(s.ends_with('\n'), "trailing newline: {s:?}");
        assert!(
            !s.contains('\n') || s.matches('\n').count() == 1,
            "single line: {s:?}"
        );
        assert!(s.contains("\"hello\":\"world\""), "compact: {s:?}");
    }
}
