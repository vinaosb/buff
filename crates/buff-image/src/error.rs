//! Error type for the `buff-image` crate.
//!
//! All fallible operations surface as [`ImageError`]. The single
//! public [`crate::Image::from_path`] / [`crate::Image::from_bytes`] /
//! [`crate::Image::save`] entry points map the underlying `image::ImageError`
//! into this enum so the crate's public surface depends only on
//! `buff-image`'s own types (Buff code never sees a raw `image::*` type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-image` operation.
#[derive(Debug, Error)]
pub enum ImageError {
    /// Filesystem I/O failure (file not found, permission denied,
    /// disk full during save, etc.). Wraps the underlying
    /// [`std::io::Error`].
    #[error("image I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The underlying `image` crate failed to decode or encode a
    /// buffer. Covers: corrupt file, truncated download, unknown
    /// format, unsupported bit depth, etc. The original message is
    /// carried verbatim so a future `BuffError` migration can wrap it.
    #[error("image codec error: {0}")]
    Codec(String),

    /// The user supplied a pixel coordinate outside the image bounds.
    /// The (x, y, width, height) tuple is included so the diagnostic
    /// can say "pixel (100, 50) out of bounds for 80x40 image".
    #[error("pixel ({x}, {y}) out of bounds for {width}x{height} image")]
    OutOfBounds {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },

    /// The user supplied an empty byte slice to [`crate::Image::from_bytes`].
    /// Distinct from [`Self::Codec`] (which fires for non-empty but
    /// unrecognised bytes) so the diagnostic can be specific.
    #[error("image from_bytes called with empty buffer")]
    EmptyBuffer,

    /// The user supplied zero or absurdly large dimensions to a
    /// constructor. Guards against accidental huge allocations.
    #[error("invalid dimensions: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: image operation panicked")]
    Panic,
}

impl From<image::ImageError> for ImageError {
    fn from(err: image::ImageError) -> Self {
        ImageError::Codec(err.to_string())
    }
}
