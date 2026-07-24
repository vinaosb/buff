//! Lexer error types.
//!
//! Wraps [`buff_lang_error::LexError`] with convenience constructors for common
//! lexer failure modes. Each named constructor attaches a stable
//! [`ErrorCode`](buff_lang_error::ErrorCode) (T124) so the user sees
//! e.g. `[Error] error[E1001]: unexpected character: '@'`.

use buff_lang_error::{Diagnostic, ErrorCode, LexError as BuffLexError, Severity, Span};

/// A lexer error. Wraps the `buff-lang-error` [`LexError`](buff_lang_error::LexError)
/// which carries a [`Diagnostic`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexerError {
    pub inner: BuffLexError,
}

impl LexerError {
    /// Create a new lexer error with the given message and source span.
    ///
    /// Prefer the named constructors below (`unexpected_char`, etc.) where
    /// possible — they attach a stable [`ErrorCode`] automatically. This
    /// generic constructor leaves `code` as `None`, so the diagnostic
    /// renders without an `E1xxx` tag.
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            inner: BuffLexError::new(Diagnostic {
                severity: Severity::Error,
                message: message.into(),
                span,
                notes: Vec::new(),
                code: None,
                // T1 (v1.25 Wave 0): new Diagnostic fields. Empty by
                // default — the lexer does not yet emit multi-span labels
                // or fix suggestions, but the struct literal must list
                // every field (no `..Default::default()` on non-Default
                // types). Adding these keeps the constructor
                // byte-identical to the pre-T1 single-span render.
                labels: Vec::new(),
                suggestions: Vec::new(),
            }),
        }
    }

    /// An unexpected character was encountered.
    pub fn unexpected_char(ch: char, span: Span) -> Self {
        Self::coded(
            ErrorCode::UnexpectedChar,
            format!("unexpected character: {:?}", ch),
            span,
        )
    }

    /// An unterminated string literal.
    pub fn unterminated_string(span: Span) -> Self {
        Self::coded(
            ErrorCode::UnterminatedString,
            "unterminated string literal",
            span,
        )
    }

    /// An invalid numeric literal.
    pub fn invalid_number(span: Span) -> Self {
        Self::coded(ErrorCode::InvalidNumber, "invalid numeric literal", span)
    }

    /// Mixed tabs and spaces in indentation.
    pub fn mixed_tabs_spaces(span: Span) -> Self {
        Self::coded(
            ErrorCode::MixedTabsSpaces,
            "mixed tabs and spaces in indentation",
            span,
        )
    }

    /// Build a lexer error that carries a stable [`ErrorCode`].
    fn coded(code: ErrorCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            inner: BuffLexError::new(Diagnostic::error(message, span).with_code(code)),
        }
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
