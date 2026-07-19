//! Lossless AST — trivia-preserving source representation (T57).
//!
//! The semantic [`crate::Decl`] AST produced by `buff-lang-parser` is *lossy*:
//! it discards whitespace, blank lines, and comments (the Buff lexer strips
//! `//` and `/* */` before tokens are emitted). That loss is the reason
//! `buff fmt` (T54) drops comments today.
//!
//! This module provides a focused, **additive** lossless layer that segments
//! the raw source into a flat sequence of [`Piece`]s — either [`Trivia`]
//! (whitespace runs, newlines, line comments, block comments) or
//! [`LosslessToken`] (any maximal run of "non-trivia" bytes, including
//! identifiers, literals, operators, strings, char literals). The pieces
//! concatenate back to the EXACT original bytes, giving:
//!
//! - **Byte-exact roundtrip**: `parse_lossless(src).to_source() == src`
//!   for any valid UTF-8 string (including comments, blank lines, mixed
//!   indent, multi-byte chars, string interpolation, regex literals, etc.).
//! - **Comment preservation**: line + nested block comments are first-class
//!   [`Trivia`] pieces, not stripped — enabling a future comment-preserving
//!   `buff fmt` (the T57 → T54 follow-up).
//! - **LSP-friendly structure**: each piece carries its byte range, and
//!   `comments()` / `tokens()` / `trivia()` iterators support editors that
//!   need to map byte offsets → trivia / tokens (e.g. for hover, folding,
//!   "go to comment above next decl").
//!
//! # Design (why a flat piece list, not a green tree)
//!
//! A full rust-analyzer-style lossless syntax tree (Rowan green-tree) is far
//! too large for one task. For T57's deliverables — byte-exact roundtrip +
//! comment preservation — a flat piece list is sufficient AND structurally
//! compatible with a future tree rewrite: a tree can be layered on top later
//! by grouping pieces into parent nodes without changing the piece model.
//!
//! # Scanner boundaries
//!
//! The scanner recognises exactly the byte sequences that must NOT be split
//! mid-piece for roundtrip to hold:
//!
//! - whitespace runs (` `, `\t`) — maximal run = one [`Trivia`]
//! - newlines (`\n`, `\r\n`, `\r`) — one each, CRLF preserved
//! - line comments `// ...` — to (not including) EOL
//! - block comments `/* ... */` — **nested** (`/* a /* b */ c */`)
//! - string literals `"..."` — with escape `\\` + `{...}` interpolation
//! - char literals `'...'` — with escape `\\`
//! - triple-quoted raw strings `"""..."""`
//! - raw strings `r"..."` (no escape / no interpolation)
//!
//! Everything else is a "boring byte run" — a maximal run of bytes that
//! don't start any of the above. This intentionally does NOT do semantic
//! tokenization (no keyword/number/operator split) — for byte-exact
//! roundtrip, opaque token text is enough. (Semantic equivalence is proved
//! via byte-exactness: if `to_source() == src`, then re-parsing through the
//! normal pipeline yields the same `Vec<Decl>`.)
//!
//! # Incremental reparse (minimal stub)
//!
//! [`LosslessTree::reparse`] is the structured hook for LSP edits: given an
//! edit range and replacement, it produces a new tree. The v1.0
//! implementation does a **full re-scan** of the new source (the API
//! signature is what matters — future versions can re-scan only the pieces
//! overlapping the edit window). Full incremental reparse is documented as
//! a v1.0-minimal stub.
//!
//! # Errors / panics
//!
//! The scanner **never panics** on any input. Unterminated strings, block
//! comments, char literals, etc. are captured as best-effort pieces running
//! to end-of-input — roundtrip still holds (the same bytes reconstruct the
//! same piece).
//!
//! # Example
//!
//! ```
//! use buff_lang_ast::lossless::parse_lossless;
//!
//! let src = "func ola() {\n    // Olá, Buff!\n    print(\"Olá, Buff!\")\n}\n";
//! let tree = parse_lossless(src);
//! assert_eq!(tree.to_source(), src);              // byte-exact roundtrip
//! assert_eq!(tree.comments().count(), 1);          // comment preserved
//! ```

