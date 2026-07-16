//! Command-line argument definitions for the `buff` binary.
//!
//! Built on [`clap`] derive. Subcommands supported in v0.1:
//!
//! - `buff build <FILE>` — compile a `.buff` file to a native executable.
//! - `buff run <FILE> [ARGS]...` — compile and immediately execute, cleaning
//!   up temporary artifacts afterwards.
//! - `buff new <NAME>` — scaffold a new Buff project in a fresh `<NAME>/`
//!   directory.
//! - `buff init` — scaffold a Buff project in the current directory.
//!
//! Future subcommands (`check`, `fmt`, `test`, `lsp`) will be added in later
//! waves.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// The top-level CLI shape parsed from `argv`.
#[derive(Parser, Debug)]
#[command(
    name = "buff",
    version,
    about = "Buff language compiler — transpiles .buff to Rust and runs rustc"
)]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The set of subcommands supported by `buff`.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Compile a `.buff` file into a native executable.
    Build {
        /// Input `.buff` source file.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Output executable path (default: `./<file-stem>` with the
        /// platform-appropriate executable extension).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Compile a `.buff` file and immediately execute it.
    ///
    /// Temporary artifacts (the generated `.rs` file and the compiled binary)
    /// are removed after the program exits.
    Run {
        /// Input `.buff` source file.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Arguments passed verbatim to the compiled program (after `--`).
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Create a new Buff project in a fresh `<NAME>/` directory.
    New {
        /// Name of the project (must be a valid Buff identifier, not a keyword).
        #[arg(value_name = "NAME")]
        name: String,
    },

    /// Initialize a Buff project in the current directory.
    Init,

    /// Discover and run `@test` functions in a `.buff` file (T35).
    Test {
        /// Input `.buff` source file containing `@test` functions.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Only run tests whose name matches this glob pattern (e.g.
        /// `test_*`). When omitted, all `@test` functions run.
        #[arg(long)]
        pattern: Option<String>,
    },
}
