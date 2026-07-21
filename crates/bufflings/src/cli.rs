//! CLI argument definitions for the `bufflings` binary.
//!
//! Built on [`clap`] derive. Mirrors the subcommand pattern from
//! `buff-lang-cli/src/cli.rs`.

use clap::{Parser, Subcommand};

/// The top-level CLI shape parsed from `argv`.
#[derive(Parser, Debug)]
#[command(
    name = "bufflings",
    version,
    about = "Bufflings — interactive exercise runner for the Buff language"
)]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The set of subcommands supported by `bufflings`.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// List all exercises with completion status.
    List,

    /// Open an exercise for editing. Prints the path and opens $EDITOR.
    Start {
        /// The exercise name (e.g. "variables1").
        name: String,
    },

    /// Verify one or all exercises against `buff check`.
    Verify {
        /// The exercise name. Omit with --all to verify everything.
        name: Option<String>,

        /// Verify all exercises. Exits 0 only if all pass.
        #[arg(long)]
        all: bool,
    },

    /// Show progress summary (X/Y exercises complete).
    Progress,

    /// Watch exercise files for changes and auto-verify on save.
    Watch,

    /// Show a hint for the given exercise.
    Hint {
        /// The exercise name.
        name: String,
    },
}
