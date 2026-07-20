//! Jupyter message `content` payloads for the message types T129a/T129b/T129c
//! handles: `kernel_info_reply`, `execute_reply`, `execute_result`,
//! `display_data`, `stream`, `error`, `shutdown_reply`.
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
/// T129b advertises real evaluation (reused from the T125 REPL
/// evaluator). T129c adds rich display (HTML tables for matrices /
/// vectors) and `?` / `??` introspection magics.
pub const BANNER: &str = "Buff kernel — execution + rich display (T129c: HTML tables for \
                          matrices/vectors, ?/?? introspection)";

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
    /// Help links rendered by Jupyter's `?` magic. Empty for T129a/b —
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
/// T129b emits BOTH the `ok` and `error` shapes per the Jupyter
/// messaging spec:
/// - [`ExecuteReply::ok`] for cells whose evaluator returned no
///   diagnostic and exit code 0.
/// - [`ExecuteReply::error`] for cells whose evaluator surfaced a
///   diagnostic (lex/parse/codegen/rustc/runtime) — the same
///   `ename`/`evalue`/`traceback` triple is echoed on the iopub
///   `error` message so front-ends render a single traceback block
///   regardless of which socket they watch first.
///
/// The `execution_count` is the kernel's monotonic counter (NOT the
/// client-supplied one) so the front-end can correlate the reply with
/// the iopub `execute_result` (which carries the same counter).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteReply {
    /// Execution status: `"ok"`, `"error"`, or `"abort"`.
    pub status: String,
    /// The 1-based cell counter (kernel-authoritative).
    pub execution_count: u64,
    /// Payload list — historically used by IPython for `?` / `??`
    /// magic. Always empty for T129a/b.
    #[serde(default)]
    pub payload: Vec<Value>,
    /// User expression evaluation result (the request's
    /// `user_expressions` dict evaluated after the cell). Always empty
    /// for T129a/b (Buff does not yet evaluate user_expressions).
    #[serde(default)]
    pub user_expressions: serde_json::Map<String, Value>,
    /// On `status = "error"`: the exception class name (Jupyter
    /// convention). Empty string on `status = "ok"`. Serialized via
    /// `#[serde(default)]` so the `ok` shape omits the field if the
    /// front-end is strict about minimality (we still emit empty
    /// string rather than `None` to match ipykernel's wire shape).
    #[serde(default)]
    pub ename: String,
    /// On `status = "error"`: the exception value (the diagnostic
    /// message). Empty string on `status = "ok"`.
    #[serde(default)]
    pub evalue: String,
    /// On `status = "error"`: list of traceback lines (no ANSI codes —
    /// Buff is not a Python kernel). Empty vec on `status = "ok"`.
    #[serde(default)]
    pub traceback: Vec<String>,
}

impl ExecuteReply {
    /// Build the success-shape `execute_reply` (status ok).
    ///
    /// Carries no payload / user_expressions / traceback — the front-end
    /// sees a clean `Out[N]:` line for the cell's `execute_result`.
    #[must_use]
    pub fn ok(execution_count: u64) -> Self {
        Self {
            status: "ok".to_string(),
            execution_count,
            payload: vec![],
            user_expressions: serde_json::Map::new(),
            ename: String::new(),
            evalue: String::new(),
            traceback: vec![],
        }
    }

    /// Build the error-shape `execute_reply` (status error) carrying
    /// the same `ename` / `evalue` / `traceback` triple as the
    /// corresponding iopub `error` message. The kernel survives the
    /// cell — the next `execute_request` gets a fresh evaluation
    /// against the SAME accumulated state (minus the failed cell's
    /// contribution, which the evaluator never committed).
    #[must_use]
    pub fn error(
        execution_count: u64,
        ename: impl Into<String>,
        evalue: impl Into<String>,
        traceback: Vec<String>,
    ) -> Self {
        Self {
            status: "error".to_string(),
            execution_count,
            payload: vec![],
            user_expressions: serde_json::Map::new(),
            ename: ename.into(),
            evalue: evalue.into(),
            traceback,
        }
    }
}

