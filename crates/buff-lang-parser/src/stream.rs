//! [`TokenStream`] — a cursor over a slice of lexer-produced tokens.
//!
//! Provides peek/advance/expect helpers and span computation for the
//! hand-rolled Pratt parser in [`crate::expr`].
//!
//! Layout tokens ([`TokenKind::Newline`], [`TokenKind::Indent`],
//! [`TokenKind::Dedent`]) are *skipped automatically* by [`peek`]/[`next`]
//! so the parser logic stays clean. Callers that care about layout should
//! construct the stream from a pre-filtered token slice.

use buff_lang_error::{Diagnostic, ErrorCode, ParseError, SourceId, Span};
use buff_lang_lexer::{Token, TokenKind};

use crate::options::Edition;

/// A read-only cursor over a slice of tokens.
///
/// Tracks the current position and the source id used to fabricate spans for
/// synthetic nodes or unexpected-EOF errors.
pub struct TokenStream<'a> {
    tokens: &'a [Token],
    pos: usize,
    source_id: SourceId,
    edition: Edition,
    matrix_row_depth: usize,
}

impl<'a> TokenStream<'a> {
    /// Construct a new cursor over `tokens` with the default
    /// [`Edition::Standard`]. The cursor does not clone; it borrows the slice
    /// for its entire lifetime.
    pub fn new(tokens: &'a [Token], source_id: SourceId) -> Self {
        Self::with_edition(tokens, source_id, Edition::default())
    }

    /// Construct a new cursor over `tokens` with a specific [`Edition`]
    /// (T57). Used by the scientific-edition entry points
    /// ([`parse_with_edition`](crate::parse_with_edition) and friends) to
    /// opt into the Julia-inspired mathematical syntax extensions.
    pub fn with_edition(tokens: &'a [Token], source_id: SourceId, edition: Edition) -> Self {
        Self {
            tokens,
            pos: 0,
            source_id,
            edition,
            matrix_row_depth: 0,
        }
    }

    /// The [`SourceId`] associated with this stream.
    pub fn source_id(&self) -> SourceId {
        self.source_id
    }

    /// The [`Edition`] this stream was constructed with (T57). Parser arms
    /// that implement scientific-edition syntax consult this to decide
    /// whether to accept the extension.
    pub fn edition(&self) -> Edition {
        self.edition
    }

    /// Returns `true` while the cursor is inside a scientific-edition matrix
    /// row parse (T57). Used by [`parse_multiplicative`](crate::expr) to
    /// suppress implicit multiplication inside `[1 2 3]` — there, whitespace
    /// is the row-element separator, not juxtaposition. The depth is a
    /// counter (not a bool) so nested matrix literals parse correctly.
    pub fn in_matrix_row(&self) -> bool {
        self.matrix_row_depth > 0
    }

    /// Increment the matrix-row nesting counter (T57). Pair with
    /// [`Self::exit_matrix_row`]; the [`parse_matrix_row`](crate::expr)
    /// helper brackets its body with enter/exit calls so the rest of the
    /// parser can consult [`Self::in_matrix_row`].
    pub fn enter_matrix_row(&mut self) {
        self.matrix_row_depth = self.matrix_row_depth.saturating_add(1);
    }

    /// Decrement the matrix-row nesting counter (T57). See
    /// [`Self::enter_matrix_row`].
    pub fn exit_matrix_row(&mut self) {
        if self.matrix_row_depth > 0 {
            self.matrix_row_depth -= 1;
        }
    }

    /// Look at the current token (skipping any layout tokens). Returns
    /// `None` at end-of-input or when an explicit [`TokenKind::Eof`] is
    /// reached.
    pub fn peek(&self) -> Option<&Token> {
        let mut i = self.pos;
        while i < self.tokens.len() {
            match self.tokens[i].kind {
                TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent => {
                    i += 1;
                }
                TokenKind::Eof => return None,
                _ => return Some(&self.tokens[i]),
            }
        }
        None
    }

    /// Convenience: just the [`TokenKind`] of the next significant token.
    pub fn peek_kind(&self) -> Option<&TokenKind> {
        self.peek().map(|t| &t.kind)
    }

