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
        Command::Build {
            file,
            output,
            release,
        } => buff_lang_cli::commands::build::run(file.as_deref(), output.as_deref(), release),
        Command::Run {
            file,
            args,
            release,
        } => buff_lang_cli::commands::run::run(&file, &args, release),
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
            buff_lang_cli::commands::test::run(file.as_deref(), pattern.as_deref())
        }
        Command::Fmt { file, check } => {
            use buff_lang_cli::commands::fmt::FmtOutcome;
            let outcome = buff_lang_cli::commands::fmt::run(&file, check)?;
            if matches!(outcome, FmtOutcome::NeedsFormat) {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Check {
            file,
            deny_warnings,
        } => {
            use buff_lang_cli::check::CheckOutcome;
            let outcome = buff_lang_cli::commands::check::run(&file, deny_warnings)?;
            if matches!(outcome, CheckOutcome::HasErrors) {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Clean => buff_lang_cli::commands::clean::run(),
        Command::Update => buff_lang_cli::commands::update::run(),
        Command::Repl => buff_lang_cli::commands::repl::run(),
        Command::Add {
            spec,
            branch,
            tag,
            rev,
        } => buff_lang_cli::commands::add::run(
            &spec,
            branch.as_deref(),
            tag.as_deref(),
            rev.as_deref(),
        ),
        Command::Login { token } => buff_lang_cli::commands::login::run(token.as_deref()),
        Command::Publish => buff_lang_cli::commands::publish::run(),
        Command::Install { name } => buff_lang_cli::commands::install::run(&name).map(|_| ()),
        Command::Deps { why } => buff_lang_cli::commands::deps::run(why.as_deref()),
        Command::Outdated => buff_lang_cli::commands::outdated::run(),
        Command::Jupyter { cmd } => buff_lang_cli::commands::jupyter::run(cmd),
        Command::Ui { cmd } => buff_lang_cli::commands::ui_dev::run(cmd),
        Command::Ssr {
            file,
            output,
            release,
        } => buff_lang_cli::commands::ssr::run(&file, output.as_deref(), release),
        Command::Coverage {
            path,
            html,
            lcov,
            output,
            release,
        } => buff_lang_cli::commands::coverage::run(
            path.as_deref(),
            html,
            lcov,
            output.as_deref(),
            release,
        ),
        Command::Debug {
            file,
            backend,
            source_map,
        } => buff_lang_cli::commands::debug::run(&file, backend.as_deref(), source_map.as_deref()),
    }
}
