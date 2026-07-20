//! `buff-jupyter` — Jupyter kernel protocol for the Buff language
//! (tasks T129a + T129b of `buff-post-v10-tooling.md`).
//!
//! Implements the Jupyter messaging wire protocol over ZMQ's 5
//! sockets (shell / iopub / stdin / control / heartbeat), HMAC-SHA256
//! message signing, the kernelspec installer consumed by
//! `buff jupyter install` / `buff jupyter start`, and the execution
//! engine that evaluates Buff cells via the T125 `Evaluator` (reused
//! as a library).
//!
//! # Scope (T129a + T129b)
//!
//! - **`kernel_info_request` → `kernel_info_reply`** — full handshake
//!   (advertises protocol_version 5.3, language_info name=buff /
//!   file_extension=.buff).
//! - **`execute_request` → `execute_reply` (ok OR error)** — real
//!   Buff evaluation via [`buff_eval::Evaluator`]. iopub emits
//!   `status busy` → (`stream` stdout/stderr AND/OR `execute_result`
//!   with `text/plain` AND/OR `error` with ename/evalue/traceback) →
//!   `status idle`. State (let-bindings, func declarations) persists
//!   across cells — the kernel owns the evaluator in its session.
//!   Errors don't kill the kernel; subsequent cells keep running
//!   against the same accumulated state.
//! - **`shutdown_request` → `shutdown_reply`** — clean shutdown.
//! - **HMAC-SHA256 signing/verification** of the 4 message frames
//!   (header / parent_header / metadata / content) per the Jupyter
//!   protocol.
//!
//! # Out of scope (T129c)
//!
//! - Rich / image display, MIME bundles, `?` / `??` introspection,
//!   widgets (T129c).
//! - Interactive `input_request` over the stdin socket (T129c+).
//! - Concurrent heartbeat / multi-front-end execution hardening
//!   (post-T129c — the evaluator call is blocking).
//!
//! # Layering
//!
//! ```text
//!              ┌─────────────────────────────────────────┐
//!   ZMQ frames │ transport::ZmqTransport trait            │
//!         ───▶ │  ├─ ZmqSocketSet  (production, this crate)│
//!              │  └─ (mock for unit tests)                │
//!              └─────────────┬───────────────────────────┘
//!                            │ Multipart (Vec<Vec<u8>>)
//!                            ▼
//!              ┌─────────────────────────────────────────┐
//!              │ kernel::Kernel<T>                       │
//!              │  parse_message → verify → dispatch      │
//!              │  build_*_reply → sign → send            │
//!              │  ┌─ Evaluator (T125) — session state    │
//!              │  └─ eval_line(code) → EvalResult        │
//!              └─────────────┬───────────────────────────┘
//!                            │ WireMessage
//!                            ▼
//!              ┌─────────────────────────────────────────┐
//!              │ wire::WireMessage / MessageHeader       │
//!              │ hmac::sign / hmac::verify              │
//!              │ messages::{KernelInfoReply, ...}        │
//!              │ connection::ConnectionFile              │
//!              │ kernelspec::KernelSpec                  │
//!              └─────────────────────────────────────────┘
//! ```
//!
//! The trait abstraction means the pure protocol layer (everything
//! below `Kernel<T>`) is unit-testable without binding a real ZMQ
//! socket — see `tests/` for the integration surface and the
//! `#[cfg(test)]` mod in `kernel.rs` for the MockTransport-driven
//! execution-engine tests.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. All fallible operations return
//! `Result<T, JupyterError>`.

pub mod connection;
pub mod error;
pub mod hmac;
pub mod kernel;
pub mod kernelspec;
pub mod messages;
pub mod transport;
pub mod wire;

pub use connection::{ConnectionFile, DEFAULT_SIGNATURE_SCHEME};
pub use error::{Error, JupyterError, JupyterResult};
pub use hmac::{sign, verify};
pub use kernel::{Kernel, IDS_MSG_DELIMITER};
pub use kernelspec::{
    buff_kernelspec_dir, install as install_kernelspec, jupyter_kernels_dir, write_kernel_json,
    KernelSpec, KERNEL_DISPLAY_NAME, KERNEL_INTERRUPT_MODE, KERNEL_LANGUAGE, KERNEL_NAME,
};
pub use messages::{
    ErrorOutput, ExecuteReply, ExecuteResult, HelpLink, KernelInfoReply, LanguageInfo,
    ShutdownReply, StreamOutput, BANNER, IMPLEMENTATION_NAME, IMPLEMENTATION_VERSION,
};
pub use transport::{Multipart, ZmqSocketSet, ZmqTransport};
pub use wire::{MessageHeader, WireMessage, PROTOCOL_VERSION};

/// Entry point for `buff jupyter start --connection-file <path>`.
///
/// Reads the connection file, binds the 5 ZMQ sockets, and enters
/// the kernel message loop. Returns when the kernel receives a
/// `shutdown_request` or the transport surfaces an unrecoverable
/// error.
///
/// # Errors
///
/// Returns [`JupyterError`] on connection-file parse failure, socket
/// bind failure, or any unrecoverable transport error during the
/// loop.
pub async fn run_kernel(connection_file: &std::path::Path) -> JupyterResult<()> {
    let conn = ConnectionFile::from_path(connection_file)?;
    conn.validate()?;
    let transport = ZmqSocketSet::bind(&conn).await?;
    let kernel = Kernel::new(transport, conn);
    kernel.run().await
}

/// Re-export of the install entry point for `buff jupyter install`.
///
/// Kept here (rather than only at `kernelspec::install`) so the
/// `buff-jupyter` public API surfaces both CLI subcommand entry
/// points at the crate root.
pub fn install() -> JupyterResult<std::path::PathBuf> {
    install_kernelspec()
}
