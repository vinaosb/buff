//! Diagnostic types — severity levels, diagnostic messages, and the top-level error enum.
//!
//! # Source-line rendering (T36)
//!
//! [`Diagnostic::render`] turns a diagnostic + the raw source text into a
//! rustc-style rendered string: the offending source line followed by a caret
//! line (`^^^`) pointing at the byte span. For multi-error files,
//! [`render_diagnostics`] concatenates several diagnostics.
//!
//! # Colored output (T43)
//!
//! [`Diagnostic::render_with_color`] and [`render_diagnostics_with_color`] add
//! ANSI escape codes: red for errors, yellow for warnings, cyan for notes,
//! green for `help:` suggestions. Use [`should_use_color`] to detect terminal
//! capability (respects `NO_COLOR` env var and piped stderr).
//!
//! # "Did you mean?" suggestions (T36)
//!
//! When the user mistypes an identifier (e.g. `pritn` instead of `print`),
//! [`levenshtein`] + [`suggest_close`] + [`format_did_you_mean`] produce a
//! deterministic "Did you mean `print`?" note. Determinism: ties in
//! Levenshtein distance are broken **alphabetically** (no HashMap order).

use crate::code::ErrorCode;
use crate::span::Span;
use std::io::IsTerminal;

// ---------------------------------------------------------------------------
// T43: ANSI color constants + terminal detection
// ---------------------------------------------------------------------------

/// ANSI escape: reset all attributes.
const RESET: &str = "\x1b[0m";
/// ANSI escape: foreground red (errors).
const RED: &str = "\x1b[31m";
/// ANSI escape: foreground yellow (warnings).
const YELLOW: &str = "\x1b[33m";
/// ANSI escape: foreground cyan (notes/info).
const CYAN: &str = "\x1b[36m";
/// ANSI escape: foreground green (suggestions/help).
const GREEN: &str = "\x1b[32m";

/// Return the ANSI escape for the given severity's color.
const fn severity_color(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => RED,
        Severity::Warning => YELLOW,
        Severity::Info => CYAN,
    }
}

/// Wrap `text` in ANSI `color` + reset, returning `color text reset`.
fn color_text(color: &str, text: &str) -> String {
    let mut out = String::with_capacity(text.len() + RESET.len() + color.len());
    out.push_str(color);
    out.push_str(text);
    out.push_str(RESET);
    out
}

/// Detect whether ANSI color should be emitted.
///
/// Returns `false` when:
/// - `NO_COLOR` environment variable is set (per https://no-color.org/)
/// - `stderr` is not a terminal (piped / redirected)
///
/// Callers can override this with an explicit `--no-color` flag (see
/// [`Diagnostic::render_with_color`]).
pub fn should_use_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    // Use the stable `IsTerminal` trait (Rust 1.70+, no atty dep).
    std::io::stderr().is_terminal()
}

/// Wrap `text` in the ANSI color for `severity`, returning the colored
/// string. Used by the CLI's [`render_diagnostic`] to color the severity
/// tag in the `<path>:<line>:<col>: [Severity]` prefix.
///
/// This is a convenience wrapper around [`color_text`] + [`severity_color`].
pub fn color_severity(text: &str, severity: Severity) -> String {
    color_text(severity_color(severity), text)
}

/// The severity of a diagnostic message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// Style of a labeled span, mirroring rustc's `rustc_span::LabelStyle`.
///
/// [`Primary`] labels point at the **main** offending location (rendered with
/// `^` carets). [`Secondary`] labels point at additional context such as the
/// declaration of the symbol involved in the error (rendered with `~` tildes).
///
/// A [`Diagnostic`] always carries one implicit primary span ([`Diagnostic::span`]);
/// entries in [`Diagnostic::labels`] can be either style to extend the render
/// with more locations (rustc-style multi-span diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelStyle {
    /// Primary — `^^^` carets under the offending source.
    Primary,
    /// Secondary — `~~~` tildes under context (a declaration site, a related
    /// span, etc.). Mirrors rustc's secondary-label visual style.
    Secondary,
}

