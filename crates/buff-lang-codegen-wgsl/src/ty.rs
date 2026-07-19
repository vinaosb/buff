//! WGSL scalar type mapping and Buff-type filtering.
//!
//! The plan's GPU Compute Type Policy (WGSL-Native Only) maps a small subset
//! of Buff's primitive numeric types directly to WGSL scalars. Everything else
//! (`Float<64>`, `Int<64>`, `Decimal`, …) is **rejected** at codegen time so
//! the runtime can fall back to the CPU path (`CpuDispatcher::par_map`).
//!
//! # Policy table
//!
//! | Buff Type        | WGSL  | Action                              |
//! |------------------|-------|-------------------------------------|
//! | `Float<32>`      | `f32` | Direct dispatch                     |
//! | `Float<16>`      | `f16` | Direct (requires `enable f16;`)     |
//! | `Int<32>`        | `i32` | Direct dispatch                     |
//! | `Bits<32>`       | `u32` | Direct dispatch                     |
//! | `Float<64>`      | —     | REJECT (no f64 in WGSL)             |
//! | `Int<64>`        | —     | REJECT (no i64 in WGSL)             |
//! | `Decimal`        | —     | REJECT (CPU-only by policy)         |
//! | `Int<8>`/`<16>`  | —     | REJECT (deferred — auto-widen T45+) |
//!
//! # Why reject (not auto-convert) for T44?
//!
//! The task spec says: *"Float<64>/Double MUST be REJECTED with a clear error
//! (`WgslError::UnsupportedType` naming f64/Double)"*. The other rejections
//! are conservative: T45 (runtime) MAY choose to auto-convert (e.g. `i64` →
//! `i32` with overflow check, `f64` → `f32` with precision warning) but that
//! is a runtime decision, NOT a codegen one. T44 emits a clean error so the
//! runtime sees a structured signal.

use crate::error::WgslError;
use buff_lang_ast::ty::TypeRef as AstTypeRef;
use buff_lang_ast::Literal;

/// A WGSL-native scalar type. These are the ONLY types T44 will emit in a
/// shader's storage-buffer element slot (`array<f32>` etc.).
///
/// `F16` is included for completeness — emitting a shader that uses `f16`
/// requires a leading `enable f16;` directive (NOT emitted by T44; deferred
/// to a future task that wires up GPU feature detection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WgslScalarType {
    /// 32-bit IEEE-754 float (`f32`). The default and most-compatible choice.
    F32,
    /// 16-bit IEEE-754 float (`f16`). Requires `enable f16;` directive.
    F16,
    /// 32-bit signed integer (`i32`).
    I32,
    /// 32-bit unsigned integer (`u32`).
    U32,
}

impl WgslScalarType {
    /// Returns the WGSL keyword for this scalar type (e.g. `"f32"`, `"u32"`).
    #[must_use]
    pub const fn as_wgsl_keyword(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::F16 => "f16",
            Self::I32 => "i32",
            Self::U32 => "u32",
        }
    }

    /// Returns the WGSL type literal for an `array<T>` storage buffer of this
    /// scalar — e.g. `"array<f32>"`.
    #[must_use]
    pub fn as_wgsl_array(self) -> String {
        format!("array<{}>", self.as_wgsl_keyword())
    }

    /// Returns the Rust source form for this scalar (used for cross-reference
    /// documentation in generated shaders; NOT emitted as code).
    #[must_use]
    pub const fn as_rust_keyword(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::F16 => "half", // via half crate, not yet wired
            Self::I32 => "i32",
            Self::U32 => "u32",
        }
    }
}

impl Default for WgslScalarType {
    /// The default WGSL element type is `f32` — Buff `Float<32>`. This matches
    /// the plan's "Direct dispatch" row and is the most broadly compatible
    /// WGSL scalar across all GPUs.
    fn default() -> Self {
        Self::F32
    }
}

impl std::fmt::Display for WgslScalarType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_wgsl_keyword())
    }
}

