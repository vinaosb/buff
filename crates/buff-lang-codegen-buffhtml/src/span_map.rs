//! Post-format span side-table — maps generated `.rs` line:col positions back
//! to `.buffhtml` byte spans.
//!
//! # Strategy
//!
//! Implements the spike-recommended approach (evidence file
//! `.sisyphus/evidence/task-133-span-mapping-spike.txt` §"RECOMMENDED
//! SPAN-MAPPING STRATEGY"):
//!
//! 1. During codegen, every lowered construct records an **anchor**: a tuple
//!    of `(anchor_text, buffhtml_span)`. `anchor_text` is a stable substring
//!    (typically an identifier or literal fragment) that prettyplease will
//!    NOT modify during formatting.
//! 2. After `prettyplease::unparse` produces the final `.rs` text, the
//!    [`SpanMapBuilder::finalize`] pass scans the text line-by-line, finding
//!    each anchor's `(line, col)` in the formatted output. The result is a
//!    sorted `Vec<(RsLineCol, BuffHtmlSpan)>` — the [`SpanMap`].
//! 3. At diagnostic-rendering time, the CLI error_mapper binary-searches the
//!    sorted table for the largest `(RsLineCol ≤ error)`, then emits the
//!    `.buffhtml` span as the diagnostic's primary location.
//!
//! This is "post-format text-region reverse mapping" (spike terminology) and
//! is strictly better than the T121b filename-translation-only fallback.

use buff_lang_error::Span;

/// 1-based line and column in the generated `.rs` text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RsLineCol {
    pub line: usize,
    pub col: usize,
}

/// Reverse-mapping table from generated `.rs` positions to `.buffhtml` spans.
///
/// Built by [`SpanMapBuilder::finalize`]. Look up via [`SpanMap::map_span`].
#[derive(Debug, Clone, Default)]
pub struct SpanMap {
    /// Sorted ascending by `RsLineCol`. Used for binary-search lookup.
    anchors: Vec<(RsLineCol, Span)>,
}

impl SpanMap {
    /// Map a generated `.rs` (line, col) to the originating `.buffhtml` span.
    ///
    /// Returns `Some(span)` for the largest `RsLineCol ≤ query` in the table
    /// (i.e. the nearest preceding anchor). Returns `None` if the table is
    /// empty or all anchors come after `query`.
    ///
    /// `line` and `col` are both 1-based to match rustc diagnostic output.
    pub fn map_span(&self, line: usize, col: usize) -> Option<Span> {
        let target = RsLineCol { line, col };
        // Binary search for the largest anchor `<= target`.
        let idx = self
            .anchors
            .partition_point(|(lc, _)| *lc <= target)
            .checked_sub(1)?;
        Some(self.anchors[idx].1)
    }

    /// Number of anchors recorded (useful for tests / diagnostics).
    pub fn len(&self) -> usize {
        self.anchors.len()
    }

    /// Whether the map contains no anchors.
    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    /// Borrow the raw sorted anchors (for tests + evidence reports).
    pub fn anchors(&self) -> &[(RsLineCol, Span)] {
        &self.anchors
    }
}

/// Incremental builder used during codegen — call [`Self::add_anchor`] while
/// lowering each AST node, then [`Self::finalize`] once the `.rs` text exists.
#[derive(Debug, Default, Clone)]
pub struct SpanMapBuilder {
    raw_anchors: Vec<RawAnchor>,
}

#[derive(Debug, Clone)]
struct RawAnchor {
    /// Identifier or literal text that should appear verbatim in the
    /// formatted `.rs` output. The finalize pass searches for this string
    /// (first occurrence on or after the previously-found line) to locate
    /// the anchor's `.rs` position.
    anchor_text: String,
    buffhtml_span: Span,
}

impl SpanMapBuilder {
    /// Record an anchor. The `anchor_text` should be a token that
    /// prettyplease will emit verbatim — typically an identifier, a number,
    /// or a string-literal body. Avoid whitespace-fragile substrings.
    pub fn add_anchor(&mut self, anchor_text: &str, buffhtml_span: Span) {
        // Skip empty / whitespace-only anchors (no stable search target).
        if anchor_text.trim().is_empty() {
            return;
        }
        self.raw_anchors.push(RawAnchor {
            anchor_text: anchor_text.to_string(),
            buffhtml_span,
        });
    }

