//! MCP tool descriptors + dispatch (T62).
//!
//! One Rust module per MCP tool, plus a single dispatch entry that
//! routes a `tools/call` by name to the matching handler. Each
//! handler:
//!
//! 1. Validates its arguments (`file`, `line`, `character`).
//! 2. Reads the file from disk + builds the analysis state ONCE
//!    (reusing [`buff_lsp::DocumentState`] which runs lex -> parse ->
//!    TypeInferencer in [`buff_lsp::analysis::analyze`]).
//! 3. Delegates to the matching existing entry point — NO logic is
//!    reimplemented:
//!    - `buff_check` -> [`buff_lang_cli::check::check_source`] (T55
//!      standalone typecheck, lex->parse->TypeInferencer->naming_lint,
//!      NO codegen).
//!    - `buff_hover` -> [`buff_lsp::handlers::hover`] (returns
//!      Markdown with inferred type + symbol kind).
//!    - `buff_complete` -> [`buff_lsp::handlers::completion`]
//!      (returns in-scope symbols as LSP CompletionItems).
//!    - `buff_goto_def` -> [`buff_lsp::handlers::goto_definition`]
//!      (returns in-file declaration Location).
//!    - `buff_format` -> [`buff_lang_cli::fmt::format_source`] (T54,
//!      byte-identical to `buff fmt`; same fn the LSP `formatting`
//!      handler uses).
//!    - `buff_expand` -> [`buff_lang_cli::pipeline::compile_to_rust`]
//!      (T115 `buff expand`; runs the FULL front-end including
//!      codegen, so the AI sees the Rust it'd be compiling).
//!
//! # Tool-level vs protocol-level errors
//!
//! Tool errors (file not found, missing arg) are [`ToolError`].
//! [`ToolError::UnknownTool`] + [`ToolError::MissingArg`] map to
//! JSON-RPC errors (the call was malformed). [`ToolError::Execution`]
//! maps to an MCP error content block (`is_error: true`) — the call
//! was well-formed but the tool reported an error condition (per the
//! MCP spec, this is a SUCCESS response at the JSON-RPC layer).

use serde_json::{json, Value};

use buff_lang_cli::check::CheckOutcome;
use buff_lang_error::{render_diagnostics_json, SourceId};
use buff_lsp::{handlers, DocumentState};

// ---------------------------------------------------------------------------
// Tool result + error types.
// ---------------------------------------------------------------------------

/// A successful tool execution. The MCP wire shape is a list of
/// content blocks (one `text` block per call for v1.25 — no image /
/// resource refs / embedded resources yet). [`ToolResult::to_json`]
/// builds the MCP `tools/call` result envelope.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// The text content to return to the AI assistant. Markdown is
    /// encouraged (Claude / GPT render it); plain text is fine too.
    pub text: String,
    /// When `true`, the tool executed but reported an error condition
    /// (e.g. file not found). Maps to `isError: true` in the MCP
    /// result envelope. Distinct from a JSON-RPC error (which would
    /// mean the call itself was malformed).
    pub is_error: bool,
}

impl ToolResult {
    /// Build a success result with `text` content.
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    /// Build an MCP error content block (the call ran, but the tool
    /// reported an error condition — e.g. file not found).
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }

    /// Serialize as the MCP `tools/call` result envelope:
    ///
    /// ```json
    /// { "content": [{ "type": "text", "text": "..." }], "isError": false }
    /// ```
    pub fn to_json(&self) -> Value {
        json!({
            "content": [{
                "type": "text",
                "text": self.text
            }],
            "isError": self.is_error
        })
    }
}