/// A labeled span attached to a [`Diagnostic`], used for multi-span rendering
/// (rustc-style). Each entry contributes its own source-line + caret block to
/// [`Diagnostic::render`], with the message shown after the caret.
///
/// Construct via [`Diagnostic::with_label`] (primary) or
/// [`Diagnostic::with_secondary_label`] (secondary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanLabel {
    /// The byte span this label points at.
    pub span: Span,
    /// The label message shown alongside the caret block (e.g.
    /// `"declared here"`, `"expected `Int`"`).
    pub label: String,
    /// Primary vs. Secondary — controls caret char (`^` vs `~`).
    pub style: LabelStyle,
}

impl SpanLabel {
    /// Build a primary-style label pointing at `span` with `label` text.
    pub fn primary(span: Span, label: impl Into<String>) -> Self {
        Self {
            span,
            label: label.into(),
            style: LabelStyle::Primary,
        }
    }

    /// Build a secondary-style label pointing at `span` with `label` text.
    pub fn secondary(span: Span, label: impl Into<String>) -> Self {
        Self {
            span,
            label: label.into(),
            style: LabelStyle::Secondary,
        }
    }
}

/// A diagnostic message with severity, message text, source span, notes,
/// optional secondary labels (multi-span), and an optional stable
/// [`ErrorCode`] (T124).
///
/// The `code`, `labels`, and `suggestions` fields are [`Option`]al /
/// empty-by-default at every existing construction site, so adding them did
/// not change any existing diagnostic output (single-span render stays
/// byte-identical when `labels` and `suggestions` are both empty).
///
/// Use [`Diagnostic::with_code`] / [`Diagnostic::with_label`] /
/// [`Diagnostic::with_suggestion`] to attach the corresponding pieces, and
/// [`Diagnostic::render`] / [`Diagnostic::fmt`] will then emit them in
/// rustc-style: `error[E1xxx]: <message>` plus per-label source-line blocks
/// plus suggestion `help:` lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub notes: Vec<String>,
    /// Optional stable error code (e.g. `E1001`). `None` for diagnostics
    /// that do not yet have a code, or for ad-hoc / uncategorised messages.
    pub code: Option<ErrorCode>,
    /// Additional labeled spans (multi-span diagnostics). Each entry renders
    /// as its own source-line + caret block below the primary span. Empty by
    /// default → existing single-span render output is unchanged.
    ///
    /// Added in v1.25 Wave 0 (T1) — see `.sisyphus/plans/buff-launch-readiness.md`.
    pub labels: Vec<SpanLabel>,
    /// Machine-readable fix suggestions. Each entry renders as a `help:`
    /// line below the notes. Empty by default → no change to existing output.
    ///
    /// Added in v1.25 Wave 0 (T1) — see `.sisyphus/plans/buff-launch-readiness.md`.
    pub suggestions: Vec<CodeSuggestion>,
}

