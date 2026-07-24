//! JSON-RPC 2.0 envelope + MCP method dispatch (T62).
//!
//! Implements the JSON-RPC 2.0 wire format + the subset of MCP
//! methods every MCP server MUST support (`initialize`,
//! `notifications/initialized`) + the methods this server supports
//! (`tools/list`, `tools/call`). Ping (`ping`) is also answered so
//! clients that probe liveness via ping get a clean ack.
//!
//! # Method dispatch
//!
//! [`handle_request`] takes a raw JSON message, decides:
//!
//! - is it a JSON-RPC request (has `id`) or notification (no `id`)?
//! - is the `method` recognised?
//! - are the `params` shape-correct?
//!
//! Returns:
//!
//! - `Ok(Some(response))` — a request was handled + has a response
//!   to send back.
//! - `Ok(None)` — a notification was handled (no response needed
//!   per JSON-RPC 2.0) OR the message had no `method` field (skip
//!   silently — clients sometimes send heartbeats with no method).
//! - `Err(McpError)` — a protocol-level error (parse error, method
//!   not found, invalid params). The caller writes the JSON-RPC
//!   error response to stdout.
//!
//! # Errors
//!
//! `McpError` carries the standard JSON-RPC error codes (per
//! <https://www.jsonrpc.org/specification#error_object>):
//!
//! | Code   | Meaning             | When                                              |
//! |--------|---------------------|---------------------------------------------------|
//! | -32700 | Parse error         | never (transport skips malformed JSON)            |
//! | -32600 | Invalid request     | message is not a valid JSON-RPC object            |
//! | -32601 | Method not found    | `method` is not one this server supports          |
//! | -32602 | Invalid params      | `params` missing / wrong shape                    |
//! | -32603 | Internal error      | tool dispatcher raised unexpectedly               |
//!
//! Tool-level errors (file not found, missing arg) are NOT
//! JSON-RPC errors — they're returned as MCP error content blocks
//! with `is_error: true` inside a SUCCESS response (per the MCP
//! spec). This distinction matters: a JSON-RPC error tears down the
//! session in some clients; an MCP error content block is recoverable.

use serde_json::{json, Value};

use crate::tools::{self, ToolError};

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 error codes (https://www.jsonrpc.org/specification).
// ---------------------------------------------------------------------------

/// Standard JSON-RPC 2.0 error code constants. Mirrored from the spec
/// table — used by [`McpError`] when building error responses. Stable
/// forever (part of the JSON-RPC 2.0 contract).
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

// ---------------------------------------------------------------------------
// Request / response envelope types.
// ---------------------------------------------------------------------------

/// A parsed JSON-RPC 2.0 request (or notification — `id` is `None`
/// for notifications). Built from the raw incoming JSON by
/// [`McpRequest::parse`].
#[derive(Debug, Clone)]
pub struct McpRequest {
    /// JSON-RPC version — always `"2.0"`. Missing or mismatched
    /// versions are rejected as invalid request per spec.
    pub jsonrpc: String,
    /// Method name (e.g. `"initialize"`, `"tools/list"`).
    pub method: String,
    /// Method params (`null` when the client sent none). The
    /// dispatcher decides whether `null` is acceptable per method.
    pub params: Value,
    /// Request id — `None` for notifications (no response expected).
    /// Echoed back in the response for requests per JSON-RPC 2.0.
    pub id: Option<Value>,
}

impl McpRequest {
    /// Parse a raw JSON message into a [`McpRequest`]. Returns
    /// [`McpError`] with [`INVALID_REQUEST`] when the message is not
    /// a well-formed JSON-RPC object (missing `jsonrpc`, `method`, or
    /// wrong `jsonrpc` value).
    ///
    /// The `id` is preserved verbatim (so string / number / null ids
    /// all round-trip — JSON-RPC 2.0 §4 says ids SHOULD be strings,
    /// numbers, or null).
    pub fn parse(raw: &Value) -> Result<Self, McpError> {
        let obj = raw
            .as_object()
            .ok_or_else(|| McpError::invalid_request("expected a JSON object"))?;
        let jsonrpc = obj
            .get("jsonrpc")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::invalid_request("missing or non-string `jsonrpc` field"))?;
        if jsonrpc != "2.0" {
            return Err(McpError::invalid_request(format!(
                "unsupported jsonrpc version `{jsonrpc}` (expected `2.0`)"
            )));
        }
        let method = obj
            .get("method")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::invalid_request("missing or non-string `method` field"))?;
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        let id = obj.get("id").cloned();
        Ok(Self {
            jsonrpc: jsonrpc.to_string(),
            method: method.to_string(),
            params,
            id,
        })
    }

    /// `true` when this is a notification (no `id` field) per JSON-RPC
    /// 2.0 §4. Notifications produce no response.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// A response to write to stdout. Either a success result or a