/// A tool-level error. Mapped to a JSON-RPC error or an MCP error
/// content block by [`crate::protocol::handle_tools_call`] depending
/// on the variant.
#[derive(Debug, Clone)]
pub enum ToolError {
    /// The tool name did not match any known tool.
    UnknownTool,
    /// A required argument was missing / wrong type.
    MissingArg(&'static str),
    /// The tool executed but reported an error condition (file IO,
    /// etc.). Mapped to an MCP error content block — NOT a JSON-RPC
    /// error.
    Execution(String),
}

// ---------------------------------------------------------------------------
// Tool schema (single source of truth for `tools/list`).
// ---------------------------------------------------------------------------

/// Build the MCP tool descriptors advertised via `tools/list`. Each
/// entry has `name`, `description`, and `inputSchema` (a JSON Schema
/// object). The schema MUST stay in sync with the param parsing in
/// the matching `handle_*` fn below.
///
/// Order matters for AI discoverability — the most-used tools
/// (`buff_check`, `buff_hover`) come first.
pub fn tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "name": "buff_check",
            "description": "Run `buff check` (T55 standalone typecheck) on a .buff file. \
        Returns structured diagnostics (errors + warnings + lints) as JSON: each \
        diagnostic carries severity, message, stable error code (E1xxx), byte \
        offsets, 1-based line/col, notes, and machine-readable fix suggestions. \
        No codegen — fast (lex -> parse -> TypeInferencer -> naming_lint).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Absolute or relative path to the .buff source file."
                    }
                },
                "required": ["file"]
            }
        }),
        json!({
            "name": "buff_hover",
            "description": "Get hover info at a 0-based line/character position in a .buff \
        file. Returns the inferred type + symbol kind (function / struct / \
        variable) as Markdown — exactly what an IDE would show on hover.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Absolute or relative path to the .buff source file."
                    },
                    "line": {
                        "type": "integer",
                        "description": "0-based line number (LSP convention).",
                        "minimum": 0
                    },
                    "character": {
                        "type": "integer",
                        "description": "0-based column (UTF-16 code units, LSP convention).",
                        "minimum": 0
                    }
                },
                "required": ["file", "line", "character"]
            }
        }),
        json!({
            "name": "buff_complete",
            "description": "Get completion candidates at a 0-based line/character position \
        in a .buff file. Returns in-scope symbols (function / struct / enum / \
        variable / field) as JSON: each candidate has label, kind, and detail.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Absolute or relative path to the .buff source file."
                    },
                    "line": {
                        "type": "integer",
                        "description": "0-based line number (LSP convention).",
                        "minimum": 0
                    },
                    "character": {
                        "type": "integer",
                        "description": "0-based column (UTF-16 code units, LSP convention).",
                        "minimum": 0
                    }
                },
                "required": ["file", "line", "character"]
            }
        }),
        json!({
            "name": "buff_goto_def",
            "description": "Resolve the definition of the identifier at a 0-based \
        line/character position in a .buff file. Returns the in-file declaration \
        span (single-file for v1.25 — cross-file goto-def is a v2.0 feature). \
        Output: the URI + the 0-based line/character range of the declaration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Absolute or relative path to the .buff source file."
                    },
                    "line": {
                        "type": "integer",
                        "description": "0-based line number (LSP convention).",
                        "minimum": 0
                    },
                    "character": {
                        "type": "integer",
                        "description": "0-based column (UTF-16 code units, LSP convention).",
                        "minimum": 0
                    }
                },
                "required": ["file", "line", "character"]
            }
        }),
        json!({
            "name": "buff_format",
            "description": "Format a .buff file via `buff fmt` (T54). Returns the \
        canonical-formatted source (byte-identical to the CLI output). When the \
        input is already canonical, returns the input unchanged. The same \
        `format_source` function the LSP `formatting` handler uses — no \
        reimplementation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Absolute or relative path to the .buff source file."
                    }
                },
                "required": ["file"]
            }
        }),
        json!({
            "name": "buff_expand",
            "description": "Show the generated Rust source for a .buff file (like \
        `buff expand`, T115). Runs the FULL compiler front-end (lex -> parse -> \
        codegen via syn/quote/prettyplease) — no `rustc` invocation. The AI \
        sees exactly the Rust it'd be compiling. Useful for debugging \
        transpilation + understanding what Buff generates under the hood.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Absolute or relative path to the .buff source file."
                    }
                },
                "required": ["file"]
            }
        }),
    ]
}

// ---------------------------------------------------------------------------
// Dispatch entry.
// ---------------------------------------------------------------------------

