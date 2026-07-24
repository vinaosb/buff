//! `buff-mcp` binary entry — thin dispatch to [`buff_mcp::run_stdio`].
//!
//! Mirrors the `buff-lsp/src/main.rs` pattern: keep `main.rs` thin
//! (no protocol logic) so integration tests can drive [`buff_mcp`]
//! directly via the library API without subprocess.

fn main() {
    if let Err(e) = buff_mcp::run_stdio() {
        eprintln!("[buff-mcp] fatal: {e}");
        std::process::exit(1);
    }
}
