//! Command implementations — one submodule per `buff` subcommand.
//!
//! Each submodule exposes a `run(...)` entry point returning [`anyhow::Result`],
//! so [`main.rs`](../main.rs) can dispatch with a single match arm.

pub mod add;
pub mod build;
pub mod check;
pub mod clean;
pub mod coverage;
pub mod debug;
pub mod deps;
pub mod fmt;
pub mod init;
pub mod install;
pub mod jupyter;
pub mod login;
pub mod new;
pub mod outdated;
pub mod publish;
pub mod registry;
pub mod repl;
pub mod run;
pub mod ssr;
pub mod test;
pub mod ui_build;
pub mod ui_dev;
pub mod ui_new;
pub mod update;
