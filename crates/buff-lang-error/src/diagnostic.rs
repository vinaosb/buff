//! Diagnostic types — severity levels, diagnostic messages, and the top-level error enum.
//!
//! # Source-line rendering (T36)
//!
//! [`Diagnostic::render`] turns a diagnostic + the raw source text into a
//! rustc-style rendered string: the offending source line followed by a caret
//! line (`^^^`) pointing at the byte span. For multi-error files,
//! [`render_diagnostics`] concatenates several diagnostics.
//!
//! # "Did you mean?" suggestions (T36)
//!
//! When the user mistypes an identifier (e.g. `pritn` instead of `print`),
//! [`levenshtein`] + [`suggest_close`] + [`format_did_you_mean`] produce a
//! deterministic "Did you mean `print`?" note. Determinism: ties in
//! Levenshtein distance are broken **alphabetically** (no HashMap order).

use crate::span::Span;

/// The severity of a diagnostic message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A diagnostic message with severity, message text, source span, and notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// Create a new error-level diagnostic.
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span,
            notes: Vec::new(),
        }
    }

    /// Create a new warning-level diagnostic.
    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            span,
            notes: Vec::new(),
        }
    }

    /// Create a new info-level diagnostic.
    pub fn info(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Info,
            message: message.into(),
            span,
            notes: Vec::new(),
        }
    }

    /// Add a note to this diagnostic.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.severity, self.message)?;
        if !self.notes.is_empty() {
            for note in &self.notes {
                write!(f, "\n  note: {}", note)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// T36: source-line + caret rendering, Levenshtein, "Did you mean?"
// ---------------------------------------------------------------------------

impl Diagnostic {
    /// Render this diagnostic against `source` as a rustc-style string:
    ///
    /// ```text
    /// [Error] the message
    ///   |
    /// 3 | let x = value
    ///   |         ^^^^^
    ///   |
    ///   note: Did you mean `print`?
    /// ```
    ///
    /// The first line is always `[<severity>] <message>`. If the diagnostic's
    /// [`Span`] lies within `source`, the offending source line plus a caret
    /// line follow. Notes are appended last, one per line.
    ///
    /// **Column accounting** is char-based (not byte-based), so multi-byte
    /// UTF-8 sequences align correctly under a single caret column — this
    /// mirrors [`SourceFile::lookup`](crate::source_map::SourceFile::lookup).
    /// Multi-line spans are clamped to the line containing `span.start`, so
    /// the caret never overflows past the source-line newline.
    ///
    /// Out-of-bounds spans (e.g. EOF errors with synthetic spans past the end
    /// of source) render only the header + notes, without any caret line.
    pub fn render(&self, source: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!("[{:?}] {}\n", self.severity, self.message));
        if let Some(rendered_line) = render_span_in_source(&self.span, source) {
            out.push_str(&rendered_line);
        }
        for note in &self.notes {
            out.push_str(&format!("  note: {}\n", note));
        }
        out
    }
}

/// Render the source line containing `span.start` plus a caret underline.
///
/// Returns `None` when `span.start` lies past the end of `source` (so the
/// caller can render just the header without a caret block).
///
/// Format:
///
/// ```text
///   |
/// N | <source line>
///   |   <padding>^^^^
///   |
/// ```
///
/// where `N` is the 1-based line number, `<padding>` is `span.start`'s
/// 0-based column in *characters* (so multi-byte chars count as 1 column),
/// and the caret count is the character width of the span clamped to the
/// current line (minimum 1).
fn render_span_in_source(span: &Span, source: &str) -> Option<String> {
    let start = span.start;
    let raw_end = span.end;

    if start > source.len() {
        return None;
    }

    // Find the byte offset of the start of the line containing `start`
    // (the char after the previous '\n', or 0 if none).
    let line_start = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);

    // Find the byte offset just past the end of this line (the '\n' or EOF).
    let line_end = source[line_start..]
        .find('\n')
        .map(|i| line_start + i)
        .unwrap_or(source.len());

    let line_text = &source[line_start..line_end];

    // 1-based line number = number of '\n' before line_start, +1.
    let line_no = source[..line_start].matches('\n').count() + 1;
    let line_no_str = line_no.to_string();

    // 0-based column in *characters* (so multi-byte UTF-8 counts as 1 col).
    let col = source[line_start..start].chars().count();

    // Caret width: characters between start and (end clamped to line_end).
    // Clamp end to the line so a span that crosses a newline doesn't draw
    // carets past the source-line boundary.
    let span_end_in_line = raw_end.min(line_end);
    let width = if span_end_in_line <= start {
        // Zero-width span: at least one caret so the user sees the location.
        1
    } else {
        source[start..span_end_in_line].chars().count().max(1)
    };

    // Gutter: line_no right-aligned to its own width, then ` | `. For
    // empty/caret lines, replace line_no with the same number of spaces
    // (plus the trailing space that separates line_no from `|`) so the
    // pipes line up across all four lines.
    let gutter_pad: String = " ".repeat(line_no_str.len() + 1);
    let caret_pad: String = " ".repeat(col);
    let carets: String = "^".repeat(width);

    let mut out = String::new();
    out.push_str(&format!("{gutter_pad}|\n"));
    out.push_str(&format!("{line_no_str} | {line_text}\n"));
    out.push_str(&format!("{gutter_pad}| {caret_pad}{carets}\n"));
    out.push_str(&format!("{gutter_pad}|\n"));
    Some(out)
}

