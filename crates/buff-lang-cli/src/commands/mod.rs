//! Command implementations — one submodule per `buff` subcommand.
//!
//! Each submodule exposes a `run(...)` entry point returning [`anyhow::Result`],
//! so [`main.rs`](../main.rs) can dispatch with a single match arm.

pub mod add;
pub mod ai;
pub mod backtrace;
pub mod bench;
pub mod bench_cold_start;
pub mod bench_compile;
pub mod bench_program;
pub mod build;
pub mod check;
pub mod clean;
pub mod coverage;
pub mod debug;
pub mod deps;
pub mod doc;
pub mod expand;
pub mod fix;
pub mod fmt;
pub mod gen;
pub mod generate;
pub mod init;
pub mod install;
pub mod jupyter;
pub mod login;
pub mod new;
pub mod outdated;
pub mod pgo;
pub mod profile;
pub mod publish;
pub mod refactor;
pub mod registry;
pub mod release;
pub mod repl;
pub mod run;
pub mod search;
pub mod ssr;
pub mod test;
pub mod ui_build;
pub mod ui_dev;
pub mod ui_new;
pub mod update;
pub mod watch;
