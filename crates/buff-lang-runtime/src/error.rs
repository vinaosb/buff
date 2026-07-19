//! Runtime error types.
//!
//! [`RuntimeError`] is the fallible result of every operation in this crate.
//! It is bridged to the compiler's top-level
//! [`buff_lang_error::BuffError`] via a `From` impl so runtime failures flow
//! through the same diagnostic pipeline as lex/parse/type/codegen errors.
//!
//! # Naming
//!
//! This type is named `RuntimeError` and lives in `buff_lang_runtime`. It is
//! distinct from [`buff_lang_error::RuntimeError`], which is a thin
//! `Diagnostic` wrapper used by the compiler's top-level enum. Use the
//! `From` impl below to bridge between the two.

use buff_lang_error::{Diagnostic, RuntimeError as BuffRuntimeError, Span};

/// Error returned by any runtime operation.
///
/// Variants are intentionally coarse-grained for T38 — finer-grained
/// variants (e.g. `ShaderCompile`, `BufferMapping`) land with T44/T45.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RuntimeError {
    /// No GPU adapter is available on this host. The runtime must never
    /// panic on this — callers (T40 thresholds) fall back to CPU.
    #[error("gpu unavailable: no adapter found")]
    GpuUnavailable,

    /// The GPU stack returned an error during adapter/device init.
    /// `detail` carries the underlying message for diagnostics.
    #[error("gpu init failed: {detail}")]
    GpuInit {
        /// Lower-level detail string (typically from `wgpu`).
        detail: String,
    },

    /// A not-yet-implemented code path was reached. T38 scaffold returns
    /// this from any real parallel/GPU logic which lands in T39/T45.
    #[error("not implemented: {feature}")]
    NotImplemented {
        /// Short stable identifier of the missing feature, e.g. `"par_map"`.
        feature: String,
    },

    /// The runtime was asked to do work it cannot do on this host
    /// (e.g. GPU op on a host with no GPU, or a CPU pool build failure).
    #[error("unsupported: {detail}")]
    Unsupported {
        /// Lower-level detail string explaining what was unsupported.
        detail: String,
    },
}

impl RuntimeError {
    /// Stable, lowercase, no-space machine-readable tag for this variant.
    ///
    /// Useful for tests and for `@prefer(gpu)` fallback logging (T49).
    /// Do not change existing tags — they are part of the public test API.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::GpuUnavailable => "gpu_unavailable",
            Self::GpuInit { .. } => "gpu_init",
            Self::NotImplemented { .. } => "not_implemented",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}

impl From<RuntimeError> for BuffRuntimeError {
    /// Bridge a runtime-crate error into the compiler's top-level error
    /// enum. Runtime failures do not carry a meaningful source span — we
    /// attach [`Span::dummy()`] and the rendered error message.
    fn from(err: RuntimeError) -> Self {
        Self::new(Diagnostic::error(err.to_string(), Span::dummy()))
    }
}
