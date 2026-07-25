//! T1 (v1.25 Wave 0) — JSON diagnostic serialization.
//!
//! [`to_json`] resolves a [`Diagnostic`] against a source string and emits
//! a [`DiagnosticJson`] that's directly serializable to JSON via
//! `serde_json::to_string`. Consumed by:
//!
//! - `buff check --error-format json` (CLI tooling — see
//!   `buff-lang-cli::commands::json_diagnostics`)
//! - LSP CodeAction conversion (see `buff-lsp::handlers`)
//! - Future tooling (CI integrations, custom reporters)
//!
//! # Shape
//!
//! ```jsonc
//! {
//!   "code":      "E1001" | null,
//!   "severity":  "Error" | "Warning" | "Info",
//!   "message":   "unexpected character: '@'",
//!   "spans": [
//!     {
//!       "style":      "Primary" | "Secondary",
//!       "label":      "..." | null,
//!       "byte_start": 8,
//!       "byte_end":   9,
//!       "line_start": 1,
//!       "col_start":  9,
//!       "line_end":   1,
//!       "col_end":    10
//!     },
//!     ...
//!   ],
//!   "notes":       ["...", ...],
//!   "suggestions": [
//!     {
//!       "byte_start":   0,
//!       "byte_end":     5,
//!       "replacement":  "print",
//!       "applicability":"MachineApplicable",
//!       "label":        "change `pritn` to `print`" | null
//!     },
//!     ...
//!   ]
//! }
//! ```
//!
//! # Design notes
//!
//! - **Line/column resolution happens here**, not in the consumer. The
//!   consumer passes raw `&str` source text; the JSON shape carries both
//!   byte offsets (for tooling that wants to map back to source) and
//!   1-based `(line, col)` (for human-readable tooling like a CI report).
//!   Mirrors `source_map.rs::SourceFile::lookup` — character-based columns
//!   (multi-byte UTF-8 counts as 1 col).
//! - **The primary span becomes the first entry in `spans`** with
//!   `style: "Primary"` and `label: null` (no per-primary label field on
//!   `Diagnostic`; the message already names the location). Additional
//!   `Diagnostic::labels` are appended in declaration order.
//! - **Out-of-bounds spans** (e.g. EOF errors with synthetic spans past
//!   the end of source) emit `byte_start`/`byte_end` unchanged but
//!   `line_*`/`col_*` as `null` (so JSON consumers can still see the byte
//!   offset without crashing on the lookup).
//! - **Stable**: the JSON shape is part of the v1.25+ public tooling
//!   contract — once shipped, field names + nesting are stable. Adding
//!   optional fields later is fine; renaming or removing is not.

use serde::Serialize;

use crate::code::ErrorCode;
use crate::diagnostic::{
    Applicability, CodeSuggestion, Diagnostic, LabelStyle, Severity, SpanLabel,
};
use crate::span::Span;

// ---------------------------------------------------------------------------
// Public JSON shape (serde derives — stable across releases)
// ---------------------------------------------------------------------------

/// Top-level JSON shape for one serialized [`Diagnostic`].
///
/// See the [module docs](self) for the full schema. Build via [`to_json`]
/// (which resolves byte offsets to line/col against source text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticJson {
    /// Stable error code (`"E1001"`) or `null` when the diagnostic has no
    /// code attached.
    pub code: Option<String>,
    /// `"Error"` / `"Warning"` / `"Info"` (matches `Severity`'s `Debug`
    /// rendering so consumers can compare against the enum name).
    pub severity: String,
    /// The diagnostic message (no code prefix; consumers format as needed).
    pub message: String,
    /// All spans the diagnostic points at — primary first, then secondary
    /// labels in declaration order. Each entry carries byte offsets AND
    /// 1-based `(line, col)` (or `null` line/col for out-of-bounds spans).
    pub spans: Vec<SpanJson>,
    /// Free-form note lines (rendered as `note: ...` in the human-readable
    /// format).
    pub notes: Vec<String>,
    /// Machine-readable fix suggestions (rendered as `help: ...` lines in
    /// the human-readable format).
    pub suggestions: Vec<SuggestionJson>,
}

/// JSON shape for one labeled span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpanJson {
    /// `"Primary"` (`^^^`) or `"Secondary"` (`~~~`) — controls caret char
    /// in the human-readable render.
    pub style: String,
    /// Per-span label (e.g. `"declared here"`, `"expected `Int`"`). `null`
    /// for the primary span (the diagnostic message names the location).
    pub label: Option<String>,
    /// Byte offset of the span start in source.
    pub byte_start: usize,
    /// Byte offset of the span end (exclusive) in source.
    pub byte_end: usize,
    /// 1-based line of `byte_start`. `null` when `byte_start` lies past the
    /// end of source (out-of-bounds span).
    pub line_start: Option<usize>,
    /// 1-based column (character-based, mirroring `SourceFile::lookup`) of
    /// `byte_start`. `null` when out of bounds.
    pub col_start: Option<usize>,
    /// 1-based line of `byte_end`. `null` when out of bounds.
    pub line_end: Option<usize>,
    /// 1-based column of `byte_end`. `null` when out of bounds.
    pub col_end: Option<usize>,
}

