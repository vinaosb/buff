//! `buff-dap` — Debug Adapter Protocol server for the Buff language.
//!
//! Implements a DAP **translation proxy** that bridges an editor
//! (VSCode via the CodeLLDB / lldb-dap / vscode-lldb adapter type)
//! to a Rust-capable backend debugger. The proxy intercepts two
//! request types and applies Buff's T60 [`SourceMap`] translation:
//!
//! - **`setBreakpoints`** — the editor sends `.buff` file + line;
//!   we translate the line to the generated `.rs` file's line and
//!   forward to the backend.
//! - **`stackTrace`** — the backend reports `.rs` frames; we
//!   translate them back to `.buff` frames for the editor.
//!
//! All other DAP requests (`initialize` / `launch` / `continue` /
//! `next` / `stepIn` / `stepOut` / `pause` / `disconnect` / `scopes` /
//! `variables` / `evaluate` / `configurationDone` / ...) pass
//! through verbatim — the backend handles them directly.
//!
//! # Architecture
//!
//! ```text
//!              ┌─────────────────────────────────────────────┐
//!   Editor     │ protocol::Message (JSON-RPC over stdio)      │
//!   (VSCode)   │  Content-Length: N\r\n\r\n{json}             │
//!      ▼       └────────────────────┬────────────────────────┘
//!      │                            │
//!      │                            ▼
//!      │       ┌─────────────────────────────────────────────┐
//!      │       │ server::run_session                          │
//!      │       │  ├─ translate_editor_to_backend              │
//!      │       │  │   (setBreakpoints: buff→rust line)        │
//!      │       │  ├─ translate_backend_to_editor              │
//!      │       │  │   (stackTrace: rust→buff line)            │
//!      │       │  └─ passthrough (everything else)            │
//!      │       └────────────────────┬────────────────────────┘
//!      │                            │
//!      │                            ▼
//!      │       ┌─────────────────────────────────────────────┐
//!      │       │ backend::BackendProcess                      │
//!      │       │  (lldb-dap / codelldb / vscode-lldb)         │
//!      │       └─────────────────────────────────────────────┘
//! ```
//!
//! # Why hand-roll (not the `dap` crate)
//!
//! DAP is small + well-documented; the protocol types mirror LSP
//! closely (which `buff-lsp` already consumes via `lsp-types`).
//! Hand-rolling avoids introducing a new workspace dependency and
//! the risk of pulling in something incompatible with the Windows
//! toolchain (the same cc-rs transitive failure class that killed
//! `chumsky`/`logos`). The translation layer — the load-bearing
//! part — is pure Rust with no dep needs.
//!
//! # Limitations (documented in `.sisyphus/evidence/task-136-debugger.txt`)
//!
//! - **`scopes` / `variables` / `evaluate` pass through** — locals
//!   work in Rust terms; Buff-level variable name translation is
//!   future work (GAP-2).
//! - **GPU shader debugging is out of scope** — WGSL shaders run on
//!   the GPU; no DAP representation (GAP-3).
//! - **Multi-file `.buff` projects** — T60 SourceMap is single-file;
//!   multi-file support requires codegen changes (GAP-1, same as
//!   T137 coverage).
//! - **Watch expressions / hot reload / reverse debugging** — v2.0.
//!
//! # References
//!
//! - Plan: `.sisyphus/plans/buff-post-v10-tooling.md` task T136.
//! - Spec: <https://microsoft.github.io/debug-adapter-protocol/specification>
//! - T60 source map: `crates/buff-lang-error/src/source_map.rs`.

pub mod backend;
pub mod error;
pub mod protocol;
pub mod server;
pub mod translation;

// Re-export the most-used types so embedding tools (CLI, tests,
// future embedders) can write `buff_dap::Backend` instead of
// `buff_dap::backend::Backend`.
pub use backend::{
    detect_backend, detect_specific, print_missing_backend_hint, spawn as spawn_backend, Backend,
    BackendProcess,
};
pub use error::{DapError, DapResult};
pub use protocol::{decode, encode, has_complete_message, Message, MessageKind};
pub use server::{run_session, SeqCounter, ServerConfig};
pub use translation::{
    translate_breakpoints_buff_to_rust, translate_stack_frame_rust_to_buff,
    translate_stack_trace_rust_to_buff, TranslatedBreakpoint, TranslatedStackFrame,
};

/// Crate version (matches `Cargo.toml`). Used to advertise the
/// adapter version in the DAP `InitializeResult`.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