/// `execute_result` content — emitted on the IOPUB socket to render
/// the rich display of the cell's return value (if any).
///
/// T129b emits a single `text/plain` MIME bundle carrying the
/// evaluated value's `Display` form. T129c adds the rich-display path:
/// matrix / vector values are surfaced as a `text/html` `<table>` PLUS
/// a `text/plain` fallback (the canonical Jupyter MIME-bundle shape
/// — every rich representation must be paired with a plain-text
/// fallback so terminal / nbconvert-text exporters don't break).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteResult {
    /// The 1-based cell counter (echoed from the request).
    pub execution_count: u64,
    /// MIME-bundle map. Each key is a MIME type, each value is the
    /// string representation under that type.
    pub data: serde_json::Map<String, Value>,
    /// Metadata map (e.g. image dimensions). Empty for T129a/b/c.
    #[serde(default)]
    pub metadata: serde_json::Map<String, Value>,
    /// Transient map (e.g. `display_id` for `display_data` updates).
    /// Empty for T129a/b/c.
    #[serde(default)]
    pub transient: serde_json::Map<String, Value>,
}

impl ExecuteResult {
    /// Build an `execute_result` carrying a single `text/plain` MIME
    /// entry. This is the path used by bare-expression cells
    /// (`2 + 3`) — the evaluator captures the expression's value and
    /// surfaces it as the cell's `Out[N]:` line.
    #[must_use]
    pub fn text(execution_count: u64, text: impl Into<String>) -> Self {
        let mut data = serde_json::Map::new();
        data.insert("text/plain".to_string(), Value::String(text.into()));
        Self {
            execution_count,
            data,
            metadata: serde_json::Map::new(),
            transient: serde_json::Map::new(),
        }
    }

    /// Build an `execute_result` carrying an arbitrary MIME bundle.
    ///
    /// Each entry is a `(MIME type, body)` pair. The caller is
    /// responsible for including a `text/plain` fallback — Jupyter
    /// fronts-ends REQUIRE a plain-text representation alongside any
    /// rich representation (HTML / PNG / JSON) so terminal exporters
    /// and screen readers don't break. Prefer
    /// [`ExecuteResult::html_with_plain`] for the common
    /// HTML-plus-plain-text case (it cannot forget the fallback).
    #[must_use]
    pub fn mime_bundle(
        execution_count: u64,
        entries: Vec<(impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let mut data = serde_json::Map::new();
        for (mime, body) in entries {
            data.insert(mime.into(), Value::String(body.into()));
        }
        Self {
            execution_count,
            data,
            metadata: serde_json::Map::new(),
            transient: serde_json::Map::new(),
        }
    }

    /// Build an `execute_result` carrying BOTH a `text/html` body and
    /// a `text/plain` fallback. Used by T129c's rich-display path for
    /// matrix / vector values: the HTML renders as a `<table>` in
    /// JupyterLab / Notebook; the plain text is the value's source
    /// form (e.g. `[1, 2, 3]`) so terminal / text-only front-ends still
    /// see a meaningful rendering.
    ///
    /// The order of insertion (html first, plain second) matches
    /// ipykernel's convention — Jupyter clients render the FIRST
    /// representation they support in the bundle's iteration order,
    /// and HTML is preferred over plain text when both are available.
    #[must_use]
    pub fn html_with_plain(
        execution_count: u64,
        html: impl Into<String>,
        plain: impl Into<String>,
    ) -> Self {
        Self::mime_bundle(
            execution_count,
            vec![("text/html", html.into()), ("text/plain", plain.into())],
        )
    }
}

/// `display_data` content — emitted on the IOPUB socket to render a
/// rich-display MIME bundle WITHOUT a cell counter (i.e. not
/// associated with the cell's `Out[N]` line). Used by `display(...)`
/// calls inside a cell (multiple displays per execution).
///
/// T129c defines the struct so the kernel can emit `display_data` for
/// future use cases (e.g. explicit `display(matrix)` calls). The
/// current kernel emits `execute_result` for the trailing bare-expr
/// value (which carries `execution_count`); `display_data` is reserved
/// for the multi-display-per-cell scenario that arrives with
/// interactive `display(...)` prelude support (post-T129c).
///
/// The shape mirrors [`ExecuteResult`] minus `execution_count` — the
/// Jupyter messaging spec mandates identical `data` / `metadata` /
/// `transient` keys on both messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayData {
    /// MIME-bundle map. Same shape as [`ExecuteResult::data`].
    pub data: serde_json::Map<String, Value>,
    /// Metadata map (e.g. image dimensions). Empty by default.
    #[serde(default)]
    pub metadata: serde_json::Map<String, Value>,
    /// Transient map (e.g. `display_id` for `display_data` updates).
    #[serde(default)]
    pub transient: serde_json::Map<String, Value>,
}