    /// Look at the token *after* the current one (skipping layout). Used to
    /// disambiguate `obj.method` vs `obj.method(...)` without committing.
    pub fn peek_second_kind(&self) -> Option<&TokenKind> {
        let mut i = self.pos;
        // Skip current token + any layout after it.
        let mut saw_real = false;
        while i < self.tokens.len() {
            match self.tokens[i].kind {
                TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent => {
                    i += 1;
                }
                TokenKind::Eof => return None,
                _ => {
                    if !saw_real {
                        saw_real = true;
                        i += 1;
                    } else {
                        return Some(&self.tokens[i].kind);
                    }
                }
            }
        }
        None
    }

    /// Advance past the current significant token and return an *owned*
    /// clone of it. Returns `None` at EOF.
    ///
    /// We return an owned [`Token`] (rather than `&Token`) because callers
    /// routinely need to keep using the stream after consuming a token, and
    /// a borrowed return would conflict with subsequent `&mut self` calls.
    ///
    /// Named `advance` (rather than `next`) so it is not confused with the
    /// [`Iterator::next`] trait method (which would trigger clippy's
    /// `should_implement_trait` lint).
    pub fn advance(&mut self) -> Option<Token> {
        // Skip any layout tokens first.
        while self.pos < self.tokens.len() {
            match self.tokens[self.pos].kind {
                TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent => self.pos += 1,
                _ => break,
            }
        }
        if self.pos < self.tokens.len() {
            // Don't return Eof as a real token.
            if matches!(self.tokens[self.pos].kind, TokenKind::Eof) {
                return None;
            }
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    /// True when the next significant token is `kind` (by structural match).
    /// Use [`matches!`](core::matches) at the call site for variants that
    /// carry data (e.g. `TokenKind::Ident(_)`).
    pub fn check(&self, kind: &TokenKind) -> bool {
        self.peek_kind() == Some(kind)
    }

    /// True when no more significant tokens remain.
    pub fn is_at_end(&self) -> bool {
        self.peek().is_none()
    }

    /// Consume the next token if its kind matches `expected_kind`. On
    /// success returns an *owned* clone of the consumed token; on failure
    /// returns a [`ParseError`] pointing at the offending (or EOF) position.
    ///
    /// `expected_kind` is compared structurally — variants carrying data
    /// (like [`TokenKind::Ident`]) only match a token with the *same* inner
    /// value. For "any identifier" matches, use [`Self::next`] plus a manual
    /// kind-check instead.
    pub fn expect(&mut self, expected_kind: TokenKind) -> Result<Token, ParseError> {
        if let Some(tok) = self.peek() {
            if tok.kind == expected_kind {
                // SAFETY: peek returned Some(tok) at index self.pos (post-
                // layout-skip). advance() will return the same token.
                Ok(self.advance().expect("peek guaranteed a token"))
            } else {
                Err(ParseError::new(
                    Diagnostic::error(
                        format!("expected `{expected_kind}`, found `{}`", tok.kind),
                        tok.span,
                    )
                    .with_code(ErrorCode::ExpectedToken),
                ))
            }
        } else {
            Err(ParseError::new(
                Diagnostic::error(
                    format!("expected `{expected_kind}`, found end of input"),
                    self.eof_span(),
                )
                .with_code(ErrorCode::ExpectedToken),
            ))
        }
    }

    /// Build an "unexpected token" error pointing at the current position.
    pub fn unexpected(&self, what: impl core::fmt::Display) -> ParseError {
        if let Some(tok) = self.peek() {
            ParseError::new(
                Diagnostic::error(format!("unexpected `{}`: {what}", tok.kind), tok.span)
                    .with_code(ErrorCode::UnexpectedToken),
            )
        } else {
            ParseError::new(
                Diagnostic::error(format!("unexpected end of input: {what}"), self.eof_span())
                    .with_code(ErrorCode::UnexpectedToken),
            )
        }
    }

    /// Span for a synthetic "end of file" position. If we have any tokens,
    /// use the last token's end offset; otherwise a dummy at offset 0.
    pub fn eof_span(&self) -> Span {
        let last = self.tokens.last();
        match last {
            Some(tok) => Span::new(tok.span.end, tok.span.end, self.source_id),
            None => Span::dummy(),
        }
    }

    /// Build a span covering `start_tok.span.start .. end_tok.span.end`,
    /// using this stream's [`SourceId`]. Used to compute parent-node spans
    /// from their children.
    pub fn span_between(start_tok: &Token, end_tok: &Token, source_id: SourceId) -> Span {
        Span::new(start_tok.span.start, end_tok.span.end, source_id)
    }

    // -----------------------------------------------------------------------
    // T25: speculative parsing — save / restore the cursor position.
    //
    // Brace disambiguation (`{` at primary position can be a closure OR a
    // map literal) requires trial parsing: try the closure shape first, and
    // on failure roll back and try the map shape. These accessors expose
    // just enough of the cursor position for that — they are additive and
    // carry no invariants beyond "restoring an earlier position is safe as
    // long as no `&mut` borrow is outstanding".
    // -----------------------------------------------------------------------

    /// Snapshot the current cursor position (T25). Pass the returned value
    /// to [`Self::restore`] to roll the cursor back to this point. Used by
    /// the speculative parser to try one shape and fall back to another.
    pub fn save(&self) -> usize {
        self.pos
    }

    /// Restore the cursor to a previously-snapshotted position (T25).
    /// The `pos` must come from a prior [`Self::save`] call on this same
    /// stream. Restoring is safe at any time (no invariants are violated
    /// by re-advancing over already-seen tokens).
    pub fn restore(&mut self, pos: usize) {
        self.pos = pos;
    }

    // -----------------------------------------------------------------------
    // T9: layout-sensitive (offside-rule) helpers.
    //
    // The peek/advance/expect helpers above transparently *skip* layout
    // tokens (Newline, Indent, Dedent). For T9 we need RAW access so the
    // parser can detect `: \n Indent ... Dedent` block shapes.
    // -----------------------------------------------------------------------

    /// Peek at the next RAW token, *including* layout tokens
    /// ([`TokenKind::Newline`] / [`TokenKind::Indent`] / [`TokenKind::Dedent`]).
    ///
    /// Returns `None` past the end of the slice. Unlike [`Self::peek`], this
    /// does *not* skip layout tokens and does *not* treat [`TokenKind::Eof`]
    /// specially (the caller can observe it).
    pub fn peek_raw(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// Convenience: just the [`TokenKind`] of the next raw token.
    pub fn peek_raw_kind(&self) -> Option<&TokenKind> {
        self.peek_raw().map(|t| &t.kind)
    }

    /// Advance past a single RAW token (does NOT skip layout tokens).
    ///
    /// Returns an *owned* clone of the consumed token, or `None` at the end
    /// of the slice. Useful for consuming [`TokenKind::Indent`] /
    /// [`TokenKind::Dedent`] / [`TokenKind::Newline`] in layout parsing.
    pub fn advance_raw(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// True when the next *raw* token has kind `kind`. Use this for layout
    /// detection (e.g. [`TokenKind::Dedent`], [`TokenKind::Colon`]) without
    /// accidentally skipping past it like [`Self::check`] would.
    pub fn check_raw(&self, kind: &TokenKind) -> bool {
        matches!(self.peek_raw_kind(), Some(k) if k == kind)
    }

    /// Consume an [`TokenKind::Indent`] token if the next raw token is one.
    /// Returns `true` if consumed, `false` otherwise. The cursor advances by
    /// exactly one raw position on success.
    pub fn consume_indent(&mut self) -> bool {
        if self.check_raw(&TokenKind::Indent) {
            self.advance_raw();
            true
        } else {
            false
        }
    }

    /// Consume a [`TokenKind::Dedent`] token if the next raw token is one.
    /// Returns `true` if consumed, `false` otherwise.
    pub fn consume_dedent(&mut self) -> bool {
        if self.check_raw(&TokenKind::Dedent) {
            self.advance_raw();
            true
        } else {
            false
        }
    }

    /// Consume a [`TokenKind::Newline`] token if the next raw token is one.
    /// Returns `true` if consumed, `false` otherwise. Useful for skipping
    /// the trailing newline that precedes an `Indent` in a layout block.
    pub fn consume_newline(&mut self) -> bool {
        if self.check_raw(&TokenKind::Newline) {
            self.advance_raw();
            true
        } else {
            false
        }
    }

    /// Build a synthetic span anchored at the *current* raw position. Used
    /// for error messages emitted by the layout parser when the cursor sits
    /// on a layout token (which `eof_span` would skip past).
    pub fn span_here(&self) -> Span {
        match self.tokens.get(self.pos) {
            Some(tok) => tok.span,
            None => self.eof_span(),
        }
    }

    // -----------------------------------------------------------------------
    // T36: error-recovery sync helper.
    //
    // When a `ParseError` occurs mid-declaration, the caller needs to skip
    // forward past the broken tokens until it reaches a point where a fresh
    // top-level declaration could begin. This is the classic "panic mode"
    // recovery strategy used by hand-rolled recursive-descent parsers.
    // -----------------------------------------------------------------------

    /// Skip tokens until the cursor sits on a **sync point** — a token that
    /// could begin a fresh top-level declaration. Used by
    /// [`parse_recovering`](crate::parse_recovering) to resume after a parse
    /// error.
    ///
    /// # Sync set
    ///
    /// The cursor stops on (i.e. does NOT consume) any of:
    ///
    /// - `func`, `async`, `enum`, `import`, `export`, `extern`, `extend`,
    ///   `trait` keywords (the top-level declaration starters),
    /// - `@` (the attribute prefix — attributes precede `func`),
    /// - end of input.
    ///
    /// Everything else (operators, literals, stray delimiters, layout tokens
    /// like `Newline`/`Indent`/`Dedent`) is consumed.
    ///
    /// # Infinite-loop safety
    ///
    /// The caller is responsible for ensuring progress: if this method is
    /// called when the cursor is *already* on a sync token, it returns
    /// immediately without advancing. The caller should detect this case
    /// (compare cursor position before/after) and force-advance if needed.
    ///
    /// Returns `true` if a sync token was found, `false` at end of input.
    pub fn sync_to_recovery_point(&mut self) -> bool {
        while let Some(tok) = self.peek() {
            match tok.kind {
                TokenKind::KwFunc
                | TokenKind::KwAsync
                | TokenKind::KwEnum
                | TokenKind::KwImport
                | TokenKind::KwExport
                | TokenKind::KwExtern
                | TokenKind::KwExtend
                | TokenKind::KwTrait
                | TokenKind::At => return true,
                _ => {
                    self.advance();
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(src: &str) -> (Vec<Token>, SourceId) {
        let sid = SourceId(7);
        let toks = buff_lang_lexer::tokenize(src, sid).expect("tokenize failed");
        (toks, sid)
    }

    #[test]
    fn empty_input_is_at_end() {
        let (toks, sid) = ts("");
        let s = TokenStream::new(&toks, sid);
        assert!(s.is_at_end());
        assert!(s.peek().is_none());
    }

    #[test]
    fn peek_does_not_consume() {
        let (toks, sid) = ts("42");
        let s = TokenStream::new(&toks, sid);
        assert_eq!(s.peek().map(|t| &t.kind), Some(&TokenKind::IntLit(42)));
        assert_eq!(s.peek().map(|t| &t.kind), Some(&TokenKind::IntLit(42)));
    }

    #[test]
    fn next_advances_cursor() {
        let (toks, sid) = ts("42 7");
        let mut s = TokenStream::new(&toks, sid);
        assert_eq!(s.advance().map(|t| t.kind), Some(TokenKind::IntLit(42)));
        assert_eq!(s.advance().map(|t| t.kind), Some(TokenKind::IntLit(7)));
        assert!(s.advance().is_none());
    }

    #[test]
    fn layout_tokens_are_skipped() {
        // Multi-line input: Newline tokens should be transparent to peek/advance.
        let (toks, sid) = ts("foo\nbar");
        let mut s = TokenStream::new(&toks, sid);
        assert_eq!(
            s.advance().map(|t| t.kind),
            Some(TokenKind::Ident("foo".into()))
        );
        assert_eq!(
            s.advance().map(|t| t.kind),
            Some(TokenKind::Ident("bar".into()))
        );
    }

    #[test]
    fn expect_matches_succeeds() {
        let (toks, sid) = ts("( )");
        let mut s = TokenStream::new(&toks, sid);
        assert!(s.expect(TokenKind::LParen).is_ok());
        assert!(s.expect(TokenKind::RParen).is_ok());
    }

    #[test]
    fn expect_mismatch_errors() {
        let (toks, sid) = ts("(");
        let mut s = TokenStream::new(&toks, sid);
        assert!(s.expect(TokenKind::LParen).is_ok());
        let err = s.expect(TokenKind::RParen).unwrap_err();
        assert!(err.diagnostic.message.contains("expected"));
    }

    #[test]
    fn eof_span_falls_back_to_last_token_end() {
        let (toks, sid) = ts("foo");
        let s = TokenStream::new(&toks, sid);
        let span = s.eof_span();
        assert_eq!(span.source_id, sid);
        assert_eq!(span.start, 3);
        assert_eq!(span.end, 3);
    }

    #[test]
    fn peek_second_kind_skips_one_real_token() {
        let (toks, sid) = ts("a . b");
        let s = TokenStream::new(&toks, sid);
        assert_eq!(s.peek_second_kind(), Some(&TokenKind::Dot));
    }

    // -----------------------------------------------------------------
    // T9 layout-helper tests
    // -----------------------------------------------------------------

    #[test]
    fn peek_raw_does_not_skip_layout() {
        // Two lines: foo\nbar -> tokens include a Newline between them.
        let (toks, sid) = ts("foo\nbar");
        let mut s = TokenStream::new(&toks, sid);
        // Consume `foo` raw.
        assert_eq!(
            s.advance_raw().map(|t| t.kind),
            Some(TokenKind::Ident("foo".into()))
        );
        // peek_raw sees the Newline; peek would skip it.
        assert_eq!(s.peek_raw_kind(), Some(&TokenKind::Newline));
        assert_eq!(s.peek_kind(), Some(&TokenKind::Ident("bar".into())));
    }

    #[test]
    fn check_raw_matches_layout_token() {
        let (toks, sid) = ts("    x"); // indent=4 then Ident(x)
                                       // Token stream: Indent, Ident("x"), Eof
        let s = TokenStream::new(&toks, sid);
        assert!(s.check_raw(&TokenKind::Indent));
        assert!(!s.check_raw(&TokenKind::Dedent));
    }

    #[test]
    fn consume_indent_advances_only_on_match() {
        let (toks, sid) = ts("    x");
        let mut s = TokenStream::new(&toks, sid);
        assert!(s.consume_indent());
        assert_eq!(s.peek_raw_kind(), Some(&TokenKind::Ident("x".into())));
        // Second call: no more Indents.
        assert!(!s.consume_indent());
    }

    #[test]
    fn consume_newline_skips_exactly_one() {
        let (toks, sid) = ts("foo\nbar");
        let mut s = TokenStream::new(&toks, sid);
        s.advance_raw(); // foo
        assert!(s.consume_newline());
        assert_eq!(s.peek_raw_kind(), Some(&TokenKind::Ident("bar".into())));
    }

    #[test]
    fn span_here_uses_current_raw_position() {
        let (toks, sid) = ts("foo\nbar");
        let mut s = TokenStream::new(&toks, sid);
        s.advance_raw(); // consume `foo`
                         // Now sitting on the Newline token.
        let sp = s.span_here();
        assert_eq!(sp.source_id, sid);
        // Newline span covers `\n` at offset 3..4.
        assert_eq!(sp.start, 3);
        assert_eq!(sp.end, 4);
    }

    #[test]
    fn advance_raw_returns_none_at_end() {
        let (toks, sid) = ts("x");
        let mut s = TokenStream::new(&toks, sid);
        let _ = s.advance_raw(); // x
        let _ = s.advance_raw(); // Eof (still get clones)
        assert!(s.advance_raw().is_none());
    }
}