    /// Scan the formatted `.rs` source for each anchor's position, then build
    /// the sorted [`SpanMap`].
    ///
    /// If an anchor's text is not found (rare — would indicate prettyplease
    /// transformed it), it is silently dropped. The downstream diagnostic
    /// mapper falls back to the enclosing `rsx!{}` block's span in that case
    /// (TODO(buffhtml-span) marker per Oracle §5 mitigation #3).
    pub fn finalize(self, rs_source: &str) -> SpanMap {
        let lines: Vec<&str> = rs_source.lines().collect();
        let mut found: Vec<(RsLineCol, Span)> = Vec::with_capacity(self.raw_anchors.len());

        // Two-pointer scan: anchors are recorded in source order; we expect
        // their .rs positions to also be in ascending order. Track the line
        // index of the most recent match so subsequent searches start there.
        let mut search_from_line = 0usize;
        for raw in &self.raw_anchors {
            let mut located: Option<RsLineCol> = None;
            // Search lines from `search_from_line` onward, looking for
            // `anchor_text` as a substring of the line.
            for (i, line) in lines.iter().enumerate().skip(search_from_line) {
                if let Some(col) = line.find(&raw.anchor_text) {
                    located = Some(RsLineCol {
                        line: i + 1,  // 1-based
                        col: col + 1, // 1-based
                    });
                    search_from_line = i; // don't go back
                    break;
                }
            }
            // Fallback: global search from the start (in case anchors are
            // out of order or prettyplease reordered them).
            if located.is_none() {
                for (i, line) in lines.iter().enumerate() {
                    if let Some(col) = line.find(&raw.anchor_text) {
                        located = Some(RsLineCol {
                            line: i + 1,
                            col: col + 1,
                        });
                        break;
                    }
                }
            }
            if let Some(lc) = located {
                found.push((lc, raw.buffhtml_span));
            }
            // Else: drop the anchor — see method doc.
        }

        found.sort_by_key(|(lc, _)| *lc);
        // Deduplicate by RsLineCol (keep the first occurrence).
        found.dedup_by_key(|(lc, _)| *lc);

        SpanMap { anchors: found }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::SourceId;

    fn span(start: usize, end: usize) -> Span {
        Span::new(start, end, SourceId(0))
    }

    #[test]
    fn empty_builder_yields_empty_map() {
        let b = SpanMapBuilder::default();
        let m = b.finalize("fn main() {}");
        assert!(m.is_empty());
        assert!(m.map_span(1, 1).is_none());
    }

    #[test]
    fn map_span_finds_nearest_preceding_anchor() {
        let mut b = SpanMapBuilder::default();
        b.add_anchor("foo", span(10, 20));
        b.add_anchor("bar", span(30, 40));
        let m = b.finalize("let foo = 1;\nlet bar = 2;\n");
        // foo is at line 1 col 5; bar at line 2 col 5.
        // Query (1, 1) → no preceding anchor (None).
        // Query (1, 5) → foo.
        // Query (1, 6) → foo (still nearest preceding).
        // Query (2, 1) → foo (nearest preceding).
        // Query (2, 5) → bar.
        // Query (3, 1) → bar.
        assert_eq!(m.map_span(1, 1), None);
        assert_eq!(m.map_span(1, 5), Some(span(10, 20)));
        assert_eq!(m.map_span(1, 6), Some(span(10, 20)));
        assert_eq!(m.map_span(2, 1), Some(span(10, 20)));
        assert_eq!(m.map_span(2, 5), Some(span(30, 40)));
        assert_eq!(m.map_span(3, 1), Some(span(30, 40)));
    }

    #[test]
    fn missing_anchor_text_is_silently_dropped() {
        let mut b = SpanMapBuilder::default();
        b.add_anchor("present", span(1, 10));
        b.add_anchor("absent_invisible_string_xyz", span(20, 30));
        let m = b.finalize("let present = 1;");
        assert_eq!(m.len(), 1);
        assert_eq!(m.map_span(1, 5), Some(span(1, 10)));
    }

    #[test]
    fn whitespace_only_anchor_is_ignored() {
        let mut b = SpanMapBuilder::default();
        b.add_anchor("   ", span(1, 10));
        let m = b.finalize("x");
        assert!(m.is_empty());
    }

    #[test]
    fn out_of_order_anchors_sorted_ascending() {
        let mut b = SpanMapBuilder::default();
        // `bar` comes first in source, `foo` second, but both are present.
        b.add_anchor("bar", span(1, 10));
        b.add_anchor("foo", span(20, 30));
        let m = b.finalize("foo\nbar\n");
        // Both should be located, and the sorted order should be ascending.
        let anchors = m.anchors();
        assert_eq!(anchors.len(), 2);
        assert!(anchors[0].0 <= anchors[1].0);
    }
}
