//! Indentation tracking for the offside rule.
//!
//! [`IndentationTracker`] maintains a stack of indentation levels and emits
//! synthetic [`TokenKind::Indent`] / [`TokenKind::Dedent`] tokens when the
//! indentation changes between lines.
//!
//! Conventions:
//! - Each tab counts as **4 columns**.
//! - Mixing tabs and spaces in the same leading-whitespace run is an error.
//! - Blank lines and comment-only lines never trigger indent checks.

use deox_error::{SourceId, Span};

use crate::error::LexerError;
use crate::token::TokenKind;

/// The number of columns represented by a single tab character.
pub const TAB_WIDTH: usize = 4;

/// Tracks indentation levels and emits synthetic Indent/Dedent tokens.
#[derive(Debug, Clone)]
pub struct IndentationTracker {
    /// Stack of indent levels (in column units). Always starts with `[0]`.
    stack: Vec<usize>,
}

impl Default for IndentationTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl IndentationTracker {
    /// Create a new tracker. The base indentation level is `0`.
    pub fn new() -> Self {
        Self { stack: vec![0] }
    }

    /// Inspect the leading whitespace of a new line and return any synthetic
    /// tokens to emit.
    ///
    /// - Returns `Indent` if the line is more indented than the previous one.
    /// - Returns one or more `Dedent` tokens if the line is less indented.
    /// - Returns nothing if the level is unchanged.
    /// - Returns [`LexerError::mixed_tabs_spaces`] if the leading whitespace
    ///   mixes tabs and spaces.
    /// - Returns a generic [`LexerError`] if the new level does not match any
    ///   previous level on the stack (inconsistent dedent).
    pub fn check_line(
        &mut self,
        indent: &str,
        source_id: SourceId,
        span_start: usize,
    ) -> Result<Vec<TokenKind>, LexerError> {
        let level = compute_level(indent, source_id, span_start)?;
        self.adjust_to_level(level, source_id, span_start, indent.len())
    }

    /// Emit a `Dedent` token for every level remaining on the stack (except
    /// the base `0`). Called once at end-of-input.
    pub fn finalize(&mut self) -> Vec<TokenKind> {
        let mut tokens = Vec::new();
        while self.stack.len() > 1 {
            self.stack.pop();
            tokens.push(TokenKind::Dedent);
        }
        tokens
    }

    fn adjust_to_level(
        &mut self,
        level: usize,
        source_id: SourceId,
        span_start: usize,
        indent_len: usize,
    ) -> Result<Vec<TokenKind>, LexerError> {
        let top = self.stack.last().copied().unwrap_or(0);
        let mut tokens = Vec::new();

        if level > top {
            self.stack.push(level);
            tokens.push(TokenKind::Indent);
        } else if level < top {
            while self.stack.last().copied().unwrap_or(0) > level {
                self.stack.pop();
                tokens.push(TokenKind::Dedent);
            }
            if self.stack.last().copied().unwrap_or(0) != level {
                return Err(LexerError::new(
                    "inconsistent indentation level",
                    Span::new(span_start, span_start + indent_len, source_id),
                ));
            }
        }
        Ok(tokens)
    }
}

/// Compute the column count for an indentation run.
///
/// Tabs count as `TAB_WIDTH` columns each. Mixing tabs and spaces is rejected.
pub fn compute_level(
    indent: &str,
    source_id: SourceId,
    span_start: usize,
) -> Result<usize, LexerError> {
    if indent.is_empty() {
        return Ok(0);
    }
    let has_tab = indent.contains('\t');
    let has_space = indent.contains(' ');
    if has_tab && has_space {
        return Err(LexerError::mixed_tabs_spaces(Span::new(
            span_start,
            span_start + indent.len(),
            source_id,
        )));
    }
    if has_tab {
        Ok(indent.len() * TAB_WIDTH)
    } else {
        Ok(indent.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid() -> SourceId {
        SourceId(0)
    }

    #[test]
    fn empty_indent_is_zero() {
        assert_eq!(compute_level("", sid(), 0).unwrap(), 0);
    }

    #[test]
    fn spaces_count_one_per_char() {
        assert_eq!(compute_level("    ", sid(), 0).unwrap(), 4);
    }

    #[test]
    fn tabs_count_four_per_char() {
        assert_eq!(compute_level("\t\t", sid(), 0).unwrap(), 8);
    }

    #[test]
    fn mixed_tabs_spaces_error() {
        assert!(compute_level("  \t", sid(), 0).is_err());
    }

    #[test]
    fn indent_increase_emits_one() {
        let mut t = IndentationTracker::new();
        let kinds = t.check_line("    ", sid(), 0).unwrap();
        assert_eq!(kinds, vec![TokenKind::Indent]);
    }

    #[test]
    fn finalize_emits_pending_dedents() {
        let mut t = IndentationTracker::new();
        t.check_line("    ", sid(), 0).unwrap();
        t.check_line("        ", sid(), 0).unwrap();
        let kinds = t.finalize();
        assert_eq!(kinds, vec![TokenKind::Dedent, TokenKind::Dedent]);
    }
}