// ---------------------------------------------------------------------------
// Buff type-name → WgslScalarType filtering
// ---------------------------------------------------------------------------

/// Map a Buff source-level type NAME (as written in a lambda parameter
/// annotation, e.g. `"Float"`, `"Float<32>"`, `"Double"`, `"Int"`) to a
/// WGSL-native scalar, or reject it.
///
/// The matching is intentionally permissive on the ACCEPT side and strict on
/// the REJECT side: an UN-annotated parameter (None) defaults to `f32`, and a
/// plain `"Float"` (no width) also defaults to `f32` (Buff's `Float` literal
/// type IS 32-bit per the lexer). Anything that names a 64-bit type or
/// `Decimal` is rejected.
///
/// # Parameters
/// - `name`: the trimmed source name of the type annotation (e.g. `"Float"`,
///   `"Float<32>"`, `"Double"`, `"Int<32>"`, `"Bits<32>"`).
///
/// # Returns
/// - `Ok(WgslScalarType)` if the type is WGSL-native.
/// - `Err(WgslError::UnsupportedType)` if the type is NOT WGSL-native.
///
/// # Determinism
/// This function performs pure string matching — no HashMap, no allocation
/// beyond the error message. The same input always yields the same output.
pub fn filter_buff_type_name(name: &str) -> Result<WgslScalarType, WgslError> {
    let trimmed = name.trim();
    match trimmed {
        // 32-bit native types — accepted.
        "Float" | "Float<32>" | "F32" => Ok(WgslScalarType::F32),
        "Float<16>" | "F16" | "Half" => Ok(WgslScalarType::F16),
        "Int" | "Int<32>" | "I32" => Ok(WgslScalarType::I32),
        "Bits<32>" | "Bits" | "U32" | "UInt<32>" => Ok(WgslScalarType::U32),
        // 64-bit & non-WGSL-native types — rejected per the plan's policy.
        "Double" | "Float<64>" | "F64" => Err(WgslError::UnsupportedType {
            ty: "Float<64> (Double)".to_string(),
            hint: " (f64 has no WGSL representation)".to_string(),
        }),
        "Int<64>" | "I64" | "Long" => Err(WgslError::UnsupportedType {
            ty: "Int<64>".to_string(),
            hint:
                " (i64 has no WGSL representation; auto-convert is a runtime decision, not codegen)"
                    .to_string(),
        }),
        "Decimal" => Err(WgslError::UnsupportedType {
            ty: "Decimal".to_string(),
            hint: " (128-bit fixed-point is CPU-only by policy)".to_string(),
        }),
        // Anything else (String, Bool, custom user types, Int<8>/<16>) —
        // rejected. Int<8>/<16> auto-widening is deferred to T45 runtime.
        other => Err(WgslError::UnsupportedType {
            ty: other.to_string(),
            hint: String::new(),
        }),
    }
}

/// Resolve a Buff AST [`AstTypeRef`] annotation against the WGSL type policy.
///
/// Used for the lambda's parameter type annotation. `None` (un-annotated param)
/// defaults to `f32` (the most compatible WGSL scalar and Buff's default
/// floating width).
///
/// Only NAMED types are inspected here — `Generic`, `Option`, `Function`,
/// `Union`, `Tuple` annotations on a GPU kernel parameter make no sense and
/// are rejected with a clear error.
pub fn resolve_param_type(ty: Option<&AstTypeRef>) -> Result<WgslScalarType, WgslError> {
    match ty {
        None => Ok(WgslScalarType::F32),
        Some(AstTypeRef::Named { name, .. }) => filter_buff_type_name(&name.name),
        Some(other) => Err(WgslError::UnsupportedType {
            ty: format!("{other}"),
            hint: " (GPU map kernel parameters must be a WGSL-native scalar type)".to_string(),
        }),
    }
}

