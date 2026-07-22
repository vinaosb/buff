//! Error types for the `buff-validate` crate.
//!
//! Every fallible operation in this crate surfaces as
//! [`ValidationError`] (single-rule failure) or
//! [`ValidationErrors`] (multi-rule aggregate). The two-type split
//! mirrors pydantic's `ValidationError` collection semantics and the
//! upstream `validator` crate's own `ValidationError` /
//! `ValidationErrors` pair.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidationError {
    /// The input field failed an `email` rule.
    #[error("field `{field}` is not a valid email address: {value}")]
    InvalidEmail { field: String, value: String },

    /// The input field failed a `url` rule.
    #[error("field `{field}` is not a valid URL: {value}")]
    InvalidUrl { field: String, value: String },

    /// The input field failed a `length` rule.
    #[error(
        "field `{field}` length {actual} is outside the required range {min}..={max}"
    )]
    InvalidLength {
        field: String,
        min: u64,
        max: u64,
        actual: u64,
    },

    /// The input field failed a `range` rule.
    #[error(
        "field `{field}` value {actual} is outside the required range {min}..={max}"
    )]
    InvalidRange {
        field: String,
        min: i64,
        max: i64,
        actual: i64,
    },

    /// The input field failed a `regex` rule.
    #[error("field `{field}` value does not match pattern `{pattern}`: {value}")]
    InvalidRegex {
        field: String,
        pattern: String,
        value: String,
    },

    /// The user-supplied regex pattern failed to compile at
    /// rule-registration time. Surfaces immediately (NOT deferred
    /// until `validate`) so the developer sees the malformed pattern
    /// next to the `Validator.with_regex(...)` call site.
    #[error("malformed regex pattern `{pattern}`: {reason}")]
    BadRegex { pattern: String, reason: String },

    /// The input map was missing a field that the schema declares a
    /// rule on. Surfaced when the validator cannot even attempt the
    /// rule because the field is absent.
    #[error("field `{field}` is required but missing from input")]
    MissingField { field: String },

    /// A rule was configured with invalid arguments (e.g. `min > max`
    /// for a length or range rule). Surfaces at rule-registration
    /// time so the developer sees the misconfiguration next to the
    /// offending `with_*` call.
    #[error("invalid rule configuration for field `{field}`: {reason}")]
    InvalidRuleConfig { field: String, reason: String },

    /// The value present in the input map could not be coerced to
    /// the type a rule requires (e.g. a non-numeric string for a
    /// `range` rule).
    #[error("field `{field}` value `{value}` cannot be used for this rule: {reason}")]
    UncoercibleValue {
        field: String,
        value: String,
        reason: String,
    },

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: validation operation panicked")]
    Panic,
}

/// Aggregate of [`ValidationError`]s collected across all rules in a
/// single [`crate::Validator::validate`] call.
///
/// Implements `IntoIterator` so calling code can iterate the
/// individual rule failures. Implements `Display` as a multi-line
/// list (one error per line) so a single `eprintln!("{errs}")` gives
/// a readable summary.
#[derive(Debug, Default, Clone)]
pub struct ValidationErrors {
    errors: Vec<ValidationError>,
}

impl ValidationErrors {
    pub fn new() -> Self {
        ValidationErrors {
            errors: Vec::new(),
        }
    }

    pub fn push(&mut self, err: ValidationError) {
        self.errors.push(err);
    }

    pub fn len(&self) -> usize {
        self.errors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, ValidationError> {
        self.errors.iter()
    }
}

impl From<Vec<ValidationError>> for ValidationErrors {
    fn from(errors: Vec<ValidationError>) -> Self {
        ValidationErrors { errors }
    }
}

impl IntoIterator for ValidationErrors {
    type Item = ValidationError;
    type IntoIter = std::vec::IntoIter<ValidationError>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.into_iter()
    }
}

impl std::fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.errors.is_empty() {
            return write!(f, "(no validation errors)");
        }
        let count = self.errors.len();
        writeln!(f, "{count} validation error(s):")?;
        for (i, err) in self.errors.iter().enumerate() {
            writeln!(f, "  {}. {err}", i + 1)?;
        }
        Ok(())
    }
}

impl std::ops::Index<usize> for ValidationErrors {
    type Output = ValidationError;

    fn index(&self, i: usize) -> &Self::Output {
        &self.errors[i]
    }
}