/// Render multiple diagnostics against the same `source`, separated by a
/// blank line. Useful for parser-error-recovery output where several errors
/// are collected in one pass.
///
/// Empty input returns the empty string.
pub fn render_diagnostics(diagnostics: &[Diagnostic], source: &str) -> String {
    let mut out = String::new();
    for (i, d) in diagnostics.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&d.render(source));
    }
    out
}

/// Classic iterative Levenshtein edit distance between two strings, counted
/// in **characters** (not bytes) so multi-byte UTF-8 sequences count as a
/// single edit each.
///
/// The distance is the minimum number of single-character insertions,
/// deletions, or substitutions needed to transform `a` into `b`.
///
/// # Examples
///
/// ```
/// # use buff_lang_error::levenshtein;
/// assert_eq!(levenshtein("print", "print"), 0);
/// assert_eq!(levenshtein("print", "prink"), 1); // one substitution
/// assert_eq!(levenshtein("kitten", "sitting"), 3);
/// ```
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    // Two-row DP: `prev` is row i-1, `curr` is row i. We swap instead of
    // reallocating an m×n matrix.
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Maximum Levenshtein distance at which a candidate is still considered a
/// "did you mean?" suggestion. Two edits covers transpositions (counted as
/// distance 2 in classic Levenshtein), single-char substitutions, and a
/// couple of typos.
pub const SUGGESTION_MAX_DISTANCE: usize = 2;

/// Pick the closest candidate to `input` from `candidates` by Levenshtein
/// distance, breaking ties **alphabetically** for determinism.
///
/// Returns `None` when no candidate is within
/// [`SUGGESTION_MAX_DISTANCE`] edits of `input`, or when `candidates` is
/// empty. The returned reference aliases into the input slice.
///
/// # Determinism
///
/// This function never depends on `HashMap` iteration order. Candidates are
/// scanned in slice order; the running best is replaced only when a strictly
/// smaller distance is found, OR an equal distance with a lexicographically
/// smaller candidate string (so the alphabetical winner is stable).
pub fn suggest_close<'a>(input: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let mut best: Option<(usize, &'a str)> = None;
    for &cand in candidates {
        // Cheap pre-filter: if the length differs by more than the threshold,
        // no sequence of edits within the threshold can match — skip the DP.
        let len_delta = cand.chars().count().saturating_sub(input.chars().count());
        if len_delta > SUGGESTION_MAX_DISTANCE
            || input.chars().count().saturating_sub(cand.chars().count()) > SUGGESTION_MAX_DISTANCE
        {
            continue;
        }
        let d = levenshtein(input, cand);
        if d > SUGGESTION_MAX_DISTANCE {
            continue;
        }
        match best {
            None => best = Some((d, cand)),
            Some((bd, bc)) => {
                if d < bd || (d == bd && cand < bc) {
                    best = Some((d, cand));
                }
            }
        }
    }
    best.map(|(_, c)| c)
}

/// Build a deterministic "Did you mean `<candidate>`?" string for an unknown
/// identifier, or `None` when no candidate is close enough.
///
/// Wraps [`suggest_close`] with the canonical note formatting used by parser
/// and type-checker error paths:
///
/// ```rust
/// # use buff_lang_error::format_did_you_mean;
/// let cands = vec!["print", "println", "printf"];
/// assert_eq!(
///     format_did_you_mean("pritn", &cands).as_deref(),
///     Some("Did you mean `print`?"),
/// );
/// ```
pub fn format_did_you_mean(input: &str, candidates: &[&str]) -> Option<String> {
    suggest_close(input, candidates).map(|c| format!("Did you mean `{c}`?"))
}

// ---------------------------------------------------------------------------
// Sub-error types — each wraps a Diagnostic
// ---------------------------------------------------------------------------

/// A lexer error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct LexError {
    pub diagnostic: Diagnostic,
}

impl LexError {
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

/// A parser error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct ParseError {
    pub diagnostic: Diagnostic,
}

impl ParseError {
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

/// A type-checking error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct TypeError {
    pub diagnostic: Diagnostic,
}

impl TypeError {
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

/// A code-generation error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct CodegenError {
    pub diagnostic: Diagnostic,
}

impl CodegenError {
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

/// A runtime error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct RuntimeError {
    pub diagnostic: Diagnostic,
}

impl RuntimeError {
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

// ---------------------------------------------------------------------------
// Top-level error enum
// ---------------------------------------------------------------------------

/// The top-level error type for the Buff compiler.
///
/// Each variant wraps a phase-specific error that carries a [`Diagnostic`].
#[derive(Debug, thiserror::Error)]
pub enum BuffError {
    #[error("Lex error: {0}")]
    Lex(#[from] LexError),
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("Type error: {0}")]
    Type(#[from] TypeError),
    #[error("Codegen error: {0}")]
    Codegen(#[from] CodegenError),
    #[error("Runtime error: {0}")]
    Runtime(#[from] RuntimeError),
}
