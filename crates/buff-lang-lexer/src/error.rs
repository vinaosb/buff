//! Lexer error types.
//!
//! Wraps [`buff_lang_error::LexError`] with convenience constructors for common
//! lexer failure modes.

use buff_lang_error::{Diagnostic, LexError as BuffLexError, Severity, Span};

/// A lexer error. Wraps the `buff-lang-error` [`LexError`](buff_lang_error::LexError)
/// which carries a [`Diagnostic`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexerError {
    pub inner: BuffLexError,
}

impl LexerError {
    /// Create a new lexer error with the given message and source span.
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            inner: BuffLexError::new(Diagnostic {
                severity: Severity::Error,
                message: message.into(),
                span,
                notes: Vec::new(),
            }),
        }
    }

    /// An unexpected character was encountered.
    pub fn unexpected_char(ch: char, span: Span) -> Self {
        Self::new(format!("unexpected character: {:?}", ch), span)
    }

    /// An unterminated string literal.
    pub fn unterminated_string(span: Span) -> Self {
        Self::new("unterminated string literal", span)
    }

    /// An invalid numeric literal.
    pub fn invalid_number(span: Span) -> Self {
        Self::new("invalid numeric literal", span)
    }

    /// Mixed tabs and spaces in indentation.
    pub fn mixed_tabs_spaces(span: Span) -> Self {
        Self::new("mixed tabs and spaces in indentation", span)
    }
}

impl std::fmt::Display for LexerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl std::error::Error for LexerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.inner)
    }
}

impl From<LexerError> for buff_lang_error::BuffError {
    fn from(e: LexerError) -> Self {
        buff_lang_error::BuffError::Lex(e.inner)
    }
}