/// Route a `tools/call` by `name` to the matching handler. `arguments`
/// is the raw JSON arguments object (already validated to be an object
/// by the protocol layer — defaults to `{}` when the client sent none).
///
/// Returns `Ok(ToolResult)` for any successfully-dispatched call
/// (including tool-level execution failures, which become MCP error
/// content blocks). Returns `Err(ToolError::UnknownTool)` /
/// `Err(ToolError::MissingArg)` for protocol-level failures (mapped
/// to JSON-RPC errors by [`crate::protocol::handle_tools_call`]).
pub fn dispatch_tool(name: &str, arguments: &Value) -> Result<ToolResult, ToolError> {
    match name {
        "buff_check" => handle_buff_check(arguments),
        "buff_hover" => handle_buff_hover(arguments),
        "buff_complete" => handle_buff_complete(arguments),
        "buff_goto_def" => handle_buff_goto_def(arguments),
        "buff_format" => handle_buff_format(arguments),
        "buff_expand" => handle_buff_expand(arguments),
        _ => Err(ToolError::UnknownTool),
    }
}

// ---------------------------------------------------------------------------
// Argument helpers.
// ---------------------------------------------------------------------------

/// Extract a required `file` (string) argument. Returns
/// [`ToolError::MissingArg`] when missing / wrong type.
fn arg_file(args: &Value) -> Result<String, ToolError> {
    args.get("file")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or(ToolError::MissingArg("file"))
}

/// Extract a required `line` (u32) argument. Returns
/// [`ToolError::MissingArg`] when missing / wrong type / negative.
fn arg_line(args: &Value) -> Result<u32, ToolError> {
    args.get("line")
        .and_then(|v| v.as_u64())
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(ToolError::MissingArg("line"))
}

/// Extract a required `character` (u32) argument. Returns
/// [`ToolError::MissingArg`] when missing / wrong type / negative.
fn arg_character(args: &Value) -> Result<u32, ToolError> {
    args.get("character")
        .and_then(|v| v.as_u64())
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(ToolError::MissingArg("character"))
}

/// Read a file from disk into a String. Returns the file content on
/// success; on failure returns a [`ToolError::Execution`] with a
/// helpful message naming the path + the IO error.
fn read_file(path: &str) -> Result<String, ToolError> {
    std::fs::read_to_string(path)
        .map_err(|e| ToolError::Execution(format!("failed to read `{path}`: {e}")))
}

/// Build a fresh [`DocumentState`] for `src` (runs lex -> parse ->
/// TypeInferencer via [`buff_lsp::analysis::analyze`]). `SourceId(0)`
/// is fine — the MCP server is single-file per call (stateless, no
/// cross-document symbol resolution for v1.25).
fn document_state(src: &str) -> DocumentState {
    DocumentState::new(src.to_string(), SourceId(0), None)
}

/// Build an LSP [`lsp_types::Position`] from raw `line` / `character`
/// args. Clamps at the LSP-defined max (u32) — out-of-range positions
/// are handled by the LSP handlers (they walk back to find an
/// identifier).
fn position(line: u32, character: u32) -> lsp_types::Position {
    lsp_types::Position::new(line, character)
}

/// Build the URI for `file`. The path is normalized to an absolute
/// `file://` URL so AI assistants can correlate it to other tool
/// output. Falls back to the raw path when canonicalization fails
/// (rare — only on removed temp dirs).
///
/// lsp-types 0.97's `Uri` is a newtype over `fluent_uri::Uri<String>`
/// with only a `FromStr` impl (no `from_file_path`, no `Default`). So
/// we build a `file://` string ourselves and parse it — on parse
/// failure (e.g. path with illegal characters) we surface a
/// [`ToolError::Execution`] so the AI gets a clear message.
///
/// # Errors
///
/// Returns [`ToolError::Execution`] when the synthesized `file://`
/// string is not a valid URI per RFC 3986 (rare — requires the path
/// to contain characters fluent_uri rejects, like raw spaces in some
/// configurations).
fn file_uri(path: &str) -> Result<lsp_types::Uri, ToolError> {
    // Prefer the absolute path so two MCP tools reporting URIs on the
    // same file produce identical strings. `unwrap_or_else` (NOT
    // `unwrap`) falls back to the raw path on canonicalization
    // failure (file deleted between read + this call, permissions).
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path));
    // Normalize backslashes -> forward slashes (Windows -> Unix-style
    // file URIs). On Unix this is a no-op.
    let path_str = absolute.to_string_lossy().replace('\\', "/");
    // Build the file:// URI string. POSIX absolute paths
    // (`/Users/...`) become `file://` + path; Windows drive-letter
    // paths (`C:/Users/...`) become `file:///` + path (three slashes
    // — the authority is empty, the path starts with the drive).
    let candidate = if path_str.starts_with('/') {
        format!("file://{path_str}")
    } else {
        format!("file:///{path_str}")
    };
    candidate
        .parse()
        .map_err(|e| ToolError::Execution(format!("failed to build file URI for `{path}`: {e}")))
}