/// JSON shape for one fix suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuggestionJson {
    /// Byte offset of the span to replace.
    pub byte_start: usize,
    /// Byte offset (exclusive) of the span to replace.
    pub byte_end: usize,
    /// Replacement text to splice in.
    pub replacement: String,
    /// `"MachineApplicable"` / `"MaybeIncorrect"` / `"HasPlaceholders"` /
    /// `"Unspecified"`.
    pub applicability: String,
    /// Human-readable label (`"change `pritn` to `print`"`) or `null`.
    pub label: Option<String>,
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

/// Resolve a [`Diagnostic`] against `source` and produce the serializable
/// [`DiagnosticJson`] shape.
///
/// `source` is the raw text the diagnostic was emitted against (the same
/// `&str` you'd pass to [`Diagnostic::render`]). Byte offsets in the
/// diagnostic's spans must be relative to this string.
///
/// See the [module docs](self) for the full JSON schema. Use
/// `serde_json::to_string(&json)` to serialize.
pub fn to_json(diag: &Diagnostic, source: &str) -> DiagnosticJson {
    let mut spans: Vec<SpanJson> = Vec::with_capacity(1 + diag.labels.len());

    // Primary span — first entry, no per-span label.
    spans.push(span_to_json(&diag.span, LabelStyle::Primary, None, source));

    // Secondary / additional primary labels — in declaration order.
    for label in &diag.labels {
        spans.push(span_to_json(
            &label.span,
            label.style,
            Some(&label.label),
            source,
        ));
    }

    let suggestions: Vec<SuggestionJson> =
        diag.suggestions.iter().map(suggestion_to_json).collect();

    DiagnosticJson {
        code: diag.code.map(|c| c.code_str().to_string()),
        severity: severity_str(diag.severity).to_string(),
        message: diag.message.clone(),
        spans,
        notes: diag.notes.clone(),
        suggestions,
    }
}

/// Serialize a list of diagnostics to a single JSON array string. Each
/// entry is one [`DiagnosticJson`] (see [`to_json`]).
///
/// Convenience wrapper around [`to_json`] + `serde_json::to_string` for
/// the common case (`buff check --error-format json` emits this on stdout).
/// Returns the serialized JSON string on success.
///
/// `serde_json::to_string` never fails on our shape (no maps, no floats,
/// no custom serializers that can error); we surface the result rather
/// than panicking so callers retain control over error handling.
pub fn render_diagnostics_json(diagnostics: &[Diagnostic], source: &str) -> String {
    let arr: Vec<DiagnosticJson> = diagnostics.iter().map(|d| to_json(d, source)).collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn span_to_json(span: &Span, style: LabelStyle, label: Option<&str>, source: &str) -> SpanJson {
    let (line_start, col_start) = lookup_line_col(source, span.start);
    let (line_end, col_end) = lookup_line_col(source, span.end);
    SpanJson {
        style: label_style_str(style).to_string(),
        label: label.map(|s| s.to_string()),
        byte_start: span.start,
        byte_end: span.end,
        line_start,
        col_start,
        line_end,
        col_end,
    }
}

fn suggestion_to_json(s: &CodeSuggestion) -> SuggestionJson {
    SuggestionJson {
        byte_start: s.span.start,
        byte_end: s.span.end,
        replacement: s.replacement.clone(),
        applicability: applicability_str(s.applicability).to_string(),
        label: s.label.clone(),
    }
}

/// Resolve a byte offset to 1-based `(line, col)` against `source`, or
/// `(None, None)` when the offset lies past the end of source.
///
/// Mirrors `SourceFile::lookup` (binary search on cached line starts +
/// character-based column) but is self-contained (no SourceMap needed) so
/// `to_json` stays a pure function of `(Diagnostic, &str)`.
fn lookup_line_col(source: &str, offset: usize) -> (Option<usize>, Option<usize>) {
    if offset > source.len() {
        return (None, None);
    }
    // Count '\n' before `offset` → 0-based line index.
    let bytes_before = &source[..offset];
    let line_idx = bytes_before.matches('\n').count();
    let line = line_idx + 1; // 1-based.

    // Walk back to the start of this line.
    let line_start = bytes_before.rfind('\n').map(|i| i + 1).unwrap_or(0);

    // Column = char count from line_start to offset (1-based).
    let col = source[line_start..offset].chars().count() + 1;

    (Some(line), Some(col))
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Error => "Error",
        Severity::Warning => "Warning",
        Severity::Info => "Info",
    }
}

fn label_style_str(s: LabelStyle) -> &'static str {
    match s {
        LabelStyle::Primary => "Primary",
        LabelStyle::Secondary => "Secondary",
    }
}

fn applicability_str(a: Applicability) -> &'static str {
    match a {
        Applicability::MachineApplicable => "MachineApplicable",
        Applicability::MaybeIncorrect => "MaybeIncorrect",
        Applicability::HasPlaceholders => "HasPlaceholders",
        Applicability::Unspecified => "Unspecified",
    }
}

