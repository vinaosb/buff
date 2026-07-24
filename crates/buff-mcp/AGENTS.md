# buff-mcp

Model Context Protocol (MCP) bridge server for the Buff language (T62,
v1.25 Wave 2a). Exposes Buff's compiler intelligence to AI assistants
(Claude, GPT, etc.) via JSON-RPC 2.0 over stdio. Wraps `buff-lsp`'s
existing pure handlers AND the CLI's standalone check / expand / format
entry points — NO logic is reimplemented, every tool delegates to a
canonical entry point.

## STRUCTURE

```
src/
├── lib.rs        # Module wiring + public API + constants (SERVER_NAME, PROTOCOL_VERSION)
├── main.rs       # Thin binary entry — calls transport::run_stdio
├── transport.rs  # stdio framed-JSON read/write (newline-delimited) + run_stdio main loop
├── protocol.rs   # JSON-RPC 2.0 envelope (McpRequest / McpResponse / McpError) + method dispatch
└── tools.rs      # MCP tool schema + dispatch + 6 tool handlers (buff_check / buff_hover / ...)
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new MCP tool | `tools.rs` (`tool_schemas` + `dispatch_tool` arm + new `handle_*` fn) |
| Change the MCP protocol version | `lib.rs::PROTOCOL_VERSION` constant |
| Change the JSON-RPC error codes | `protocol.rs` constants (PARSE_ERROR / INVALID_REQUEST / etc.) |
| Change the stdio framing | `transport.rs::read_message` / `write_message` |
| Add a new JSON-RPC method | `protocol.rs::dispatch` (new match arm + handler fn) |
| Change how tool results are serialized | `tools.rs::ToolResult::to_json` |
| Tune the AI-facing tool descriptions | `tools.rs::tool_schemas` (the `description` fields — these are what the AI sees) |

## CONVENTIONS (this crate only)

- **NO `unwrap`/`expect`/`panic!` in non-test code.** The whole codebase rule applies here too. The `file_uri` helper returns `Result<lsp_types::Uri, ToolError>` rather than unwrapping — the lsp-types 0.97 `Uri` (a newtype over `fluent_uri::Uri<String>`) has only `FromStr` (no `Default`, no `from_file_path`), so parse failures propagate as `ToolError::Execution`.

- **Reuse, don't reimplement.** Every MCP tool delegates to an existing entry point:
  - `buff_check` → `buff_lang_cli::check::check_source` (T55)
  - `buff_hover` → `buff_lsp::handlers::hover`
  - `buff_complete` → `buff_lsp::handlers::completion`
  - `buff_goto_def` → `buff_lsp::handlers::goto_definition`
  - `buff_format` → `buff_lang_cli::fmt::format_source` (T54 — same fn the LSP `formatting` handler uses)
  - `buff_expand` → `buff_lang_cli::pipeline::compile_to_rust` (T115)
  Adding a new MCP tool that does something the LSP / CLI already does is a bug — wire the existing fn instead.

- **Stateless per tool call.** Every `tools/call` reads the file from disk + builds a fresh `DocumentState`. No session state is kept across calls (the server is stateless beyond the `initialize` handshake). This mirrors how `buff-lsp` v1.2 reparses on every `didChange`.

- **Tool errors vs protocol errors.** [`ToolError::UnknownTool`] / [`ToolError::MissingArg`] → JSON-RPC errors (the call was malformed — tears down session in strict clients). [`ToolError::Execution`] → MCP error content block (`isError: true` — the call was well-formed but the tool reported an error condition like "file not found"). This distinction is mandated by the MCP spec: a JSON-RPC error is catastrophic; an MCP error content block is recoverable.

- **All log output to stderr.** The JSON stream on stdout MUST stay clean. Use `eprintln!` (never `println!`) for any diagnostic output. Mirrors `buff-lsp::server` + `buff-jupyter`.

- **Transport is stdio, newline-delimited.** One JSON object per line. NO Content-Length headers (that's LSP, not MCP). NO HTTP server, NO WebSocket — the T62 spec explicitly forbids both. The protocol is implemented directly (no external MCP SDK dep).

- **Protocol version pinned.** `PROTOCOL_VERSION = "2024-11-05"` (the stable baseline every shipping MCP server + client supports as of v1.25). Bumping is coordinated with buff-lsp version bumps — never change ad-hoc.

- **Tests in `tests/`.** Unit tests live inline (`#[cfg(test)] mod tests`); integration tests that drive the protocol end-to-end would live in `tests/`. The current tests write unique temp files via `std::env::temp_dir` + process-id + nanosecond timestamp to avoid collisions across parallel test runs.

