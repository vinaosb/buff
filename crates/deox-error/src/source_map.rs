//! Source map — maps source IDs to file content and provides line/column lookup.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::span::{ByteOffset, SourceId};

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
#[derive(Debug, Clone)]
pub struct SourceMap {
    sources: HashMap<SourceId, SourceFile>,
}

impl SourceMap {
    /// Create an empty source map.
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
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