// Note: this module is ADDITIVE ONLY. It does NOT modify the existing
// lexer, parser, or semantic AST node types. It is self-contained in the
// ast crate (no dependency on buff-lang-lexer or buff-lang-parser — adding
// either as a non-dev dep would create a cycle, since both depend on
// buff-lang-ast).

// No tab characters in this file — Buff source convention (and the Buff
// lexer rejects tabs). 4-space indent throughout.

// ---------------------------------------------------------------------------
// Piece types
// ---------------------------------------------------------------------------

/// Kind of a [`Trivia`] piece.
///
/// Trivia is the "lossy" content that the semantic AST drops: whitespace,
/// newlines, and comments. Capturing it as typed pieces (rather than opaque
/// strings) lets future tooling (LSP folding, `buff fmt` comment
/// preservation) treat each kind specifically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriviaKind {
    /// A run of ` ` and/or `\t` bytes. NOT including newlines.
    Whitespace,
    /// A single newline (`\n`, `\r\n`, or `\r`). CRLF is preserved as one
    /// piece, never split.
    Newline,
    /// A line comment `// ...` running to (not including) the next EOL.
    LineComment,
    /// A block comment `/* ... */`, possibly nested (`/* a /* b */ c */`).
    /// Unterminated block comments are captured best-effort (run to EOF).
    BlockComment,
}

/// A trivia piece: whitespace, newline, or comment.
///
/// See [`TriviaKind`] for the discriminant taxonomy. The `text` field is the
/// raw bytes of the piece (e.g. `"    "`, `"\n"`, `"// hello"`,
/// `"/* a /* b */ c */"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Trivia {
    /// Which kind of trivia this is.
    pub kind: TriviaKind,
    /// The raw source text of the piece (preserved byte-for-byte).
    pub text: String,
}

impl Trivia {
    /// Construct a new trivia piece.
    pub fn new(kind: TriviaKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }

    /// Byte length of this trivia piece.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether this trivia piece is empty (should not occur for valid input,
    /// but defensive — empty text is allowed).
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// A lossless token: a maximal run of "non-trivia" bytes — i.e. anything
/// that isn't whitespace, a newline, a comment, or a string/char literal.
///
/// This intentionally does NOT distinguish identifiers / numbers / operators
/// / keywords / regex literals. For byte-exact roundtrip, opaque text is
/// sufficient. Semantic tokenization belongs to the existing
/// `buff-lang-lexer` (which DOES classify, but strips comments — that's the
/// gap T57 closes).
///
/// String literals, char literals, triple-quoted strings, and raw strings
/// ARE represented as a single `LosslessToken` each (their boundaries are
/// recognised by the scanner so they aren't mis-split internally — e.g.
/// `"a // b"` is one token, not a token + comment).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LosslessToken {
    /// The raw source text of the token (preserved byte-for-byte).
    pub text: String,
}

impl LosslessToken {
    /// Construct a new lossless token.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// Byte length of this token.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether this token is empty (should not occur for valid input).
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// A piece in the lossless tree: either [`Trivia`] or [`LosslessToken`].
///
/// Each piece also carries its byte range in the original source
/// (`start`, `end`) so editors can map cursor offsets → pieces without
/// rescanning. `text` is stored alongside for convenience (it equals
/// `&src[start..end]`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Piece {
    /// Whitespace, newline, or comment.
    Trivia {
        /// Kind of trivia.
        kind: TriviaKind,
        /// Inclusive start byte offset in the original source.
        start: usize,
        /// Exclusive end byte offset in the original source.
        end: usize,
        /// The raw source text (`src[start..end]`).
        text: String,
    },
    /// A non-trivia token (identifier, literal, operator, string, etc.).
    Token {
        /// Inclusive start byte offset in the original source.
        start: usize,
        /// Exclusive end byte offset in the original source.
        end: usize,
        /// The raw source text (`src[start..end]`).
        text: String,
    },
}

impl Piece {
    /// Inclusive start byte offset of this piece in the original source.
    pub fn start(&self) -> usize {
        match self {
            Piece::Trivia { start, .. } | Piece::Token { start, .. } => *start,
        }
    }

    /// Exclusive end byte offset of this piece in the original source.
    pub fn end(&self) -> usize {
        match self {
            Piece::Trivia { end, .. } | Piece::Token { end, .. } => *end,
        }
    }

