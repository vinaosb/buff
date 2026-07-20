//! Jupyter message `content` payloads for the message types T129a
//! handles: `kernel_info_reply`, `execute_reply`, `execute_result`,
//! `stream`, `shutdown_reply`.
//!
//! Each struct serializes to the exact JSON shape Jupyter clients
//! expect (verified against the canonical
//! [`jupyter_client`] Python reference and the `evcxr_jupyter` Rust
//! kernel).
//!
//! [`jupyter_client`]: https://jupyter-client.readthedocs.io/en/latest/messaging.html

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Implementation name advertised in `kernel_info_reply`.
pub const IMPLEMENTATION_NAME: &str = "buff";

/// Implementation version advertised in `kernel_info_reply`.
///
/// Mirrors the workspace version pinned at the root `Cargo.toml`.
pub const IMPLEMENTATION_VERSION: &str = "1.0.0";

/// Human-readable banner printed by Jupyter consoles on connect.
///
/// Explicitly flags this as a scaffold — execution arrives in T129b.
pub const BANNER: &str = "Buff kernel — T129a scaffold (handshake only; \
execution deferred to T129b)";

/// The `language_info` field of a `kernel_info_reply`.
///
/// Jupyter clients use this for syntax highlighting (Pygments /
/// CodeMirror mode), `?` introspection labels, and the file extension
/// for `%%writefile` magics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageInfo {
    /// Programming language name (lowercase, no spaces).
    pub name: String,
    /// Language version (compiler version, not protocol version).
    pub version: String,
    /// MIME type for source files.
    pub mimetype: String,
    /// File extension including leading dot.
    pub file_extension: String,
    /// Pygments lexer name (for syntax highlighting in nbconvert /
    /// JupyterLab). Falls back to `text` if unknown.
    #[serde(default)]
    pub pygments_lexer: Option<String>,
    /// CodeMirror mode name (for JupyterLab cell rendering).
    #[serde(default)]
    pub codemirror_mode: Option<Value>,
    /// Nbconvert lexer name (deprecated alias of `pygments_lexer` but
    /// still emitted by ipykernel for backwards compat).
    #[serde(default)]
    pub nbconvert_exporter: Option<String>,
}

impl LanguageInfo {
    /// Build the canonical Buff [`LanguageInfo`].
    #[must_use]
    pub fn buff() -> Self {
        Self {
            name: "buff".to_string(),
            version: IMPLEMENTATION_VERSION.to_string(),
            mimetype: "text/x-buff".to_string(),
            file_extension: ".buff".to_string(),
            pygments_lexer: Some("buff".to_string()),
            codemirror_mode: Some(Value::String("buff".to_string())),
            nbconvert_exporter: None,
        }
    }
}

/// `kernel_info_reply` content — the first message every Jupyter
/// client expects after connecting (it drives banner rendering +
/// highlighting + protocol negotiation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelInfoReply {
    /// Protocol version (mirrors
    /// [`crate::wire::PROTOCOL_VERSION`] = `"5.3"`).
    pub protocol_version: String,
    /// Implementation name (`"buff"`).
    pub implementation: String,
    /// Implementation version (mirrors workspace version).
    pub implementation_version: String,
    /// Language info for highlighting / introspection.
    pub language_info: LanguageInfo,
    /// Banner string printed on connect.
    pub banner: String,
    /// Help links rendered by Jupyter's `?` magic. Empty for T129a —
    /// we do not yet host docs.
    #[serde(default)]
    pub help_links: Vec<HelpLink>,
}

/// One entry in `kernel_info_reply.help_links`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelpLink {
    /// Display text.
    pub text: String,
    /// Target URL.
    pub url: String,
}

impl KernelInfoReply {
    /// Build the canonical Buff `kernel_info_reply` content.
    #[must_use]
    pub fn buff() -> Self {
        Self {
            protocol_version: crate::wire::PROTOCOL_VERSION.to_string(),
            implementation: IMPLEMENTATION_NAME.to_string(),
            implementation_version: IMPLEMENTATION_VERSION.to_string(),
            language_info: LanguageInfo::buff(),
            banner: BANNER.to_string(),
            help_links: vec![],
        }
    }
}

/// `execute_reply` content — emitted on the SHELL socket in response
/// to an `execute_request`.
///
/// T129a ALWAYS returns `status = "ok"` with no payload / user
/// expressions — the actual evaluation is T129b. The
/// `execution_count` is copied from the request (so the client can
/// correlate the reply with the input cell).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteReply {
    /// Execution status: `"ok"` (T129a always), `"error"`, `"abort"`.
    pub status: String,
    /// The 1-based cell counter (echoed from the request).
    pub execution_count: u64,
    /// Payload list — historically used by IPython for `?` / `??`
    /// magic. Always empty for T129a.
    #[serde(default)]
    pub payload: Vec<Value>,
    /// User expression evaluation result (the request's
    /// `user_expressions` dict evaluated after the cell). Always empty
    /// for T129a.
    #[serde(default)]
    pub user_expressions: serde_json::Map<String, Value>,
}

impl ExecuteReply {
    /// Build the T129a stub `execute_reply` (status ok, no payloads).
    #[must_use]
    pub fn stub_ok(execution_count: u64) -> Self {
        Self {
            status: "ok".to_string(),
            execution_count,
            payload: vec![],
            user_expressions: serde_json::Map::new(),
        }
    }
}

