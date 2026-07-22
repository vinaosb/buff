//! Error type for the `buff-email` crate.
//!
//! All fallible operations surface as [`EmailError`]. The public
//! constructors + mutators + sender map the underlying `lettre` /
//! `handlebars` errors into this enum so the crate's public surface
//! depends only on `buff-email`'s own types (Buff code never sees a
//! raw `lettre::*` or `handlebars::*` type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-email`
/// operation.
#[derive(Debug, Error)]
pub enum EmailError {
    /// The user supplied an invalid RFC 5322 mailbox address to
    /// [`crate::Email::new`]. The original (invalid) string + the
    /// lettre parser's error message are carried verbatim so a future
    /// `BuffError` migration can wrap them.
    #[error("invalid email address {addr:?}: {reason}")]
    InvalidAddress { addr: String, reason: String },

    /// The SMTP relay hostname supplied to [`crate::SmtpClient::new`]
    /// was rejected by lettre's relay builder. Typically an empty
    /// string or invalid DNS syntax. DOES NOT cover TCP / TLS / auth
    /// failures — those surface as [`Self::Smtp`] at the first send.
    #[error("invalid SMTP relay: {0}")]
    InvalidRelay(String),

    /// The handlebars template source supplied to
    /// [`crate::Email::html`] failed to compile. The lettre / Mailbox
    /// layer never sees this variant — it's purely a handlebars
    /// registration error (unbalanced `{{ }}`, unknown helper, etc.).
    #[error("email template parse error: {0}")]
    TemplateParse(String),

    /// The handlebars template compiled but failed to render against
    /// the given JSON context. Typically: invalid JSON, missing
    /// variable referenced by the template, type mismatch (calling
    /// `.len()` on a non-iterable, etc.).
    #[error("email template render error: {0}")]
    TemplateRender(String),

    /// The email could not be assembled into a valid MIME message at
    /// [`crate::SmtpClient::send`] time. Typically: no plain body AND
    /// no html body AND no attachments (an empty email), or a header
    /// construction failure (oversized subject, etc.).
    #[error("email build error: {0}")]
    Build(String),

    /// A file queued via [`crate::Email::attach`] could not be read
    /// at send time. The (filename, io-error-message) pair is carried
    /// so the diagnostic can name the offending attachment.
    #[error("could not read attachment {0:?}: {1}")]
    AttachmentIo(String, String),

    /// The SMTP server rejected the message at send time. Covers
    /// every `lettre::transport::smtp::Error` variant: refused
    /// connection, TLS handshake failure, auth failure, malformed
    /// RCPT / DATA, transient 4xx / permanent 5xx SMTP reply codes.
    /// The original lettre error message is carried verbatim.
    #[error("SMTP send error: {0}")]
    Smtp(String),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: email operation panicked")]
    Panic,
}
