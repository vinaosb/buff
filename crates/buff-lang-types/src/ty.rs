//! The compile-time type representation for the Buff language.
//!
//! [`Type`] is the *resolved* type of an expression — produced by the
//! [`TypeInferencer`](crate::TypeInferencer) — and is distinct from
//! [`TypeRef`](buff_lang_ast::TypeRef), which is a *reference* to a type written
//! in source annotations.
//!
//! v0.1 supports **only** primitive types. v0.5 will add collections and
//! user-defined types.

use std::fmt;

/// The compile-time type of a Buff expression.
///
/// v0.1 supports ONLY primitive types. v0.5 adds collections/user types.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// A signed integer, `Int<W>` (defaults to `Int<64>`).
    Int { width: IntWidth },
    /// An unsigned integer (`Bits<W>`, defaults to `Bits<8>`).
    Bits { width: IntWidth },
    /// A floating-point type, `Float<W>` (defaults to `Float<32>`).
    Float { width: FloatWidth },
    /// A 64-bit float (`Double`, i.e. `Float<64>`).
    Double,
    /// A boolean (`Bool`).
    Bool,
    /// A UTF-8 string (`String`).
    String,
    /// A single Unicode scalar value (`Char`). (T21 — additive.)
    ///
    /// Maps to Rust's `char` type (a 4-byte Unicode scalar value). Distinct
    /// from `String` (a UTF-8 byte buffer): `'A'` is `Char`, `"A"` is
    /// `String`. Not GPU-eligible (no WGSL scalar) — always CPU.
    Char,
    /// A 128-bit fixed-point decimal (`Decimal`). The type exists in v0.1 but
    /// full arithmetic support arrives in v0.5.
    Decimal,
    /// Unknown / a placeholder emitted after a type error to suppress
    /// cascading diagnostics.
    Unknown,
    /// The absence of a value (for functions without a return, or `if`
    /// expressions without an `else` branch).
    Void,
    /// A generic vector/array type: `Vector<T>` (T99 — prelude `args()`).
    ///
    /// Maps to Rust's `Vec<T>`. The element type is boxed so the enum
    /// variant can carry any inner type. Full collection support (indexing,
    /// iteration, methods) arrives in T23.
    Vector(Box<Type>),
    /// An optional value: `Option<T>` (T99 — prelude `env()`).
    ///
    /// Maps to Rust's `Option<T>`. Used by `env("HOME")` which returns
    /// `Option<String>`.
    Option(Box<Type>),
}

/// The width of an integer type (`Int` or `Bits`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntWidth {
    W8,
    W16,
    W32,
    W64,
    W128,
}

/// The width of a floating-point type (`Float`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatWidth {
    W16,
    W32,
    W64,
}

impl Type {
    /// The default integer type: `Int<64>`.
    pub fn int_default() -> Self {
        Type::Int {
            width: IntWidth::W64,
        }
    }

    /// The default float type: `Float<32>`.
    pub fn float_default() -> Self {
        Type::Float {
            width: FloatWidth::W32,
        }
    }

    /// The 64-bit float type: `Double`.
    pub fn double() -> Self {
        Type::Double
    }

    /// The byte type: `Bits<8>`.
    pub fn byte() -> Self {
        Type::Bits {
            width: IntWidth::W8,
        }
    }

    /// The boolean type: `Bool`.
    pub fn bool() -> Self {
        Type::Bool
    }

    /// The string type: `String`.
    pub fn string() -> Self {
        Type::String
    }

    /// The char type: `Char` (a single Unicode scalar value). (T21.)
    pub fn char() -> Self {
        Type::Char
    }

    /// Returns `true` if this type is numeric (integer, byte, float, double, or decimal).
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Type::Int { .. }
                | Type::Bits { .. }
                | Type::Float { .. }
                | Type::Double
                | Type::Decimal
        )
    }

    /// Returns `true` if this type is floating-point-like
    /// (`Float`, `Double`, or `Decimal`).
    pub fn is_float_like(&self) -> bool {
        matches!(self, Type::Float { .. } | Type::Double | Type::Decimal)
    }

    /// Returns `true` if this type is integer-like (`Int` or `Bits`).
    pub fn is_integer_like(&self) -> bool {
        matches!(self, Type::Int { .. } | Type::Bits { .. })
    }

    /// Returns `true` if this type is eligible for GPU (WGSL) dispatch.
    ///
    /// Only the WGSL-native 32-bit scalar primitives are eligible:
    /// `Float<32>`, `Int<32>`, `Bits<32>`, and `Bool`. Wider widths,
    /// `Double` (f64 has no WGSL scalar), and especially [`Type::Decimal`]
    /// (128-bit fixed-point, no GPU representation) are **not** GPU-eligible
    /// and must run on the CPU (Rayon) path.
    ///
    /// This is **type metadata only** in v0.5 — there is no dispatch engine
    /// yet (that arrives in v1.0). The predicate is consumed directly by
    /// tests now and will feed the v1.0 heterogeneous dispatch analyzer.
    pub fn is_gpu_eligible(&self) -> bool {
        matches!(
            self,
            Type::Float {
                width: FloatWidth::W32
            } | Type::Int {
                width: IntWidth::W32
            } | Type::Bits {
                width: IntWidth::W32
            } | Type::Bool
        )
    }

    /// Create a `Vector<T>` type.
    pub fn vector(elem: Type) -> Self {
        Type::Vector(Box::new(elem))
    }

    /// Create an `Option<T>` type.
    pub fn option(inner: Type) -> Self {
        Type::Option(Box::new(inner))
    }

    /// Returns `true` if this type **must** run on the CPU (never GPU).
    ///
    /// [`Type::Decimal`] is the canonical case: 128-bit fixed-point decimals
    /// have no WGSL representation, so any expression involving a Decimal is
    /// forced onto the CPU/Rayon path. This is the complement of
    /// [`is_gpu_eligible`](Self::is_gpu_eligible) for the Decimal case, but
    /// also flags `Double` (no f64 in WGSL) and non-32-bit widths.
    pub fn must_run_on_cpu(&self) -> bool {
        !self.is_gpu_eligible()
    }
}