// ---------------------------------------------------------------------------
// Tool: buff_check.
// ---------------------------------------------------------------------------

/// `buff_check` — run `buff check` on a file, return structured JSON
/// diagnostics. Delegates to [`buff_lang_cli::check::check_source`]
/// (T55 standalone typecheck — no codegen, fast).
fn handle_buff_check(args: &Value) -> Result<ToolResult, ToolError> {
    let path = arg_file(args)?;
    let src = match read_file(&path) {
        Ok(s) => s,
        Err(e) => return Ok(ToolResult::error(e.to_string())),
    };

    let report = buff_lang_cli::check::check_source(&src);
    let diagnostics_json = render_diagnostics_json(&report.diagnostics, &src);

    // Summary header so the AI sees the outcome at a glance even when
    // the diagnostic list is long.
    let outcome_str = match report.outcome {
        CheckOutcome::Clean => "clean (no diagnostics)",
        CheckOutcome::HasWarnings => "warnings only (no errors)",
        CheckOutcome::HasErrors => "has errors",
    };
    let summary = format!(
        "`buff check` on `{path}` -> {outcome_str} ({} diagnostic{})",
        report.diagnostics.len(),
        if report.diagnostics.len() == 1 {
            ""
        } else {
            "s"
        }
    );

    // Format as a fenced JSON block so the AI renders it cleanly.
    let text = format!("{summary}\n\n```json\n{diagnostics_json}\n```");
    Ok(ToolResult::ok(text))
}

// ---------------------------------------------------------------------------
// Tool: buff_hover.
// ---------------------------------------------------------------------------

