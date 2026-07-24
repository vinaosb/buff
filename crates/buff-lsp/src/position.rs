//! Position translation between Buff byte offsets and LSP `Position`s.
//!
//! The LSP spec ([`Position`]) measures columns in **UTF-16 code units**, not
//! bytes and not Unicode scalar values. Buff's [`SourceFile::lookup`] returns
//! `(line, col)` in characters (scalar values). The two are equivalent for
//! the BMP range (1 char = 1 UTF-16 unit) but diverge for astral characters
//! (`🚀`, `𝕏`, …) which are 1 char = 2 UTF-16 units.
//!
//! [`Position`]: lsp_types::Position
//! [`SourceFile::lookup`]: buff_lang_error::SourceFile
//!
//! We translate Buff's character-based column to UTF-16 by counting how many
//! UTF-16 code units precede the offset within its line. We translate LSP
//! `Position` to a byte offset by walking the source line and finding the
//! byte position whose UTF-16 prefix length matches the requested column.
//!
//! # Why not reuse `SourceMap`?
//!
//! `SourceMap::lookup` returns character-based columns; LSP needs UTF-16.
//! Doing the conversion via `SourceMap` would lose the UTF-16 distinction
//! for astral characters. The line-index computation is the same algorithm
//! (`compute_line_starts`), so this module is the LSP-specific counterpart
//! of `buff_lang_error::source_map`.

/// Per-file line metadata cached for fast byte ↔ LSP-position conversion.
///
/// Build one of these per `didOpen` / `didChange`, store it alongside the
/// document text, and use it for every LSP request that touches positions.
///
/// Cloning is cheap (line_starts is a `Vec<usize>`); for repeated queries on
/// the same snapshot, hold one [`LineIndex`] and call its methods.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the start of each line. Line 0 starts at byte 0.
    /// Always non-empty: an empty source has `line_starts = vec![0]`.
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a [`LineIndex`] over the given source text.
    ///
    /// Newlines follow the same rules as
    /// [`SourceFile::new`](buff_lang_error::SourceFile::new): a single
    /// `\n` (or `\r\n`) terminates a line. Bare `\r` is also counted as a
    /// line break to stay forgiving with classic Mac files.
    pub fn new(src: &str) -> Self {
        let mut starts = vec![0usize];
        let bytes = src.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'\n' {
                starts.push(i + 1);
                i += 1;
            } else if b == b'\r' {
                // CRLF: line start is after the `\n`. Bare CR: after the CR.
                let next = if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    i + 2
                } else {
                    i + 1
                };
                starts.push(next);
                i = next;
            } else {
                i += 1;
            }
        }
        Self {
            line_starts: starts,
        }
    }

    /// Number of lines in the indexed source (1-based count; an empty
    /// source returns 1).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Find the line index (0-based) containing `byte_offset`. Clamped to
    /// `[0, line_count-1]`. `byte_offset` values past EOF map to the last
    /// line.
    pub fn line_of(&self, byte_offset: usize) -> usize {
        match self.line_starts.binary_search(&byte_offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1).min(self.line_starts.len() - 1),
        }
    }

    /// The byte offset where `line` (0-based) begins. Clamped to the last
    /// line if out of range.
    pub fn line_start(&self, line: usize) -> usize {
        let idx = line.min(self.line_starts.len() - 1);
        self.line_starts[idx]
    }

    /// Convenience: the byte offset where the line containing `byte_offset`
    /// begins. Used by inlay-hint computation to slice a line out of the
    /// source for cheap text checks (T46).
    pub fn line_start_byte(&self, src: &str, byte_offset: usize) -> usize {
        let _ = src;
        self.line_start(self.line_of(byte_offset))
    }

    /// Convenience: the byte offset just past the end of the line
    /// containing `byte_offset` (exclusive of the trailing newline).
    pub fn line_end_byte(&self, src: &str, byte_offset: usize) -> usize {
        self.line_end(src, self.line_of(byte_offset))
    }

    /// The byte offset just past the end of `line` (exclusive): the next
    /// line's start, or source length for the last line. Excludes the
    /// trailing newline.
    fn line_end(&self, src: &str, line: usize) -> usize {
        let next = line + 1;
        if next < self.line_starts.len() {
            // The next line's start is one past the newline; back up over
            // the newline byte(s).
            let nl_pos = self.line_starts[next].saturating_sub(1);
            // Handle CRLF: back up over the \r if present.
            let bytes = src.as_bytes();
            let adjusted = if nl_pos > 0 && bytes.get(nl_pos.saturating_sub(1)) == Some(&b'\r') {
                nl_pos - 1
            } else {
                nl_pos
            };
            adjusted
        } else {
            src.len()
        }
    }

    /// Convert a byte offset into the source to an LSP [`Position`].
    ///
    /// The column is the count of UTF-16 code units from the line start to
    /// the offset (0-based), per the LSP spec.
    pub fn lsp_position(&self, src: &str, byte_offset: usize) -> lsp_types::Position {
        let off = byte_offset.min(src.len());
        let line = self.line_of(off);
        let line_start = self.line_start(line);
        let col_utf16 = utf16_length(&src[line_start..off]);
        lsp_types::Position {
            line: line as u32,
            character: col_utf16 as u32,
        }
    }

    /// Convert an LSP [`Position`] to a byte offset in the source.
    ///
    /// Walks the line containing `position.line` and finds the byte whose
    /// UTF-16 prefix length matches `position.character`. Out-of-range
    /// characters are clamped to line end.
    pub fn byte_offset(&self, src: &str, position: lsp_types::Position) -> usize {
        let line = (position.line as usize).min(self.line_starts.len() - 1);
        let line_start = self.line_start(line);
        let line_end_excl = self.line_end(src, line);
        let line_slice = &src[line_start..line_end_excl];

        // Walk the slice accumulating UTF-16 units; stop at the target.
        let target = position.character as usize;
        let mut utf16_so_far = 0usize;
        let mut byte_cursor = 0usize;
        for (byte_idx, ch) in line_slice.char_indices() {
            if utf16_so_far >= target {
                break;
            }
            utf16_so_far += ch.len_utf16();
            byte_cursor = byte_idx + ch.len_utf8();
        }
        line_start + byte_cursor
    }

    /// Convert a Buff [`Span`](buff_lang_error::Span) to an LSP [`Range`].
    pub fn lsp_range(&self, src: &str, span: buff_lang_error::Span) -> lsp_types::Range {
        let start = self.lsp_position(src, span.start);
        let end = self.lsp_position(src, span.end);
        lsp_types::Range::new(start, end)
    }
}

