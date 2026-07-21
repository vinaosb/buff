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
            target,
        } => buff_lang_cli::commands::build::run(
            file.as_deref(),
            output.as_deref(),
            release,
            target.as_deref(),
        ),
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
            template,
        } => {
            // T0-C1: --template <name> takes precedence over the legacy
            // boolean flags; if both are set, --template wins and a warning
            // is logged via the error mapper (rather than failing — UX
            // matches Cargo's `--bin`/`--lib` precedence).
            let kind = if let Some(name) = &template {
                scaffold::template_from_name(name).map_err(anyhow::Error::msg)?
            } else {
                scaffold::template_from_flags(lib, server, gpu, workspace)
                    .map_err(anyhow::Error::msg)?
            };
            buff_lang_cli::commands::new::run(&name, kind)
        }
        Command::Init => buff_lang_cli::commands::init::run(),
        Command::Doc => buff_lang_cli::commands::doc::run(std::path::Path::new(".")),
        Command::Release { level } => {
            let lvl = buff_lang_cli::commands::release::BumpLevel::from_str(&level)
                .ok_or_else(|| {
                    anyhow::Error::msg(format!(
                        "unknown release level `{level}` (valid: patch, minor, major)"
                    ))
                })?;
            buff_lang_cli::commands::release::run(lvl, std::path::Path::new("."))
        }
        Command::Gen { kind, name } => {
            let k = buff_lang_cli::commands::gen::kind_from_str(&kind)
                .ok_or_else(|| {
                    anyhow::Error::msg(format!(
                        "unknown generator kind `{kind}` \
                         (valid: module, test, example)"
                    ))
                })?;
            buff_lang_cli::commands::gen::run(k, &name)
        }
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
        Command::Backtrace { log, buffmap } => {
            buff_lang_cli::commands::backtrace::run(&log, buffmap.as_deref())
        }
    }
}