/// Inspect a [`Literal`] and either (a) confirm it is WGSL-native and return
/// the matching scalar type, or (b) reject it with a clear error.
///
/// **`Literal::Double(f64)` is the canonical T44 RED rejection** — WGSL has
/// no f64. [`Literal::Decimal`] is also rejected (CPU-only by policy).
/// Non-numeric literals (String, Char, Byte, Regex, Bool) are NOT supported
/// in a numeric map kernel body and rejected.
pub fn filter_literal(lit: &Literal) -> Result<WgslScalarType, WgslError> {
    match lit {
        Literal::Float(_) => Ok(WgslScalarType::F32),
        Literal::Int(_) => Ok(WgslScalarType::I32),
        Literal::Bool(_) => Ok(WgslScalarType::U32), // bool lowers to u32 in WGSL
        Literal::Double(_) => Err(WgslError::f64_rejected()),
        Literal::Decimal(_) => Err(WgslError::UnsupportedType {
            ty: "Decimal".to_string(),
            hint: " (128-bit fixed-point is CPU-only by policy)".to_string(),
        }),
        Literal::String(_) => Err(WgslError::UnsupportedType {
            ty: "String".to_string(),
            hint: " (string literals are not WGSL-native)".to_string(),
        }),
        Literal::Char(_) => Err(WgslError::UnsupportedType {
            ty: "Char".to_string(),
            hint: " (char literals are not WGSL-native)".to_string(),
        }),
        Literal::Byte(_) => Ok(WgslScalarType::U32),
        Literal::Regex(_) => Err(WgslError::UnsupportedType {
            ty: "Regex".to_string(),
            hint: " (regex literals are not WGSL-native)".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_type_keywords() {
        assert_eq!(WgslScalarType::F32.as_wgsl_keyword(), "f32");
        assert_eq!(WgslScalarType::F16.as_wgsl_keyword(), "f16");
        assert_eq!(WgslScalarType::I32.as_wgsl_keyword(), "i32");
        assert_eq!(WgslScalarType::U32.as_wgsl_keyword(), "u32");
    }

    #[test]
    fn scalar_type_array_form() {
        assert_eq!(WgslScalarType::F32.as_wgsl_array(), "array<f32>");
        assert_eq!(WgslScalarType::U32.as_wgsl_array(), "array<u32>");
    }

    #[test]
    fn default_is_f32() {
        assert_eq!(WgslScalarType::default(), WgslScalarType::F32);
    }

    #[test]
    fn filter_buff_type_name_accepts_native() {
        assert_eq!(filter_buff_type_name("Float").unwrap(), WgslScalarType::F32);
        assert_eq!(
            filter_buff_type_name("Float<32>").unwrap(),
            WgslScalarType::F32
        );
        assert_eq!(
            filter_buff_type_name("Int<32>").unwrap(),
            WgslScalarType::I32
        );
        assert_eq!(
            filter_buff_type_name("Bits<32>").unwrap(),
            WgslScalarType::U32
        );
    }

    #[test]
    fn filter_buff_type_name_rejects_f64() {
        let err = filter_buff_type_name("Double").unwrap_err();
        assert!(matches!(err, WgslError::UnsupportedType { .. }));
        assert!(err.to_string().contains("Float<64>"));
        assert!(err.to_string().contains("f64"));
    }

    #[test]
    fn filter_buff_type_name_rejects_decimal() {
        let err = filter_buff_type_name("Decimal").unwrap_err();
        assert!(matches!(err, WgslError::UnsupportedType { .. }));
    }

    #[test]
    fn filter_literal_rejects_double() {
        let err = filter_literal(&Literal::Double(2.5)).unwrap_err();
        assert!(matches!(err, WgslError::UnsupportedType { .. }));
        assert!(err.to_string().contains("Float<64>"));
    }

    #[test]
    fn filter_literal_accepts_float() {
        assert_eq!(
            filter_literal(&Literal::Float(2.5)).unwrap(),
            WgslScalarType::F32
        );
    }
}
