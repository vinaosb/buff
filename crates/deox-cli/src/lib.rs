//! Deox CLI library — exposes the compiler pipeline and command
//! implementations so that integration tests (and future embedders) can drive
//! `deox build` / `deox run` programmatically without spawning a subprocess.
//!
//! The thin [`main.rs`](../main.rs) binary parses CLI flags via [`clap`] and
//! dispatches into [`commands::build`] / [`commands::run`].
//!
//! ## Pipeline overview
//!
//! ```text
//!   .deox source
//!        │
//!        ▼  read_to_string
//!   String
//!        │
//!        ▼  deox_lexer::tokenize
//!   Vec<Token>
//!        │
//!        ▼  deox_parser::parse
//!   Vec<Decl>
//!        │
//!        ▼  deox_codegen_rust::generate_rust
//!   String  (valid Rust source)
//!        │
//!        ▼  pipeline::compile_rust_to_exe (rustc --edition 2021)
//!   native executable
//! ```
//!
//! Type-checking (deox-types) is already integrated *inside* codegen (for
//! `let`-binding annotations) in T12, so the CLI does not run a separate
//! typecheck pass. v0.1 treats type errors as warnings (deferred to v0.5).

pub mod cli;
pub mod commands;
pub mod error_mapper;
pub mod pipeline;
pub mod scaffold;