impl IntWidth {
    /// Returns the bit-width of this integer width as a `u8`.
    pub fn bits(&self) -> u8 {
        match self {
            IntWidth::W8 => 8,
            IntWidth::W16 => 16,
            IntWidth::W32 => 32,
            IntWidth::W64 => 64,
            IntWidth::W128 => 128,
        }
    }
}

impl FloatWidth {
    /// Returns the bit-width of this float width as a `u8`.
    pub fn bits(&self) -> u8 {
        match self {
            FloatWidth::W16 => 16,
            FloatWidth::W32 => 32,
            FloatWidth::W64 => 64,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int { width } => write!(f, "Int<{}>", width.bits()),
            Type::Bits { width } => write!(f, "Bits<{}>", width.bits()),
            Type::Float { width } => write!(f, "Float<{}>", width.bits()),
            Type::Double => f.write_str("Double"),
            Type::Bool => f.write_str("Bool"),
            Type::String => f.write_str("String"),
            Type::Char => f.write_str("Char"),
            Type::Decimal => f.write_str("Decimal"),
            Type::Unknown => f.write_str("Unknown"),
            Type::Void => f.write_str("Void"),
            Type::Vector(elem) => write!(f, "Vector<{elem}>"),
            Type::Option(inner) => write!(f, "Option<{inner}>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_display_variants() {
        assert_eq!(Type::int_default().to_string(), "Int<64>");
        assert_eq!(Type::byte().to_string(), "Bits<8>");
        assert_eq!(Type::float_default().to_string(), "Float<32>");
        assert_eq!(Type::double().to_string(), "Double");
        assert_eq!(Type::bool().to_string(), "Bool");
        assert_eq!(Type::string().to_string(), "String");
        assert_eq!(Type::char().to_string(), "Char");
        assert_eq!(Type::Decimal.to_string(), "Decimal");
        assert_eq!(Type::Unknown.to_string(), "Unknown");
        assert_eq!(Type::Void.to_string(), "Void");
    }

    #[test]
    fn numeric_classification() {
        assert!(Type::int_default().is_numeric());
        assert!(Type::byte().is_numeric());
        assert!(Type::float_default().is_numeric());
        assert!(Type::double().is_numeric());
        assert!(Type::Decimal.is_numeric());
        assert!(!Type::bool().is_numeric());
        assert!(!Type::string().is_numeric());

        assert!(Type::float_default().is_float_like());
        assert!(Type::double().is_float_like());
        assert!(!Type::int_default().is_float_like());

        assert!(Type::int_default().is_integer_like());
        assert!(Type::byte().is_integer_like());
        assert!(!Type::float_default().is_integer_like());
    }

    // T20: GPU/CPU dispatch type-metadata predicates.
    #[test]
    fn gpu_cpu_dispatch_metadata() {
        // WGSL-native 32-bit scalars are GPU-eligible.
        assert!(Type::float_default().is_gpu_eligible()); // Float<32>
        assert!(Type::Bool.is_gpu_eligible());
        assert!(Type::Int {
            width: IntWidth::W32
        }
        .is_gpu_eligible());
        assert!(Type::Bits {
            width: IntWidth::W32
        }
        .is_gpu_eligible());

        // Decimal is NEVER GPU-eligible — it must run on CPU (Rayon).
        assert!(!Type::Decimal.is_gpu_eligible());
        assert!(Type::Decimal.must_run_on_cpu());

        // Double (f64) and wide integers are also CPU-only (no WGSL scalar).
        assert!(!Type::Double.is_gpu_eligible());
        assert!(Type::Double.must_run_on_cpu());
        assert!(!Type::int_default().is_gpu_eligible()); // Int<64>
        assert!(!Type::byte().is_gpu_eligible()); // Bits<8>

        // Predicate complementarity for Decimal.
        assert_ne!(
            Type::Decimal.is_gpu_eligible(),
            Type::Decimal.must_run_on_cpu()
        );
    }
}
