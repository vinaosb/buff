//! `buff ui` — UI subcommand dispatcher (T131 + T132).
//!
//! Dispatches to:
//!
//! - `buff ui dev [PATH] [--port <N>]` — boot the dev server (T131).
//!   Delegates to [`crate::ui_dev::serve`] under a multi-thread tokio runtime.
//!
//! - `buff ui new --desktop <NAME>` — scaffold a Tauri 2.0 desktop app (T132).
//!   Delegates to [`crate::commands::ui_new::run`].
//!
//! - `buff ui build --desktop [PATH]` — build a Tauri 2.0 desktop app (T132).
//!   Delegates to [`crate::commands::ui_build::run`].
//!
//! # Errors
//!
//! Each subcommand returns [`anyhow::Error`] on failure. See the individual
//! subcommand docs for specific error conditions.

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::cli::UiCmd;
use crate::ui_dev;

/// Entry point for `buff ui <cmd>`.
///
/// Dispatches to the appropriate subcommand handler based on the
/// [`UiCmd`] variant.
pub fn run(cmd: UiCmd) -> Result<()> {
    match cmd {
        UiCmd::Dev { path, port } => run_dev(&path, port),
        UiCmd::New { name, desktop } => run_new(&name, desktop),
        UiCmd::Build { desktop, path } => run_build(desktop, &path),
    }
}

/// `buff ui new --desktop <NAME>` — scaffold a Tauri 2.0 desktop app.
///
/// Validates the project name and writes all template files into a new
/// `<NAME>/` directory. Refuses to overwrite an existing directory.
fn run_new(name: &str, desktop: bool) -> Result<()> {
    if !desktop {
        bail!("`buff ui new` requires `--desktop` flag (the only supported target in v1.8)");
    }
    crate::commands::ui_new::run(name)
}

/// `buff ui build --desktop [PATH]` — build a Tauri 2.0 desktop app.
///
/// Checks that the Tauri CLI is installed, then shells out to
/// `cargo tauri build` in the project directory.
fn run_build(desktop: bool, path: &Path) -> Result<()> {
    if !desktop {
        bail!("`buff ui build` requires `--desktop` flag (the only supported target in v1.8)");
    }
    crate::commands::ui_build::run(Some(path))
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
