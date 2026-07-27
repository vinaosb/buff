//! `buffup` — version manager for the Buff language (binary entry).
//!
//! Thin dispatch wrapper around the [`buffup`] library so integration
//! tests can drive the full pipeline without spawning a subprocess.
//! All real logic lives in the [`buffup`] library crate.
//!
//! # Exit codes
//!
//! - `0` — success, OR `--help` / `--version` invocation (clap exits
//!   early via [`clap::Parser::parse`]).
//! - `1` — command-level error (network failure, version not
//!   installed, etc.). The error message is prefixed with
//!   `buffup: ` and printed to stderr.
//! - `2` — argv parse failure (clap default). Surfaces via clap's
//!   own `Error::exit`.

use clap::Parser;

use buffup::cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `Cli::parse()` reads `std::env::args()` directly and handles
    // `--help` / `--version` / unknown-flag cases with clap's
    // conventional exit codes (0 for help/version, 2 for parse
    // errors). It never returns an `Err` — it calls
    // `std::process::exit` internally.
    let cli: Cli = Cli::parse();

    let result = match cli.command {
        Command::Install { version, skip_checksum } => {
            buffup::commands::install::run(version, skip_checksum).await
        }
        Command::Default { version } => buffup::commands::default_cmd::run(version),
        Command::List => buffup::commands::list::run(),
        Command::Update => buffup::commands::update::run(),
    };

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("buffup: {e}");
            std::process::exit(1);
        }
    }
}
