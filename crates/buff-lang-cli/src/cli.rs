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
//! - `buff add <SPEC> [--branch <X> | --tag <X> | --rev <X>]` — add a
//!   dependency to the project's `buff.toml`. `<SPEC>` selects the
//!   dependency kind:
//!   - `git+<URL>` (T122) — git-source Buff package dependency. The
//!     repo is cloned to `~/.buff/git/<hash>/` (reused on subsequent
//!     adds) and recorded under the `[git-dependencies]` section.
//!   - `<name>` or `<name>@<req>` (T127) — registry-source Buff
//!     package dependency. `<name>` is resolved against the buff
//!     registry (HTTP `/api/v1/resolve`), and recorded under
//!     `[registry-dependencies]`. Registry URL from
//!     `$BUFF_REGISTRY_URL` (default `http://127.0.0.1:7878`).
//! - `buff login [<TOKEN>]` — authenticate with the buff registry;
//!   store the bearer token in `~/.buff/credentials` (T127).
//! - `buff publish` — pack the current project's `.buff` source into a
//!   tarball and upload it to the registry via `POST /api/v1/publish`
//!   (T127).
//! - `buff install <NAME>` — install a binary package from the
//!   registry: resolve latest version, download tarball, unpack into
//!   `~/.buff/install/<name>/<version>/` (T127).
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

    /// Discover and run `@test` functions in a `.buff` file (T35), OR
    /// run all tests in a project / workspace via `cargo test` (T123).
    ///
    /// When `<FILE>` is provided, discovers `@test` functions via the Buff
    /// test runner. When omitted, reads `buff.toml` from the current
    /// directory and shells out to `cargo test` (workspace mode fans out
    /// to all members automatically).
    Test {
        /// Input `.buff` source file containing `@test` functions. Omit
        /// to run `cargo test` at the project / workspace root (T123).
        #[arg(value_name = "FILE")]
        file: Option<PathBuf>,

        /// Only run tests whose name matches this glob pattern (e.g.
        /// `test_*`). When omitted, all `@test` functions run. Only
        /// meaningful in single-file mode (ignored by `cargo test`).
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

    /// Launch an interactive read-eval-print loop (T125a).
    ///
    /// Type Buff expressions or statements; the REPL evaluates each line
    /// via the full Buff pipeline (lex → parse → codegen → rustc → run)
    /// and prints the result. State (let-bindings, func declarations)
    /// accumulates in-memory for the session. Ctrl-D or Ctrl-C exits.
    ///
    /// The REPL adds NO new compilation logic — it consumes `buff-eval`
    /// (T125-prep) exclusively. Persistence across sessions is deferred
    /// to T125c.
    Repl,

    /// Add a dependency to the project's `buff.toml` (T122 git, T127 registry).
    ///
    /// `<SPEC>` selects the dependency kind:
    ///
    /// - `git+<URL>` (T122): clones the repo to
    ///   `~/.buff/git/<sha256(url)[..16]>/` and records it under
    ///   `[git-dependencies]`. Re-running `buff add` with the same URL
    ///   reuses the existing checkout without re-cloning. Qualifiers
    ///   mirror Cargo's git-dep flags:
    ///   - `--branch <NAME>` — clone the given branch.
    ///   - `--tag <NAME>` — clone the given tag.
    ///   - `--rev <SHA>` — clone then `git checkout <SHA>` to pin a
    ///     specific commit.
    /// - `<name>` or `<name>@<req>` (T127): resolves `<name>` against
    ///   the buff registry (`$BUFF_REGISTRY_URL`, default
    ///   `http://127.0.0.1:7878`), fetches its metadata, and records
    ///   it under `[registry-dependencies]`. The `--branch`/`--tag`/
    ///   `--rev` flags are ignored on this path (a warning is logged).
    Add {
        /// Dependency spec. Either `git+<URL>` (git path) or a bare
        /// `<name>` / `<name>@<req>` (registry path). The kind is
        /// detected at runtime — see [`commands::add::is_registry_spec`]
        /// for the exact shape rules.
        #[arg(value_name = "SPEC")]
        spec: String,

        /// Clone the given branch (mutually-exclusive with `--tag`/`--rev`
        /// in practice; if multiple are set, `--rev` > `--tag` > `--branch`
        /// precedence applies at clone time). Git-path only.
        #[arg(long)]
        branch: Option<String>,

        /// Clone the given tag (passed to `git clone --branch`).
        /// Git-path only.
        #[arg(long)]
        tag: Option<String>,

        /// Clone then `git checkout` the given commit-ish (SHA, short
        /// or long). Pins the checkout to an immutable ref unlike
        /// `--branch`/`--tag`. Git-path only.
        #[arg(long)]
        rev: Option<String>,
    },

    /// Authenticate with the buff registry (T127).
    ///
    /// Stores the bearer token in `~/.buff/credentials` (TOML:
    /// `token = "<value>"`). The token is sent on subsequent
    /// `buff publish` calls via `Authorization: Bearer <token>`.
    ///
    /// When `<TOKEN>` is omitted the CLI reads one line from stdin
    /// (mirrors the `cargo login` UX). For the v1.6 milestone, the
    /// registry ships static-token provisioning — a real GitHub OAuth
    /// flow is deferred (see `buff-registry` crate docs).
    Login {
        /// The bearer token to store. If omitted, the CLI reads one
        /// line from stdin.
        #[arg(value_name = "TOKEN")]
        token: Option<String>,
    },

    /// Pack the current project's `.buff` source into a tarball and
    /// upload it to the buff registry (T127).
    ///
    /// Reads `buff.toml` from the current directory for `[package].name`
    /// and `[package].version`, walks `src/` recursively into a tarball,
    /// and POSTs to `/api/v1/publish` with the stored bearer token
    /// (set via `buff login`).
    ///
    /// Per-version tarball signing and `.buffignore` are deferred.
    Publish,

    /// Install a binary package from the buff registry (T127).
    ///
    /// Resolves `<NAME>` against `/api/v1/resolve/<name>?req=*`
    /// (latest), downloads the tarball via
    /// `/api/v1/download/<name>/<version>` (anonymous, no auth), and
    /// unpacks it into `~/.buff/install/<name>/<version>/`.
    ///
    /// Building the downloaded source into a native binary is
    /// deferred (the registry ships raw `.buff` source tarballs, NOT
    /// pre-built binaries).
    Install {
        /// The package name to install (validated against the
        /// `[a-z0-9_-]` charset the registry enforces).
        #[arg(value_name = "NAME")]
        name: String,
    },
}
