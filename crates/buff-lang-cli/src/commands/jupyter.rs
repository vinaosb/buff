//! `buff jupyter install` / `buff jupyter start` — Jupyter kernel
//! management subcommands (T129a).
//!
//! Thin shims around the `buff-jupyter` crate. All real protocol /
//! HMAC / kernelspec logic lives in `crates/buff-jupyter/`.
//!
//! # `buff jupyter install`
//!
//! Writes the kernelspec `kernel.json` into the Jupyter data dir.
//! Prefers shelling out to `jupyter kernelspec install --replace
//! --name=buff <tempdir>` when `jupyter` is on `PATH`; falls back to
//! a direct write into `<data_dir>/kernels/buff/kernel.json` when
//! `jupyter` is missing (e.g. on a build host that does not yet have
//! Jupyter installed).
//!
//! On success, prints the absolute path of the installed `kernel.json`
//! so the user can verify by inspection if `jupyter kernelspec list`
//! is not available.
//!
//! # `buff jupyter start --connection-file <PATH>`
//!
//! Boots the kernel message loop using the connection JSON that
//! Jupyter wrote at launch time. Reads the connection file, binds the
//! 5 ZMQ sockets (shell / iopub / stdin / control / heartbeat), and
//! enters the dispatch loop. Returns when the kernel receives a
//! `shutdown_request` or the transport surfaces an unrecoverable
//! error.
//!
//! This subcommand is normally invoked indirectly via the `argv`
//! template in `kernel.json` — users do not type it directly. It is
//! exposed at the CLI so manual launches for debugging work (e.g.
//! `buff jupyter start --connection-file /tmp/kernel-12345.json`).
//!
//! # Errors
//!
//! Returns [`anyhow::Error`] on any failure surfaced by the
//! `buff-jupyter` crate (missing connection file, port already in
//! use, unsupported transport / signature scheme, etc.).

use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::JupyterCmd;
use buff_jupyter::{install as install_kernel, run_kernel};

/// Entry point for `buff jupyter <cmd>`.
///
/// Dispatches to [`run_install`] or [`run_start`] based on the
/// subcommand.
pub fn run(cmd: JupyterCmd) -> Result<()> {
    match cmd {
        JupyterCmd::Install => run_install(),
        JupyterCmd::Start { connection_file } => run_start(&connection_file),
    }
}

/// `buff jupyter install` — delegates to `buff_jupyter::install()`.
fn run_install() -> Result<()> {
    let path = install_kernel().map_err(|e| anyhow::Error::msg(e.to_string()))?;
    eprintln!("Installed Buff kernelspec at: {}", path.display());
    eprintln!("Verify with: jupyter kernelspec list");
    Ok(())
}

/// `buff jupyter start --connection-file <PATH>` — needs the tokio
/// runtime to drive the async kernel loop. We construct a multi-
/// thread runtime here (mirrors the `buff-registry` main binary's
/// `#[tokio::main]` shape) so the kernel can fan out the heartbeat /
/// shell / control tasks.
fn run_start(connection_file: &Path) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to construct tokio runtime")?;
    rt.block_on(async {
        run_kernel(connection_file)
            .await
            .map_err(|e| anyhow::Error::msg(e.to_string()))
    })?;
    Ok(())
}