    /// Byte length of this piece.
    pub fn len(&self) -> usize {
        self.end() - self.start()
    }

    /// Whether this piece is empty (zero-length). Should not occur for
    /// valid input — pieces always advance the cursor — but exposed for
    /// `len`/`is_empty` symmetry.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The raw source text of this piece.
    pub fn text(&self) -> &str {
        match self {
            Piece::Trivia { text, .. } | Piece::Token { text, .. } => text,
        }
    }

    /// Whether this piece is a [`Trivia`].
    pub fn is_trivia(&self) -> bool {
        matches!(self, Piece::Trivia { .. })
    }

    /// Whether this piece is a [`LosslessToken`].
    pub fn is_token(&self) -> bool {
        matches!(self, Piece::Token { .. })
    }

    /// If this piece is a [`Trivia`], return its kind; otherwise `None`.
    pub fn trivia_kind(&self) -> Option<TriviaKind> {
        match self {
            Piece::Trivia { kind, .. } => Some(*kind),
            Piece::Token { .. } => None,
        }
    }

    /// If this piece is a comment ([`TriviaKind::LineComment`] or
    /// [`TriviaKind::BlockComment`]), return its kind; otherwise `None`.
    pub fn comment_kind(&self) -> Option<TriviaKind> {
        match self {
            Piece::Trivia {
                kind: TriviaKind::LineComment | TriviaKind::BlockComment,
                ..
            } => self.trivia_kind(),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// LosslessTree
// ---------------------------------------------------------------------------

/// A lossless representation of a Buff source file.
///
/// Stores the original source bytes plus a flat list of [`Piece`]s that
/// segment it. Roundtrip is guaranteed byte-exact:
///
/// - [`LosslessTree::to_source`] returns the original bytes verbatim.
/// - [`LosslessTree::pieces_to_source`] reconstructs by walking pieces
///   (used in tests to prove the pieces alone roundtrip).
///
/// Construct via [`parse_lossless`] or [`LosslessTree::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LosslessTree {
    /// The original source. Stored in full so `to_source()` is trivially
    /// byte-exact and so piece text slicing (`&src[start..end]`) is always
    /// valid UTF-8.
    src: String,
    /// Flat list of pieces, in source order, covering the full src range
    /// with no gaps or overlaps.
    pieces: Vec<Piece>,
}

impl LosslessTree {
    /// Parse a source string into a lossless tree.
    ///
    /// Never panics. Unterminated strings / block comments / char literals
    /// are captured best-effort (run to EOF); roundtrip still holds.
    pub fn parse(src: &str) -> Self {
        let pieces = scan(src);
        Self {
            src: src.to_string(),
            pieces,
        }
    }

    /// The original source bytes (byte-exact).
    pub fn src(&self) -> &str {
        &self.src
    }

    /// Reconstruct the source by returning the stored original. This is
    /// THE byte-exact roundtrip primitive: `parse_lossless(s).to_source()
    /// == s` for any valid UTF-8 string `s`.
    pub fn to_source(&self) -> String {
        self.src.clone()
    }

    /// Reconstruct the source by concatenating piece texts (proves the
    /// pieces alone — without the stored `src` — roundtrip byte-exact).
    /// Used in tests; useful for verifying piece integrity after an edit.
    pub fn pieces_to_source(&self) -> String {
        let mut out = String::with_capacity(self.src.len());
        for p in &self.pieces {
            out.push_str(p.text());
        }
        out
    }

    /// The flat list of pieces (source order).
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// Number of pieces (trivia + tokens).
    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// Number of trivia pieces.
    pub fn trivia_count(&self) -> usize {
        self.pieces.iter().filter(|p| p.is_trivia()).count()
    }

    /// Number of token pieces.
    pub fn token_count(&self) -> usize {
        self.pieces.iter().filter(|p| p.is_token()).count()
    }

    /// Number of comment pieces (line + block).
    pub fn comment_count(&self) -> usize {
        self.pieces
            .iter()
            .filter(|p| p.comment_kind().is_some())
            .count()
    }

    /// Iterator over trivia pieces (whitespace + newlines + comments).
    pub fn trivia(&self) -> impl Iterator<Item = &Piece> {
        self.pieces.iter().filter(|p| p.is_trivia())
    }

    /// Iterator over token pieces.
    pub fn tokens(&self) -> impl Iterator<Item = &Piece> {
        self.pieces.iter().filter(|p| p.is_token())
    }

    /// Iterator over comment pieces only (line + block). Useful for the
    /// future comment-preserving `buff fmt`.
    pub fn comments(&self) -> impl Iterator<Item = &Piece> {
        self.pieces.iter().filter(|p| p.comment_kind().is_some())
    }

    /// Find the piece index containing the given byte offset, if any.
    ///
    /// Returns `None` for offsets beyond the end of source. Useful for LSP
    /// cursor → piece mapping. Time complexity: O(log n) via binary search
    /// (pieces are sorted by start offset with no gaps).
    pub fn piece_at(&self, byte_offset: usize) -> Option<&Piece> {
        // Binary search by piece start. Pieces are non-overlapping and
        // sorted ascending by start; the piece containing `offset` is the
        // last piece whose `start <= offset`.
        let idx = self.pieces.partition_point(|p| p.start() <= byte_offset);
        if idx == 0 {
            return None;
        }
        let p = &self.pieces[idx - 1];
        if byte_offset < p.end() {
            Some(p)
        } else {
            None
        }
    }

    /// Incremental-reparse hook (v1.0-minimal stub).
    ///
    /// Produces a new [`LosslessTree`] for the source after applying an
    /// edit: replace `src[start..end]` with `replacement`. The v1.0
    /// implementation does a **full re-scan** of the new source. The API
    /// signature is the structured hook for future incremental re-scan
    /// (re-tokenize only the pieces overlapping the edit window, then
    /// reuse the unchanged prefix/suffix).
    ///
    /// Out-of-bounds / inverted ranges are silently clamped to `[0,
    /// src.len()]` — the resulting tree still roundtrips the (clamped)
    /// edit. This never panics.
    pub fn reparse(&self, start: usize, end: usize, replacement: &str) -> Self {
        let src_len = self.src.len();
        let s = start.min(src_len);
        let e = end.max(s).min(src_len);
        let mut new_src = String::with_capacity(src_len - (e - s) + replacement.len());
        new_src.push_str(&self.src[..s]);
        new_src.push_str(replacement);
        new_src.push_str(&self.src[e..]);
        Self::parse(&new_src)
    }
}

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

/// Parse a source string into a [`LosslessTree`]. Convenience alias for
/// [`LosslessTree::parse`].
pub fn parse_lossless(src: &str) -> LosslessTree {
    LosslessTree::parse(src)
}

/// Scan a source string into a flat list of [`Piece`]s.
///
/// The pieces cover the entire source range with no gaps or overlaps.
fn scan(src: &str) -> Vec<Piece> {
    let bytes = src.as_bytes();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut pos = 0usize;
    let len = bytes.len();

    while pos < len {
        let start = pos;
        let bytes_ref = &bytes[pos..];
        if let Some((kind, consumed)) = scan_trivia(bytes_ref) {
            let text = src[start..start + consumed].to_string();
            pieces.push(Piece::Trivia {
                kind,
                start,
                end: start + consumed,
                text,
            });
            pos = start + consumed;
        } else if let Some(consumed) = scan_token_like(bytes_ref) {
            let text = src[start..start + consumed].to_string();
            pieces.push(Piece::Token {
                start,
                end: start + consumed,
                text,
            });
            pos = start + consumed;
        } else {
            // Defensive: if no scanner matched (shouldn't happen —
            // scan_token_like always advances at least 1 byte), advance
            // one byte as a single-char token to guarantee progress.
            let text = src[start..start + 1].to_string();
            pieces.push(Piece::Token {
                start,
                end: start + 1,
                text,
            });
            pos = start + 1;
        }
    }

    pieces
}

/// Try to scan a trivia piece (whitespace / newline / line comment / block
/// comment) starting at `bytes[0]`. Returns `(kind, consumed_bytes)` on
/// match, or `None` if no trivia starts here.
fn scan_trivia(bytes: &[u8]) -> Option<(TriviaKind, usize)> {
    let first = bytes[0];

    // Whitespace run (spaces + tabs, NOT newlines).
    if first == b' ' || first == b'\t' {
        let mut p = 1usize;
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
            p += 1;
        }
        return Some((TriviaKind::Whitespace, p));
    }

    // Newline: \n | \r\n | \r
    if first == b'\n' {
        return Some((TriviaKind::Newline, 1));
    }
    if first == b'\r' {
        let n = if bytes.len() > 1 && bytes[1] == b'\n' {
            2
        } else {
            1
        };
        return Some((TriviaKind::Newline, n));
    }

    // Line comment `//...` to (not including) EOL.
    if first == b'/' && bytes.len() > 1 && bytes[1] == b'/' {
        let mut p = 2usize;
        while p < bytes.len() && bytes[p] != b'\n' && bytes[p] != b'\r' {
            p += 1;
        }
        return Some((TriviaKind::LineComment, p));
    }

    // Block comment `/* ... */` with nesting. Unterminated runs to EOF.
    if first == b'/' && bytes.len() > 1 && bytes[1] == b'*' {
        let mut p = 2usize;
        let mut depth = 1usize;
        while p < bytes.len() && depth > 0 {
            if bytes[p] == b'/' && p + 1 < bytes.len() && bytes[p + 1] == b'*' {
                depth += 1;
                p += 2;
            } else if bytes[p] == b'*' && p + 1 < bytes.len() && bytes[p + 1] == b'/' {
                depth -= 1;
                p += 2;
            } else {
                p += 1;
            }
        }
        return Some((TriviaKind::BlockComment, p));
    }

    None
}

/// Scan a "token-like" piece: a string / char / raw-string literal, or a
/// maximal run of "boring" bytes (anything not starting trivia or a
/// string). Returns the number of bytes consumed (always ≥ 1), or `None`
/// if the position starts with trivia (caller should call `scan_trivia`
/// first).
///
/// Strings / chars / raw-strings / triple-strings are recognised here so
/// they're emitted as ONE `Piece::Token` (their internal bytes — including
/// `//`, `/*`, `"`, etc. — must not be mis-split).
fn scan_token_like(bytes: &[u8]) -> Option<usize> {
    let first = bytes[0];

    // Triple-quoted raw string `"""..."""` — check BEFORE single `"`.
    if first == b'"' && bytes.len() >= 3 && bytes[1] == b'"' && bytes[2] == b'"' {
        return Some(scan_triple_string_len(bytes));
    }

    // Single-quote string `"..."` with escape + `{...}` interpolation.
    // Interpolation braces nest: `"a {b + "}"}"` — the inner `"}"`
    // must NOT close the string.
    if first == b'"' {
        return Some(scan_string_len(bytes));
    }

    // Char literal `'...'` with escape.
    if first == b'\'' {
        return Some(scan_char_len(bytes));
    }

    // Raw string `r"..."` — only at token start (caller guarantees we're
    // at the first byte of a token; mid-token `r` is part of an
    // identifier like "bar").
    if first == b'r' && bytes.len() > 1 && bytes[1] == b'"' {
        return Some(scan_raw_string_len(bytes));
    }

    // Otherwise: maximal run of "boring" bytes — bytes that don't start
    // trivia and don't start a string/char. This naturally groups
    // identifiers, numbers, operators, regex literals, etc. into one
    // opaque token.
    let mut p = 1usize;
    while p < bytes.len() {
        let b = bytes[p];
        // Break before any trivia starter.
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            break;
        }
        // Break before `//` or `/*`.
        if b == b'/' && p + 1 < bytes.len() && (bytes[p + 1] == b'/' || bytes[p + 1] == b'*') {
            break;
        }
        // Break before a string / char starter (`"`, `'`).
        if b == b'"' || b == b'\'' {
            break;
        }
        // Note: do NOT break before `r"..."` mid-token — `r` only starts a
        // raw string at the FIRST byte of a token (handled above). Mid-
        // token `r` is part of an identifier (e.g. "bar") and must stay.
        p += 1;
    }
    Some(p)
}

