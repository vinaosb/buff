//! Deox CLI — Command-line interface for the Deox language transpiler.
//!
//! This is the thin binary entry point. All real logic lives in the
//! [`deox_cli`] library so that integration tests can drive the pipeline
//! without spawning a subprocess.

use anyhow::Result;
use clap::Parser;

use deox_cli::cli::{Cli, Command};

fn main() -> Result<()> {
    let args = Cli::parse();
    match args.command {
        Command::Build { file, output } => deox_cli::commands::build::run(&file, output.as_deref()),
        Command::Run { file, args } => deox_cli::commands::run::run(&file, &args),
    }
}
