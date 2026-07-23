//! Command-line argument definitions for the `buff` binary.
//!
//! Built on [`clap`] derive. Subcommands supported:
//!
//! - `buff build <FILE> [--release] [--minimal]` — compile a `.buff` file to
//!   a native executable. `--release` (T56) compiles with `-C opt-level=3
//!   -C lto=fat -C codegen-units=1` for maximum-speed optimization at the
//!   cost of slower compile times. `--minimal` (T60) compiles with
//!   `-C opt-level=z -C panic=abort -C strip=symbols -C lto=true
//!   -C codegen-units=1` for minimum binary size (target: <5 MB for
//!   console-template apps). Default (neither flag) is the fast-debug
//!   profile (mirrors `cargo build` vs `cargo build --release` vs
//!   `cargo build --profile minimal`). When both flags are set, `--minimal`
//!   takes precedence (mirrors cargo's `--profile` semantics).
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

        /// Build with size-minimization optimizations (T60):
        /// `-C opt-level=z -C panic=abort -C strip=symbols -C lto=true
        /// -C codegen-units=1`. Slowest to compile, smallest binary.
        /// Use when the size budget matters more than runtime speed
        /// (Lambda layers, embedded wasm shells, distribution images).
        /// Target: <5 MB for console-template apps.
        ///
        /// Takes precedence over `--release` when both are set (mirrors
        /// cargo's `--profile` semantics — a more-specific profile wins).
        /// Opt-in: default (omitted) keeps the fast-debug profile.
        #[arg(long)]
        minimal: bool,

        /// Build with Profile-Guided Optimization (T62). Automates the
        /// 3-step rustc/LLVM PGO flow:
        ///
        /// - `buff build --pgo <FILE>` (Phase 1 — instrument): compiles
        ///   with `-C profile-generate=./target/pgo-data` so the binary
        ///   emits edge-profiling counters into `./target/pgo-data/`
        ///   on every run.
        /// - **Phase 2 (manual)**: run the instrumented binary against a
        ///   representative workload (e.g. `./pgo_demo && ./pgo_benchmark`,
        ///   or your app's real test suite). Counter data is written as
        ///   `*.profraw` files.
        /// - `buff build --pgo --use <FILE>` (Phase 3 — profile-guided
        ///   rebuild): merges the `.profraw` files via `llvm-profdata`
        ///   into `./target/pgo-data/merged.profdata`, then recompiles
        ///   with `-C profile-use=./target/pgo-data/merged.profdata`.
        ///   LLVM uses the profile to drive inlining + block layout,
        ///   typically yielding 10%+ speedup vs `--release` on
        ///   compute-heavy code.
        ///
        /// **`llvm-profdata` requirement**: Phase 3 (`--pgo --use`)
        /// requires `llvm-profdata` on `PATH` (`rustup component add
        /// llvm-tools-preview`). When missing, the build surfaces a
        /// stderr note + exits non-zero.
        ///
        /// Opt-in: default (omitted) keeps the fast-debug profile.
        /// Independent of `--release`/`--minimal`/`--fast` (PGO is an
        /// orthogonal axis — it instruments OR consumes a profile, it
        /// does not select a size/speed knob).
        #[arg(long)]
        pgo: bool,

        /// Phase-3 selector for `--pgo` (T62). Only meaningful when
        /// `--pgo` is also set. When omitted (default), `--pgo` runs
        /// Phase 1 (instrument). When set, `--pgo` runs Phase 3
        /// (profile-guided rebuild using the merged profile data).
        ///
        /// Has no effect on its own — `buff build --use` (without
        /// `--pgo`) is a no-op flag that falls through to the normal
        /// build path.
        #[arg(long = "use")]
        pgo_use: bool,

        /// Build with the no-optimization "fast" profile (T55):
        /// `-C opt-level=0 -C debuginfo=0`. Fastest possible compile,
        /// slowest runtime. The dev inner-loop mode for "does it
        /// compile + run?" feedback. Distinct from `--minimal` (which
        /// optimizes for binary SIZE) and `--release` (which optimizes
        /// for runtime SPEED) — `--fast` optimizes for COMPILE speed.
        ///
        /// Lowest precedence: `--minimal` and `--release` both override
        /// it when set together (a user who passes `--release --fast`
        /// clearly wants the optimised binary).
        ///
        /// Opt-in: default (omitted) keeps the fast-debug profile
        /// (`-O`, which runs `opt-level=2`). `--fast` is strictly faster
        /// to compile than the default because it skips LLVM optimisation
        /// entirely.
        #[arg(long)]
        fast: bool,

        /// Bypass the generated-Rust cache (T55). By default `buff build`
        /// caches the codegen output keyed on a SHA-256 hash of the
        /// `.buff` source — a cache hit skips the entire lex → parse →
        /// codegen pass. `--no-cache` forces a full front-end re-run.
        ///
        /// Use this when you suspect stale cache content (e.g. after a
        /// compiler upgrade — the cache key is source-only, so a new
        /// compiler version would serve the old codegen output).
        ///
        /// Opt-in: default (omitted) keeps caching ON.
        #[arg(long)]
        no_cache: bool,

        /// Wrap the rustc invocation in `sccache` for cross-project crate
        /// caching (T55). When sccache is on `PATH`, the rustc call
        /// becomes `sccache rustc ...` so compiled crates are shared
        /// across projects. Also writes a `.cargo/config.toml` snippet
        /// (`rustc-wrapper = "sccache"`) so subsequent bare `cargo
        /// build` / `cargo test` invocations go through sccache too.
        ///
        /// When sccache is requested but NOT installed, the build falls
        /// back to bare `rustc` with a stderr note (never fails the build).
        ///
        /// Opt-in: default (omitted) does NOT use sccache (it has side
        /// effects — runs a background server, writes to
        /// `~/.cache/sccache/`).
        #[arg(long)]
        sccache: bool,

        /// T1: cross-compile for `<TRIPLE>` (e.g.
        /// `x86_64-unknown-linux-gnu`, `wasm32-wasi`). Forwards to
        /// cargo's `--target` flag. Pass `list` to print the
        /// Buff-supported target set and exit (no build performed).
        #[arg(long, value_name = "TRIPLE")]
        target: Option<String>,
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
    /// (T112): `--lib`, `--server`, `--gpu`, `--workspace`. Alternatively,
    /// use `--template <NAME>` (T0-C1) to pick from the 7 v2 templates:
    /// `console`, `lib`, `web`, `ml`, `game`, `pipeline`, `workspace`.
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

        /// T0-C1: pick a template by name. One of: `console`, `lib`,
        /// `web`, `ml`, `game`, `pipeline`, `workspace`. Mutually
        /// exclusive with the legacy `--lib`/`--server`/`--gpu`/
        /// `--workspace` flags.
        #[arg(long, value_name = "NAME")]
        template: Option<String>,
    },

    /// `buff gen <kind> <name>` — generate boilerplate files (T0-C2).
    ///
    /// Three generators reduce the manual steps of starting a new
    /// module / test / example:
    ///
    /// - `buff gen module <name>` — creates `src/modules/<name>.buff`
    ///   (with `export func placeholder()`) plus
    ///   `tests/unit/test_<name>.buff` (with a `@test` stub that imports
    ///   the new module).
    /// - `buff gen test <name>` — creates `tests/unit/<name>.buff` with
    ///   a single `@test` stub fn.
    /// - `buff gen example <name>` — creates `examples/<name>.buff` with
    ///   a `func main():` stub.
    ///
    /// Refuses to clobber existing files. Parent directories are created
    /// on demand.
    Gen {
        /// Generator kind: `module`, `test`, or `example`.
        #[arg(value_name = "KIND")]
        kind: String,

        /// Name of the artifact (must be a valid Buff identifier).
        #[arg(value_name = "NAME")]
        name: String,
    },

    /// `buff doc` — emit placeholder per-crate HTML API docs (T0-E3).
    ///
    /// Walks `src/` for `.buff` files and emits `docs/<package>/index.html`
    /// per crate, plus a top-level `docs/index.html` linking them.
    /// **Rendering is scaffold-only in v1.13** — full Rustdoc-quality
    /// rendering arrives in v1.18+.
    Doc,

    /// `buff release <patch|minor|major>` — bump version, update
    /// CHANGELOG, tag git (T0-I3 scaffold).
    ///
    /// Verifies clean working tree, bumps the version in `buff.toml`,
    /// prepends a section to `CHANGELOG.md`, stages both files, and
    /// creates a git tag `v<X.Y.Z>`. **Does NOT invoke `buff publish`
    /// in v1.13** — registry integration arrives with v1.14.
    Release {
        /// Bump level: `patch`, `minor`, or `major`.
        #[arg(value_name = "LEVEL")]
        level: String,
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

    /// Print the project's dependency tree (T128).
    ///
    /// Reads `buff.toml` from the current directory and renders every
    /// declared dependency across all three dependency kinds
    /// (`[rust-deps]`, `[git-dependencies]`,
    /// `[registry-dependencies]`) in cargo-tree style: package name,
    /// version requirement, and source. Mirrors `cargo tree` so the
    /// output shape is familiar to Rust developers.
    ///
    /// Pass `--why <PKG>` to print the chain explaining why `<PKG>`
    /// is present (which section declares it, what version / source,
    /// and the root package that requires it). Useful for debugging
    /// "where did this dependency come from?".
    Deps {
        /// Show the chain explaining why `<PKG>` is included.
        /// Prints the declaring section + version + the root
        /// package that requires `<PKG>`.
        #[arg(long, value_name = "PKG")]
        why: Option<String>,
    },

    /// Report outdated registry dependencies (T128).
    ///
    /// For every entry under `[registry-dependencies]`, queries the
    /// buff registry (`$BUFF_REGISTRY_URL`, default
    /// `http://127.0.0.1:7878`) for the latest published version
    /// and prints any whose pinned version requirement resolves to
    /// a version older than the registry's absolute latest. Mirrors
    /// `cargo outdated`.
    ///
    /// Entries that fail to resolve (registry unreachable, package
    /// unknown, semver parse failure) are surfaced as warnings
    /// rather than aborting the whole report.
    Outdated,

    /// `buff search [QUERY]` — search the buff registry (T70).
    ///
    /// Calls `GET /api/v1/search?q=<QUERY>` on the buff registry
    /// (`$BUFF_REGISTRY_URL`, default `http://127.0.0.1:7878`) and
    /// prints each result with quality badges inline:
    ///
    /// ```text
    /// [verified] [maintained] [tested 85%] [documented 72%] buff-dataframe 1.0.0
    /// ```
    ///
    /// When `<QUERY>` is omitted, all published packages are listed.
    /// The query is a case-insensitive substring match against
    /// package names (mirrors `cargo search`).
    Search {
        /// The search query (case-insensitive substring). Omit to
        /// list all published packages.
        #[arg(value_name = "QUERY")]
        query: Option<String>,
    },

    /// `buff ai` — AI assistant integration (T65).
    ///
    /// Generates an "AI context pack" describing the Buff language + the
    /// current project so users can paste it into Copilot / Claude / etc.
    /// Also runs `buff check` on AI-generated `.buff` code via `verify`.
    ///
    /// # Subcommands
    ///
    /// - `buff ai context [--project <PATH>] [--output <PATH>]` — emit a
    ///   Markdown context pack to stdout (or `--output <file>`). The pack
    ///   includes: language syntax summary, available prelude types +
    ///   functions, per-Type method signatures, current project structure
    ///   (when run inside a project), and pointers to the examples/
    ///   directory. `--project <PATH>` overrides the project root
    ///   (default: current directory). Pure-offline — does NOT call any
    ///   AI APIs; the user copies the pack into their AI tool of choice.
    /// - `buff ai verify <FILE>` — type-check AI-generated Buff code via
    ///   the existing T55 standalone typecheck pipeline. Returns the same
    ///   exit codes as `buff check` (0 clean, 0 warnings, 1 errors) plus
    ///   AI-friendly hints (e.g. "did you mean `print` instead of
    ///   `prnt`?") appended to error diagnostics.
    Ai {
        /// The subcommand to run.
        #[command(subcommand)]
        cmd: AiCmd,
    },

    /// Jupyter kernel management (T129a).
    ///
    /// Subcommands:
    ///
    /// - `buff jupyter install` — write the kernelspec `kernel.json`
    ///   into the Jupyter data dir (prefers shelling out to
    ///   `jupyter kernelspec install`; falls back to a direct write
    ///   when `jupyter` is not on `PATH`).
    /// - `buff jupyter start --connection-file <PATH>` — boot the
    ///   kernel message loop using the connection JSON that Jupyter
    ///   wrote at launch time. Used internally by Jupyter; users do
    ///   not invoke this directly.
    Jupyter {
        /// The subcommand to run.
        #[command(subcommand)]
        cmd: JupyterCmd,
    },

    /// UI dev-server management (T131).
    ///
    /// Subcommands:
    ///
    /// - `buff ui dev [PATH] [--port <N>]` — boot the dev server that
    ///   watches `.buff` files in the project, serves static assets +
    ///   the generated Wasm bundle over HTTP on `localhost:8080`
    ///   (overridable via `--port`), and live-reloads connected
    ///   browsers via WebSocket on `.buff` save. LIVE RELOAD (full
    ///   page refresh) is the v1.8 deliverable; true state-preserving
    ///   HMR is explicitly v1.9+ work.
    Ui {
        /// The subcommand to run.
        #[command(subcommand)]
        cmd: UiCmd,
    },

    /// `buff coverage [PATH] [--html] [--lcov] [--output <PATH>] [--release]`
    /// — collect + render `.buff` source coverage (T137).
    ///
    /// Wraps `cargo llvm-cov` (preferred) or `cargo-tarpaulin` (fallback),
    /// captures Rust-level line coverage on the generated `.rs` file,
    /// and reverse-maps the hits back to `.buff` lines via the T60
    /// [`SourceMap`](buff_lang_error::SourceMap).
    ///
    /// # Modes
    ///
    /// - Default (no flags) — prints a per-file coverage summary to stdout.
    /// - `--html` — writes a self-contained HTML report (use `--output`
    ///   to set the path; default `coverage/index.html`).
    /// - `--lcov` — writes an LCOV `.info` tracefile (default
    ///   `coverage/lcov.info`).
    /// - `--html --lcov` — writes both.
    ///
    /// # Tool detection
    ///
    /// Detects `cargo llvm-cov` first (preferred — faster, native
    /// LCOV emitter, workspaces). Falls back to `cargo-tarpaulin`.
    /// When neither is installed, prints a install hint + exits
    /// non-zero (see `.sisyphus/evidence/task-137-coverage-USER-ACTION.txt`
    /// for the PowerShell install recipe).
    ///
    /// # Build-host limitation
    ///
    /// The mapping layer + report generation are fully local-buildable
    /// and unit-tested. The actual llvm-cov / tarpaulin invocation
    /// requires the tool to be installed on the host — see the
    /// USER-ACTION recipe for the install + run walkthrough.
    Coverage {
        /// Input `.buff` source file (omit to coverage-run the
        /// whole project — currently a single-file pipeline only).
        #[arg(value_name = "FILE")]
        path: Option<PathBuf>,

        /// Emit a self-contained HTML report (in addition to the
        /// stdout summary).
        #[arg(long)]
        html: bool,

        /// Emit an LCOV `.info` tracefile (in addition to the stdout
        /// summary).
        #[arg(long)]
        lcov: bool,

        /// Output path. For `--html`, defaults to `coverage/index.html`.
        /// For `--lcov`, defaults to `coverage/lcov.info`. When both
        /// are set, `--output` is treated as a directory prefix.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Run the underlying coverage tool with release optimizations
        /// (`cargo llvm-cov --release` / `cargo tarpaulin --release`).
        /// Off by default — debug coverage is faster to collect, which
        /// usually matters more in the tight edit-test loop than the
        /// runtime speedup.
        #[arg(long)]
        release: bool,
    },

    /// `buff ssr <FILE>` — Server-Side Render a `.buffhtml` Single-File
    /// Component to HTML on stdout (T135).
    ///
    /// Parses + codegens the `.buffhtml` via the existing T133 path
    /// (`pipeline::compile_buffhtml_to_rust`), wraps the generated
    /// component fn in an SSR driver `fn main()` that calls
    /// `buff_ui_dioxus::render_to_string(ComponentFn)`, compiles via
    /// `rustc` (host target — no `wasm32-unknown-unknown`), runs the
    /// binary, and forwards the rendered HTML to stdout (or
    /// `--output <file>` when provided).
    ///
    /// **Event handlers are ignored during SSR** (no user to click);
    /// only the initial state of any `use_signal` is rendered. To
    /// re-attach interactivity in the browser, ship the same component
    /// compiled to wasm32 + call `dioxus::launch` against the SSR
    /// output (hydration recipe at
    /// `.sisyphus/evidence/task-135-hydration-USER-ACTION.txt`).
    ///
    /// Mobile (iOS/Android) is documented as a USER ACTION recipe at
    /// `.sisyphus/evidence/task-135-mobile-USER-ACTION.txt`.
    Ssr {
        /// Input `.buffhtml` Single-File Component source file.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Output HTML path (default: stdout). When provided, the
        /// rendered HTML is written to `<OUTPUT>` (overwriting if it
        /// exists) instead of being printed to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Build the SSR driver binary with release optimizations
        /// (`-C opt-level=3 -C lto=fat -C codegen-units=1`). Off by
        /// default — debug compiles are faster, which usually matters
        /// more for SSR's tight edit-render loop than runtime speed.
        #[arg(long)]
        release: bool,
    },

    /// `buff debug <FILE> [--backend <lldb-dap|codelldb|vscode-lldb>]
    /// [--source-map <PATH>] [--release]` — Debug Adapter Protocol
    /// server (T136).
    ///
    /// Launches the `buff-dap` translation proxy that bridges an
    /// editor (VSCode via CodeLLDB / lldb-dap adapter type) to a
    /// Rust-capable backend debugger. The proxy intercepts two DAP
    /// request types and applies Buff's T60 [`SourceMap`]
    /// translation:
    ///
    /// - `setBreakpoints` — translates `.buff` line → generated `.rs`
    ///   line before forwarding to the backend.
    /// - `stackTrace` — translates `.rs` frames → `.buff` frames for
    ///   the editor to render.
    ///
    /// All other DAP requests pass through verbatim. The lifecycle
    /// handshake (initialize / launch / continue / disconnect) is
    /// proxied unchanged.
    ///
    /// # Backend selection
    ///
    /// When `--backend <NAME>` is omitted, the server auto-detects
    /// the best installed backend in preference order: `lldb-dap`
    /// (preferred, ships with llvm) → `codelldb` → `vscode-lldb`.
    /// When none is found, prints an install hint + exits non-zero
    /// (USER ACTION — see
    /// `.sisyphus/evidence/task-136-debugger-USER-ACTION.txt`).
    ///
    /// # Build-host limitation
    ///
    /// The translation layer + protocol surface are fully
    /// local-buildable and unit-tested. The actual lldb-dap /
    /// codelldb invocation requires the backend to be installed on
    /// the host — see the USER-ACTION recipe for the install + run
    /// walkthrough.
    Debug {
        /// Input `.buff` source file to debug.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Backend debugger to spawn. Auto-detected when omitted.
        /// One of: `lldb-dap`, `codelldb`, `vscode-lldb`.
        #[arg(long, value_name = "NAME")]
        backend: Option<String>,

        /// Explicit source-map JSON file path. When omitted, the
        /// CLI re-runs the front-end pipeline to regenerate the
        /// `.rs` file alongside the `.buff` and uses the identity
        /// mapping (rust_line == buff_line) — the v1.10 stopgap
        /// until codegen emits real source-map markers (see
        /// `task-136-debugger.txt` GAP-1).
        #[arg(long, value_name = "PATH")]
        source_map: Option<PathBuf>,
    },

    /// `buff backtrace <LOG>` — post-process a captured Rust panic log
    /// into a Buff-source-mapped stack trace (T24).
    ///
    /// Reads a Rust panic log / core-dump backtrace from `<LOG>` (or
    /// stdin when `<LOG>` is `-`), loads the `.buffmap` sidecar (from
    /// `BUFF_MAP_PATH` env var or `<LOG>.buffmap` or
    /// `<current_exe>.buffmap`), and prints a Buff-stack-trace by
    /// reverse-mapping each Rust frame to its originating `.buff`
    /// source location.
    ///
    /// **Offline use**: this subcommand does NOT invoke rustc or the
    /// Buff pipeline. It's a pure post-processor over a recorded Rust
    /// trace + a `.buffmap` sidecar — useful for incident review,
    /// bug-report triage, and CI-failure forensics.
    ///
    /// When no `.buffmap` is found, prints the input log unchanged +
    /// exits with a warning (defensive — the user can still inspect
    /// the raw Rust trace).
    Backtrace {
        /// Path to the captured Rust panic log / backtrace. Use `-`
        /// to read from stdin.
        #[arg(value_name = "LOG")]
        log: PathBuf,

        /// Explicit `.buffmap` source-map path. When omitted, the
        /// subcommand discovers one via `BUFF_MAP_PATH` or sibling
        /// file conventions.
        #[arg(long, value_name = "PATH")]
        buffmap: Option<PathBuf>,
    },

    /// `buff bench-compile` — measure + record compile times across
    /// project sizes (T55).
    ///
    /// Synthesises deterministic small/medium/large `.buff` fixtures
    /// (5 / 50 / 200 functions), times the full pipeline
    /// (codegen + rustc) on each, and appends a dated row to
    /// `benchmarks/compile-speed.md`. When a previous report exists,
    /// prints a delta comparison (faster / slower / unchanged) so
    /// regressions are visible at a glance.
    ///
    /// The benchmark is deterministic (same fixtures every run) so
    /// cross-commit comparisons are meaningful. Wall-clock times are
    /// host-dependent — use the delta column, not absolute numbers,
    /// to judge regressions.
    ///
    /// # Output
    ///
    /// - Prints a per-tier summary table to stdout.
    /// - Appends a dated row to `benchmarks/compile-speed.md`
    ///   (created if missing) in the current directory.
    BenchCompile,

    /// `buff bench-cold-start` — measure + record native-binary
    /// cold-start time (T61).
    ///
    /// Compiles a minimal `.buff` fixture (`print("hello")`) to a
    /// native executable via the Buff pipeline → rustc, then times
    /// the wall-clock duration from process spawn to first byte on
    /// stdout across N runs (default 10). The median, min, and max
    /// are reported.
    ///
    /// **Acceptance target**: Buff cold-start < 50 ms (matching
    /// bare Rust). The benchmark emits a JSON + Markdown report at
    /// `benchmarks/cold-start.{json,md}` documenting the
    /// methodology + comparison table for Go / Rust / Java / Python
    /// on AWS Lambda + Cloudflare Workers.
    ///
    /// # MVP scope
    ///
    /// This subcommand measures **the Buff binary only** — it does
    /// NOT spawn Go / Rust / Java / Python programs (those reference
    /// numbers are documented in `benchmarks/cold-start.md` from
    /// published third-party benchmarks). It does NOT deploy to AWS
    /// Lambda or Cloudflare Workers — the local measurement is a
    /// faithful proxy for the cold-start component (process spawn +
    /// first output) since neither runtime adds per-language
    /// overhead on top of the native binary.
    ///
    /// # Output
    ///
    /// - Prints a per-run + summary table to stdout.
    /// - Writes `benchmarks/cold-start.json` (machine-readable).
    /// - Writes/appends `benchmarks/cold-start.md` (human-readable).
    BenchColdStart,

    /// `buff refactor` — non-interactive refactoring tools (T66).
    ///
    /// Three subcommands operate on `.buff` source by parsing it to
    /// AST, applying a transformation, and writing the canonical
    /// formatted output back:
    ///
    /// - `buff refactor rename <OLD> <NEW> [--files <GLOB>]` — rename
    ///   an identifier across one file (when `--files` is omitted or
    ///   points to a single file) or across every `.buff` file under
    ///   a directory tree (when `--files` points to a directory). The
    ///   MVP does NOT resolve scopes; it renames every textual match
    ///   in the AST identifier nodes (function names, struct names,
    ///   let-binding names, references, etc.). Future work: scope-
    ///   aware rename that respects shadowing.
    ///
    /// - `buff refactor extract-function <FILE> <START>-<END> <NAME>`
    ///   — lift the contiguous range of statements on lines
    ///   `[START, END]` (1-indexed, inclusive on both ends) inside
    ///   the FIRST function in `<FILE>` into a NEW top-level function
    ///   named `<NAME>` (with no parameters and no return type for
    ///   MVP — the body is moved verbatim and replaced at the
    ///   extraction site by a call to the new function). `<START>`/`<END>`
    ///   must fall inside the same function body; an error is reported
    ///   otherwise.
    ///
    /// - `buff refactor inline-variable <FILE> <NAME>` — find the
    ///   first `let <NAME> = expr` binding in `<FILE>`, replace
    ///   every subsequent `Ident(<NAME>)` reference in the same
    ///   function body with `expr`, and remove the original `let`.
    ///   MVP scope: the binding's initializer must be a Literal or
    ///   an Ident (a side-effect-free expression we can safely
    ///   duplicate); more complex initializers (calls, method
    ///   chains) are reported with a clear "unsupported initializer
    ///   shape" error rather than risk changing semantics.
    ///
    /// All three subcommands are non-interactive (CLI-only). LSP
    /// code-action integration is deferred — the spec's "LSP gains
    /// code actions for interactive versions" is explicitly a
    /// follow-up.
    Refactor {
        /// The subcommand to run.
        #[command(subcommand)]
        cmd: RefactorCmd,
    },

    /// Watch `.buff` files for changes + rebuild on save. Debounced 500ms.
    ///
    /// Usage: `buff watch <PATH> [--exec <CMD>]`. Recursively watches
    /// the file or directory at `<PATH>` (default: `.`) and on each
    /// `.buff` change runs [`commands::build::run`] with the changed
    /// file. When `--exec <CMD>` is set, the command runs after each
    /// rebuild (e.g. `--exec "buff test"` or `--exec "./serve.sh"`);
    /// CMD is interpreted via `sh -c` on Unix + `cmd /c` on Windows.
    ///
    /// Loop until Ctrl-C / SIGINT. **T64 SIMPLIFIED SCOPE**: this
    /// commit ships the standalone file-watcher + rebuild loop only.
    /// Server route hot-swap (the original T64 spec) is deferred to a
    /// later commit — `buff watch` does NOT touch `buff-web`.
    Watch {
        /// File or directory to watch (recursive). Default: current
        /// working directory. A single `.buff` file is also accepted
        /// (the watcher then observes its containing directory so
        /// sibling module edits also trigger rebuilds).
        #[arg(value_name = "PATH", default_value = ".")]
        path: PathBuf,

        /// Optional command to run after each successful rebuild. The
        /// command runs via `sh -c <CMD>` on Unix + `cmd /c <CMD>` on
        /// Windows. Failures are surfaced as stderr notes but do NOT
        /// exit the watch loop (you typically want to keep iterating
        /// even when the post-build hook fails).
        #[arg(long, value_name = "CMD")]
        exec: Option<String>,
    },
}