/// Length of a `"..."` string literal starting at `bytes[0] == b'"'`,
/// including the closing quote. Handles `\\` escapes and `{...}`
/// interpolation nesting. Unterminated runs to EOF (best-effort).
fn scan_string_len(bytes: &[u8]) -> usize {
    let mut p = 1usize; // past opening `"`
    let mut brace_depth = 0i32;
    while p < bytes.len() {
        let b = bytes[p];
        if b == b'\\' && p + 1 < bytes.len() {
            // Escaped byte — consume the next byte literally (it can be
            // a `"` or `\` that doesn't close the string).
            p += 2;
            continue;
        }
        if b == b'"' && brace_depth == 0 {
            p += 1; // include closing quote
            return p;
        }
        if b == b'{' {
            brace_depth += 1;
        } else if b == b'}' && brace_depth > 0 {
            brace_depth -= 1;
        }
        p += 1;
    }
    // Unterminated — return what we have (roundtrip still holds).
    p
}

/// Length of a `"""..."""` triple-quoted raw string starting at
/// `bytes[0..3] == b""\""""`. No escape / no interpolation. Unterminated
/// runs to EOF.
fn scan_triple_string_len(bytes: &[u8]) -> usize {
    let mut p = 3usize; // past opening `"""`
    while p + 2 < bytes.len() {
        if bytes[p] == b'"' && bytes[p + 1] == b'"' && bytes[p + 2] == b'"' {
            return p + 3; // include closing `"""`
        }
        p += 1;
    }
    // Closing `"""` exactly at the tail (p == len-3 → p+2 == len-1, which
    // is excluded by the loop's `< len` condition). Re-check explicitly.
    if p + 2 == bytes.len() - 1
        && bytes.len() >= 6
        && bytes[bytes.len() - 3] == b'"'
        && bytes[bytes.len() - 2] == b'"'
        && bytes[bytes.len() - 1] == b'"'
    {
        return bytes.len();
    }
    // Unterminated — return what we have (roundtrip still holds).
    bytes.len()
}

