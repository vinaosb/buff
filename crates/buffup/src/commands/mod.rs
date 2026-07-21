//! Subcommand implementations for buffup.
//!
//! Each subcommand lives in its own module named after the CLI
//! keyword. The `default_cmd` module is named with a `_cmd` suffix
//! to avoid shadowing Rust's `std::default::Default` trait — the CLI
//! keyword itself remains `default` (set via the variant name in
//! [`crate::cli::Command::Default`]).

pub mod default_cmd;
pub mod install;
pub mod list;
pub mod update;