/// Subcommands of `buff refactor` (T66).
#[derive(Subcommand, Debug)]
pub enum RefactorCmd {
    /// `buff refactor rename <OLD> <NEW> [--files <PATH>]` — rename
    /// an identifier across one file or a directory tree of `.buff`
    /// files. Rewrites the file(s) in place using the canonical
    /// Buff formatter.
    Rename {
        /// The current identifier name (must be a valid Buff
        /// identifier, not a keyword).
        #[arg(value_name = "OLD")]
        old: String,

        /// The new identifier name (must be a valid Buff identifier,
        /// not a keyword).
        #[arg(value_name = "NEW")]
        new: String,

        /// Path to the file or directory to rewrite. When omitted,
        /// operates on every `.buff` file under the current working
        /// directory (recursive walk). When a single file, rewrites
        /// just that file. When a directory, rewrites every `.buff`
        /// file under it.
        #[arg(long, value_name = "PATH")]
        files: Option<PathBuf>,
    },

    /// `buff refactor extract-function <FILE> <START>-<END> <NAME>`
    /// — lift the statements on lines `[START, END]` (1-indexed,
    /// inclusive) inside the first function of `<FILE>` into a new
    /// top-level function `<NAME>`, replacing the lifted range with
    /// a call to the new function.
    ExtractFunction {
        /// Input `.buff` source file.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Line range as `<START>-<END>` (1-indexed, inclusive).
        /// Example: `5-8` extracts lines 5, 6, 7, 8.
        #[arg(value_name = "RANGE")]
        range: String,

        /// Name of the new function. Must be a valid Buff identifier
        /// and not a keyword.
        #[arg(value_name = "NAME")]
        name: String,
    },