/// Length of a `'...'` char literal starting at `bytes[0] == b'\''`.
/// Handles `\\` escapes. Stops at EOL or EOF (best-effort if
/// unterminated) — newlines can't be inside a char literal.
fn scan_char_len(bytes: &[u8]) -> usize {
    let mut p = 1usize; // past opening `'`
    while p < bytes.len() {
        let b = bytes[p];
        if b == b'\\' && p + 1 < bytes.len() {
            p += 2;
            continue;
        }
        if b == b'\'' {
            return p + 1; // include closing `'`
        }
        if b == b'\n' || b == b'\r' {
            // Char literal can't span lines — stop here (best-effort).
            return p;
        }
        p += 1;
    }
    p
}

/// Length of a `r"..."` raw string starting at `bytes[0] == b'r'`. No
/// escape / no interpolation — first `"` after the opening wins.
/// Unterminated runs to EOF.
fn scan_raw_string_len(bytes: &[u8]) -> usize {
    let mut p = 2usize; // past `r"`
    while p < bytes.len() {
        if bytes[p] == b'"' {
            return p + 1; // include closing `"`
        }
        p += 1;
    }
    p
}

// ---------------------------------------------------------------------------
// Unit-level smoke tests (most lossless tests live in
// crates/buff-lang-ast/tests/lossless_tests.rs).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_empty_source_roundtrips() {
        let tree = parse_lossless("");
        assert_eq!(tree.to_source(), "");
        assert_eq!(tree.pieces_to_source(), "");
        assert_eq!(tree.piece_count(), 0);
    }

    #[test]
    fn lossless_single_token_roundtrips() {
        let tree = parse_lossless("foo");
        assert_eq!(tree.to_source(), "foo");
        assert_eq!(tree.token_count(), 1);
        assert_eq!(tree.trivia_count(), 0);
    }

    #[test]
    fn lossless_whitespace_only_roundtrips() {
        let src = "   \t  ";
        let tree = parse_lossless(src);
        assert_eq!(tree.to_source(), src);
        assert_eq!(tree.pieces_to_source(), src);
        assert_eq!(tree.token_count(), 0);
        assert_eq!(tree.trivia_count(), 1);
    }

    #[test]
    fn lossless_comment_preservation_basic() {
        let src = "// hello\nfoo\n";
        let tree = parse_lossless(src);
        assert_eq!(tree.to_source(), src);
        assert_eq!(tree.comment_count(), 1);
        let comments: Vec<_> = tree.comments().collect();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text(), "// hello");
    }
}
