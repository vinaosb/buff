//! Command implementations — one submodule per `buff` subcommand.
//!
//! Each submodule exposes a `run(...)` entry point returning [`anyhow::Result`],
//! so [`main.rs`](../main.rs) can dispatch with a single match arm.

pub mod build;
pub mod init;
pub mod new;
pub mod run;
pub mod test;