/// JSON-RPC error — both carry the request id + `jsonrpc: "2.0"`
/// envelope. Built by [`handle_request`] + the dispatcher.
#[derive(Debug, Clone)]
pub enum McpResponse {
    /// A successful response. The inner `Value` is the `result` field
    /// of the JSON-RPC envelope.
    Success(Value),
    /// A JSON-RPC error response. The inner `Value` is the full error
    /// envelope (`{ jsonrpc, id, error: { code, message, data? } }`).
    Error(Value),
}

/// A JSON-RPC 2.0 protocol-level error. Carries one of the standard
/// error codes ([`PARSE_ERROR`]..[`INTERNAL_ERROR`]) + a message +
/// the request id (when known — `null` for parse errors that failed
/// before the id could be extracted).
#[derive(Debug, Clone)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    pub id: Option<Value>,
}

impl McpError {
    /// Build a [`PARSE_ERROR`] (code -32700). Unused today (transport
    /// skips malformed JSON) but exposed for completeness.
    pub fn parse_error(msg: impl Into<String>) -> Self {
        Self {
            code: PARSE_ERROR,
            message: msg.into(),
            id: None,
        }
    }

    /// Build an [`INVALID_REQUEST`] (code -32600).
    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self {
            code: INVALID_REQUEST,
            message: msg.into(),
            id: None,
        }
    }

    /// Build a [`METHOD_NOT_FOUND`] (code -32601) carrying the request id.
    pub fn method_not_found(method: &str, id: Option<Value>) -> Self {
        Self {
            code: METHOD_NOT_FOUND,
            message: format!("method `{method}` is not supported by {SERVER_NAME}"),
            id,
        }
    }

    /// Build an [`INVALID_PARAMS`] (code -32602) carrying the request id.
    pub fn invalid_params(msg: impl Into<String>, id: Option<Value>) -> Self {
        Self {
            code: INVALID_PARAMS,
            message: msg.into(),
            id,
        }
    }

    /// Build an [`INTERNAL_ERROR`] (code -32603) carrying the request id.
    pub fn internal_error(msg: impl Into<String>, id: Option<Value>) -> Self {
        Self {
            code: INTERNAL_ERROR,
            message: msg.into(),
            id,
        }
    }

    /// Serialize this error as a full JSON-RPC error response envelope.
    pub fn to_json(&self) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": self.id,
            "error": {
                "code": self.code,
                "message": self.message,
            }
        })
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

/// Server name advertised to MCP clients (re-exported for error
/// messages — kept in sync with [`crate::SERVER_NAME`]).
const SERVER_NAME: &str = crate::SERVER_NAME;

// ---------------------------------------------------------------------------
// Top-level dispatch.
// ---------------------------------------------------------------------------

