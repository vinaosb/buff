//! Error type for `.buffhtml` parsing.
//!
//! Wraps [`buff_lang_error::Span`] for diagnostic locations. Re-uses the
//! existing `ParseError` machinery downstream (the CLI error_mapper already
//! understands `Span`).

use buff_lang_error::Span;
use thiserror::Error;

/// Error produced by the `.buffhtml` lexer or parser.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuffHtmlParseError {
    #[error("buffhtml lex error at {span:?}: {message}")]
    Lex { message: String, span: Span },
    #[error("buffhtml parse error at {span:?}: {message}")]
    Parse { message: String, span: Span },
}

impl BuffHtmlParseError {
    pub fn lex(message: impl Into<String>, span: Span) -> Self {
        Self::Lex {
            message: message.into(),
            span,
        }
    }

    pub fn parse(message: impl Into<String>, span: Span) -> Self {
        Self::Parse {
            message: message.into(),
            span,
        }
    }

    /// Span where the error occurred (lex or parse).
    pub fn span(&self) -> Span {
        match self {
            Self::Lex { span, .. } | Self::Parse { span, .. } => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_construction_carries_span() {
        let span = Span::new(1, 2, buff_lang_error::SourceId(0));
        let e = BuffHtmlParseError::lex("boom", span);
        assert!(matches!(e, BuffHtmlParseError::Lex { .. }));
        assert_eq!(e.span(), span);
        assert!(format!("{e}").contains("boom"));
    }
}
