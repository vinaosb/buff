//! `buff-lsp` — Language Server Protocol binary entry point.
//!
//! Reads JSON-RPC over stdio (one message per line, header-prefixed per
//! the LSP base protocol) and dispatches to [`buff_lsp::server`].
//!
//! # Launch
//!
//! ```text
//! buff-lsp                  # uses stdio transport (the default for VSCode)
//! ```
//!
//! The T118 VSCode extension bundles this binary and launches it with
//! `command: 'buff-lsp'` in the extension's `package.json` language-server
//! config. No flags, no TCP transport for v1.2 — stdio only.

fn main() {
    // Errors here are unrecoverable (broken pipe = client disconnected,
    // JSON error = protocol violation). Surface them on stderr and exit
    // non-zero so the client logs the failure. We must NOT write the error
    // to stdout (that's the JSON-RPC channel).
    if let Err(e) = buff_lsp::run_stdio() {
        eprintln!("buff-lsp: {e}");
        std::process::exit(1);
    }
}
