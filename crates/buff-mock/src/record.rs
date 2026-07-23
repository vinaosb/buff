//! Recorded call data: argument values, return values, call signatures.
//!
//! These types are the "wire format" between the mock runtime (which
//! records what was called) and the user's test (which inspects the
//! records). They are intentionally [`Clone`] + [`PartialEq`] so tests
//! can compare them with `assert_eq!`.
//!
//! # No `Debug`-only arguments
//!
//! Buff values crossing the FFI boundary into mock state are
//! represented by the closed [`ArgumentValue`] enum. This avoids the
//! `dyn Debug` trap (which would force every recorded argument to
//! implement `Debug` — too restrictive for some trait signatures)
//! while still allowing precise argument matching via [`PartialEq`].

use std::fmt;

/// A type-erased argument value captured at the call site.
///
/// Closed enum — adding a variant is a breaking change (semver-major).
/// The set covers every primitive the Buff type system maps to at the
/// Rust boundary; user-defined types are captured via [`Self::Other`]
/// (best-effort `Debug` string).
#[derive(Debug, Clone, PartialEq)]
pub enum ArgumentValue {
    /// Rust `i64`. Maps from Buff `Int`.
    Int(i64),
    /// Rust `f64`. Maps from Buff `Float` / `Double`.
    Float(f64),
    /// Rust `String`. Maps from Buff `String`.
    String(String),
    /// Rust `bool`. Maps from Buff `Bool`.
    Bool(bool),
    /// A captured value whose type the mock framework does not model
    /// directly. The `debug` string carries a best-effort `format!("{:?}")`
    /// rendering for assertion and diagnostic purposes — exact equality
    /// is unreliable across types.
    Other { type_name: String, debug: String },
}

impl ArgumentValue {}

impl fmt::Display for ArgumentValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArgumentValue::Int(i) => write!(f, "{i}"),
            ArgumentValue::Float(fl) => write!(f, "{fl}"),
            ArgumentValue::String(s) => write!(f, "{s:?}"),
            ArgumentValue::Bool(b) => write!(f, "{b}"),
            ArgumentValue::Other { type_name, debug } => {
                write!(f, "<{type_name}>{debug}")
            }
        }
    }
}

/// A type-erased return value programmed via
/// [`ExpectationBuilder::returning`](crate::ExpectationBuilder::returning).
///
/// Closed enum mirroring [`ArgumentValue`] — same primitive coverage.
/// The mock's codegen-generated trait impl matches the variant against
/// the trait method's declared return type and unwraps the inner value.
#[derive(Debug, Clone, PartialEq)]
pub enum ReturnValue {
    /// Rust `i64`. Returned for trait methods declared `-> Int`.
    Int(i64),
    /// Rust `f64`. Returned for trait methods declared `-> Float` / `Double`.
    Float(f64),
    /// Rust `String`. Returned for trait methods declared `-> String`.
    String(String),
    /// Rust `bool`. Returned for trait methods declared `-> Bool`.
    Bool(bool),
    /// Rust unit `()`. Returned for trait methods with no return type.
    Unit,
}

impl fmt::Display for ReturnValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReturnValue::Int(i) => write!(f, "{i}"),
            ReturnValue::Float(fl) => write!(f, "{fl}"),
            ReturnValue::String(s) => write!(f, "{s:?}"),
            ReturnValue::Bool(b) => write!(f, "{b}"),
            ReturnValue::Unit => f.write_str("()"),
        }
    }
}

/// A captured call: the method name + the arguments it was invoked with.
///
/// Stored in [`MockState`](crate::MockState) for every dispatch and
/// surfaced via [`Mock::calls`](crate::Mock::calls) /
/// [`SpyHandle::calls`](crate::SpyHandle::calls).
#[derive(Debug, Clone, PartialEq)]
pub struct CallRecord {
    /// The method name on the mocked trait (`"greet"`, `"compute"`, …).
    pub method: String,
    /// The captured arguments, in declaration order. Empty for
    /// zero-argument methods.
    pub args: Vec<ArgumentValue>,
}

impl CallRecord {
    /// Construct a record with the given method name and an empty arg list.
    /// Convenience for tests that only care about call counts.
    #[must_use]
    pub fn for_method(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            args: Vec::new(),
        }
    }
}

impl fmt::Display for CallRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", self.method)?;
        for (i, a) in self.args.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{a}")?;
        }
        f.write_str(")")
    }
}
