//! Command-line argument definitions for the `buff` binary.
//!
//! Built on [`clap`] derive. Subcommands supported:
//!
//! - `buff build <FILE> [--release]` — compile a `.buff` file to a native
//!   executable. `--release` (T56) compiles with `-C opt-level=3 -C lto=fat
//!   -C codegen-units=1` for maximum optimization at the cost of slower
//!   compile times; default is the fast-debug profile (mirrors `cargo build`
//!   vs `cargo build --release`).
//! - `buff run <FILE> [ARGS]... [--release]` — compile and immediately
//!   execute, cleaning up temporary artifacts afterwards. `--release` selects
//!   the release optimization profile (T56).
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
//! - `buff add <SPEC> [--branch <X> | --tag <X> | --rev <X>]` — add a git
//!   dependency to the project's `buff.toml`. `<SPEC>` is `git+<URL>` (e.g.
//!   `git+https://github.com/user/lib.buff`). The repo is cloned to
//!   `~/.buff/git/<hash>/` (reused on subsequent adds) and recorded under
//!   the `[git-dependencies]` section of `buff.toml` (T122).
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
    /// Compile a `.buff` file or project into a native executable.
    ///
    /// When invoked with a `.buff` file, compiles that single file directly
    /// via the Buff pipeline → rustc (v0.1 behavior).
    ///
    /// When invoked without a file (in a project with `buff.toml`), generates
    /// `Cargo.toml` from the manifest and shells out to `cargo build` (T120).
    Build {
        /// Input `.buff` source file (optional — omit to build the project
        /// in the current directory via `cargo build`).
        #[arg(value_name = "FILE")]
        file: Option<PathBuf>,

        /// Output executable path (default: `./<file-stem>` with the
        /// platform-appropriate executable extension). Only used when
        /// compiling a single `.buff` file.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Build with release optimizations: `-C opt-level=3 -C lto=fat
        /// -C codegen-units=1` (T56). Slower to compile, faster to run.
        /// Mirrors `cargo build --release`. Default (omitted) keeps the
        /// fast-debug profile used since v0.1.
        #[arg(long)]
        release: bool,
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

        /// Build with release optimizations before executing (T56). Off by
        /// default — debug compiles are faster, which usually matters more
        /// for `buff run`'s tight edit-run loop than the runtime speedup.
        #[arg(long)]
        release: bool,
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

    /// Remove the `target/` build directory (wraps `cargo clean`).
    ///
    /// Deletes all build artifacts. Equivalent to `cargo clean` in the
    /// project root. No effect on source files.
    Clean,

    /// Update all dependencies (wraps `cargo update`).
    ///
    /// Regenerates `Cargo.lock` with the latest compatible versions
    /// matching the version requirements in `buff.toml`. Equivalent to
    /// `cargo update` in the project root.
    Update,

    /// Add a git dependency to the project's `buff.toml` (T122).
    ///
    /// `<SPEC>` is `git+<URL>` (e.g. `git+https://github.com/user/lib.buff`).
    /// The repo is cloned to `~/.buff/git/<sha256(url)[..16]>/` — re-running
    /// `buff add` with the same URL reuses the existing checkout without
    /// re-cloning. Qualifiers mirror Cargo's git-dep flags:
    ///
    /// - `--branch <NAME>` — clone the given branch.
    /// - `--tag <NAME>` — clone the given tag.
    /// - `--rev <SHA>` — clone then `git checkout <SHA>` to pin a specific
    ///   commit.
    ///
    /// The new entry is recorded under `[git-dependencies]` in `buff.toml`,
    /// and `generate_cargo_toml` emits a local-path dependency pointing at
    /// the cloned checkout for offline-friendly builds.
    Add {
        /// Git dependency spec: `git+<URL>` (the `git+` prefix is mandatory
        /// and is stripped before passing `<URL>` to `git clone`). Examples:
        /// `git+https://github.com/u/lib.buff`,
        /// `git+https://github.com/u/lib.git`,
        /// `git+file:///path/to/local/repo`.
        #[arg(value_name = "SPEC")]
        spec: String,

        /// Clone the given branch (mutually-exclusive with `--tag`/`--rev`
        /// in practice; if multiple are set, `--rev` > `--tag` > `--branch`
        /// precedence applies at clone time).
        #[arg(long)]
        branch: Option<String>,

        /// Clone the given tag (passed to `git clone --branch`).
        #[arg(long)]
        tag: Option<String>,

        /// Clone then `git checkout` the given commit-ish (SHA, short or
        /// long). Pins the checkout to an immutable ref unlike `--branch`
        /// /`--tag`.
        #[arg(long)]
        rev: Option<String>,
    },
}