impl DisplayData {
    /// Build a `display_data` carrying both `text/html` and
    /// `text/plain` (mirroring [`ExecuteResult::html_with_plain`]
    /// minus the `execution_count`).
    #[must_use]
    pub fn html_with_plain(html: impl Into<String>, plain: impl Into<String>) -> Self {
        let mut data = serde_json::Map::new();
        data.insert("text/html".to_string(), Value::String(html.into()));
        data.insert("text/plain".to_string(), Value::String(plain.into()));
        Self {
            data,
            metadata: serde_json::Map::new(),
            transient: serde_json::Map::new(),
        }
    }
}

/// `stream` content — emitted on the IOPUB socket to render stdout /
/// stderr output captured from the spawned Buff program.
///
/// T129b emits one `stream` message per non-empty captured stream:
/// - [`StreamOutput::stdout`] for `EvalResult::stdout` when the cell
///   produced no value (e.g. `print("hi")`).
/// - [`StreamOutput::stderr`] for `EvalResult::stderr` when a runtime
///   panic leaked bytes to the child's stderr (rare; usually paired
///   with a diagnostic which is surfaced as an [`ExecuteReply::error`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamOutput {
    /// Stream name: `"stdout"` or `"stderr"`.
    pub name: String,
    /// The text to append to the stream.
    pub text: String,
}

impl StreamOutput {
    /// Build a `stream` message body for the `stdout` channel.
    #[must_use]
    pub fn stdout(text: impl Into<String>) -> Self {
        Self {
            name: "stdout".to_string(),
            text: text.into(),
        }
    }

    /// Build a `stream` message body for the `stderr` channel.
    #[must_use]
    pub fn stderr(text: impl Into<String>) -> Self {
        Self {
            name: "stderr".to_string(),
            text: text.into(),
        }
    }
}

/// `error` content — emitted on the IOPUB socket when an
/// `execute_request` surfaces a diagnostic (lex/parse/codegen/rustc
/// or runtime). Mirrors ipykernel's `error` payload: the
/// `ename` / `evalue` / `traceback` triple is also echoed in the
/// `execute_reply` so clients watching either socket see one
/// consistent traceback block.
///
/// T129b fills `ename` with a coarse category (`"Error"`) and puts
/// the full diagnostic message + any captured stderr in the
/// traceback vec. ANSI color codes are NOT emitted (Buff is not a
/// Python kernel); front-ends render the traceback as plain text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorOutput {
    /// Exception class name. T129b uses `"Error"` (generic) — finer
    /// categorization (SyntaxError vs RuntimeError) is post-T129c.
    pub ename: String,
    /// Exception value — the diagnostic's `Display` form (e.g.
    /// `"[Error] eval: program exited with code 101"`).
    pub evalue: String,
    /// Traceback lines. Each entry is one logical line of context
    /// (captured stderr from rustc / runtime panics, then the
    /// diagnostic). No ANSI escapes.
    pub traceback: Vec<String>,
}

