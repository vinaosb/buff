//! Buff CLI library â€” exposes the compiler pipeline and command
//! implementations so that integration tests (and future embedders) can drive
//! `buff build` / `buff run` programmatically without spawning a subprocess.
//!
//! The thin [`main.rs`](../main.rs) binary parses CLI flags via [`clap`] and
//! dispatches into [`commands::build`] / [`commands::run`].
//!
//! ## Pipeline overview
//!
//! ```text
//!   .buff source
//!        â”‚
//!        â–¼  read_to_string
//!   String
//!        â”‚
//!        â–¼  buff_lang_lexer::tokenize
//!   Vec<Token>
//!        â”‚
//!        â–¼  buff_lang_parser::parse
//!   Vec<Decl>
//!        â”‚
//!        â–¼  buff_lang_codegen_rust::generate_rust
//!   String  (valid Rust source)
//!        â”‚
//!        â–¼  pipeline::compile_rust_to_exe (rustc --edition 2021)
//!   native executable
//! ```
//!
//! Type-checking (buff-lang-types) is already integrated *inside* codegen (for
//! `let`-binding annotations) in T12, so the CLI does not run a separate
//! typecheck pass. v1.0 treats type errors as warnings (standalone typecheck
//! pass is post-v1.0 work).

// Boxing the large error types (CodegenError etc. returned through the
// pipeline) would reshape the public `compile_to_rust` / `compile_rust_to_exe`
// surface and every command consumer. Out of scope â€” matches the same
// documented trade-off applied in buff-eval / buff-lang-codegen-rust /
// buff-lang-types. Allowed at the crate level.
#![allow(clippy::result_large_err)]

pub mod bench_harness;
pub mod cli;
pub mod commands;
pub mod config;
pub mod coverage;
// P0.26: re-exports from extracted sibling crates.
pub use buff_lang_check as check;
pub use buff_lang_check::naming_lint;
pub use buff_lang_fmt as fmt;
pub use buff_lang_pipeline as pipeline;
pub use buff_lang_pipeline::compile_speed;
pub use buff_lang_pipeline::error_mapper;
pub use buff_lang_pipeline::incremental;
pub use buff_lang_pipeline::rustc_invoke;
// T1: multi-file project compilation pipeline (CLI-specific).
pub mod project_pipeline;
pub mod scaffold;
pub mod test_runner;
pub mod ui_dev;
