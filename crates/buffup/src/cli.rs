//! Command-line argument definitions for the `buffup` binary.
//!
//! Built on [`clap`] derive. Subcommands supported:
//!
//! - `buffup install <version>` — download a pre-built Buff binary
//!   from GitHub Releases and unpack it into
//!   `~/.buff/versions/<version>/`.
//! - `buffup default <version>` — point the `~/.buff/bin/buff`
//!   symlink (Unix) or copy (Windows fallback) at the named installed
//!   version.
//! - `buffup list` — enumerate installed versions, marking the active
//!   one with `*`.
//! - `buffup update` — self-update (NOT YET IMPLEMENTED — see
//!   [`crate::commands::update`]).
//!
//! The variant name `Default` shadows the [`std::default::Default`]
//! trait, but this is contained inside the [`Command`] enum and never
//! derived on the enum itself, so there is no ambiguity. The CLI
//! keyword remains the lowercase `default` per the task spec
//! (T139 L2448).

use clap::{error::Error as ClapError, Parser, Subcommand};

/// The top-level CLI shape parsed from `argv`.
#[derive(Parser, Debug)]
#[command(
    name = "buffup",
    version,
    about = "Buff version manager — install and switch between Buff releases"
)]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The set of subcommands supported by `buffup`.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Download and install a Buff version from GitHub Releases.
    Install {
        /// Version to install (semver `MAJOR.MINOR.PATCH`, e.g. `1.0.0`).
        version: String,

        /// Skip SHA-256 checksum verification (NOT RECOMMENDED).
        #[arg(long, default_value_t = false)]
        skip_checksum: bool,
    },

    /// Set the active version by pointing `~/.buff/bin/buff` at the
    /// named installed version's binary.
    Default {
        /// Version to mark as active (must already be installed).
        version: String,
    },

    /// List installed versions; the active one is marked with `*`.
    List,

    /// Self-update buffup (NOT YET IMPLEMENTED — prints guidance).
    Update,
}

/// Re-export of [`clap::error::Error`] so [`BuffupError::Clap`] can wrap
/// it without forcing every consumer to depend on `clap` directly.
pub type ClapResult<T> = std::result::Result<T, ClapError>;