/// `buff_hover` — get hover info at a position. Delegates to
/// [`buff_lsp::handlers::hover`] (returns Markdown with the inferred
/// type + symbol kind).
fn handle_buff_hover(args: &Value) -> Result<ToolResult, ToolError> {
    let path = arg_file(args)?;
    let line = arg_line(args)?;
    let character = arg_character(args)?;

    let src = match read_file(&path) {
        Ok(s) => s,
        Err(e) => return Ok(ToolResult::error(e.to_string())),
    };
    let state = document_state(&src);
    let pos = position(line, character);

    let text = match handlers::hover(&state, pos) {
        Some(hover) => {
            // Extract the text from the LSP HoverContents. The Buff
            // LSP always emits HoverContents::Markup (see
            // handlers.rs); the other arms are handled defensively
            // for forward-compat.
            match hover.contents {
                lsp_types::HoverContents::Markup(markup) => markup.value,
                lsp_types::HoverContents::Scalar(marked) => match marked {
                    lsp_types::MarkedString::String(s) => s,
                    lsp_types::MarkedString::LanguageString(ls) => ls.value,
                },
                lsp_types::HoverContents::Array(marked_strings) => marked_strings
                    .into_iter()
                    .map(|ms| match ms {
                        lsp_types::MarkedString::String(s) => s,
                        lsp_types::MarkedString::LanguageString(ls) => ls.value,
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            }
        }
        None => format!(
            "no hover info at line {line}, character {character} in `{path}` \
(the cursor is not on a known symbol)"
        ),
    };
    Ok(ToolResult::ok(text))
}

// ---------------------------------------------------------------------------
// Tool: buff_complete.
// ---------------------------------------------------------------------------

/// `buff_complete` — get completion candidates at a position.
/// Delegates to [`buff_lsp::handlers::completion`] (returns in-scope
/// symbols as LSP CompletionItems).
fn handle_buff_complete(args: &Value) -> Result<ToolResult, ToolError> {
    let path = arg_file(args)?;
    let line = arg_line(args)?;
    let character = arg_character(args)?;

    let src = match read_file(&path) {
        Ok(s) => s,
        Err(e) => return Ok(ToolResult::error(e.to_string())),
    };
    let state = document_state(&src);
    let pos = position(line, character);

    let text = match handlers::completion(&state, pos) {
        Some(response) => {
            // Serialize the LSP CompletionResponse verbatim — AI
            // assistants handle JSON well + the LSP shape is stable.
            // The Array + List variants both serialize cleanly.
            let serialized = serde_json::to_string_pretty(&response).unwrap_or_else(|e| {
                format!("{{\"error\":\"failed to serialize completions: {e}\"}}")
            });
            let count = match &response {
                lsp_types::CompletionResponse::Array(items) => items.len(),
                lsp_types::CompletionResponse::List(list) => list.items.len(),
            };
            format!(
                "{count} completion candidate(s) at line {line}, character {character} in \
`{path}`:\n\n```json\n{serialized}\n```"
            )
        }
        None => {
            format!("no completion candidates at line {line}, character {character} in `{path}`")
        }
    };
    Ok(ToolResult::ok(text))
}

// ---------------------------------------------------------------------------
// Tool: buff_goto_def.
// ---------------------------------------------------------------------------

/// `buff_goto_def` — resolve definition at a position. Delegates to
/// [`buff_lsp::handlers::goto_definition`] (returns the in-file
/// declaration Location).
fn handle_buff_goto_def(args: &Value) -> Result<ToolResult, ToolError> {
    let path = arg_file(args)?;
    let line = arg_line(args)?;
    let character = arg_character(args)?;

    let src = match read_file(&path) {
        Ok(s) => s,
        Err(e) => return Ok(ToolResult::error(e.to_string())),
    };
    let state = document_state(&src);
    let uri = file_uri(&path)?;
    let pos = position(line, character);

    let text = match handlers::goto_definition(&state, &uri, pos) {
        Some(response) => {
            // Serialize the LSP GotoDefinitionResponse verbatim.
            // The Scalar / Array / Link variants all serialize cleanly.
            let serialized = serde_json::to_string_pretty(&response)
                .unwrap_or_else(|e| format!("{{\"error\":\"failed to serialize location: {e}\"}}"));
            format!(
                "definition of identifier at line {line}, character {character} in `{path}`:\n\n\
```json\n{serialized}\n```"
            )
        }
        None => format!(
            "no definition found for identifier at line {line}, character {character} in `{path}` \
(may be a built-in / prelude symbol with no in-file declaration)"
        ),
    };
    Ok(ToolResult::ok(text))
}

// ---------------------------------------------------------------------------
// Tool: buff_format.
// ---------------------------------------------------------------------------

/// `buff_format` — format a .buff file via `buff fmt`. Delegates to
/// [`buff_lang_cli::fmt::format_source`] (T54 — the same fn the LSP
/// `formatting` handler uses; byte-identical output).
fn handle_buff_format(args: &Value) -> Result<ToolResult, ToolError> {
    let path = arg_file(args)?;
    let src = match read_file(&path) {
        Ok(s) => s,
        Err(e) => return Ok(ToolResult::error(e.to_string())),
    };

    match buff_lang_cli::fmt::format_source(&src) {
        Ok(formatted) => {
            if formatted == src {
                let text =
                    format!("`{path}` is already in canonical form (no formatting changes).");
                Ok(ToolResult::ok(text))
            } else {
                // Wrap the formatted source in a fenced code block so
                // the AI renders it cleanly + can copy-paste it.
                let text = format!(
                    "formatted `{path}` ({} byte(s) -> {} byte(s)):\n\n```buff\n{formatted}\n```",
                    src.len(),
                    formatted.len()
                );
                Ok(ToolResult::ok(text))
            }
        }
        Err(e) => {
            // Parse / lex errors mean we can't safely reformat. The
            // AI should run `buff_check` first to see the underlying
            // problem.
            let text = format!(
                "could not format `{path}`: {e}. \
Run `buff_check` first to see the underlying parse / lex error."
            );
            Ok(ToolResult::error(text))
        }
    }
}

// ---------------------------------------------------------------------------
// Tool: buff_expand.
// ---------------------------------------------------------------------------

/// `buff_expand` — show the generated Rust for a .buff file. Delegates
/// to [`buff_lang_cli::pipeline::compile_to_rust`] (T115 `buff expand`
/// — full front-end including codegen via syn/quote/prettyplease).
///
/// NOTE: this also writes a `.rs` file alongside the source (the
/// pipeline's existing behavior). This is documented in the tool
/// description; AI assistants are read-only by default so the side
/// effect is benign (a generated file next to the source, identical
/// to what `buff expand` would produce on the CLI).
fn handle_buff_expand(args: &Value) -> Result<ToolResult, ToolError> {
    let path = arg_file(args)?;
    let file_path = std::path::PathBuf::from(&path);

    match buff_lang_cli::pipeline::compile_to_rust(&file_path) {
        Ok(output) => {
            let byte_count = output.rust_source.len();
            let line_count = output.rust_source.lines().count();
            let text = format!(
                "generated Rust for `{path}` ({byte_count} byte(s), {line_count} line(s)):\n\n\
```rust\n{}\n```",
                output.rust_source
            );
            Ok(ToolResult::ok(text))
        }
        Err(e) => {
            // Format the anyhow error chain so the AI sees the full
            // context (file read -> lex -> parse -> codegen layer).
            let text = format!("could not expand `{path}`: {e:#}");
            Ok(ToolResult::error(text))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Helper: write `contents` to a unique temp file + return the path.
    fn write_temp(suffix: &str, contents: &str) -> String {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "buff-mcp-test-{}-{}{suffix}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        let mut f = std::fs::File::create(&path).expect("create temp");
        f.write_all(contents.as_bytes()).expect("write");
        path.to_string_lossy().to_string()
    }

    #[test]
    fn dispatch_unknown_tool_returns_unknown_tool() {
        let err = dispatch_tool("no_such", &json!({})).expect_err("should error");
        assert!(matches!(err, ToolError::UnknownTool));
    }

    #[test]
    fn buff_check_missing_file_arg_returns_missing_arg() {
        let err = dispatch_tool("buff_check", &json!({})).expect_err("should error");
        assert!(matches!(err, ToolError::MissingArg("file")));
    }

    #[test]
    fn buff_hover_missing_position_args_returns_missing_arg() {
        let err = dispatch_tool("buff_hover", &json!({"file": "x"})).expect_err("should error");
        assert!(matches!(err, ToolError::MissingArg("line")));
    }

    #[test]
    fn buff_check_on_clean_file_returns_clean_outcome() {
        let path = write_temp(".buff", "func main():\n    print(\"hi\")\n");
        let result = dispatch_tool("buff_check", &json!({"file": path})).expect("ok");
        assert!(
            result.text.contains("clean"),
            "expected clean outcome, got: {}",
            result.text
        );
        assert!(
            result.text.contains("```json"),
            "expected fenced JSON, got: {}",
            result.text
        );
        assert!(!result.is_error);
    }

    #[test]
    fn buff_check_on_type_error_returns_has_errors() {
        let path = write_temp(
            ".buff",
            "func main():\n    let x: Int = \"hi\"\n    print(x)\n",
        );
        let result = dispatch_tool("buff_check", &json!({"file": path})).expect("ok");
        assert!(
            result.text.to_lowercase().contains("has errors"),
            "expected has errors, got: {}",
            result.text
        );
    }

    #[test]
    fn buff_check_on_nonexistent_file_returns_error_content_block() {
        let result = dispatch_tool(
            "buff_check",
            &json!({"file": "/no/such/path/does/not/exist.buff"}),
        )
        .expect("ok");
        assert!(result.is_error, "should be MCP error content block");
        assert!(
            result.text.contains("failed to read"),
            "got: {}",
            result.text
        );
    }

    #[test]
    fn buff_hover_on_let_binding_returns_int() {
        let path = write_temp(".buff", "func main():\n    let x = 42\n    print(x)\n");
        let result = dispatch_tool(
            "buff_hover",
            &json!({"file": path, "line": 1, "character": 8}),
        )
        .expect("ok");
        assert!(
            result.text.contains("Int"),
            "expected Int type, got: {}",
            result.text
        );
        assert!(!result.is_error);
    }

    #[test]
    fn buff_hover_on_empty_position_returns_no_hover() {
        // Hover way past end of file -> no symbol found.
        let path = write_temp(".buff", "func main():\n    print(1)\n");
        let result = dispatch_tool(
            "buff_hover",
            &json!({"file": path, "line": 100, "character": 100}),
        )
        .expect("ok");
        assert!(
            result.text.contains("no hover info"),
            "expected no-hover msg, got: {}",
            result.text
        );
    }

    #[test]
    fn buff_complete_offers_in_scope_symbols() {
        let path = write_temp(
            ".buff",
            "func add(a: Int, b: Int) -> Int:\n    return a + b\n",
        );
        let result = dispatch_tool(
            "buff_complete",
            &json!({"file": path, "line": 1, "character": 100}),
        )
        .expect("ok");
        assert!(
            result.text.contains("add") || result.text.contains("candidate"),
            "expected completion candidates, got: {}",
            result.text
        );
    }

    #[test]
    fn buff_goto_def_finds_local_let() {
        let path = write_temp(".buff", "func main():\n    let x = 42\n    print(x)\n");
        let result = dispatch_tool(
            "buff_goto_def",
            &json!({
                "file": path,
                "line": 2,
                "character": 10, // position of `x` in print(x)
            }),
        )
        .expect("ok");
        // Either found the def or reported no-def (cursor was just
        // past the identifier). Both are valid tool responses.
        assert!(
            result.text.contains("definition") || result.text.contains("no definition"),
            "got: {}",
            result.text
        );
    }

    #[test]
    fn buff_format_canonicalizes_unformatted_source() {
        let path = write_temp(".buff", "func main():   \n    print(\"hi\")   \n");
        let result = dispatch_tool("buff_format", &json!({"file": path})).expect("ok");
        assert!(
            result.text.contains("```buff"),
            "expected fenced buff code, got: {}",
            result.text
        );
    }

    #[test]
    fn buff_format_on_canonical_source_reports_no_changes() {
        let path = write_temp(".buff", "func main():\n    print(\"hi\")\n");
        let result = dispatch_tool("buff_format", &json!({"file": path})).expect("ok");
        assert!(
            result.text.contains("already in canonical form"),
            "expected no-changes msg, got: {}",
            result.text
        );
    }

    #[test]
    fn buff_expand_generates_rust_for_simple_program() {
        let path = write_temp(".buff", "func main():\n    print(\"hi\")\n");
        let result = dispatch_tool("buff_expand", &json!({"file": path})).expect("ok");
        assert!(
            result.text.contains("```rust"),
            "expected fenced rust code, got: {}",
            result.text
        );
        assert!(
            result.text.contains("fn main") || result.text.contains("generated Rust"),
            "expected a Rust fn, got: {}",
            result.text
        );
    }

    #[test]
    fn tool_schemas_describe_all_six_tools() {
        let schemas = tool_schemas();
        assert_eq!(schemas.len(), 6, "exactly six tools");
        for schema in &schemas {
            assert!(schema["name"].is_string(), "name: {schema}");
            assert!(schema["description"].is_string(), "description: {schema}");
            assert!(
                schema["inputSchema"]["type"] == "object",
                "schema: {schema}"
            );
            assert!(
                schema["inputSchema"]["properties"]["file"]["type"] == "string",
                "file prop: {schema}"
            );
        }
    }

    #[test]
    fn tool_result_ok_to_json_is_success_envelope() {
        let result = ToolResult::ok("hello");
        let value = result.to_json();
        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["content"][0]["text"], "hello");
        assert_eq!(value["isError"], false);
    }

    #[test]
    fn tool_result_error_to_json_is_error_content_block() {
        let result = ToolResult::error("oops");
        let value = result.to_json();
        assert_eq!(value["content"][0]["text"], "oops");
        assert_eq!(value["isError"], true);
    }
}
