//! Command-line argument definitions for the `deox` binary.
//!
//! Built on [`clap`] derive. Subcommands supported in v0.1:
//!
//! - `deox build <FILE>` — compile a `.deox` file to a native executable.
//! - `deox run <FILE> [ARGS]...` — compile and immediately execute, cleaning
//!   up temporary artifacts afterwards.
//! - `deox new <NAME>` — scaffold a new Deox project in a fresh `<NAME>/`
//!   directory.
//! - `deox init` — scaffold a Deox project in the current directory.
//!
//! Future subcommands (`check`, `fmt`, `test`, `lsp`) will be added in later
//! waves.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// The top-level CLI shape parsed from `argv`.
#[derive(Parser, Debug)]
#[command(
    name = "deox",
    version,
    about = "Deox language compiler — transpiles .deox to Rust and runs rustc"
)]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The set of subcommands supported by `deox`.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Compile a `.deox` file into a native executable.
    Build {
        /// Input `.deox` source file.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Output executable path (default: `./<file-stem>` with the
        /// platform-appropriate executable extension).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Compile a `.deox` file and immediately execute it.
    ///
    /// Temporary artifacts (the generated `.rs` file and the compiled binary)
    /// are removed after the program exits.
    Run {
        /// Input `.deox` source file.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Arguments passed verbatim to the compiled program (after `--`).
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Create a new Deox project in a fresh `<NAME>/` directory.
    New {
        /// Name of the project (must be a valid Deox identifier, not a keyword).
        #[arg(value_name = "NAME")]
        name: String,
    },

    /// Initialize a Deox project in the current directory.
    Init,
}
