//! Error type for the `buff-xml` crate.
//!
//! All fallible operations surface as [`XmlError`]. The single
//! public [`crate::XmlDocument::from_str`] /
//! [`crate::XmlDocument::to_string`] entry points map the underlying
//! `quick_xml::Error` into this enum so the crate's public surface
//! depends only on `buff-xml`'s own types (Buff code never sees a
//! raw `quick_xml::*` type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-xml` operation.
#[derive(Debug, Error)]
pub enum XmlError {
    /// The XML source is malformed (unclosed tag, invalid syntax, etc.).
    /// Wraps the underlying `quick_xml::Error` message.
    #[error("XML parse error: {0}")]
    Parse(String),

    /// The XML source is empty (zero-length string).
    #[error("XML from_str called with empty string")]
    EmptyInput,

    /// The document has no root element (e.g. only whitespace or comments).
    #[error("XML document has no root element")]
    NoRootElement,

    /// An XPath query matched no elements.
    #[error("XPath query '{0}' matched no elements")]
    XPathNoMatch(String),

    /// Serialization failed (e.g. invalid characters in output).
    #[error("XML serialization error: {0}")]
    Serialize(String),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: XML operation panicked")]
    Panic,
}

impl From<quick_xml::Error> for XmlError {
    fn from(err: quick_xml::Error) -> Self {
        XmlError::Parse(err.to_string())
    }
}
