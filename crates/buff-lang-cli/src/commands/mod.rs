//! Command implementations — one submodule per `buff` subcommand.
//!
//! Each submodule exposes a `run(...)` entry point returning [`anyhow::Result`],
//! so [`main.rs`](../main.rs) can dispatch with a single match arm.

pub mod add;
pub mod build;
pub mod check;
pub mod clean;
pub mod fmt;
pub mod init;
pub mod new;
pub mod repl;
pub mod run;
pub mod test;
pub mod update;