/// Handle one raw JSON message: parse it as a JSON-RPC request,
/// dispatch on the method, and return the response (or `None` for
/// notifications).
///
/// This is the entry the transport loop calls. It owns the JSON-RPC
/// envelope construction (success responses + error responses) so
/// the individual method handlers can return plain `Result<Value,
/// McpError>`.
pub fn handle_request(raw: &Value) -> Result<Option<McpResponse>, McpError> {
    // Parse the JSON-RPC envelope. On error, the id is unknown — the
    // error response carries `null` per spec.
    let request = match McpRequest::parse(raw) {
        Ok(r) => r,
        Err(mut e) => {
            // Try to recover the id so the client can correlate the
            // error to their request (JSON-RPC 2.0 §5.1).
            e.id = raw.get("id").cloned();
            return Err(e);
        }
    };

    // Notifications (no id) -> no response. We still dispatch them so
    // `notifications/initialized` clears any handshake state. The
    // dispatcher returns Ok(None) for notifications.
    let id = request.id.clone();
    let response_value: Option<Value> = match dispatch(&request) {
        Ok(value) => value,
        Err(mut e) => {
            e.id = id;
            return Err(e);
        }
    };

    Ok(response_value.map(|result| {
        McpResponse::Success(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
    }))
}

/// Inner dispatcher: route a parsed [`McpRequest`] to the matching
/// method handler. Returns `Ok(Some(result))` for requests (the
/// `result` field of the JSON-RPC success response), `Ok(None)` for
/// notifications (no response), `Err(McpError)` for protocol errors.
///
/// Each method handler owns its param validation + returns the
/// `result` value (or an [`McpError`]). Tool calls (`tools/call`)
/// are forwarded to [`crate::tools::dispatch_tool`] — that layer
/// translates tool-level errors (file not found, missing args) into
/// MCP error content blocks (NOT JSON-RPC errors — those tear down
/// sessions in some clients).
pub fn dispatch(request: &McpRequest) -> Result<Option<Value>, McpError> {
    match request.method.as_str() {
        "initialize" => Ok(Some(handle_initialize())),
        "notifications/initialized" => {
            // Client finished the handshake. We have no handshake
            // state to clear (the server is stateless post-init), so
            // this is a no-op notification. No response per JSON-RPC.
            Ok(None)
        }
        "ping" => Ok(Some(json!({}))),
        "tools/list" => Ok(Some(handle_tools_list())),
        "tools/call" => {
            let id = request.id.clone();
            let result = handle_tools_call(&request.params, id.clone())?;
            Ok(Some(result))
        }
        other => {
            if request.is_notification() {
                // Unknown notification — ignore silently (clients
                // sometimes send progress notifications the server
                // didn't subscribe to).
                Ok(None)
            } else {
                Err(McpError::method_not_found(other, request.id.clone()))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Method handlers.
// ---------------------------------------------------------------------------

/// Build the `initialize` result: protocol version + server info +
/// capabilities (tools only for v1.25).
fn handle_initialize() -> Value {
    json!({
        "protocolVersion": crate::PROTOCOL_VERSION,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": crate::SERVER_NAME,
            "version": crate::CRATE_VERSION,
        }
    })
}

/// Build the `tools/list` result: the array of tool descriptors
/// (name + description + JSON Schema for arguments). Delegates to
/// [`tools::tool_schemas`] so the schema lives next to the tool
/// implementations (single source of truth).
fn handle_tools_list() -> Value {
    json!({
        "tools": tools::tool_schemas()
    })
}

/// Build the `tools/call` result (or return an [`McpError`] for
/// protocol-level failures). Translates tool-level errors into MCP
/// error content blocks per the spec.
///
/// `params` shape: `{ name: string, arguments?: object }`.
fn handle_tools_call(params: &Value, id: Option<Value>) -> Result<Value, McpError> {
    let obj = params.as_object().ok_or_else(|| {
        McpError::invalid_params("`tools/call` params must be an object", id.clone())
    })?;
    let name = obj.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
        McpError::invalid_params(
            "`tools/call` params missing string `name` field",
            id.clone(),
        )
    })?;
    let arguments = obj.get("arguments").cloned().unwrap_or(json!({}));

    match tools::dispatch_tool(name, &arguments) {
        Ok(result) => Ok(result.to_json()),
        Err(ToolError::UnknownTool) => {
            Err(McpError::method_not_found(&format!("tools/{name}"), id))
        }
        Err(ToolError::MissingArg(arg)) => Err(McpError::invalid_params(
            format!("tool `{name}` is missing required argument `{arg}`"),
            id,
        )),
        // Tool-level execution failures (file not found, IO error)
        // are returned as MCP error content blocks — NOT JSON-RPC
        // errors. The MCP spec mandates this: the call succeeded
        // (the server understood + executed it); the tool itself
        // reported an error condition.
        Err(ToolError::Execution(msg)) => Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("error: {msg}")
            }],
            "isError": true
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_request_with_id() {
        let raw = json!({"jsonrpc":"2.0","method":"ping","id":7});
        let req = McpRequest::parse(&raw).expect("parse");
        assert_eq!(req.method, "ping");
        assert_eq!(req.id, Some(json!(7)));
        assert!(!req.is_notification());
    }

    #[test]
    fn parse_notification_without_id() {
        let raw = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        let req = McpRequest::parse(&raw).expect("parse");
        assert!(req.is_notification());
        assert!(req.id.is_none());
    }

    #[test]
    fn parse_rejects_missing_jsonrpc() {
        let raw = json!({"method":"ping","id":1});
        let err = McpRequest::parse(&raw).expect_err("should reject");
        assert_eq!(err.code, INVALID_REQUEST);
    }

    #[test]
    fn parse_rejects_wrong_jsonrpc_version() {
        let raw = json!({"jsonrpc":"1.0","method":"ping","id":1});
        let err = McpRequest::parse(&raw).expect_err("should reject");
        assert_eq!(err.code, INVALID_REQUEST);
        assert!(err.message.contains("1.0"));
    }

    #[test]
    fn parse_rejects_non_object() {
        let raw = json!("just a string");
        let err = McpRequest::parse(&raw).expect_err("should reject");
        assert_eq!(err.code, INVALID_REQUEST);
    }

    #[test]
    fn handle_initialize_advertises_tools_capability() {
        let result = handle_initialize();
        assert_eq!(result["protocolVersion"], crate::PROTOCOL_VERSION);
        assert_eq!(result["serverInfo"]["name"], crate::SERVER_NAME);
        assert_eq!(result["serverInfo"]["version"], crate::CRATE_VERSION);
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[test]
    fn handle_tools_list_returns_all_six_tools() {
        let result = handle_tools_list();
        let tools = result["tools"].as_array().expect("tools array");
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"buff_check"), "names: {names:?}");
        assert!(names.contains(&"buff_hover"), "names: {names:?}");
        assert!(names.contains(&"buff_complete"), "names: {names:?}");
        assert!(names.contains(&"buff_goto_def"), "names: {names:?}");
        assert!(names.contains(&"buff_format"), "names: {names:?}");
        assert!(names.contains(&"buff_expand"), "names: {names:?}");
    }

    #[test]
    fn dispatch_initialize_returns_success_response() {
        let raw = json!({"jsonrpc":"2.0","method":"initialize","id":1});
        let response = handle_request(&raw).expect("ok").expect("some response");
        match response {
            McpResponse::Success(value) => {
                assert_eq!(value["id"], 1);
                assert_eq!(value["result"]["serverInfo"]["name"], crate::SERVER_NAME);
            }
            McpResponse::Error(value) => panic!("expected success, got error: {value}"),
        }
    }

    #[test]
    fn dispatch_notification_returns_no_response() {
        let raw = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        let response = handle_request(&raw).expect("ok");
        assert!(response.is_none(), "notifications produce no response");
    }

    #[test]
    fn dispatch_ping_responds_with_empty_object() {
        let raw = json!({"jsonrpc":"2.0","method":"ping","id":"abc"});
        let response = handle_request(&raw).expect("ok").expect("response");
        match response {
            McpResponse::Success(value) => {
                assert_eq!(value["id"], "abc");
                assert_eq!(value["result"], json!({}));
            }
            McpResponse::Error(v) => panic!("expected success, got error: {v}"),
        }
    }

    #[test]
    fn dispatch_unknown_method_returns_method_not_found() {
        let raw = json!({"jsonrpc":"2.0","method":"resources/list","id":3});
        let err = handle_request(&raw).expect_err("should error");
        assert_eq!(err.code, METHOD_NOT_FOUND);
        assert_eq!(err.id, Some(json!(3)));
    }

    #[test]
    fn dispatch_unknown_notification_is_silently_ignored() {
        let raw = json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": {"progress": 50}
        });
        let response = handle_request(&raw).expect("ok");
        assert!(response.is_none(), "unknown notifications are ignored");
    }

    #[test]
    fn dispatch_invalid_jsonrpc_envelope_returns_invalid_request() {
        let raw = json!({"method":"ping","id":1});
        let err = handle_request(&raw).expect_err("should error");
        assert_eq!(err.code, INVALID_REQUEST);
        // id recovery — the envelope parse failed but id was present.
        assert_eq!(err.id, Some(json!(1)));
    }

    #[test]
    fn dispatch_tools_call_unknown_tool_returns_method_not_found() {
        let raw = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": "no_such_tool", "arguments": {}},
            "id": 9
        });
        let err = handle_request(&raw).expect_err("should error");
        assert_eq!(err.code, METHOD_NOT_FOUND);
        assert_eq!(err.id, Some(json!(9)));
    }

    #[test]
    fn mcp_error_serializes_to_full_envelope() {
        let err = McpError {
            code: METHOD_NOT_FOUND,
            message: "method `foo` is not supported".to_string(),
            id: Some(json!(42)),
        };
        let value = err.to_json();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], 42);
        assert_eq!(value["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(value["error"]["message"], "method `foo` is not supported");
    }
}