impl Diagnostic {
    /// Create a new error-level diagnostic.
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span,
            notes: Vec::new(),
            code: None,
            labels: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// Create a new warning-level diagnostic.
    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            span,
            notes: Vec::new(),
            code: None,
            labels: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// Create a new info-level diagnostic.
    pub fn info(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Info,
            message: message.into(),
            span,
            notes: Vec::new(),
            code: None,
            labels: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// Add a note to this diagnostic.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Attach a stable [`ErrorCode`] to this diagnostic.
    ///
    /// Mirrors [`Diagnostic::with_note`] as a consuming builder. When `code`
    /// is `Some`, [`render`](Self::render) and [`Display`](impl std::fmt::Display)
    /// emit the code as `error[E1xxx]: <message>` immediately after the
    /// severity tag.
    pub fn with_code(mut self, code: ErrorCode) -> Self {
        self.code = Some(code);
        self
    }

    /// Attach a [`LabelStyle::Primary`] labeled span (multi-span diagnostic).
    ///
    /// Renders as an additional source-line + `^^^` caret block below the
    /// primary span, with `label` shown after the carets. Mirrors rustc's
    /// primary label rendering.
    pub fn with_label(mut self, span: Span, label: impl Into<String>) -> Self {
        self.labels.push(SpanLabel::primary(span, label));
        self
    }

    /// Attach a [`LabelStyle::Secondary`] labeled span (multi-span diagnostic).
    ///
    /// Renders as an additional source-line + `~~~` tilde block below the
    /// primary span, with `label` shown after the tildes. Mirrors rustc's
    /// secondary-label rendering (typically used for declaration sites or
    /// related spans that give context for the primary error).
    pub fn with_secondary_label(mut self, span: Span, label: impl Into<String>) -> Self {
        self.labels.push(SpanLabel::secondary(span, label));
        self
    }

    /// Attach a pre-built [`SpanLabel`] (useful when the style is decided at
    /// runtime; for the common cases prefer [`with_label`](Self::with_label) /
    /// [`with_secondary_label`](Self::with_secondary_label)).
    pub fn with_span_label(mut self, label: SpanLabel) -> Self {
        self.labels.push(label);
        self
    }

    /// Attach a machine-readable fix suggestion.
    ///
    /// Renders as a `help: ` line below the notes, with the replacement text
    /// shown. When the suggestion is [`Applicability::MachineApplicable`],
    /// tooling (LSP CodeAction, `buff check --error-format json`) can apply
    /// it without user review.
    pub fn with_suggestion(
        mut self,
        span: Span,
        replacement: impl Into<String>,
        applicability: Applicability,
    ) -> Self {
        self.suggestions.push(CodeSuggestion {
            span,
            replacement: replacement.into(),
            applicability,
            label: None,
        });
        self
    }

    /// Like [`with_suggestion`](Self::with_suggestion) but also attaches a
    /// human-readable `label` (e.g. `"change `pritn` to `print`"`) that
    /// renders alongside the replacement text in the `help:` line.
    pub fn with_labeled_suggestion(
        mut self,
        span: Span,
        replacement: impl Into<String>,
        applicability: Applicability,
        label: impl Into<String>,
    ) -> Self {
        self.suggestions.push(CodeSuggestion {
            span,
            replacement: replacement.into(),
            applicability,
            label: Some(label.into()),
        });
        self
    }
}

// ---------------------------------------------------------------------------
// T1 (v1.25 Wave 0): Suggestion API — Applicability + CodeSuggestion
// ---------------------------------------------------------------------------

/// How confidently a [`CodeSuggestion`] can be auto-applied by tooling
/// (LSP CodeAction, `buff check --error-format json` consumers, future
/// `buff fix` rewriter). Mirrors rustc's `Applicability` enum verbatim.
///
/// Tooling SHOULD treat the variant as a hint, not a hard rule: an
/// `Unspecified` suggestion is still legal to surface as a CodeAction — it
/// just means the user must review the change before applying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    /// Always correct — safe to apply without user review. The canonical
    /// example is fixing a typo: `pritn` → `print`.
    MachineApplicable,
    /// Probably correct but not certain. Tooling SHOULD show the suggestion
    /// for manual review rather than auto-applying. Example: suggesting a
    /// field name that may shadow an existing binding.
    MaybeIncorrect,
    /// Contains placeholders (e.g. `<>`, `<name>`) that the user must fill
    /// in. Renders with the placeholders visible; tooling MAY apply but MUST
    /// leave the cursor on the first placeholder.
    HasPlaceholders,
    /// No applicability hint. Tooling should let the user decide. Use when
    /// the suggestion is intentionally vague (e.g. an example fix that may
    /// not apply to the specific case).
    Unspecified,
}

impl Applicability {
    /// `true` for [`MachineApplicable`](Self::MachineApplicable) — the
    /// convenience predicate tooling uses to decide "auto-apply or not".
    pub fn is_machine_applicable(self) -> bool {
        matches!(self, Applicability::MachineApplicable)
    }
}

impl std::fmt::Display for Applicability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Applicability::MachineApplicable => "MachineApplicable",
            Applicability::MaybeIncorrect => "MaybeIncorrect",
            Applicability::HasPlaceholders => "HasPlaceholders",
            Applicability::Unspecified => "Unspecified",
        };
        f.write_str(s)
    }
}

