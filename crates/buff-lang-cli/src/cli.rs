//! Command-line argument definitions for the `buff` binary.
//!
//! Built on [`clap`] derive. Subcommands supported:
//!
//! - `buff build <FILE>` — compile a `.buff` file to a native executable.
//! - `buff run <FILE> [ARGS]...` — compile and immediately execute, cleaning
//!   up temporary artifacts afterwards.
//! - `buff new <NAME> [--lib|--server|--gpu|--workspace]` — scaffold a new
//!   Buff project in a fresh `<NAME>/` directory. Default (no flag) produces
//!   a runnable binary; the flags select alternative starter layouts (T112).
//! - `buff init` — scaffold a Buff project in the current directory.
//! - `buff fmt <FILE> [--check]` — format a `.buff` file in place (T54).
//!   `--check` exits non-zero without writing when the file isn't already
//!   formatted (mirrors `cargo fmt --check`).
//! - `buff check <FILE> [-D/--deny-warnings]` — type-check + naming-convention
//!   linter (T55). Runs lex + parse + type inference WITHOUT codegen or
//!   rustc (faster than `buff build`). Type errors → exit 1. Lint warnings
//!   (e.g. camelCase function names) → exit 0 by default, exit 1 with `-D`.
//!
//! Future subcommands (`doc`, `watch`, `lsp`) will be added in later waves.

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
    ///
    /// By default scaffolds a runnable binary (`src/main.buff`). Pass exactly
    /// one of the template flags to select an alternative starter layout
    /// (T112): `--lib`, `--server`, `--gpu`, `--workspace`.
    New {
        /// Name of the project (must be a valid Buff identifier, not a keyword).
        #[arg(value_name = "NAME")]
        name: String,

        /// Scaffold a library module (`src/lib.buff`, no `main`).
        #[arg(long)]
        lib: bool,

        /// Scaffold an async-server starter template (v1.0 runtime to run).
        #[arg(long)]
        server: bool,

        /// Scaffold a GPU-dispatch starter with `@prefer(gpu)` hints
        /// (v1.0 runtime to run).
        #[arg(long)]
        gpu: bool,

        /// Scaffold a multi-crate workspace layout (`crates/core`,
        /// `crates/utils`).
        #[arg(long)]
        workspace: bool,
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

    /// Format a `.buff` file into canonical form (T54).
    ///
    /// By default rewrites the file in place. Pass `--check` to verify
    /// without writing (exits non-zero if the file isn't already
    /// formatted, mirroring `cargo fmt --check`).
    Fmt {
        /// Input `.buff` source file.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Verify the file is already formatted; do NOT write. Exits with
        /// code 1 if the file would be reformatted.
        #[arg(long)]
        check: bool,
    },

    /// Type-check and lint a `.buff` file WITHOUT running codegen (T55).
    ///
    /// Faster than `buff build` because it skips the syn/quote/prettyplease
    /// codegen pass and the `rustc` compilation. Type errors exit 1.
    /// Naming-convention warnings (e.g. camelCase function names) exit 0
    /// by default; pass `--deny-warnings` / `-D` to treat them as errors.
    Check {
        /// Input `.buff` source file.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Treat lint warnings as errors (exit non-zero on any warning).
        /// Mirrors `rustc -D warnings` / `cargo clippy -- -D warnings`.
        #[arg(short = 'D', long = "deny-warnings")]
        deny_warnings: bool,
    },
}
