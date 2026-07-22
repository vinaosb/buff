//! Error type for the `buff-archive` crate.
//!
//! All fallible operations surface as [`ArchiveError`]. The public
//! entry points ([`crate::Archive::compress_dir`] /
//! [`crate::Archive::extract`] /
//! [`crate::Archive::compress_bytes`] /
//! [`crate::Archive::decompress_bytes`]) map every underlying crate's
//! error into this enum so the public surface depends only on
//! `buff-archive`'s own types (Buff code never sees a raw `zip::*` /
//! `tar::*` / `flate2::*` / `ruzstd::*` error type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-archive` operation.
#[derive(Debug, Error)]
pub enum ArchiveError {
    /// Filesystem I/O failure (file not found, permission denied,
    /// disk full during write, etc.). Wraps the underlying
    /// [`std::io::Error`].
    #[error("archive I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The underlying `zip` crate failed to read or write a ZIP
    /// archive. Covers: corrupt central directory, unsupported
    /// compression method (e.g. a ZIP entry compressed with a codec
    /// we disabled, like zstd/bzip2/lzma), invalid UTF-8 in entry
    /// name, etc. The original message is carried verbatim so a
    /// future `BuffError` migration can wrap it.
    #[error("zip codec error: {0}")]
    Zip(String),

    /// The underlying `tar` crate failed to read or write a tarball.
    /// Covers: corrupt archive, entry too large, unsupported header
    /// format, etc.
    #[error("tar codec error: {0}")]
    Tar(String),

    /// The underlying `flate2` crate failed to gzip-compress or
    /// gzip-decompress a byte stream. Covers: corrupt gzip header,
    /// CRC mismatch, truncated stream, etc.
    #[error("gzip codec error: {0}")]
    Gzip(String),

    /// The underlying `ruzstd` crate failed to zstd-compress or
    /// zstd-decompress a byte stream. Covers: corrupt zstd frame
    /// header, unsupported frame format, decoder state machine
    /// failure, etc.
    #[error("zstd codec error: {0}")]
    Zstd(String),

    /// The user requested a single-stream byte-level operation
    /// ([`crate::Archive::compress_bytes`] /
    /// [`crate::Archive::decompress_bytes`]) on a multi-file archive
    /// format (`Zip` or `Tar`). Byte-level ops are only defined for
    /// single-stream codecs (`Gz` / `Zstd`); multi-file archives
    /// must go through the dir-level APIs.
    #[error("format {format:?} does not support byte-stream operations; use compress_dir / extract instead")]
    UnsupportedForByteStream { format: crate::Format },

    /// The user passed an empty byte slice to a single-stream
    /// operation, or pointed `compress_dir` at an empty/missing
    /// directory. Distinct from [`Self::Io`] (which fires for real
    /// I/O failures) so the diagnostic can be specific.
    #[error("archive operation called with empty input")]
    EmptyInput,

    /// The user pointed [`crate::Archive::extract`] at a path whose
    /// extension does not map to any of the four supported formats
    /// (`.zip` / `.tar` / `.gz` / `.zst`).
    #[error("cannot detect archive format from path {path}; expected .zip / .tar / .gz / .zst")]
    UnknownFormat { path: String },

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: archive operation panicked")]
    Panic,
}

impl From<zip::result::ZipError> for ArchiveError {
    fn from(err: zip::result::ZipError) -> Self {
        ArchiveError::Zip(err.to_string())
    }
}

impl From<std::str::Utf8Error> for ArchiveError {
    /// A non-UTF-8 entry name inside a ZIP/tar archive maps to
    /// [`ArchiveError::Zip`] / [`ArchiveError::Tar`] contextually;
    /// the wrapping crate surfaces the message via `Display`.
    fn from(err: std::str::Utf8Error) -> Self {
        ArchiveError::Zip(format!("non-utf8 entry name: {err}"))
    }
}