/// A machine-readable fix suggestion attached to a [`Diagnostic`].
///
/// Says: "the bytes at [`span`] should be replaced with `replacement`, and
/// the change is `applicability`-confident". Mirrors rustc's `CodeSuggestion`
/// (simplified — single replacement string per suggestion; rustc's
/// `Substitution` enum supports fragments we don't need yet).
///
/// Attach via [`Diagnostic::with_suggestion`] /
/// [`Diagnostic::with_labeled_suggestion`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSuggestion {
    /// The byte range whose text should be replaced.
    pub span: Span,
    /// The replacement text (the new bytes that go in place of `span`).
    pub replacement: String,
    /// How confidently tooling can auto-apply this suggestion.
    pub applicability: Applicability,
    /// Optional human-readable label shown alongside the replacement in the
    /// `help:` line (e.g. `"change `pritn` to `print`"`).
    pub label: Option<String>,
}

impl CodeSuggestion {
    /// Render this suggestion as a single `help:` line (no source-line
    /// block — that's the caller's job; this just formats the payload).
    ///
    /// Format:
    ///
    /// ```text
    ///   help: <label>: replace with `<replacement>` (<applicability>)
    /// ```
    ///
    /// When `label` is `None`, the leading "`<label>:` " is dropped:
    ///
    /// ```text
    ///   help: replace with `<replacement>` (<applicability>)
    /// ```
    pub fn render_help_line(&self) -> String {
        let applicability = self.applicability;
        match &self.label {
            Some(label) => format!(
                "  help: {}: replace with `{}` ({})\n",
                label, self.replacement, applicability
            ),
            None => format!(
                "  help: replace with `{}` ({})\n",
                self.replacement, applicability
            ),
        }
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(code) => write!(
                f,
                "[{:?}] error[{}]: {}",
                self.severity,
                code.code_str(),
                self.message
            )?,
            None => write!(f, "[{:?}] {}", self.severity, self.message)?,
        }
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
    /// line follow. **Multi-span** labels ([`Diagnostic::labels`]) each
    /// render as their own source-line + caret block below the primary span,
    /// with their label message appended after the caret chars. **Notes**
    /// ([`Diagnostic::notes`]) and **suggestions**
    /// ([`Diagnostic::suggestions`]) appear last as `note:` / `help:` lines.
    ///
    /// **Column accounting** is char-based (not byte-based), so multi-byte
    /// UTF-8 sequences align correctly under a single caret column — this
    /// mirrors [`SourceFile::lookup`](crate::source_map::SourceFile::lookup).
    /// Multi-line spans are clamped to the line containing `span.start`, so
    /// the caret never overflows past the source-line newline.
    ///
    /// Out-of-bounds spans (e.g. EOF errors with synthetic spans past the end
    /// of source) render only the header + notes, without any caret line.
    ///
    /// **Backward compatibility**: when `labels` and `suggestions` are both
    /// empty (the default for all pre-T1 construction sites), the output is
    /// byte-identical to the pre-T1 single-span render.
    pub fn render(&self, source: &str) -> String {
        let mut out = String::new();
        match self.code {
            Some(code) => out.push_str(&format!(
                "[{:?}] error[{}]: {}\n",
                self.severity,
                code.code_str(),
                self.message
            )),
            None => out.push_str(&format!("[{:?}] {}\n", self.severity, self.message)),
        }
        if let Some(rendered_line) = render_span_in_source(&self.span, source) {
            out.push_str(&rendered_line);
        }
        // Multi-span labels: each contributes its own source-line + caret
        // block, with the label message appended after the caret chars.
        // Caret char is `^` for Primary (matches the primary span's carets)
        // and `~` for Secondary (rustc convention).
        for label in &self.labels {
            if let Some(rendered_label) =
                render_span_label_in_source(&label.span, source, label.style, &label.label)
            {
                out.push_str(&rendered_label);
            } else if !label.label.is_empty() {
                // Span is out of bounds; still surface the label as a
                // note-style line so the user sees the context.
                out.push_str(&format!("  = {}\n", label.label));
            }
        }
        for note in &self.notes {
            out.push_str(&format!("  note: {}\n", note));
        }
        for suggestion in &self.suggestions {
            out.push_str(&suggestion.render_help_line());
        }
        out
    }

