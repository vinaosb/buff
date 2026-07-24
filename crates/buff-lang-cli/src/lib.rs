//! Buff CLI library — exposes the compiler pipeline and command
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
//!        │
//!        ▼  read_to_string
//!   String
//!        │
//!        ▼  buff_lang_lexer::tokenize
//!   Vec<Token>
//!        │
//!        ▼  buff_lang_parser::parse
//!   Vec<Decl>
//!        │
//!        ▼  buff_lang_codegen_rust::generate_rust
//!   String  (valid Rust source)
//!        │
//!        ▼  pipeline::compile_rust_to_exe (rustc --edition 2021)
//!   native executable
//! ```
//!
//! Type-checking (buff-lang-types) is already integrated *inside* codegen (for
//! `let`-binding annotations) in T12, so the CLI does not run a separate
//! typecheck pass. v1.0 treats type errors as warnings (standalone typecheck
//! pass is post-v1.0 work).

pub mod bench_harness;
pub mod check;
pub mod cli;
pub mod commands;
pub mod compile_speed;
pub mod config;
pub mod coverage;
pub mod error_mapper;
pub mod fmt;
// T7: salsa-based incremental compilation front-end. The `incremental`
// module wraps the lex + parse + typecheck passes in salsa tracked
// queries so unchanged input files skip re-processing on subsequent
// compiles within a single CLI session. Opt-in via `--incremental` /
// `--no-incremental` CLI flags (default: ON for dev Debug builds, OFF
// for Release/Minimal). Correctness is unaffected — salsa is purely a
// memoization cache layered above the existing `pipeline` module.
pub mod incremental;
pub mod naming_lint;
pub mod pipeline;
// T1: multi-file project compilation pipeline (parse_project + cargo).
pub mod project_pipeline;
pub mod scaffold;
pub mod test_runner;
pub mod ui_dev;
