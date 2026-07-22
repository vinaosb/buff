//! Error type for the `buff-i18n` crate.
//!
//! All fallible operations surface as [`I18nError`]. The single
//! public entry points ([`crate::I18n::new`], [`crate::I18n::load`],
//! [`crate::I18n::add_resource`], [`crate::I18n::set_fallback`]) map
//! the underlying `fluent_bundle` / `unic_langid` errors into this
//! enum so the crate's public surface depends only on `buff-i18n`'s
//! own types (Buff code never sees a raw `fluent_bundle::*` /
//! `unic_langid::*` type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-i18n` operation.
#[derive(Debug, Error)]
pub enum I18nError {
    /// The user supplied a string that is not a valid Unicode Language
    /// Identifier (BCP 47). Example: `"en_US"` (underscore not allowed
    /// — BCP 47 mandates a hyphen: `"en-US"`). Carries the underlying
    /// `unic_langid` parser message verbatim so a future `BuffError`
    /// migration can wrap it.
    #[error("invalid locale identifier: {0}")]
    InvalidLocale(String),

    /// The user called [`crate::I18n::load`] or
    /// [`crate::I18n::set_fallback`] with a locale that has no
    /// resources loaded. The locale tag itself is valid; we just have
    /// nothing to translate it with. Suggests calling
    /// `i18n.add_resource(locale, ftl)` first.
    #[error("no resources loaded for locale: {0}")]
    LocaleNotLoaded(String),

    /// The Fluent `.ftl` source failed to parse. Carries the parser
    /// error message verbatim. Distinct from [`Self::LocaleNotLoaded`]
    /// (which fires before parsing begins) and from
    /// [`Self::Duplicate`] (which fires when add_resource succeeds at
    /// the parser level but rejects the resource for overriding an
    /// existing message id).
    #[error("fluent resource parse error: {0}")]
    ResourceParse(String),

    /// `add_resource` was called with messages whose identifiers
    /// collide with messages already loaded for that locale. The
    /// caller can either switch to a non-overlapping resource, or
    /// (internal-only) we can switch to `add_resource_overriding`.
    /// Surfaced as an error so users see the conflict rather than
    /// silently losing translations.
    #[error("fluent resource duplicates existing message ids: {0}")]
    Duplicate(String),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: i18n operation panicked")]
    Panic,
}
