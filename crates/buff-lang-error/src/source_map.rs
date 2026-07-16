//! Source map — maps source IDs to file content and provides line/column lookup.
//!
//! [`SourceMap`] serves two roles:
//!
//! 1. **Front-end** — maps [`SourceId`] → [`SourceFile`] so the compiler can
//!    resolve byte offsets to 1-based `(line, col)` pairs for diagnostics.
//! 2. **Back-end** (T16) — maps Buff [`Span`]s ↔ generated-Rust line numbers
//!    so that `rustc` and runtime errors that reference the intermediate `.rs`
//!    file can be translated back to the original `.buff` source location.
//!
//! The back-end mapping is populated during codegen (see
//! [`CodegenContext::record_mapping`][ccrm]) and consumed by
//! `buff_lang_cli::error_mapper`.
//!
//! [ccrm]: ../../buff_lang_codegen_rust/context/struct.CodegenContext.html#method.record_mapping

use std::collections::HashMap;
use std::path::PathBuf;

use crate::span::{ByteOffset, SourceId, Span};

/// A source file with cached line-start byte offsets.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub content: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    /// Create a new source file, computing line-start offsets.
    pub fn new(path: PathBuf, content: String) -> Self {
        let line_starts = compute_line_starts(&content);
        Self {
            path,
            content,
            line_starts,
        }
    }

    /// Look up the 1-based line and column for a given byte offset.
    ///
    /// Column is measured in **characters** (not bytes), so multi-byte UTF-8
    /// sequences count as a single column.
    pub fn lookup(&self, offset: ByteOffset) -> Option<(usize, usize)> {
        if offset > self.content.len() {
            return None;
        }

        // Binary search for the line containing this offset.
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };

        let line_start = self.line_starts[line_idx];
        let line = line_idx + 1; // 1-based

        // Count characters (not bytes) from line_start to offset.
        let col = self.content[line_start..offset].chars().count() + 1; // 1-based

        Some((line, col))
    }
}

/// Compute the byte offset of each line start in a string.
fn compute_line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, c) in content.char_indices() {
        if c == '\n' {
            starts.push(i + c.len_utf8());
        }
    }
    starts
}

/// A collection of source files indexed by [`SourceId`].
///
/// In addition to front-end file/offset lookup, the map stores a **bidirectional
/// Buff ↔ Rust line mapping** (T16). Each entry records that a Buff [`Span`]
/// corresponds to a particular 1-based line in the generated Rust source, so
/// that `rustc` diagnostics and runtime panics referencing the `.rs` file can
/// be translated back to the original `.buff` location.
///
/// The mapping is populated during codegen via [`SourceMap::add_mapping`].
#[derive(Debug, Clone)]
pub struct SourceMap {
    sources: HashMap<SourceId, SourceFile>,

    /// Rust line (1-based) → Buff [`Span`]. Populated during codegen.
    rust_to_buff: HashMap<usize, Span>,

    /// Buff [`Span`] → Rust line (1-based). Reverse of [`SourceMap::rust_to_buff`].
    buff_to_rust: HashMap<Span, usize>,
}

impl SourceMap {
    /// Create an empty source map.
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
            rust_to_buff: HashMap::new(),
            buff_to_rust: HashMap::new(),
        }
    }

    /// Add a source file with the given ID, path, and content.
    pub fn add_source(&mut self, id: SourceId, path: PathBuf, content: String) {
        self.sources.insert(id, SourceFile::new(path, content));
    }

    /// Look up the 1-based line and column for a byte offset in the given source.
    pub fn lookup(&self, id: SourceId, offset: ByteOffset) -> Option<(usize, usize)> {
        self.sources.get(&id).and_then(|sf| sf.lookup(offset))
    }

    // -----------------------------------------------------------------
    // Buff ↔ Rust line mapping (T16)
    // -----------------------------------------------------------------

    /// Record that a Buff `span` maps to a specific 1-based `rust_line` in the
    /// generated Rust source.
    ///
    /// If the same `rust_line` or `span` was previously recorded, the later
    /// call wins (the previous entry is overwritten).
    pub fn add_mapping(&mut self, buff_span: Span, rust_line: usize) {
        self.rust_to_buff.insert(rust_line, buff_span);
        self.buff_to_rust.insert(buff_span, rust_line);
    }

    /// Given a Rust line number, return the corresponding Buff [`Span`].
    ///
    /// Returns an exact match when `rust_line` was recorded via
    /// [`add_mapping`](Self::add_mapping). Otherwise, falls back to the
    /// **closest recorded line at or below** `rust_line` — this mirrors how
    /// `rustc`/panic locations point at the *start* of the statement that
    /// failed, so the nearest mapped statement above is the best candidate.
    ///
    /// Returns `None` when no mapping at or below `rust_line` exists.
    pub fn lookup_buff(&self, rust_line: usize) -> Option<Span> {
        if let Some(s) = self.rust_to_buff.get(&rust_line) {
            return Some(*s);
        }
        // Find the closest recorded line at or below `rust_line`.
        self.rust_to_buff
            .iter()
            .filter(|(rl, _)| **rl <= rust_line)
            .max_by_key(|(rl, _)| **rl)
            .map(|(_, s)| *s)
    }

    /// Given a Buff `span`, return the corresponding 1-based Rust line number.
    ///
    /// Returns `None` if no mapping was recorded for this exact span.
    pub fn lookup_rust(&self, buff_span: Span) -> Option<usize> {
        self.buff_to_rust.get(&buff_span).copied()
    }

    /// Returns `true` if no Buff ↔ Rust line mappings have been recorded.
    pub fn is_line_map_empty(&self) -> bool {
        self.rust_to_buff.is_empty()
    }
}

impl Default for SourceMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_line_starts_single_line() {
        let starts = compute_line_starts("hello world");
        assert_eq!(starts, vec![0]);
    }

    #[test]
    fn compute_line_starts_multi_line() {
        let starts = compute_line_starts("line1\nline2\nline3");
        assert_eq!(starts, vec![0, 6, 12]);
    }

    #[test]
    fn compute_line_starts_trailing_newline() {
        let starts = compute_line_starts("a\nb\n");
        assert_eq!(starts, vec![0, 2, 4]);
    }
}
