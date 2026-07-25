//! `buff-mcp` — Model Context Protocol (MCP) bridge server for the Buff
//! language (T62, v1.25 Wave 2a).
//!
//! Exposes Buff's compiler intelligence to AI assistants (Claude, GPT,
//! etc.) via the Model Context Protocol — JSON-RPC 2.0 over stdio. The
//! server wraps [`buff_lsp`]'s existing pure handlers AND the CLI's
//! standalone check / expand / format entry points — NO logic is
//! reimplemented, every tool delegates to a canonical entry point.
//!
//! # Why MCP?
//!
//! MCP (https://spec.modelcontextprotocol.io/) is the open protocol
//! for connecting AI assistants to external tools. By wrapping
//! `buff-lsp` behind MCP, an AI assistant can:
//!
//! - **Diagnose** Buff code (`buff_check` — run `buff check`, return
//!   structured diagnostics the AI can reason about).
//! - **Hover** at a position (`buff_hover` — return inferred type +
//!   symbol kind as Markdown).
//! - **Complete** at a position (`buff_complete` — return in-scope
//!   symbols as JSON).
//! - **Goto definition** (`buff_goto_def` — return the in-file
//!   declaration span).
//! - **Format** source (`buff_format` — return the canonical-formatted
//!   source, byte-identical to `buff fmt`).
//! - **Expand** to Rust (`buff_expand` — return the generated Rust
//!   source, like `buff expand`).
//!
//! The AI gets the SAME answers an IDE would — the wrapper is a thin
//! transport translation, not a re-implementation.
//!
//! # Protocol
//!
//! The server speaks the MCP subset every MCP client (Claude Desktop,
//! Cursor, Continue, custom clients) supports:
//!
//! - **Transport**: stdio (newline-delimited JSON-RPC 2.0). One JSON
//!   object per line on stdin (requests / notifications) + stdout
//!   (responses). All log output goes to stderr so the JSON stream is
//!   never corrupted (mirrors `buff-lsp` / `buff-jupyter`).
//! - **Lifecycle**: `initialize` -> `initialized` notification ->
//!   `tools/list` -> `tools/call`* -> shutdown. The server is
//!   stateless beyond the handshake (every tool call reads the file
//!   from disk + builds a fresh [`buff_lsp::DocumentState`]).
//! - **Capabilities**: `tools` only (no `resources`, no `prompts`, no
//!   `sampling` — v1.25 ships a read-only intelligence surface).
//!
//! The protocol is implemented directly (no external MCP SDK dep) per
//! the T62 spec — keeping the dependency surface minimal matches the
//! "no C library, no Docker" hard rule + the wider workspace stance
//! that killed chumsky/logos/zmq.
//!
//! # Architecture
//!
//! ```text
//! stdin (JSON-RPC request) --> transport::read_message
//!                                  |
//!                                  v
//!                              protocol::dispatch
//!                                  |
//!        +-------------------------+-------------------------+
//!        v                         v                         v
//!  initialize /             tools/list                  tools/call
//!  initialized              (static tool schema)            |
//!                                                             v
//!                                                         tools::dispatch
//!                                                             |
//!        +--------------+--------------+--------------+--------+--------------+
//!        v              v              v              v             v              v
//!  buff_check      buff_hover    buff_complete  buff_goto_def  buff_format   buff_expand
//!  (CLI check)    (lsp handlers)  (lsp handlers) (lsp handlers) (CLI fmt)    (CLI pipeline)
//!        |              |              |              |             |              |
//!        v              v              v              v             v              v
//!  buff-lang-cli  buff-lsp::DocumentState + handlers      buff-lang-cli  buff-lang-cli
//!  check_source                                            fmt::format   pipeline::
//!                                                          source        compile_to_rust
//!                                                             |
//!                                                             v
//!                                              transport::write_message -> stdout
//! ```
//!
//! Every tool returns a [`ToolResult`] (a list of MCP content blocks —
//! one `text` block per call for v1.25). The protocol layer wraps it
//! in a JSON-RPC 2.0 success response. Tool-level errors (file not
//! found, missing args) are returned as MCP error content with
//! `is_error: true` (NOT JSON-RPC errors, which are reserved for
//! protocol-level failures per the MCP spec).
//!
//! # Crate layout
//!
//! - [`transport`] — stdio framed-JSON read/write (newline-delimited).
//! - [`protocol`] — JSON-RPC 2.0 envelope + method dispatch.
//! - [`tools`] — MCP tool schema + per-tool dispatch.
//! - `main.rs` — thin binary entry that calls [`run_stdio`].
//!
//! # References
//!
//! - MCP spec: https://spec.modelcontextprotocol.io/
//! - Plan: `.sisyphus/plans/buff-launch-readiness.md` task T62.
//! - Per-crate conventions: `crates/buff-mcp/AGENTS.md`.

// Boxing the MCP error type would reshape the public dispatch / run_stdio
// surface. Out of scope; allowed at the crate level.
#![allow(clippy::result_large_err)]

pub mod protocol;
pub mod tools;
pub mod transport;

pub use protocol::{dispatch, handle_request, McpError, McpRequest, McpResponse};
pub use tools::{dispatch_tool, tool_schemas, ToolError, ToolResult};
pub use transport::{read_message, run_stdio, write_message};

/// Crate version (matches `Cargo.toml`). Advertised in the MCP
/// `initialize` result's `serverInfo.version` field.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Server name advertised to MCP clients. Stable forever — clients
/// use this to identify the server in their config + UI.
pub const SERVER_NAME: &str = "buff-mcp";

/// Protocol version this server speaks. Follows the MCP spec's
/// `YYYY-MM-DD` date-stamp convention. Pinned to `2024-11-05` (the
/// stable baseline every shipping MCP server + client supports as of
/// v1.25; bumping is coordinated with the buff-lsp version bumps).
pub const PROTOCOL_VERSION: &str = "2024-11-05";