    /// `buff refactor inline-variable <FILE> <NAME>` — inline the
    /// first `let <NAME> = expr` binding in `<FILE>`: every
    /// subsequent `Ident(<NAME>)` reference (in the same function
    /// body) is replaced by `expr`, and the `let` itself is removed.
    InlineVariable {
        /// Input `.buff` source file.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Name of the `let` binding to inline.
        #[arg(value_name = "NAME")]
        name: String,
    },
}

/// Subcommands of `buff ai` (T65).
#[derive(Subcommand, Debug)]
pub enum AiCmd {
    /// Generate an AI context pack (Markdown) describing the Buff
    /// language surface + the current project's structure. Emit to
    /// stdout by default, or `--output <file>` to write to disk.
    Context {
        /// Output path. When omitted, the pack is printed to stdout.
        /// When set, the pack is written to `<OUTPUT>` (overwriting
        /// if it exists) instead of being printed.
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,

        /// Project root to scan for `.buff` sources. Default: the
        /// current working directory. When the path has no `.buff`
        /// files (and no `buff.toml`), the project-structure section
        /// is omitted from the pack (the language surface is always
        /// emitted).
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
    },

    /// Type-check AI-generated `.buff` code via the T55 standalone
    /// typecheck pipeline. Reports errors with AI-friendly hints
    /// appended (e.g. suggestions for misspelled prelude functions).
    /// Same exit-code semantics as `buff check` (0 clean, 0 warnings,
    /// 1 errors).
    Verify {
        /// Input `.buff` source file produced by an AI tool.
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
}

/// Subcommands of `buff jupyter`.
#[derive(Subcommand, Debug)]
pub enum JupyterCmd {
    /// Write the kernelspec `kernel.json` into the Jupyter data dir
    /// so `jupyter console --kernel buff` (and JupyterLab / Notebook)
    /// can discover + launch the Buff kernel.
    ///
    /// Prefers shelling out to `jupyter kernelspec install --replace
    /// --name=buff <tempdir>` when `jupyter` is on `PATH`. Falls back
    /// to writing directly into `<JUPYTER_DATA_DIR>/kernels/buff/`
    /// (resolved from `$JUPYTER_DATA_DIR`, `$APPDATA` on Windows,
    /// `$HOME/Library/Jupyter` on macOS, `~/.local/share/jupyter` on
    /// Linux).
    Install,

