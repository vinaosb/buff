//! Crate-local error type for `buff-pipeline`.
//!
//! Mirrors the workspace-wide error pattern: `thiserror::Error` derive,
//! String-typed `detail` field per variant (no raw upstream error leak —
//! AGENTS.md "map to buff_lang_error::*Error variants"), and a crate-
//! local `PipelineResult<T>` alias.
//!
//! # FFI safety
//!
//! `PipelineError` is `Send + Sync` (all fields are owned `String`).
//! No raw pointers, no lifetimes exposed.

use thiserror::Error;

/// Fallible result for every public `buff-pipeline` operation.
pub type PipelineResult<T> = Result<T, PipelineError>;

/// Error raised by `buff-pipeline` operations.
///
/// Every variant carries a human-readable `detail` String. The detail
/// is the upstream error's `Display` output — callers can pattern-match
/// on the variant for coarse dispatch and read `detail` for diagnostics.
///
/// # Stability
///
/// Variants are append-only. Once shipped they are NEVER renumbered,
/// reused, or silently removed (matches the ErrorCode stability rule
/// from AGENTS.md §19, applied to crate-local error enums too).
#[derive(Debug, Error)]
pub enum PipelineError {
    /// File-system I/O failure (open / read / write / flush).
    ///
    /// Raised by `Source::from_csv` (file not found, permission denied)
    /// and `Sink::to_csv` / `Sink::to_json` (create file failed, write
    /// failed). The `detail` carries the underlying `std::io::Error`
    /// `Display` output.
    #[error("pipeline I/O error: {detail}")]
    Io {
        /// Human-readable upstream `io::Error` string.
        detail: String,
    },

    /// CSV (de)serialization failure.
    ///
    /// Raised by `Source::from_csv` (malformed record, reader build
    /// failure) and `Sink::to_csv` (writer flush / record serialization
    /// failure). The `detail` carries the underlying `csv::Error`
    /// `Display` output.
    #[error("pipeline CSV error: {detail}")]
    Csv {
        /// Human-readable upstream `csv::Error` string.
        detail: String,
    },

    /// JSON (de)serialization failure.
    ///
    /// Raised by `Sink::to_json` when `serde_json::to_writer_pretty`
    /// fails (usually a non-serializable inner type — unlikely for the
    /// MVP since most `Vec<T: Serialize>` inputs are plain). The
    /// `detail` carries the underlying `serde_json::Error` `Display`
    /// output.
    #[error("pipeline JSON error: {detail}")]
    Json {
        /// Human-readable upstream `serde_json::Error` string.
        detail: String,
    },

    /// Tokio runtime construction failure.
    ///
    /// Raised by `Pipeline::run` when `tokio::runtime::Builder::build`
    /// fails. Extremely unlikely on a healthy host (the multi-thread
    /// runtime builds in virtually every environment); usually
    /// indicates the process is so starved of resources that even one
    /// worker thread can't spawn. The `detail` carries the underlying
    /// `std::io::Error` from tokio's thread-spawn path.
    #[error("pipeline tokio runtime error: {detail}")]
    Runtime {
        /// Human-readable tokio runtime-build error string.
        detail: String,
    },

    /// Invalid pipeline configuration.
    ///
    /// Reserved for future validation failures (zero-capacity channels,
    /// negative worker counts, missing source). The current MVP clamps
    /// most degenerate inputs to safe defaults (e.g. `workers.max(1)`,
    /// `chunk_size.max(1)`) so this variant is unused today; it exists
    /// as a forward-compatible extension point so future strict
    /// validation can return `Err` without adding a new variant.
    #[error("invalid pipeline configuration: {detail}")]
    Config {
        /// Human-readable description of the configuration violation.
        detail: String,
    },
}

// ---------------------------------------------------------------------------
// Convenience From impls — let `?` propagate upstream errors into
// PipelineError without manual `.map_err(|e| PipelineError::X { detail:
// e.to_string() })` boilerplate at every call site. Mirrors the pattern
// used in buff-cache / buff-dataframe / buff-image.
// ---------------------------------------------------------------------------

impl From<std::io::Error> for PipelineError {
    fn from(err: std::io::Error) -> Self {
        Self::Io {
            detail: err.to_string(),
        }
    }
}

impl From<csv::Error> for PipelineError {
    fn from(err: csv::Error) -> Self {
        Self::Csv {
            detail: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for PipelineError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json {
            detail: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_propagates_via_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let pipe_err: PipelineError = io_err.into();
        assert!(matches!(pipe_err, PipelineError::Io { .. }));
        assert!(pipe_err.to_string().contains("missing"));
    }

    #[test]
    fn display_includes_detail() {
        let err = PipelineError::Config {
            detail: "workers must be > 0".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "invalid pipeline configuration: workers must be > 0"
        );
    }
}
