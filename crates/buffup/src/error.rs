//! Error type for the buffup CLI.
//!
//! All fallible operations surface as [`BuffupError`]. The binary
//! entry point in `main.rs` maps every variant to a non-zero exit
//! code via the `Display` impl (which [`std::process::exit`] then
//! surfaces to the user).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. The only `unwrap`-shaped
//! call in the crate is `unwrap_or_else` (the safe variant that
//! never panics), used in `main.rs` for the argv[0] fallback.

use thiserror::Error;

/// The single error type returned by every fallible buffup operation.
#[derive(Debug, Error)]
pub enum BuffupError {
    /// Filesystem I/O failure (creating `~/.buff/versions/`, removing
    /// an existing symlink, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Underlying HTTP transport failure (DNS, connection refused,
    /// TLS handshake, body stream truncated, etc.). Does NOT cover
    /// non-2xx status codes — see [`Self::HttpStatus`].
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// The GitHub Releases API (or tarball download) returned a
    /// non-2xx status. The most common case is `404` when the user
    /// requests a version that has not been published yet — the
    /// `commands::install` module special-cases 404 with a clearer
    /// "GitHub Releases don't exist yet" message before re-returning
    /// this variant.
    #[error("HTTP status {0}")]
    HttpStatus(u16),

    /// The user-supplied version string is not a valid semver
    /// (`MAJOR.MINOR.PATCH`). Examples: `1.0` (missing patch),
    /// `1.0.0-beta` (pre-release tags are accepted by `semver` but
    /// not by the GitHub Releases tagging scheme used here).
    #[error("invalid version: {0}")]
    Parse(#[from] semver::Error),

    /// JSON parse failure on the GitHub Releases API response. The
    /// schema is small ([`crate::github::Release`]) but if GitHub
    /// changes the response shape this is what surfaces.
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// Tarball extraction failure. Wraps the underlying `tar::Error`
    /// or `flate2::Error` — the message carries the original cause.
    #[error("tarball extract error: {0}")]
    Extract(String),

    /// Creating or replacing the active-version symlink/copy at
    /// `~/.buff/bin/buff` failed. Distinct from [`Self::Io`] so the
    /// CLI can surface actionable "rerun as admin / enable Developer
    /// Mode" guidance on Windows when the symlink syscall fails.
    #[error("symlink error: {0}")]
    Symlink(String),

    /// `dirs::home_dir()` returned `None` — the user's home
    /// directory cannot be resolved. Usually means `$HOME` is unset
    /// on Unix or the Windows user profile is corrupted.
    #[error("could not resolve user home directory (set $HOME / USERPROFILE or BUFFUP_HOME)")]
    HomeDir,

    /// The requested command is not yet implemented. Currently only
    /// emitted by `buffup update` (self-update). The CLI prints a
    /// guidance message to stderr BEFORE returning this variant so
    /// the user gets actionable output even though the operation
    /// fails.
    #[error("not implemented: {0}")]
    NotImplemented(&'static str),

    /// `buffup install <ver>` was called for a version that is
    /// already installed. Surfaces the version so the user knows
    /// what to `buffup default` or what to manually delete.
    #[error("version {0} is already installed")]
    VersionAlreadyInstalled(semver::Version),

    /// `buffup default <ver>` was called for a version that is NOT
    /// installed. Tells the user to `buffup install` first.
    #[error("version {0} is not installed; run `buffup install {0}` first")]
    VersionNotInstalled(semver::Version),

    /// The installed version directory exists but does not contain a
    /// `buff` (Unix) or `buff.exe` (Windows) binary. Usually means
    /// a partially-extracted install — the fix is to delete the
    /// version dir and re-run `buffup install`.
    #[error("no `buff` binary found inside {0}; the install may be corrupted")]
    BinaryMissing(String),

    /// `clap` argv parsing failure. Wrapping the original
    /// [`clap::error::Error`] lets the library entry propagate
    /// help/version requests without special-casing exit codes.
    #[error("{0}")]
    Clap(#[from] clap::error::Error),

    /// SHA-256 checksum mismatch between the downloaded tarball and
    /// the expected sidecar value. Indicates corruption or tampering.
    #[error(
        "Checksum mismatch!\n  Expected: {expected}\n  Actual:   {actual}\n\
         The downloaded file may be corrupted or tampered with.\n\
         Use --skip-checksum to bypass (NOT RECOMMENDED)."
    )]
    ChecksumMismatch { expected: String, actual: String },
}
