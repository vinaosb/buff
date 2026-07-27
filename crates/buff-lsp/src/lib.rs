#![allow(clippy::all, dead_code, unused_imports, mismatched_lifetime_syntaxes)]
//! `buff-lsp` â€” Language Server Protocol server for the Buff language.
//!
//! Built on the [`lsp_server`] crate (rust-analyzer's JSON-RPC scaffold)
//! and [`lsp_types`] (the canonical LSP 3.17 type set). Drives Buff's
//! existing compiler front-end â€” [`buff_lang_lexer`], [`buff_lang_parser`],
//! [`buff_lang_types`] â€” to implement:
//!
//! 1. **Diagnostics** â€” `textDocument/publishDiagnostics` on didOpen /
//!    didChange (debounced 300ms idle).
//! 2. **Hover** â€” `textDocument/hover` returns the inferred type + symbol
//!    kind for the identifier under the cursor.
//! 3. **Completion** â€” `textDocument/completion` offers local scope
//!    symbols + top-level decls (no fuzzy, no auto-import).
//! 4. **Goto definition** â€” `textDocument/definition` resolves an
//!    identifier to its in-file declaration (single-file for v1.2).
//! 5. **Document symbols** â€” `textDocument/documentSymbol` outlines funcs
//!    / structs / enums.
//! 6. **Formatting** â€” `textDocument/formatting` routes through
//!    [`buff_lang_fmt`] (the T54 `buff fmt` logic, reused as-is).
//!
//! # Typecheck-only mode
//!
//! The LSP runs [`buff_lang_types::TypeInferencer`] directly â€” no Rust
//! codegen is generated. This is the standalone typecheck the T117 spec
//! asks for, distinct from the CLI's `buff check` (which surfaces warnings
//! generated during codegen).
//!
//! # Architecture
//!
//! ```text
//! didOpen / didChange  â”€â”€â–¶  state::DocumentState  â”€â”€â–¶  analysis::analyze
//!                            â””â”€ LineIndex                â”œâ”€ tokenize
//!                            â””â”€ DocumentAnalysis         â”œâ”€ parse_recovering
//!                                  â””â”€ diagnostics        â””â”€ TypeInferencer (per fn)
//!                                  â””â”€ SymbolIndex
//!                                  â””â”€ TypeBindingIndex
//!
//! hover / completion / definition / documentSymbol / formatting
//!                                  â”‚
//!                                  â–¼
//!                           handlers::* â€” pure fns on DocumentState
//! ```
//!
//! The handlers are pure functions; [`server::run_stdio`] is the only
//! place that touches I/O. Tests in `tests/` drive the handlers directly
//! without subprocess, mirroring how `buff-lang-cli` tests drive the
//! pipeline.
//!
//! # Crate layout
//!
//! - [`position`] â€” UTF-16-aware byte â†” LSP-position conversion.
//! - [`analysis`] â€” runs the front-end and produces diagnostics + symbol
//!   indices.
//! - [`symbol`] â€” flat symbol tables for hover / completion / goto-def.
//! - [`state`] â€” per-document cached state (text + analysis + line index).
//! - [`handlers`] â€” pure LSP request handlers.
//! - [`server`] â€” stdio transport + main loop with debounced diagnostics.
//! - `main.rs` â€” thin binary entry that calls [`server::run_stdio`].
//!
//! # References
//!
//! - Plan: `.sisyphus/plans/buff-post-v10-tooling.md` task T117 (lines
//!   617-714).
//! - Per-crate conventions: `crates/buff-lsp/AGENTS.md`.

// Boxing the LSP error type would reshape the public `run_stdio` surface
// and all handler Result signatures. Out of scope; allowed at crate level.
#![allow(clippy::result_large_err)]

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
