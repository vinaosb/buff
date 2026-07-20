//! Crate-local error type for the Jupyter kernel scaffold.
//!
//! Mirrors the workspace's established `thiserror` + per-crate enum
//! pattern (see `buff-registry::RegistryError`, `buff-repl::ReplError`,
//! `buff-lsp::LspError`). Every fallible operation in this crate
//! returns [`Result<T, JupyterError>`]; the CLI boundary
//! (`buff_lang_cli::commands::jupyter`) wraps the error via
//! `anyhow::Error::msg` so the user sees a clean diagnostic.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Every error path is a typed variant with context.

/// The error type emitted by every fallible operation in `buff-jupyter`.
#[derive(Debug, thiserror::Error)]
pub enum JupyterError {
    /// The connection file (JSON written by Jupyter) could not be read
    /// or parsed. The Jupyter client writes `~/.local/share/jupyter/
    /// runtime/kernel-<pid>.json` per kernel launch and passes its path
    /// to the kernel via `--connection-file`; if that file is missing /
    /// unreadable / malformed, the kernel cannot bind its sockets.
    #[error("failed to read connection file '{path}': {message}")]
    ConnectionFileRead {
        /// Filesystem path that was being read.
        path: String,
        /// Lower-level error message (io / serde).
        message: String,
    },

    /// The connection file parsed successfully but contained an
    /// unsupported value (e.g. `signature_scheme` other than
    /// `hmac-sha256`, or `transport` other than `tcp`). The v1.x
    /// kernel scaffold supports the Jupyter default (TCP + HMAC-SHA256)
    /// only — inproc / ipc / pgm transports and SHA-non-256 schemes are
    /// explicitly out of scope for T129a.
    #[error("unsupported connection-file value: {field}={value} (T129a supports tcp + hmac-sha256 only)")]
    UnsupportedConnectionValue {
        /// Field name (`transport`, `signature_scheme`, etc.).
        field: String,
        /// The unsupported value the file contained.
        value: String,
    },

    /// HMAC signature verification failed — the received signature did
    /// not match the recomputed HMAC of the message frames. Per the
    /// Jupyter messaging protocol, signed messages whose HMAC does
    /// not verify MUST be dropped silently (a malicious or
    /// misbehaving client is treated as if it never sent the
    /// message). The kernel surfaces the failure as a typed error
    /// and the dispatch loop logs + drops the message.
    #[error("HMAC signature mismatch (expected {expected}, got {actual})")]
    HmacMismatch {
        /// Expected hex signature (recomputed from frames + key).
        expected: String,
        /// Actual hex signature received on the wire.
        actual: String,
    },

    /// The message frames do not conform to the Jupyter wire layout
    /// (`[ids..., <IDS|MSG>, hmac_hex, header, parent_header, metadata,
    /// content]`). The kernel requires at least 5 frames after the
    /// `<IDS|MSG>` delimiter (hmac + the 4 JSON frames); fewer means
    /// the client is sending malformed protocol data.
    #[error("malformed wire message: expected >= {expected} frames after <IDS|MSG> delimiter, got {actual}")]
    MalformedWire {
        /// Minimum frame count expected by the protocol.
        expected: usize,
        /// Actual frame count received.
        actual: usize,
    },

    /// A JSON frame failed to deserialize into the expected struct
    /// (header / parent_header / metadata / content). The kernel logs
    /// the failing frame's `msg_type` so a misbehaving client can be
    /// diagnosed without dropping the entire session.
    #[error("failed to deserialize {frame} frame as JSON: {message}")]
    FrameDeserialize {
        /// Human-readable frame label (`header`, `content`, etc.).
        frame: String,
        /// Lower-level serde error message.
        message: String,
    },

    /// An unknown `msg_type` was received. The kernel does not crash —
    /// it logs the unknown type and continues — but the dispatch
    /// surface returns this variant so callers can decide how to
    /// surface the miss.
    #[error("unknown msg_type '{msg_type}' — no handler registered")]
    UnknownMessageType {
        /// The msg_type field from the header.
        msg_type: String,
    },

    /// A ZMQ socket operation failed (bind / recv / send). The kernel
    /// surfaces the failure rather than retrying — the loop's caller
    /// (the CLI) terminates the process.
    #[error("ZMQ socket error: {0}")]
    Zmq(String),

    /// A generic I/O failure (file write for the kernelspec, etc.).
    #[error("I/O error: {0}")]
    Io(String),

    /// A JSON serialization failure (building a reply content, writing
    /// the kernelspec `kernel.json`, etc.).
    #[error("JSON error: {0}")]
    Json(String),
}

impl From<std::io::Error> for JupyterError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

impl From<serde_json::Error> for JupyterError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err.to_string())
    }
}

/// Convenience alias used throughout the crate.
pub type JupyterResult<T> = Result<T, JupyterError>;

/// Stub `Error` re-export for the workspace convention checklist (the
/// workspace's anti-pattern list forbids raw `unwrap` but permits typed
/// `Result` flows — this alias mirrors `RegistryError` / `ReplError`).
#[allow(clippy::module_name_repetitions)]
pub use JupyterError as Error;