/// `execute_result` content — emitted on the IOPUB socket to render
/// the rich display of the cell's return value (if any).
///
/// T129a emits a single text/plain MIME bundle with the placeholder
/// "execution not yet implemented (T129b)". Real rich display (HTML,
/// images, etc.) arrives in T129c.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteResult {
    /// The 1-based cell counter (echoed from the request).
    pub execution_count: u64,
    /// MIME-bundle map. Each key is a MIME type, each value is the
    /// string representation under that type.
    pub data: serde_json::Map<String, Value>,
    /// Metadata map (e.g. image dimensions). Empty for T129a.
    #[serde(default)]
    pub metadata: serde_json::Map<String, Value>,
    /// Transient map (e.g. `display_id` for `display_data` updates).
    /// Empty for T129a.
    #[serde(default)]
    pub transient: serde_json::Map<String, Value>,
}

impl ExecuteResult {
    /// Build the T129a stub `execute_result` (single text/plain line).
    #[must_use]
    pub fn stub(execution_count: u64) -> Self {
        let mut data = serde_json::Map::new();
        data.insert(
            "text/plain".to_string(),
            Value::String("execution not yet implemented (T129b)".to_string()),
        );
        Self {
            execution_count,
            data,
            metadata: serde_json::Map::new(),
            transient: serde_json::Map::new(),
        }
    }
}

/// `stream` content — emitted on the IOPUB socket to render stdout /
/// stderr output.
///
/// T129a also emits this as a fallback when a frontend ignores
/// `execute_result` (some consoles only render streams). The text is
/// the same placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamOutput {
    /// Stream name: `"stdout"` or `"stderr"`.
    pub name: String,
    /// The text to append to the stream.
    pub text: String,
}

impl StreamOutput {
    /// Build a T129a stub `stream` message body on the `stdout` channel.
    #[must_use]
    pub fn stub_stdout() -> Self {
        Self {
            name: "stdout".to_string(),
            text: "execution not yet implemented (T129b)\n".to_string(),
        }
    }
}

/// `shutdown_request` / `shutdown_reply` content.
///
/// The reply echoes the request's `restart` flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownReply {
    /// Whether the client is restarting the kernel after shutdown
    /// (echoed from the request).
    pub restart: bool,
    /// Status: `"ok"` (always for T129a).
    pub status: String,
}

impl ShutdownReply {
    /// Build the canonical `shutdown_reply` (status ok, restart echoed).
    #[must_use]
    pub fn ok(restart: bool) -> Self {
        Self {
            restart,
            status: "ok".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_info_reply_buff_canonical_fields() {
        let r = KernelInfoReply::buff();
        assert_eq!(r.protocol_version, "5.3");
        assert_eq!(r.implementation, "buff");
        assert_eq!(r.implementation_version, IMPLEMENTATION_VERSION);
        assert_eq!(r.language_info.name, "buff");
        assert_eq!(r.language_info.file_extension, ".buff");
        assert_eq!(r.language_info.mimetype, "text/x-buff");
        assert!(r.banner.contains("T129a"));
        assert!(r.help_links.is_empty());
    }

    #[test]
    fn kernel_info_reply_round_trips_serde() {
        let r = KernelInfoReply::buff();
        let json = serde_json::to_string(&r).expect("serialize");
        let r2: KernelInfoReply = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, r2);
    }

    #[test]
    fn kernel_info_reply_has_required_json_keys() {
        let r = KernelInfoReply::buff();
        let v: Value = serde_json::to_value(&r).expect("to_value");
        let obj = v.as_object().expect("object");
        for key in [
            "protocol_version",
            "implementation",
            "implementation_version",
            "language_info",
            "banner",
            "help_links",
        ] {
            assert!(obj.contains_key(key), "missing key {key}");
        }
        let li = obj["language_info"].as_object().expect("language_info");
        for key in [
            "name",
            "version",
            "mimetype",
            "file_extension",
            "pygments_lexer",
            "codemirror_mode",
        ] {
            assert!(li.contains_key(key), "missing language_info.{key}");
        }
    }

    #[test]
    fn execute_reply_stub_status_ok() {
        let r = ExecuteReply::stub_ok(42);
        assert_eq!(r.status, "ok");
        assert_eq!(r.execution_count, 42);
        assert!(r.payload.is_empty());
        assert!(r.user_expressions.is_empty());
    }

    #[test]
    fn execute_result_stub_carries_placeholder_text() {
        let r = ExecuteResult::stub(7);
        assert_eq!(r.execution_count, 7);
        let plain = r
            .data
            .get("text/plain")
            .and_then(Value::as_str)
            .expect("text/plain");
        assert!(plain.contains("T129b"));
    }

    #[test]
    fn stream_stub_stdout_mentions_t129b() {
        let s = StreamOutput::stub_stdout();
        assert_eq!(s.name, "stdout");
        assert!(s.text.contains("T129b"));
    }

    #[test]
    fn shutdown_reply_echoes_restart() {
        let r = ShutdownReply::ok(true);
        assert!(r.restart);
        assert_eq!(r.status, "ok");
    }
}
