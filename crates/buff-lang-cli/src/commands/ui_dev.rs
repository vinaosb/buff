//! `buff ui dev` — UI dev server subcommand (T131).
//!
//! Thin shim that delegates to [`buff_lang_cli::ui_dev::serve`] under a
//! multi-thread tokio runtime (mirrors the `buff jupyter start` pattern:
//! the dev server is a long-running async process that fans out the
//! HTTP server + file watcher + WebSocket broadcaster tasks).
//!
//! All real logic (HTTP handlers, notify watcher with 200 ms debounce,
//! cargo+wasm-bindgen rebuild path, WS protocol, client JS injection)
//! lives in [`crate::ui_dev`].
//!
//! # Errors
//!
//! Returns [`anyhow::Error`] when:
//! - The project `<PATH>` does not exist or cannot be canonicalised.
//! - The TCP bind on `127.0.0.1:<port>` fails (port already in use).
//! - The notify watcher fails to install on the project root.
//! - The HTTP server surfaces an unrecoverable error.
//!
//! Ctrl-C / SIGINT triggers graceful shutdown and a clean `Ok(())`.

use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::UiCmd;
use crate::ui_dev;

/// Entry point for `buff ui <cmd>`.
///
/// Dispatches to [`run_dev`] for the `dev` subcommand. Future `buff ui`
/// subcommands (e.g. `buff ui build` for production bundling) would
/// dispatch from here.
pub fn run(cmd: UiCmd) -> Result<()> {
    match cmd {
        UiCmd::Dev { path, port } => run_dev(&path, port),
    }
}

/// `buff ui dev [PATH] [--port <PORT>]` — boots the dev server.
///
/// Constructs a multi-thread tokio runtime (mirrors `buff jupyter
/// start`'s shape) so the HTTP server, notify watcher, and WebSocket
/// broadcaster can fan out across worker threads.
fn run_dev(path: &Path, port: u16) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to construct tokio runtime")?;
    rt.block_on(async {
        ui_dev::serve(path, port).await.map_err(|e| {
            // Format the dev-server error with its chain so the user
            // sees the underlying io / notify / hyper cause.
            anyhow::Error::msg(e.to_string())
        })
    })
}
