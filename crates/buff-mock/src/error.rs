//! Errors raised by the mock framework.
//!
//! Every public API that can fail returns [`Result<_, MockError>`]. The
//! error is [`thiserror::Error`]-derived and carries enough context for a
//! test-reporter to print a useful diagnostic without further introspection.
//!
//! # No panics
//!
//! Consistent with the project hard rule (`README.md`), no public API in
//! this crate panics. [`MockError`] is the single failure surface.

use std::sync::PoisonError;

/// An error returned by the mock framework.
///
/// # Variants
///
/// - [`VerifyFailed`](Self::VerifyFailed) — `Mock::verify()` detected an
///   unmet expectation (wrong call count, missing method, unexpected
///   arguments). Carries a human-readable message suitable for direct
///   inclusion in a test failure report.
/// - [`UnexpectedCall`](Self::UnexpectedCall) — a method was invoked on
///   the mock but no matching expectation existed (and no `returning`
///   default was registered). The mock could not produce a value.
/// - [`UnknownMethod`](Self::UnknownMethod) — the user referenced a
///   method name that does not exist on the mocked trait. Carries the
///   trait name + method name for diagnostic context.
/// - [`Poisoned`](Self::Poisoned) — the interior-mutable mock state
///   could not be locked because a previous holder panicked. Mocking
///   intentionally treats this as a recoverable error rather than a
///   panic; the test can either propagate it or assert via `unwrap()`
///   in test code (where `unwrap` is permitted by project rule).
#[derive(Debug, thiserror::Error)]
pub enum MockError {
    /// `Mock::verify()` detected one or more unmet expectations. The
    /// message enumerates every failed expectation (method name,
    /// expected count, observed count) so a test failure is
    /// self-explanatory.
    #[error("mock verification failed: {0}")]
    VerifyFailed(String),

    /// A trait method was called on the mock but no `expect().returning()`
    /// matched the call. Carries the method name and the call arguments
    /// (best-effort `Debug` rendering).
    #[error("unexpected call to `{method}` with args ({args}): no matching expectation")]
    UnexpectedCall { method: String, args: String },

    /// The user referenced a method that does not exist on the mocked
    /// trait. Carries the trait name + method name so the diagnostic
    /// reads `unknown method `bar` on trait `Foo``.
    #[error("unknown method `{method}` on trait `{trait_name}`")]
    UnknownMethod {
        method: String,
        trait_name: String,
    },

    /// The interior-mutable mock state was poisoned by a panicking
    /// lock-holder. Recovery is to propagate this as a test failure
    /// (the mock is no longer trustworthy).
    #[error("mock state poisoned (a previous lock-holder panicked)")]
    Poisoned,

    /// Codegen lowering failed. Returned by `lower_mock_for_trait` when
    /// the input [`TraitDecl`](buff_lang_ast::TraitDecl) has a shape the
    /// lowering pass cannot yet handle (e.g. generic supertraits).
    #[error("mock lowering failed for trait `{trait_name}`: {reason}")]
    LoweringFailed { trait_name: String, reason: String },
}

impl<T> From<PoisonError<T>> for MockError {
    /// Convert a [`PoisonError`] from `.lock()` into a [`MockError::Poisoned`].
    ///
    /// Lets mock-state consumers write `.lock()?` idiomatically without
    /// importing [`PoisonError`] at every call site.
    fn from(_: PoisonError<T>) -> Self {
        MockError::Poisoned
    }
}

/// Convenience alias used throughout the crate.
pub type MockResult<T> = Result<T, MockError>;