## DEPENDENCIES

- `serde` / `serde_json` (JSON-RPC serialization)
- `thiserror` (error derives — for `ToolError` if it grows)
- `buff-lsp` (provides `DocumentState` + `handlers::{hover, completion, goto_definition}`)
- `buff-lang-cli` (provides `check::check_source` T55, `fmt::format_source` T54, `pipeline::compile_to_rust` T115)
- `buff-lang-error` (provides `render_diagnostics_json` T1 + `SourceId`)
- `lsp-types` (provides `Position`, `Uri`, `HoverContents`, `MarkedString`, `CompletionResponse`, `GotoDefinitionResponse` — all consumed/produced by the `buff_lsp::handlers::*` fns)

## MCP TOOLS (v1.25 surface)

| Tool | Args | Delegates to | Returns |
|---|---|---|---|
| `buff_check` | `file` | `buff_lang_cli::check::check_source` (T55) | Structured JSON diagnostics (code / severity / message / spans / notes / suggestions) |
| `buff_hover` | `file`, `line`, `character` | `buff_lsp::handlers::hover` | Markdown: symbol name + kind + inferred type |
| `buff_complete` | `file`, `line`, `character` | `buff_lsp::handlers::completion` | JSON array of in-scope CompletionItems |
| `buff_goto_def` | `file`, `line`, `character` | `buff_lsp::handlers::goto_definition` | JSON Location (URI + 0-based line/character range) |
| `buff_format` | `file` | `buff_lang_cli::fmt::format_source` (T54) | Canonical-formatted source (fenced ```buff block) |
| `buff_expand` | `file` | `buff_lang_cli::pipeline::compile_to_rust` (T115) | Generated Rust (fenced ```rust block) |

All `line` / `character` args are **0-based** (LSP convention), not 1-based.

## LAUNCH (for AI assistants)

Configure in Claude Desktop's `claude_desktop_config.json` (or equivalent):

```json
{
  "mcpServers": {
    "buff": {
      "command": "buff-mcp",
      "transport": "stdio"
    }
  }
}
```

No flags, no TCP transport for v1.25.

## DEFERRED (v1.26+)

- **MCP `resources`**: expose Buff source files as MCP resources (so AI can read/write Buff files via the protocol, not just analyze them). v1.25 ships `tools` only.
- **MCP `prompts`**: ship built-in prompt templates ("explain this Buff error", "convert this Rust to Buff"). v1.25 ships `tools` only.
- **MCP `sampling`**: let the server ask the AI to generate code (e.g. auto-fix suggestions). v1.25 is read-only intelligence.
- **Cross-file goto-def**: currently single-file (mirrors buff-lsp v1.2). Cross-file is a v2.0 feature tracked with buff-lsp.
- **`buff_document_symbols` tool**: the `buff_lsp::handlers::document_symbols` fn exists but isn't exposed as an MCP tool yet — add when AI assistants ask for outline data.
- **`buff_references` tool**: find-references is not yet in buff-lsp v1.2; add the MCP tool when buff-lsp ships it.
- **Session state / multi-file**: the server is stateless per call (re-reads the file each time). A future `didOpen` / `didChange`-style session layer would let the server cache analysis across calls (mirrors buff-lsp's state model).