impl ErrorOutput {
    /// Build an `error` message body from a coarse category, the
    /// diagnostic's `Display` form, and the traceback vec (already
    /// assembled by the caller).
    #[must_use]
    pub fn new(
        ename: impl Into<String>,
        evalue: impl Into<String>,
        traceback: Vec<String>,
    ) -> Self {
        Self {
            ename: ename.into(),
            evalue: evalue.into(),
            traceback,
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
    /// Status: `"ok"` (always for T129a/b).
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
        // T129b: banner now advertises execution (NOT scaffold).
        assert!(
            r.banner.to_lowercase().contains("execution")
                || r.banner.to_lowercase().contains("t129b"),
            "banner must advertise execution: {}",
            r.banner
        );
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
    fn execute_reply_ok_has_status_ok_and_empty_error_fields() {
        let r = ExecuteReply::ok(42);
        assert_eq!(r.status, "ok");
        assert_eq!(r.execution_count, 42);
        assert!(r.payload.is_empty());
        assert!(r.user_expressions.is_empty());
        assert!(r.ename.is_empty());
        assert!(r.evalue.is_empty());
        assert!(r.traceback.is_empty());
    }

    #[test]
    fn execute_reply_error_carries_traceback() {
        let r = ExecuteReply::error(
            7,
            "Error",
            "eval: program exited with code 101",
            vec![
                "thread 'main' panicked at 'oops'".to_string(),
                "[Error] eval: program exited with code 101".to_string(),
            ],
        );
        assert_eq!(r.status, "error");
        assert_eq!(r.execution_count, 7);
        assert_eq!(r.ename, "Error");
        assert_eq!(r.evalue, "eval: program exited with code 101");
        assert_eq!(r.traceback.len(), 2);
    }

    #[test]
    fn execute_reply_round_trips_serde_both_shapes() {
        let ok = ExecuteReply::ok(3);
        let json = serde_json::to_string(&ok).expect("serialize ok");
        let ok2: ExecuteReply = serde_json::from_str(&json).expect("deserialize ok");
        assert_eq!(ok, ok2);

        let err = ExecuteReply::error(4, "Error", "boom", vec!["line1".to_string()]);
        let json = serde_json::to_string(&err).expect("serialize err");
        let err2: ExecuteReply = serde_json::from_str(&json).expect("deserialize err");
        assert_eq!(err, err2);
    }

    #[test]
    fn execute_result_text_carries_text_plain_mime() {
        let r = ExecuteResult::text(7, "42");
        assert_eq!(r.execution_count, 7);
        let plain = r
            .data
            .get("text/plain")
            .and_then(Value::as_str)
            .expect("text/plain");
        assert_eq!(plain, "42");
        assert!(r.metadata.is_empty());
        assert!(r.transient.is_empty());
    }

    #[test]
    fn execute_result_html_with_plain_carries_both_mimes() {
        // T129c: matrix / vector rich display must always pair an
        // HTML representation with a text/plain fallback so terminal
        // / screen-reader / text-only nbconvert exporters still see
        // a meaningful rendering.
        let r = ExecuteResult::html_with_plain(7, "<table></table>", "[1, 2, 3]");
        assert_eq!(r.execution_count, 7);
        let html = r
            .data
            .get("text/html")
            .and_then(Value::as_str)
            .expect("text/html must be present");
        let plain = r
            .data
            .get("text/plain")
            .and_then(Value::as_str)
            .expect("text/plain fallback must be present");
        assert_eq!(html, "<table></table>");
        assert_eq!(plain, "[1, 2, 3]");
    }

    #[test]
    fn execute_result_mime_bundle_round_trips_serde() {
        let r = ExecuteResult::mime_bundle(
            9,
            vec![
                ("text/html", "<table><tr><td>1</td></tr></table>"),
                ("text/plain", "[1]"),
            ],
        );
        let json = serde_json::to_string(&r).expect("serialize");
        let r2: ExecuteResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, r2);
        assert_eq!(r2.data.len(), 2);
    }

    #[test]
    fn display_data_html_with_plain_shape() {
        // T129c: display_data has the SAME shape as execute_result
        // MINUS execution_count. Verify the constructor produces the
        // expected keys.
        let d = DisplayData::html_with_plain("<table></table>", "[1, 2]");
        let v: Value = serde_json::to_value(&d).expect("to_value");
        let obj = v.as_object().expect("object");
        // Required keys per Jupyter messaging spec.
        assert!(obj.contains_key("data"));
        assert!(obj.contains_key("metadata"));
        assert!(obj.contains_key("transient"));
        // execution_count MUST be absent (display_data has none).
        assert!(
            !obj.contains_key("execution_count"),
            "display_data must not carry execution_count"
        );
        let data = obj["data"].as_object().expect("data object");
        assert!(data.contains_key("text/html"));
        assert!(data.contains_key("text/plain"));
    }

    #[test]
    fn stream_stdout_and_stderr_constructors_select_correct_channel() {
        let s = StreamOutput::stdout("hi\n");
        assert_eq!(s.name, "stdout");
        assert_eq!(s.text, "hi\n");
        let s = StreamOutput::stderr("oops\n");
        assert_eq!(s.name, "stderr");
        assert_eq!(s.text, "oops\n");
    }

    #[test]
    fn error_output_round_trips_serde() {
        let e = ErrorOutput::new(
            "Error",
            "eval: program exited with code 101",
            vec!["line1".to_string(), "line2".to_string()],
        );
        let json = serde_json::to_string(&e).expect("serialize");
        let e2: ErrorOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(e, e2);
        assert_eq!(e2.traceback.len(), 2);
    }

    #[test]
    fn shutdown_reply_echoes_restart() {
        let r = ShutdownReply::ok(true);
        assert!(r.restart);
        assert_eq!(r.status, "ok");
    }
}
