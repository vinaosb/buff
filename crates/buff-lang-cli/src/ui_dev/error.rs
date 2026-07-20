//! Errors raised by the `buff ui dev` server (T131).
//!
//! The dev server sits outside the compiler pipeline proper (lex /
//! parse / codegen / type), so its errors do NOT extend
//! `buff_lang_error::BuffError` — those variants cover compiler-internal
//! failures. Dev-server failures (TCP bind, file-watch install,
//! cargo+wasm-bindgen shell-out, WS handshake) get their own
//! [`UiDevError`] enum here, derived via `thiserror` so the existing
//! `anyhow::Error::msg(e.to_string())` boundary in
//! [`crate::commands::ui_dev`] still produces a clean chain.
//!
//! Design note: we use a dedicated error type (rather than `anyhow`)
//! so unit tests can pattern-match on the variant and assert which
//! failure path fired. The compiler pipeline's `BuffError` is the same
//! shape (thiserror enum) for the same reason.

use std::path::PathBuf;

/// Errors raised by the dev server.
#[derive(Debug, thiserror::Error)]
pub enum UiDevError {
    /// The project root supplied on the CLI does not exist or is not
    /// a directory. The dev server needs a real directory to serve
    /// `<root>/static/` from and to install a notify watcher on.
    #[error("project root not found or not a directory: {path}")]
    ProjectRootNotFound { path: PathBuf },

    /// `std::fs::canonicalize` failed on the project root. Usually
    /// surfaces when the path has a broken symlink component or the
    /// process lacks read permission on a parent directory.
    #[error("failed to canonicalize project root `{path}`: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The TCP bind on `127.0.0.1:<port>` failed. The common case is
    /// the port already being in use (another dev server, or another
    /// process grabbed it before we did). The user-facing fix is to
    /// pass `--port <N>` with a different value.
    #[error("failed to bind 127.0.0.1:{port}: {source}")]
    Bind {
        port: u16,
        #[source]
        source: std::io::Error,
    },

    /// The `notify` watcher failed to install on the project root.
    /// Rare in practice (notify uses ReadDirectoryChangesW on Windows,
    /// inotify on Linux, kqueue / FSEvents on macOS — all reliable),
    /// but a misconfigured kernel (e.g. inotify `/proc/sys/fs/inotify/`
    /// limits exhausted) can surface it.
    #[error("failed to install file watcher on `{path}`: {message}")]
    WatcherInstall { path: PathBuf, message: String },

    /// The Buff front-end (`pipeline::compile_to_rust`) failed on a
    /// watched `.buff` file. This is NOT fatal — the dev server
    /// broadcasts the error to connected browsers (red banner overlay)
    /// and continues watching. The variant is still surfaced to the
    /// broadcaster so the browser overlay can render the full message.
    #[error("buff compile failed for `{file}`: {message}")]
    BuffCompile { file: PathBuf, message: String },

    /// The cargo + wasm-bindgen shell-out failed. Like
    /// [`Self::BuffCompile`] this is NOT fatal — browsers see a red
    /// banner; the dev server keeps running.
    #[error("cargo/wasm-bindgen build failed: {message}")]
    WasmBuild { message: String },

    /// An IO error reading a static asset or wasm bundle from disk
    /// while serving an HTTP request. Surfaced via the HTTP 500 path
    /// rather than killing the dev server.
    #[error("io error reading `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Generic catch-all for errors that do not fit a more specific
    /// variant (e.g. axum body-extraction failures). Kept so we never
    /// have to panic in non-test code.
    #[error("{0}")]
    Other(String),
}

impl UiDevError {
    /// Construct a [`Self::Other`] from any displayable value. Used
    /// internally to wrap unlikely errors (e.g. axum body-extraction
    /// failures) without panicking.
    #[must_use]
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other(message.into())
    }
}
