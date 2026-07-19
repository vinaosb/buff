//! Per-document state — text + analysis cached on each `didChange`.
//!
//! [`DocumentState`] is the LSP-internal mirror of an open Buff file. It
//! stores the latest text, a [`LineIndex`] for byte ↔ LSP-position
//! translation, and the most recent [`DocumentAnalysis`] produced by
//! running the front-end over the text.

use buff_lang_error::SourceId;

use crate::analysis::{analyze, DocumentAnalysis};
use crate::position::LineIndex;

/// The state of one open Buff document.
#[derive(Debug, Clone)]
pub struct DocumentState {
    /// The latest text (LSP text documents are owned by the server per
    /// open file; we keep a full snapshot since the LSP `didChange` events
    /// send either full or incremental edits, and v1.2 only supports the
    /// full-sync mode declared at startup).
    pub text: String,
    /// Per-URI stable source id. Used in every Span the analysis emits so
    /// goto-def can map a Span back to this document.
    pub source_id: SourceId,
    /// Version counter from LSP (the `didOpen` / `didChange` version
    /// field). Echoed back to the client in diagnostics.
    pub version: Option<i32>,
    /// Cached analysis. Refreshed by [`DocumentState::reanalyze`].
    pub analysis: DocumentAnalysis,
    /// Cached line index. Derived from `text`; rebuilt on every reanalyze.
    pub lines: LineIndex,
}

impl DocumentState {
    /// Create a fresh state for `text`, immediately running the analysis.
    pub fn new(text: String, source_id: SourceId, version: Option<i32>) -> Self {
        let lines = LineIndex::new(&text);
        let analysis = analyze(&text, source_id);
        Self {
            text,
            source_id,
            version,
            analysis,
            lines,
        }
    }

    /// Replace the text + version and rebuild the line index + analysis.
    pub fn update(&mut self, text: String, version: Option<i32>) {
        self.text = text;
        self.version = version;
        self.lines = LineIndex::new(&self.text);
        self.analysis = analyze(&self.text, self.source_id);
    }

    /// Force a re-analysis of the current text (used by tests after
    /// manually mutating state).
    pub fn reanalyze(&mut self) {
        self.lines = LineIndex::new(&self.text);
        self.analysis = analyze(&self.text, self.source_id);
    }
}
