use thiserror::Error;

/// The single error type returned by every fallible `buff-template` operation.
#[derive(Debug, Error)]
pub enum TemplateError {
    /// The template source string could not be parsed by handlebars.
    #[error("template parse error: {0}")]
    Parse(String),

    /// The template rendered but the context was missing a referenced variable.
    #[error("template render error: {0}")]
    Render(String),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: template operation panicked")]
    Panic,
}