/// Count the number of UTF-16 code units required to encode `s`.
fn utf16_length(s: &str) -> usize {
    s.chars().map(|c| c.len_utf16()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::{SourceId, Span};

    #[test]
    fn empty_source_has_one_line() {
        let idx = LineIndex::new("");
        assert_eq!(idx.line_count(), 1);
    }

    #[test]
    fn ascii_position_round_trip() {
        let src = "let x = 42";
        let idx = LineIndex::new(src);
        // Byte offset 4 = the `x`.
        let pos = idx.lsp_position(src, 4);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 4);
        // Round trip.
        assert_eq!(idx.byte_offset(src, pos), 4);
    }

    #[test]
    fn multibyte_utf16_column() {
        // 'é' is 2 bytes UTF-8 but 1 UTF-16 unit. '🚀' is 4 bytes UTF-8 but
        // 2 UTF-16 units (surrogate pair).
        let src = "é🚀x";
        let idx = LineIndex::new(src);
        // Byte offset 6 = 'x' (after é=2 bytes + 🚀=4 bytes).
        let pos = idx.lsp_position(src, 6);
        // Column in UTF-16: é=1 unit, 🚀=2 units → x is at col 3.
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 3);
        // Round trip back to byte 6.
        assert_eq!(idx.byte_offset(src, pos), 6);
    }

    #[test]
    fn multiline_offsets() {
        let src = "aaa\nbbb\nccc";
        let idx = LineIndex::new(src);
        // Byte offset 5 = the second `b` of "bbb" (byte 4='b', 5='b', 6='b').
        let pos = idx.lsp_position(src, 5);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 1);
        assert_eq!(idx.byte_offset(src, pos), 5);
    }

    #[test]
    fn position_past_eof_clamps() {
        let src = "abc";
        let idx = LineIndex::new(src);
        let pos = idx.lsp_position(src, 999);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 3);
        // Round-trip past end clamps to end-of-source.
        let far = lsp_types::Position {
            line: 99,
            character: 99,
        };
        assert_eq!(idx.byte_offset(src, far), 3);
    }

    #[test]
    fn range_from_span() {
        let src = "hello\nworld";
        let idx = LineIndex::new(src);
        let span = Span::new(7, 10, SourceId(0)); // "orl" of "world"
        let range = idx.lsp_range(src, span);
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 1);
        assert_eq!(range.end.line, 1);
        assert_eq!(range.end.character, 4);
    }

    #[test]
    fn crlf_line_endings() {
        let src = "aaa\r\nbbb";
        let idx = LineIndex::new(src);
        // "bbb" starts at byte 5 (after `aaa\r\n`).
        assert_eq!(idx.line_start(1), 5);
        let pos = idx.lsp_position(src, 6);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 1);
    }

    #[test]
    fn utf16_length_basic() {
        assert_eq!(utf16_length(""), 0);
        assert_eq!(utf16_length("abc"), 3);
        assert_eq!(utf16_length("é"), 1);
        assert_eq!(utf16_length("🚀"), 2);
        assert_eq!(utf16_length("é🚀x"), 4);
    }
}
