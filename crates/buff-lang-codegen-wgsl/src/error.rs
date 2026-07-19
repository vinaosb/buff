//! Error types for the WGSL codegen crate.
//!
//! All errors are recoverable: there is NO `unwrap`/`expect`/`panic!` in this
//! crate — every lowering failure returns [`Result<_, WgslError>`]. The
//! variants carry enough structured data for a caller to render a precise
//! diagnostic ("Buff `Double` (Float<64>) is not WGSL-native; use `Float` for
//! GPU dispatch").
//!
//! **Stability contract**: this enum is `#[non_exhaustive]`-ish in spirit — new
//! variants may be added in future tasks (T45+, when the runtime feeds real
//! Buff types through here) without breaking callers that match exhaustively.
//! Callers SHOULD prefer the displayed `Display` text for user-facing messages
//! and use the structured fields only for programmatic handling (e.g. routing
//! `UnsupportedType` to a CPU-fallback path).

use thiserror::Error;

/// The single error type emitted by [`crate::generate_wgsl`] and
/// [`crate::WgslCodegen::generate`].
///
/// Every variant is `Clone + PartialEq + Eq` so tests can assert exact error
/// shapes (`assert_eq!(result, Err(WgslError::UnsupportedType { ... }))`).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WgslError {
    /// A Buff type that has no WGSL-native representation was used in a GPU
    /// kernel position.
    ///
    /// **Primary case (RED spec for T44):** `Float<64>` (Buff `Double`,
    /// Rust `f64`) is REJECTED — WGSL has no f64 scalar. The runtime must
    /// fall back to the CPU (`CpuDispatcher::par_map`) for `Float<64>` data
    /// OR the user must re-write the kernel with `Float<32>` (f32).
    ///
    /// **Other rejected types** (per the plan's GPU Compute Type Policy):
    /// - `Int<64>` — WGSL has no i64 (auto-convert + overflow-check is T45's
    ///   runtime job; T44 rejects to keep the codegen conservative).
    /// - `Decimal` — CPU-only by policy.
    /// - BFloat16/FP8/FP4/NF4/Trit — DEFERRED to v2.0.
    ///
    /// The `ty` field is the human-readable type name (e.g. `"Float<64>"`,
    /// `"Double"`, `"Int<64>"`, `"Decimal"`).
    #[error("WGSL does not support type `{ty}` — use a WGSL-native scalar (Float<32>=f32, Float<16>=f16, Int<32>=i32, Bits<32>=u32) for GPU dispatch{hint}")]
    UnsupportedType {
        /// Human-readable type name (e.g. `"Float<64>"`, `"Double"`).
        ty: String,
        /// Optional extra hint (e.g. `" (f64 has no WGSL representation)"`).
        /// Empty string when no additional hint applies.
        hint: String,
    },

    /// An AST expression node we don't yet know how to lower to WGSL.
    ///
    /// The `detail` field is a short human-readable description of the
    /// unsupported construct (e.g. `"function calls are not supported in GPU
    /// map kernels"`, `"string literals are not WGSL-native"`).
    #[error("WGSL codegen cannot lower expression: {detail}")]
    UnsupportedExpr {
        /// Short human-readable description of the unsupported construct.
        detail: String,
    },

    /// The AST node passed in is not a single-parameter numeric map lambda
    /// `{ x => <expr> }` (which is the only shape T44 lowers — it is the
    /// kernel of a `.par_map(...)` call).
    ///
    /// `got` is a short description of the actual node kind (e.g.
    /// `"binary op"`, `"0 params"`, `"3 params"`).
    #[error("expected a single-parameter map lambda `{{ x => <expr> }}`, got {got}")]
    NotMapLambda {
        /// Short description of what was supplied instead.
        got: String,
    },

    /// The lambda body block has zero or more than one top-level expression
    /// statements. T44 only lowers a single-expression body. Multi-statement
    /// bodies (e.g. `{ x => let y = x + 1; y * 2 }`) are deferred to a later
    /// task — the runtime can still CPU-fallback them today.
    #[error("GPU map kernel body must be a single expression; got {count} statement(s){hint}")]
    InvalidLambdaBody {
        /// Number of top-level statements in the lambda body block.
        count: usize,
        /// Optional extra hint. Empty string when none applies.
        hint: String,
    },
}

impl WgslError {
    /// Construct an `UnsupportedType` error for a Buff `Double` / `Float<64>`
    /// value. This is the canonical RED-spec rejection for T44.
    #[must_use]
    pub fn f64_rejected() -> Self {
        Self::UnsupportedType {
            ty: "Float<64> (Double)".to_string(),
            hint: " (f64 has no WGSL representation)".to_string(),
        }
    }
}
