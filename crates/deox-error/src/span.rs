//! Span types — byte offsets, source IDs, and source spans.

/// A byte offset into a source file.
pub type ByteOffset = usize;

/// A unique identifier for a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId(pub u32);

/// A span of source code, identified by byte offsets and a source file ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: ByteOffset,
    pub end: ByteOffset,
    pub source_id: SourceId,
}

impl Span {
    /// Create a new span from byte offsets and a source ID.
    pub fn new(start: ByteOffset, end: ByteOffset, source_id: SourceId) -> Self {
        Self {
            start,
            end,
            source_id,
        }
    }

    /// Create a dummy span (used for synthetic nodes or error recovery).
    pub fn dummy() -> Self {
        Self {
            start: 0,
            end: 0,
            source_id: SourceId(0),
        }
    }
}