// Allow callers to construct `ErrorCode` from the JSON `code` string if
// they want to (used by future `buff fix` round-tripping). Kept in this
// module so the conversion lives next to the serialize side.
impl ErrorCode {
    /// Reverse of [`code_str`](Self::code_str) — parse an `"E1xxx"` string
    /// back to the enum variant. Returns `None` for unknown / malformed
    /// strings (so consumers can do lossy round-tripping).
    ///
    /// Added in T1 (v1.25 Wave 0) to support JSON round-tripping for
    /// future `buff fix` / reporter tooling. Does NOT renumber codes —
    /// it only matches already-shipped variants.
    pub fn from_code_str(s: &str) -> Option<ErrorCode> {
        ErrorCode::all()
            .iter()
            .find(|&&c| c.code_str() == s)
            .copied()
    }
}

// Re-export the SpanLabel style conversion for tests / consumers that
// want to build a SpanJson directly (rare — most go through `to_json`).
impl SpanLabel {
    /// JSON-style style string (`"Primary"` / `"Secondary"`) for this label.
    pub fn style_str(&self) -> &'static str {
        label_style_str(self.style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Diagnostic;
    use crate::span::{SourceId, Span};

    /// Sanity: primary span appears as `spans[0]` with style "Primary".
    #[test]
    fn json_primary_span_is_first_with_primary_style() {
        let src = "let x = 1";
        let diag = Diagnostic::error("bad", Span::new(0, 3, SourceId(0)));
        let j = to_json(&diag, src);
        assert_eq!(j.spans.len(), 1);
        assert_eq!(j.spans[0].style, "Primary");
        assert_eq!(j.spans[0].byte_start, 0);
        assert_eq!(j.spans[0].byte_end, 3);
        assert_eq!(j.spans[0].line_start, Some(1));
        assert_eq!(j.spans[0].col_start, Some(1));
    }

    /// Labels append to `spans` in declaration order.
    #[test]
    fn json_labels_append_after_primary_in_order() {
        let src = "let x = value\nlet y = 1";
        let diag = Diagnostic::error("mismatch", Span::new(8, 13, SourceId(0)))
            .with_secondary_label(Span::new(4, 5, SourceId(0)), "declared here")
            .with_label(Span::new(20, 21, SourceId(0)), "related");
        let j = to_json(&diag, src);
        assert_eq!(j.spans.len(), 3);
        assert_eq!(j.spans[0].style, "Primary");
        assert_eq!(j.spans[1].style, "Secondary");
        assert_eq!(j.spans[1].label.as_deref(), Some("declared here"));
        assert_eq!(j.spans[2].style, "Primary");
        assert_eq!(j.spans[2].label.as_deref(), Some("related"));
    }

    /// Out-of-bounds span yields `null` line/col but keeps byte offsets.
    #[test]
    fn json_out_of_bounds_span_yields_null_linecol() {
        let src = "short";
        let diag = Diagnostic::error("eof", Span::new(99, 100, SourceId(0)));
        let j = to_json(&diag, src);
        assert_eq!(j.spans[0].byte_start, 99);
        assert_eq!(j.spans[0].line_start, None);
        assert_eq!(j.spans[0].col_start, None);
    }

    /// Suggestions serialize with applicability + label.
    #[test]
    fn json_suggestion_round_trips_applicability_and_label() {
        let src = "pritn(\"hi\")";
        let diag = Diagnostic::error("unknown identifier", Span::new(0, 5, SourceId(0)))
            .with_labeled_suggestion(
                Span::new(0, 5, SourceId(0)),
                "print",
                Applicability::MachineApplicable,
                "change `pritn` to `print`",
            );
        let j = to_json(&diag, src);
        assert_eq!(j.suggestions.len(), 1);
        assert_eq!(j.suggestions[0].replacement, "print");
        assert_eq!(j.suggestions[0].applicability, "MachineApplicable");
        assert_eq!(
            j.suggestions[0].label.as_deref(),
            Some("change `pritn` to `print`")
        );
    }

    /// `ErrorCode::from_code_str` round-trips for every shipped code.
    #[test]
    fn json_error_code_round_trips() {
        for &code in ErrorCode::all() {
            let s = code.code_str();
            assert_eq!(ErrorCode::from_code_str(s), Some(code));
        }
        // Unknown string → None (no speculative variant created).
        assert_eq!(ErrorCode::from_code_str("E9999"), None);
        assert_eq!(ErrorCode::from_code_str("not-a-code"), None);
    }

    /// Multi-line source: line/col resolution is correct across newlines.
    #[test]
    fn json_linecol_resolves_across_newlines() {
        //            01234 5 6789...
        let src = "line1\nline2\nline3";
        // byte 6 = 'l' of line2 → (line=2, col=1)
        let diag = Diagnostic::error("x", Span::new(6, 7, SourceId(0)));
        let j = to_json(&diag, src);
        assert_eq!(j.spans[0].line_start, Some(2));
        assert_eq!(j.spans[0].col_start, Some(1));
    }
}
