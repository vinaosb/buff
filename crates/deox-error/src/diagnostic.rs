//! Diagnostic types — severity levels, diagnostic messages, and the top-level error enum.

use crate::span::Span;

/// The severity of a diagnostic message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A diagnostic message with severity, message text, source span, and notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// Create a new error-level diagnostic.
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span,
            notes: Vec::new(),
        }
    }

    /// Create a new warning-level diagnostic.
    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            span,
            notes: Vec::new(),
        }
    }

    /// Create a new info-level diagnostic.
    pub fn info(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Info,
            message: message.into(),
            span,
            notes: Vec::new(),
        }
    }

    /// Add a note to this diagnostic.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.severity, self.message)?;
        if !self.notes.is_empty() {
            for note in &self.notes {
                write!(f, "\n  note: {}", note)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Sub-error types — each wraps a Diagnostic
// ---------------------------------------------------------------------------

/// A lexer error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct LexError {
    pub diagnostic: Diagnostic,
}

impl LexError {
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

/// A parser error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct ParseError {
    pub diagnostic: Diagnostic,
}

impl ParseError {
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

/// A type-checking error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct TypeError {
    pub diagnostic: Diagnostic,
}

impl TypeError {
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

/// A code-generation error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct CodegenError {
    pub diagnostic: Diagnostic,
}

impl CodegenError {
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

/// A runtime error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct RuntimeError {
    pub diagnostic: Diagnostic,
}

impl RuntimeError {
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

// ---------------------------------------------------------------------------
// Top-level error enum
// ---------------------------------------------------------------------------

/// The top-level error type for the Deox compiler.
///
/// Each variant wraps a phase-specific error that carries a [`Diagnostic`].
#[derive(Debug, thiserror::Error)]
pub enum DeoxError {
    #[error("Lex error: {0}")]
    Lex(#[from] LexError),
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("Type error: {0}")]
    Type(#[from] TypeError),
    #[error("Codegen error: {0}")]
    Codegen(#[from] CodegenError),
    #[error("Runtime error: {0}")]
    Runtime(#[from] RuntimeError),
}
