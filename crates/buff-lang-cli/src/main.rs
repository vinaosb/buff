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
            minimal,
            fast,
            no_cache,
            incremental,
            no_incremental,
            sccache,
            target,
            pgo,
            pgo_use,
            linker,
            debuginfo,
            backend,
            explain,
            detect_races,
        } => {
            // T62: --pgo intercepts the normal build path and dispatches
            // to commands::pgo (3-phase PGO orchestrator). PGO is
            // orthogonal to --release/--minimal/--fast (it instruments OR
            // consumes a profile), so those flags are ignored when --pgo
            // is set. When --pgo is absent, the normal build path runs
            // unchanged (backward compat).
            let linker_choice = buff_lang_cli::pipeline::linker_from_str(&linker)?;
            let debuginfo_choice = buff_lang_cli::pipeline::debuginfo_from_str(&debuginfo)?;
            let backend_choice = buff_lang_cli::pipeline::backend_from_str(&backend)?;
            if explain {
                // T6: --explain sets the env var so the compiled binary's
                // runtime can emit dispatch diagnostics. The env var is
                // inherited by the rustc-compiled binary.
                std::env::set_var("BUFF_EXPLAIN_DISPATCH", "1");
            }
            if pgo {
                buff_lang_cli::commands::pgo::run(file.as_deref(), output.as_deref(), pgo_use, None)
            } else {
                buff_lang_cli::commands::build::run(
                    file.as_deref(),
                    output.as_deref(),
                    release,
                    minimal,
                    fast,
                    no_cache,
                    incremental,
                    no_incremental,
                    sccache,
                    target.as_deref(),
                    linker_choice,
                    debuginfo_choice,
                    backend_choice,
                    detect_races,
                )
            }
        }
        Command::Run {
            file,
            args,
            release,
            incremental,
            no_incremental,
            linker,
            debuginfo,
            backend,
            target,
            sccache,
            explain,
            detect_races,
        } => {
            let linker_choice = buff_lang_cli::pipeline::linker_from_str(&linker)?;
            let debuginfo_choice = buff_lang_cli::pipeline::debuginfo_from_str(&debuginfo)?;
            let backend_choice = buff_lang_cli::pipeline::backend_from_str(&backend)?;
            if explain {
                // T6: --explain sets the env var so the compiled binary's
                // runtime can emit dispatch diagnostics.
                std::env::set_var("BUFF_EXPLAIN_DISPATCH", "1");
            }
            buff_lang_cli::commands::run::run(
                &file,
                &args,
                release,
                incremental,
                no_incremental,
                sccache,
                linker_choice,
                debuginfo_choice,
                backend_choice,
                target.as_deref(),
                detect_races,
            )
        }
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
        Command::Doc {
            output,
            open,
            serve,
            port,
        } => {
            if serve {
                buff_lang_cli::commands::doc::run_serve(
                    std::path::Path::new("."),
                    output.as_deref(),
                    port,
                )
            } else {
                buff_lang_cli::commands::doc::run(
                    std::path::Path::new("."),
                    output.as_deref(),
                    open,
                )
            }
        }
        Command::Release { level } => {
            let lvl =
                buff_lang_cli::commands::release::BumpLevel::from_str(&level).ok_or_else(|| {
                    anyhow::Error::msg(format!(
                        "unknown release level `{level}` (valid: patch, minor, major)"
                    ))
                })?;
            buff_lang_cli::commands::release::run(lvl, std::path::Path::new("."))
        }
        Command::Gen { kind, name } => {
            let k = buff_lang_cli::commands::gen::kind_from_str(&kind).ok_or_else(|| {
                anyhow::Error::msg(format!(
                    "unknown generator kind `{kind}` \
                         (valid: module, test, example)"
                ))
            })?;
            buff_lang_cli::commands::gen::run(k, &name)
        }
        Command::Test {
            path,
            filter,
            update,
            detect_races,
        } => buff_lang_cli::commands::test::run(&path, filter.as_deref(), update, detect_races),
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
            error_format,
            target,
            no_color,
        } => {
            use buff_lang_cli::check::{CheckOutcome, ErrorFormat};
            let format = ErrorFormat::from_str(&error_format);
            let outcome = buff_lang_cli::commands::check::run(
                &file,
                deny_warnings,
                format,
                target.as_deref(),
                no_color,
            )?;
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
        Command::Expand { file, output } => {
            buff_lang_cli::commands::expand::run(&file, output.as_deref())
        }
        Command::Deps { why } => buff_lang_cli::commands::deps::run(why.as_deref()),
        Command::Outdated => buff_lang_cli::commands::outdated::run(),
        Command::Search { query } => buff_lang_cli::commands::search::run(query.as_deref()),
        Command::Ai { cmd } => {
            use buff_lang_cli::check::CheckOutcome;
            let outcome = buff_lang_cli::commands::ai::run(cmd)?;
            if matches!(outcome, CheckOutcome::HasErrors) {
                std::process::exit(1);
            }
            Ok(())
        }
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
        Command::BenchCompile => buff_lang_cli::commands::bench_compile::run(),
        Command::BenchColdStart => buff_lang_cli::commands::bench_cold_start::run(),
        Command::Bench {
            output,
            fixtures_dir,
            no_backend,
        } => buff_lang_cli::commands::bench::run(
            output.as_deref(),
            fixtures_dir.as_deref(),
            no_backend,
        ),
        Command::Refactor { cmd } => buff_lang_cli::commands::refactor::run(cmd),
        Command::BenchProgram {
            file,
            iterations,
            warmup,
        } => buff_lang_cli::commands::bench_program::run(&file, iterations, warmup),
        Command::Generate {
            template,
            name,
            output,
            list,
        } => {
            if list {
                buff_lang_cli::commands::generate::print_template_list();
                return Ok(());
            }
            buff_lang_cli::commands::generate::run(&template, &name, output)
        }
        Command::Watch { file, interval } => {
            // The `--interval MS` CLI flag is accepted for forward
            // compatibility (a persistent salsa DB across watch cycles
            // would use it for the poll cadence — see T7); the current
            // legacy watch loop polls on `notify` events, so the value
            // is honoured only as a stderr note when set below the
            // default 500 ms.
            if interval != 500 {
                eprintln!(
                    "note: --interval {interval} is accepted but the current watch \
                     implementation uses the notify event-driven loop (T7 lays \
                     the foundation for an interval-based polling mode); the \
                     flag will take effect once the persistent salsa DB lands."
                );
            }
            buff_lang_cli::commands::watch::run(&file, None)
        }
        Command::Profile {
            file,
            alloc,
            output,
        } => buff_lang_cli::commands::profile::run(&file, alloc, output.as_deref()),
        Command::Fix { file, dry_run } => {
            buff_lang_cli::commands::fix::run(&file, dry_run)?;
            Ok(())
        }
    }
}
