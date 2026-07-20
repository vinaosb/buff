//! `buff-lsp` — Language Server Protocol server for the Buff language.
//!
//! Built on the [`lsp_server`] crate (rust-analyzer's JSON-RPC scaffold)
//! and [`lsp_types`] (the canonical LSP 3.17 type set). Drives Buff's
//! existing compiler front-end — [`buff_lang_lexer`], [`buff_lang_parser`],
//! [`buff_lang_types`] — to implement:
//!
//! 1. **Diagnostics** — `textDocument/publishDiagnostics` on didOpen /
//!    didChange (debounced 300ms idle).
//! 2. **Hover** — `textDocument/hover` returns the inferred type + symbol
//!    kind for the identifier under the cursor.
//! 3. **Completion** — `textDocument/completion` offers local scope
//!    symbols + top-level decls (no fuzzy, no auto-import).
//! 4. **Goto definition** — `textDocument/definition` resolves an
//!    identifier to its in-file declaration (single-file for v1.2).
//! 5. **Document symbols** — `textDocument/documentSymbol` outlines funcs
//!    / structs / enums.
//! 6. **Formatting** — `textDocument/formatting` routes through
//!    [`buff_lang_cli::fmt`] (the T54 `buff fmt` logic, reused as-is).
//!
//! # Typecheck-only mode
//!
//! The LSP runs [`buff_lang_types::TypeInferencer`] directly — no Rust
//! codegen is generated. This is the standalone typecheck the T117 spec
//! asks for, distinct from the CLI's `buff check` (which surfaces warnings
//! generated during codegen).
//!
//! # Architecture
//!
//! ```text
//! didOpen / didChange  ──▶  state::DocumentState  ──▶  analysis::analyze
//!                            └─ LineIndex                ├─ tokenize
//!                            └─ DocumentAnalysis         ├─ parse_recovering
//!                                  └─ diagnostics        └─ TypeInferencer (per fn)
//!                                  └─ SymbolIndex
//!                                  └─ TypeBindingIndex
//!
//! hover / completion / definition / documentSymbol / formatting
//!                                  │
//!                                  ▼
//!                           handlers::* — pure fns on DocumentState
//! ```
//!
//! The handlers are pure functions; [`server::run_stdio`] is the only
//! place that touches I/O. Tests in `tests/` drive the handlers directly
//! without subprocess, mirroring how `buff-lang-cli` tests drive the
//! pipeline.
//!
//! # Crate layout
//!
//! - [`position`] — UTF-16-aware byte ↔ LSP-position conversion.
//! - [`analysis`] — runs the front-end and produces diagnostics + symbol
//!   indices.
//! - [`symbol`] — flat symbol tables for hover / completion / goto-def.
//! - [`state`] — per-document cached state (text + analysis + line index).
//! - [`handlers`] — pure LSP request handlers.
//! - [`server`] — stdio transport + main loop with debounced diagnostics.
//! - `main.rs` — thin binary entry that calls [`server::run_stdio`].
//!
//! # References
//!
//! - Plan: `.sisyphus/plans/buff-post-v10-tooling.md` task T117 (lines
//!   617-714).
//! - Per-crate conventions: `crates/buff-lsp/AGENTS.md`.

pub mod analysis;
pub mod handlers;
pub mod position;
pub mod server;
pub mod state;
pub mod symbol;

// Re-export the most-used types so embedding tools (and tests) can write
// `buff_lsp::DocumentState` instead of `buff_lsp::state::DocumentState`.
pub use analysis::{analyze, DocumentAnalysis};
pub use position::LineIndex;
pub use server::{run_stdio, ServerState};
pub use state::DocumentState;
pub use symbol::{SymbolEntry, SymbolIndex, TypeBindingIndex};

/// Crate version (matches `Cargo.toml`). Used to advertise server version
/// in the LSP `InitializeResult`.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