    /// Boot the kernel message loop. Invoked by Jupyter via the
    /// `argv` template in `kernel.json` (not directly by users).
    Start {
        /// Path to the connection JSON Jupyter wrote for this kernel
        /// session (passed in via the `{connection_file}` template
        /// substitution).
        #[arg(long, value_name = "PATH")]
        connection_file: PathBuf,
    },
}

/// Subcommands of `buff ui`.
#[derive(Subcommand, Debug)]
pub enum UiCmd {
    /// `buff ui new --desktop <NAME>` — scaffold a new Tauri 2.0 desktop app
    /// project in a fresh `<NAME>/` directory (T132).
    ///
    /// Produces a runnable Tauri project with a Buff-Wasm-Dioxus frontend
    /// (the T130 counter example). The scaffolded project includes:
    ///
    /// - `buff.toml` — project manifest with `[ui]` section.
    /// - `src/main.buff` — Buff UI entry point.
    /// - `src-tauri/` — Tauri 2.0 Rust project (Cargo.toml, tauri.conf.json,
    ///   build.rs, src/main.rs, src/lib.rs).
    /// - `static/index.html` — HTML shell that loads the Wasm bundle.
    ///
    /// Requires the Tauri CLI (`cargo install tauri-cli`) to build the native
    /// binary. The Wasm frontend is built via `buff ui build --desktop`.
    New {
        /// Name of the desktop app project (must be a valid Buff identifier).
        #[arg(value_name = "NAME")]
        name: String,

        /// Scaffold a Tauri 2.0 desktop app (the only supported target
        /// for `buff ui new` in v1.8).
        #[arg(long)]
        desktop: bool,
    },