    /// Like [`render`](Self::render) but with ANSI color codes.
    ///
    /// When `use_color` is `false`, output is identical to [`render`].
    /// Callers should compute `use_color` via [`should_use_color`] and
    /// allow a `--no-color` CLI flag to force-disable.
    pub fn render_with_color(&self, source: &str, use_color: bool) -> String {
        if !use_color {
            return self.render(source);
        }
        let mut out = String::new();
        match self.code {
            Some(code) => {
                let severity_tag = format!("[{:?}]", self.severity);
                let colored_severity = color_text(severity_color(self.severity), &severity_tag);
                out.push_str(&format!(
                    "{} error[{}]: {}\n",
                    colored_severity,
                    code.code_str(),
                    self.message
                ));
            }
            None => {
                let severity_tag = format!("[{:?}]", self.severity);
                let colored_severity = color_text(severity_color(self.severity), &severity_tag);
                out.push_str(&format!("{} {}\n", colored_severity, self.message));
            }
        }
        if let Some(rendered_line) =
            render_span_in_source_with_color(&self.span, source, self.severity)
        {
            out.push_str(&rendered_line);
        }
        for label in &self.labels {
            if let Some(rendered_label) = render_span_label_in_source_with_color(
                &label.span,
                source,
                label.style,
                &label.label,
                self.severity,
            ) {
                out.push_str(&rendered_label);
            } else if !label.label.is_empty() {
                out.push_str(&format!("  = {}\n", label.label));
            }
        }
        for note in &self.notes {
            out.push_str(&format!("  {}note: {}{}\n", CYAN, note, RESET));
        }
        for suggestion in &self.suggestions {
            let help_line = suggestion.render_help_line();
            // Wrap the "help:" prefix and the backtick-quoted replacement in green.
            // The original line is "  help: replace with `...`" or "  help: label: replace with `...`"
            out.push_str(GREEN);
            out.push_str(&help_line);
            out.push_str(RESET);
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

/// Like [`render_span_in_source`] but with ANSI color on the caret line,
/// using `severity` to choose the color (red for errors, yellow for warnings,
/// cyan for info/notes).
fn render_span_in_source_with_color(
    span: &Span,
    source: &str,
    severity: Severity,
) -> Option<String> {
    let start = span.start;
    let raw_end = span.end;

    if start > source.len() {
        return None;
    }

    let line_start = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = source[line_start..]
        .find('\n')
        .map(|i| line_start + i)
        .unwrap_or(source.len());
    let line_text = &source[line_start..line_end];

    let line_no = source[..line_start].matches('\n').count() + 1;
    let line_no_str = line_no.to_string();
    let col = source[line_start..start].chars().count();
    let span_end_in_line = raw_end.min(line_end);
    let width = if span_end_in_line <= start {
        1
    } else {
        source[start..span_end_in_line].chars().count().max(1)
    };

    let gutter_pad: String = " ".repeat(line_no_str.len() + 1);
    let caret_pad: String = " ".repeat(col);
    let carets: String = "^".repeat(width);
    let color = severity_color(severity);

    let mut out = String::new();
    out.push_str(&format!("{gutter_pad}|\n"));
    out.push_str(&format!("{line_no_str} | {line_text}\n"));
    out.push_str(&format!(
        "{gutter_pad}| {caret_pad}{color}{carets}{RESET}\n"
    ));
    out.push_str(&format!("{gutter_pad}|\n"));
    Some(out)
}

/// Render a labeled span (multi-span diagnostics) — same shape as
/// [`render_span_in_source`] but with the caret char chosen by `style`
/// (`^` for [`LabelStyle::Primary`], `~` for [`LabelStyle::Secondary`])
/// and the label message appended after the caret chars.
///
/// Format when the label is non-empty:
///
/// ```text
///   |
/// N | <source line>
///   |   <padding>^^^^ <label>
///   |
/// ```
///
/// When the label is empty, the caret line has no trailing message (matches
/// the primary span's caret-only shape). Returns `None` when `span.start`
/// lies past the end of `source`.
fn render_span_label_in_source(
    span: &Span,
    source: &str,
    style: LabelStyle,
    label: &str,
) -> Option<String> {
    let start = span.start;
    let raw_end = span.end;

    if start > source.len() {
        return None;
    }

    let line_start = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = source[line_start..]
        .find('\n')
        .map(|i| line_start + i)
        .unwrap_or(source.len());
    let line_text = &source[line_start..line_end];

    let line_no = source[..line_start].matches('\n').count() + 1;
    let line_no_str = line_no.to_string();
    let col = source[line_start..start].chars().count();
    let span_end_in_line = raw_end.min(line_end);
    let width = if span_end_in_line <= start {
        1
    } else {
        source[start..span_end_in_line].chars().count().max(1)
    };

    let gutter_pad: String = " ".repeat(line_no_str.len() + 1);
    let caret_pad: String = " ".repeat(col);
    let caret_char = match style {
        LabelStyle::Primary => '^',
        LabelStyle::Secondary => '~',
    };
    let carets: String = std::iter::repeat_n(caret_char, width).collect();
    let trailing = if label.is_empty() {
        String::new()
    } else {
        format!(" {label}")
    };

    let mut out = String::new();
    out.push_str(&format!("{gutter_pad}|\n"));
    out.push_str(&format!("{line_no_str} | {line_text}\n"));
    out.push_str(&format!("{gutter_pad}| {caret_pad}{carets}{trailing}\n"));
    out.push_str(&format!("{gutter_pad}|\n"));
    Some(out)
}

/// Like [`render_span_label_in_source`] but with ANSI color on the caret line,
/// using the diagnostic's `severity` color for primary labels. The label text
/// is also wrapped in color.
fn render_span_label_in_source_with_color(
    span: &Span,
    source: &str,
    style: LabelStyle,
    label: &str,
    severity: Severity,
) -> Option<String> {
    let start = span.start;
    let raw_end = span.end;

    if start > source.len() {
        return None;
    }

    let line_start = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = source[line_start..]
        .find('\n')
        .map(|i| line_start + i)
        .unwrap_or(source.len());
    let line_text = &source[line_start..line_end];

    let line_no = source[..line_start].matches('\n').count() + 1;
    let line_no_str = line_no.to_string();
    let col = source[line_start..start].chars().count();
    let span_end_in_line = raw_end.min(line_end);
    let width = if span_end_in_line <= start {
        1
    } else {
        source[start..span_end_in_line].chars().count().max(1)
    };

    let gutter_pad: String = " ".repeat(line_no_str.len() + 1);
    let caret_pad: String = " ".repeat(col);
    let caret_char = match style {
        LabelStyle::Primary => '^',
        LabelStyle::Secondary => '~',
    };
    let carets: String = std::iter::repeat_n(caret_char, width).collect();
    let color = severity_color(severity);
    let trailing = if label.is_empty() {
        String::new()
    } else {
        format!(" {color}{label}{RESET}")
    };

    let mut out = String::new();
    out.push_str(&format!("{gutter_pad}|\n"));
    out.push_str(&format!("{line_no_str} | {line_text}\n"));
    out.push_str(&format!(
        "{gutter_pad}| {caret_pad}{color}{carets}{RESET}{trailing}\n"
    ));
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

/// Like [`render_diagnostics`] but with ANSI color codes.
///
/// Delegates to [`Diagnostic::render_with_color`] for each diagnostic.
/// When `use_color` is `false`, output is identical to [`render_diagnostics`].
pub fn render_diagnostics_with_color(
    diagnostics: &[Diagnostic],
    source: &str,
    use_color: bool,
) -> String {
    if !use_color {
        return render_diagnostics(diagnostics, source);
    }
    let mut out = String::new();
    for (i, d) in diagnostics.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&d.render_with_color(source, true));
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
