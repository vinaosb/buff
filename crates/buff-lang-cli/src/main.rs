//! Buff CLI — Command-line interface for the Buff language transpiler.
//!
//! This is the thin binary entry point. All real logic lives in the
//! [`buff_lang_cli`] library so that integration tests can drive the pipeline
//! without spawning a subprocess.

use anyhow::Result;
use clap::Parser;

use buff_lang_cli::cli::{Cli, Command};
use buff_lang_cli::scaffold;

fn main() -> Result<()> {
    let args = Cli::parse();
    match args.command {
        Command::Build { file, output } => {
            buff_lang_cli::commands::build::run(&file, output.as_deref())
        }
        Command::Run { file, args } => buff_lang_cli::commands::run::run(&file, &args),
        Command::New {
            name,
            lib,
            server,
            gpu,
            workspace,
        } => {
            let template = scaffold::template_from_flags(lib, server, gpu, workspace)
                .map_err(anyhow::Error::msg)?;
            buff_lang_cli::commands::new::run(&name, template)
        }
        Command::Init => buff_lang_cli::commands::init::run(),
        Command::Test { file, pattern } => {
            buff_lang_cli::commands::test::run(&file, pattern.as_deref())
        }
    }
}
