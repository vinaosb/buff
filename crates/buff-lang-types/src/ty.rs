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
    /// A 2-D matrix type: `Matrix<T>` (T24 — flat contiguous storage).
    ///
    /// Maps to the builtin `Matrix<T>` struct emitted by the Rust codegen:
    /// `struct Matrix<T> { data: Vec<T>, rows: usize, cols: usize }`. Storage
    /// is a **single flat `Vec<T>`** (row-major, `row * cols + col` indexing)
    /// so the buffer is contiguous and directly GPU-transferable (no
    /// `Vec<Vec<T>>` nesting). This is the canonical GPU-ready collection —
    /// a `Matrix<Float<32>>` of `rows * cols` elements can be uploaded to a
    /// WGSL storage buffer verbatim.
    ///
    /// The element type is boxed, mirroring [`Type::Vector`]. Element-type
    /// inference from `Matrix.new(rows, cols)` is deferred (the constructor
    /// carries no element evidence by itself); `let m: Matrix<Int> = ...`
    /// annotations and 2-D indexing `m[r, c]` both flow through this variant.
    Matrix(Box<Type>),
    /// An optional value: `Option<T>` (T99 — prelude `env()`).
    ///
    /// Maps to Rust's `Option<T>`. Used by `env("HOME")` which returns
    /// `Option<String>`.
    Option(Box<Type>),
    /// A hash-map type: `Map<K, V>` (T25 — keyed dictionary collection).
    ///
    /// Maps to Rust's `std::collections::HashMap<K, V>`. The key and value
    /// types are each boxed so the enum can carry any inner types. The map
    /// literal `{"k": v, ...}` (note: braces + colon-separated entries) lowers
    /// to `HashMap::from([("k", v), ...])`. Map method dispatch
    /// (`.get`/`.insert`/`.contains`/`.remove`/`.len`) is handled by the Rust
    /// codegen via the standard `HashMap` inherent methods (`.contains` maps
    /// to `contains_key`).
    ///
    /// Both type params are inferred from the first entry of a literal;
    /// literals with mixed key/value kinds fall back to the first entry's
    /// types (a future task will enforce uniformity).
    Map(Box<Type>, Box<Type>),
    /// A result type: `Result<T, E>` (T30 — prelude error-handling enum).
    ///
    /// Maps 1:1 to Rust's `std::result::Result<T, E>`. Mirrors [`Type::Option`]
    /// (T28): `Result` is a **built-in prelude enum** whose variants `Ok(T)`
    /// and `Err(E)` resolve WITHOUT a user declaration and WITHOUT being
    /// reserved keywords. The Ok type (first param) and Err type (second
    /// param) are each boxed, mirroring [`Type::Map`]'s two-param shape.
    ///
    /// `Ok(x)` infers `Result<T, Unknown>` (the Err type is pinned by context
    /// — e.g. a `let x: Result<Int, Error> = Ok(42)` annotation — or stays
    /// `Unknown`). `Err(e)` infers `Result<Unknown, E>` symmetrically. The
    /// `?` postfix operator (`Expr::Try`) propagates the Err and yields the
    /// Ok type `T`.
    ///
    /// This is **additive** (T30): no existing variant was renamed, reordered,
    /// or had its payload altered. All exhaustive `match`es on `Type` were
    /// extended with an arm for the new variant: `Display`, `buff_type_to_syn`
    /// (codegen), `typeref_to_type` (inferencer + exhaustiveness), and the
    /// prelude-seeded enum registry (`build_enum_registry_with_prelude`).
    Result(Box<Type>, Box<Type>),
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

    /// Create a `Matrix<T>` type (T24). The element type is the inner `T`;
    /// storage is a flat `Vec<T>` in the emitted Rust struct.
    pub fn matrix(elem: Type) -> Self {
        Type::Matrix(Box::new(elem))
    }

    /// Create an `Option<T>` type.
    pub fn option(inner: Type) -> Self {
        Type::Option(Box::new(inner))
    }

    /// Create a `Map<K, V>` type (T25). Maps to Rust's
    /// `std::collections::HashMap<K, V>`. Both params are boxed so the
    /// enum variant carries them inline without recursion through the
    /// enum's own padding.
    pub fn map(key: Type, value: Type) -> Self {
        Type::Map(Box::new(key), Box::new(value))
    }

    /// Create a `Result<T, E>` type (T30). Maps 1:1 to Rust's
    /// `std::result::Result<T, E>`. Mirrors [`Type::option`] (T28) for the
    /// error-handling prelude enum. Both params are boxed, mirroring
    /// [`Type::map`].
    pub fn result(ok: Type, err: Type) -> Self {
        Type::Result(Box::new(ok), Box::new(err))
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
            Type::Matrix(elem) => write!(f, "Matrix<{elem}>"),
            Type::Option(inner) => write!(f, "Option<{inner}>"),
            Type::Map(key, value) => write!(f, "Map<{key}, {value}>"),
            Type::Result(ok, err) => write!(f, "Result<{ok}, {err}>"),
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
