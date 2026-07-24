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
///
/// # Span preservation (T50)
///
/// Every variant carries an optional [`Span`] that, when set, maps the
/// error back to the originating `.buff` source location. The
/// [`From<RuntimeError> for BuffRuntimeError`] bridge consults this span
/// when bridging into the compiler's diagnostic pipeline — falling back
/// to [`Span::dummy()`] when `None`. Codegen sites that know the Buff
/// span at compile time SHOULD set this field so runtime errors surface
/// with meaningful source locations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RuntimeError {
    /// No GPU adapter is available on this host. The runtime must never
    /// panic on this — callers (T40 thresholds) fall back to CPU.
    #[error("gpu unavailable: no adapter found")]
    GpuUnavailable {
        /// Optional Buff source span for error mapping (T50).
        span: Option<Span>,
    },

    /// The GPU stack returned an error during adapter/device init.
    /// `detail` carries the underlying message for diagnostics.
    #[error("gpu init failed: {detail}")]
    GpuInit {
        /// Lower-level detail string (typically from `wgpu`).
        detail: String,
        /// Optional Buff source span for error mapping (T50).
        span: Option<Span>,
    },

    /// A not-yet-implemented code path was reached. T38 scaffold returns
    /// this from any real parallel/GPU logic which lands in T39/T45.
    #[error("not implemented: {feature}")]
    NotImplemented {
        /// Short stable identifier of the missing feature, e.g. `"par_map"`.
        feature: String,
        /// Optional Buff source span for error mapping (T50).
        span: Option<Span>,
    },

    /// The runtime was asked to do work it cannot do on this host
    /// (e.g. GPU op on a host with no GPU, or a CPU pool build failure).
    #[error("unsupported: {detail}")]
    Unsupported {
        /// Lower-level detail string explaining what was unsupported.
        detail: String,
        /// Optional Buff source span for error mapping (T50).
        span: Option<Span>,
    },
}

impl RuntimeError {
    /// Stable, lowercase, no-space machine-readable tag for this variant.
    ///
    /// Useful for tests and for `@prefer(gpu)` fallback logging (T49).
    /// Do not change existing tags — they are part of the public test API.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::GpuUnavailable { .. } => "gpu_unavailable",
            Self::GpuInit { .. } => "gpu_init",
            Self::NotImplemented { .. } => "not_implemented",
            Self::Unsupported { .. } => "unsupported",
        }
    }

    /// Attach a Buff source [`Span`] to this error for source-location
    /// mapping (T50). Returns `self` with the span set, enabling a
    /// builder-style pattern:
    ///
    /// ```ignore
    /// RuntimeError::GpuInit { detail: "…".into(), span: None }
    ///     .with_span(some_buff_span)
    /// ```
    #[must_use]
    pub fn with_span(mut self, span: Span) -> Self {
        match &mut self {
            Self::GpuUnavailable { span: s }
            | Self::GpuInit { span: s, .. }
            | Self::NotImplemented { span: s, .. }
            | Self::Unsupported { span: s, .. } => {
                *s = Some(span);
            }
        }
        self
    }

    /// Extract the optional Buff source [`Span`] from this error, if any.
    #[must_use]
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::GpuUnavailable { span }
            | Self::GpuInit { span, .. }
            | Self::NotImplemented { span, .. }
            | Self::Unsupported { span, .. } => *span,
        }
    }
}

impl From<RuntimeError> for BuffRuntimeError {
    /// Bridge a runtime-crate error into the compiler's top-level error
    /// enum. Uses the error's optional [`Span`] when set (T50); falls
    /// back to [`Span::dummy()`] when `None`.
    fn from(err: RuntimeError) -> Self {
        let span = err.span().unwrap_or_else(Span::dummy);
        Self::new(Diagnostic::error(err.to_string(), span))
    }
}
