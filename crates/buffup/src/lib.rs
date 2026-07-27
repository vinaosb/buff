//! `buffup` — version manager for the Buff language.
//!
//! Downloads pre-built Buff binaries from
//! [GitHub Releases](https://github.com/buff-lang/buff/releases) and
//! installs them under `~/.buff/versions/<ver>/`. The active version
//! is selected via a symlink (Unix) or copy/junction (Windows) at
//! `~/.buff/bin/buff`, which the user adds to their `PATH`.
//!
//! ```text
//!                  ┌──────────────────────────────────────────┐
//!   buffup install │ 1. fetch GitHub Releases API             │
//!       1.0.0  ──▶ │ 2. download gzip tarball                 │
//!                  │ 3. unpack into ~/.buff/versions/1.0.0/   │
//!                  └──────────────────────────────────────────┘
//!
//!                  ┌──────────────────────────────────────────┐
//!   buffup default │ 1. validate target version is installed  │
//!       1.0.0  ──▶ │ 2. locate <ver>/buff binary              │
//!                  │ 3. symlink ~/.buff/bin/buff -> <ver>/buff│
//!                  └──────────────────────────────────────────┘
//!
//!   buffup list  ──▶ enumerate ~/.buff/versions/ + mark active
//!
//!   buffup update ─▶ self-update (NOT YET IMPLEMENTED — see
//!                    commands::update)
//! ```
//!
//! # Cross-platform behavior
//!
//! - **Unix** (`unix` cfg): `std::os::unix::fs::symlink` creates the
//!   active-version pointer. No privileges required.
//! - **Windows** (`windows` cfg): `std::os::windows::fs::symlink_file`
//!   is attempted first (requires Developer Mode or admin). On failure
//!   the command falls back to a plain file copy, which works without
//!   privileges but does NOT auto-track subsequent reinstalls of the
//!   same version (each `buffup default <ver>` rewrites the copy).
//!
//! # Hermetic testing
//!
//! Integration tests in `tests/` override two env vars to avoid
//! touching real network or the user's home directory:
//!
//! - `BUFFUP_HOME` — redirects `~/.buff/` to a `tempfile::TempDir`.
//! - `BUFFUP_GITHUB_API` — redirects the GitHub Releases base URL
//!   (`https://api.github.com/repos/buff-lang/buff` by default) to a
//!   `httpmock::MockServer` base URL.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. All fallible operations surface as [`BuffupError`]
//! (mapped to exit code 1 by [`main.rs`](../main.rs)).

pub mod cli;
pub mod commands;
pub mod error;
pub mod github;
pub mod paths;

pub use cli::{Cli, Command};
pub use error::BuffupError;
pub use github::{Asset, Release, GITHUB_API_BASE, GITHUB_API_BASE_ENV};

use clap::Parser;

/// Dispatch a parsed [`Command`] to the matching [`commands`] module.
///
/// Used by the binary entry point in [`main.rs`](../main.rs) AFTER
/// `Cli::parse()` has already handled `--help` / `--version` / argv
/// errors with clap's conventional exit codes (0 / 0 / 2). Surfaces
/// real command failures as [`BuffupError`].
pub async fn dispatch(command: Command) -> Result<(), BuffupError> {
    match command {
        Command::Install {
            version,
            skip_checksum,
        } => commands::install::run(version, skip_checksum).await,
        Command::Default { version } => commands::default_cmd::run(version),
        Command::List => commands::list::run(),
        Command::Update => commands::update::run(),
    }
}

/// Entry point used by tests and any programmatic consumer that wants
/// to pass argv as a `Vec<String>` (rather than reading `std::env::args()`
/// directly). The binary entry in [`main.rs`](../main.rs) calls
/// [`dispatch`] instead because `Cli::parse()` handles clap's
/// help/version exit codes correctly.
pub async fn run(args: Vec<String>) -> Result<(), BuffupError> {
    let cli = Cli::try_parse_from(args).map_err(BuffupError::Clap)?;
    dispatch(cli.command).await
}