    /// `buff ui build --desktop [PATH]` — build a Tauri 2.0 desktop app
    /// native binary (T132).
    ///
    /// Detects whether `cargo-tauri` is installed. If missing, prints a
    /// helpful install instruction and exits non-zero. If present, shells
    /// out to `cargo tauri build` in the project directory.
    ///
    /// The Wasm frontend must already be built (or the Tauri `beforeBuildCommand`
    /// in `tauri.conf.json` will handle it). The output binary is placed in
    /// `src-tauri/target/release/` by default.
    Build {
        /// Build a Tauri 2.0 desktop app (the only supported target
        /// for `buff ui build` in v1.8).
        #[arg(long)]
        desktop: bool,

        /// Project root directory containing the `src-tauri/` subdirectory.
        /// Defaults to the current directory.
        #[arg(value_name = "PATH", default_value = ".")]
        path: PathBuf,
    },

    /// `buff ui dev [PATH] [--port <N>]` — boot the dev server (T131).
    ///
    /// Watches `.buff` files in `<PATH>` (default: current directory)
    /// for changes, debounces them by 200 ms, and on each change:
    ///
    /// 1. Re-runs the Buff front-end (`pipeline::compile_to_rust`) on
    ///    every changed `.buff` file. If lex/parse/codegen fails, the
    ///    error is broadcast to connected browsers as a
    ///    `{type:"error", message:"..."}` WebSocket frame; the browser
    ///    shows a red banner with the message and a Reconnect button.
    /// 2. Attempts `cargo build --target wasm32-unknown-unknown` on
    ///    the project (when a `Cargo.toml` is present) + re-runs
    ///    `wasm-bindgen --target web` to refresh the served Wasm
    ///    bundle. Failures are surfaced as the same error-overlay
    ///    frame; success broadcasts `{type:"reload"}`.
    /// 3. On `{type:"reload"}` the browser does a full
    ///    `location.reload()` (LIVE RELOAD). State-preserving HMR is
    ///    explicitly v1.9+ work.
    ///
    /// The HTTP server serves `<PATH>/static/` as static assets, falls
    /// back to `<PATH>/target/wasm32-unknown-unknown/<profile>/` for
    /// `.wasm` bundles, and injects a small (<1 KB) client-side
    /// script into served HTML that opens the WebSocket and handles
    /// reload / error / disconnect events.
    ///
    /// Blocks until Ctrl-C / SIGINT.
    Dev {
        /// Project root to serve + watch. Defaults to the current
        /// directory. Must contain (or be the parent of) the
        /// `.buff` sources to watch. A `<PATH>/static/` directory
        /// and/or a `<PATH>/Cargo.toml` are picked up automatically
        /// when present.
        #[arg(value_name = "PATH", default_value = ".")]
        path: PathBuf,

        /// Port to bind the dev server on. Default: 8080. The server
        /// binds `127.0.0.1:<port>` (loopback only — never exposed to
        /// the network; matches Vite / cargo-leptos defaults).
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
}
