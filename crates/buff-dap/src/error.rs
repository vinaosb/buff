//! Crate-local error type for the DAP server.
//!
//! All fallible operations return [`DapResult<T>`]; the CLI boundary
//! maps [`DapError`] into [`anyhow::Error`] (mirrors `buff-jupyter` /
//! `buff-lsp` precedent). Errors are user-facing: every variant
//! surfaces enough context to diagnose without a debugger.

/// Specialized [`Result`] for DAP operations.
pub type DapResult<T> = Result<T, DapError>;

/// Errors surfaced by the `buff-dap` server + translation layer.
///
/// Each variant is reachable from real failure modes (no backend
/// installed, broken stdio framing, JSON parse failure, missing
/// source-map entry). Variant messages are user-facing — they are
/// stringified at the CLI boundary and shown on stderr.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DapError {
    /// No DAP-capable backend debugger (lldb-dap / codelldb /
    /// vscode-lldb) was found on `PATH`. The user must install one
    /// (USER ACTION — see `.sisyphus/evidence/task-136-debugger-USER-ACTION.txt`).
    #[error(
        "no DAP backend debugger found on PATH — install lldb-dap \
         (preferred, ships with llvm), codelldb, or vscode-lldb. \
         See .sisyphus/evidence/task-136-debugger-USER-ACTION.txt \
         for the install recipe."
    )]
    NoBackend,

    /// I/O failure on the stdio transport (broken pipe = client
    /// disconnected, etc.). Generally unrecoverable — the main loop
    /// exits non-zero and the client logs the failure.
    #[error("stdio transport io error: {0}")]
    Io(String),

    /// JSON (de)serialization failure for a DAP wire message.
    /// Includes the raw payload excerpt for diagnosis.
    #[error("json error: {0}")]
    Json(String),

    /// Backend subprocess exited with a non-zero status. Includes
    /// the exit code (when captured) + a trimmed stderr excerpt.
    #[error("backend exited with status {status}: {stderr_excerpt}")]
    BackendExited {
        /// The exit status (numeric when available).
        status: String,
        /// First ~200 chars of backend stderr (for diagnosis).
        stderr_excerpt: String,
    },

    /// A required DAP protocol field was missing or malformed.
    /// The DAP spec mandates `seq` / `type` / `command` on every
    /// request; missing any of these is a protocol violation.
    #[error("malformed dap message: {0}")]
    MalformedMessage(String),

    /// The requested source-map file (.buff) could not be read or
    /// does not exist. Includes the path for context.
    #[error("source map file error: {0}")]
    SourceMapFile(String),
}
