//! Display + rich-output helpers - extracted from `kernel.rs` (T106 mechanical split).
//!
//! Pure functions for building Jupyter reply payloads: error traceback
//! formatting, rich HTML rendering (tables for Vector/Matrix), introspection
//! (`?`/`??` prefix detection), ISO-8601 timestamps, and HTML escaping.
//! None of these depend on the Kernel struct; they are re-exported back
//! via `pub use display::*`.

use buff_eval::{EvalResult, ResolvedType};
/// Build the (evalue, traceback) pair for an error-shaped reply.
///
/// `evalue` is the diagnostic's `Display` form (the canonical
/// `[Error] <message>` rendering) when a diagnostic is present, or a
/// synthesized "exited with code N" string when only the exit code
/// signals failure (the diagnostic was None but exit_code != 0 — a
/// defensive branch that should not normally fire given the
/// evaluator always sets a diagnostic on non-zero exit).
///
/// `traceback` is the captured stderr (split into lines so
/// front-ends can render each traceback frame distinctly) followed
/// by the diagnostic's `Display` form. Empty stderr contributes no
/// lines. No ANSI escapes are injected (Buff is not a Python kernel).
pub fn build_error_payload(result: &EvalResult) -> (String, Vec<String>) {
    let evalue = result
        .diagnostic
        .as_ref()
        .map(|d| d.to_string())
        .unwrap_or_else(|| {
            format!(
                "Buff execution failed (exit code {})",
                result
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "<signal>".to_string())
            )
        });

    let mut traceback: Vec<String> = Vec::new();
    if !result.stderr.is_empty() {
        for line in result.stderr.lines() {
            traceback.push(line.to_string());
        }
        // Ensure at least one blank separator between stderr and the
        // diagnostic if both are present (mirrors the visual layout
        // ipykernel uses for Python tracebacks).
        if result.diagnostic.is_some() && !line_ends_with_blank(&result.stderr) {
            traceback.push(String::new());
        }
    }
    if let Some(d) = &result.diagnostic {
        traceback.push(d.to_string());
    }

    (evalue, traceback)
}

/// `true` if `s` ends with a blank line — i.e. the string ends with
/// `\n\n` (a trailing newline followed by another newline = a blank
/// line), or is just `\n` alone. Used by [`build_error_payload`] to
/// decide whether to inject a blank separator between the stderr
/// block and the diagnostic line.
pub fn line_ends_with_blank(s: &str) -> bool {
    s.ends_with("\n\n") || s == "\n"
}

// ---------------------------------------------------------------------------
// T129c: introspection + rich-display helpers (free functions).
//
// These are kept module-private and free-standing so they can be unit-
// tested in isolation (see the `tests` mod below) and so the dispatch
// logic in `handle_execute_request` reads as a short branch table
// rather than a tangle of inline closures.
// ---------------------------------------------------------------------------

/// A parsed `?name` / `??name` introspection query.
///
/// Produced by [`parse_introspection`] when a cell's trimmed source
/// matches the magic prefix shape. Consumed by
/// [`Kernel::handle_introspection`] to build the `execute_result`
/// text/plain payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Introspection {
    /// `?name` — surface type info / help for the name.
    Help(String),
    /// `??name` — surface source / best-available definition for the name.
    Source(String),
}

/// Detect a `?name` / `??name` introspection magic at the start of a
/// cell.
///
/// Returns `Some(Introspection)` when the trimmed cell is EXACTLY
/// `?name` or `??name` (single name, no other code). Multi-line cells
/// and cells with trailing content (`?x + 1`) return `None` — the
/// introspection magic is a strict cell-level prefix, not a mid-cell
/// operator. The check is performed BEFORE normal evaluation in
/// [`Kernel::handle_execute_request`] so the query doesn't spawn
/// `rustc` unnecessarily.
///
/// `??` is checked BEFORE `?` (longer prefix wins) so `??x` is
/// classified as [`Introspection::Source`] and not
/// [`Introspection::Help`] of name `"?x"`.
pub fn parse_introspection(code: &str) -> Option<Introspection> {
    let trimmed = code.trim();
    // `??name` — source / definition query.
    if let Some(rest) = trimmed.strip_prefix("??") {
        let name = rest.trim();
        if is_valid_buff_ident(name) {
            return Some(Introspection::Source(name.to_string()));
        }
        return None;
    }
    // `?name` — help / type query.
    if let Some(rest) = trimmed.strip_prefix('?') {
        let name = rest.trim();
        if is_valid_buff_ident(name) {
            return Some(Introspection::Help(name.to_string()));
        }
        return None;
    }
    None
}

/// `true` if `s` is a valid Buff identifier: starts with an
/// alphabetic char or `_`, followed by zero or more alphanumeric / `_`
/// chars. Unicode alphabetic chars are accepted (Buff's lexer accepts
/// them per `examples/ola.buff`'s PT-BR naming convention).
pub fn is_valid_buff_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().expect("non-empty");
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// `true` if `ty` is `Vector<_>` or `Matrix<_>` — the rich-display
/// eligible collection types.
pub fn is_matrix_or_vector(ty: &ResolvedType) -> bool {
    matches!(ty, ResolvedType::Vector(_) | ResolvedType::Matrix(_))
}

/// `true` when the cell should take the T129c rich-display-literal
/// shortcut: the resolved type is `Vector<_>` / `Matrix<_>` AND the
/// source expression is a `[...]`-shaped literal.
///
/// The literal-shape check (rather than accepting any Vector/Matrix
/// expression) is the conservative path: it ensures we render only
/// values whose source form is also their canonical plain-text
/// representation (e.g. `[1, 2, 3]`). Non-literal expressions (e.g.
/// `[1, 2, 3].map({ x => x * 2 })`) fall through to the normal
/// compile-and-run path, which may surface a `Vec<T>: Display` rustc
/// error today — that's a codegen gap documented as post-T129c work.
pub fn is_rich_display_literal(ty: Option<&ResolvedType>, expr_src: &str) -> bool {
    let Some(t) = ty else { return false };
    if !is_matrix_or_vector(t) {
        return false;
    }
    is_collection_literal(expr_src)
}

/// `true` if `src` looks like a Buff collection literal — non-empty,
/// starts with `[`, ends with `]`. Used by [`is_rich_display_literal`]
/// as the cheap syntactic check that the expression is a literal
/// (rather than a computed expression whose type happens to be
/// Vector/Matrix).
pub fn is_collection_literal(src: &str) -> bool {
    let s = src.trim();
    s.len() >= 2 && s.starts_with('[') && s.ends_with(']')
}

/// Format a value's source form (e.g. `[1, 2, 3]` or `[[1, 2], [3, 4]]`)
/// as an HTML `<table>` for rich display.
///
/// Handles both 1-D (`Vector<T>`) and 2-D (`Matrix<T>`) shapes:
/// - 1-D vectors render as a single row of `<td>` cells.
/// - 2-D matrices render as one `<tr>` per row, with `<td>` cells in
///   each row.
///
/// Cell text is HTML-escaped via [`html_escape`] so numeric/string
/// content with `<`, `>`, `&`, `"`, `'` is rendered safely.
pub fn format_rich_html(value: &str) -> String {
    let rows = parse_rich_rows(value);
    let mut html = String::from("<table>");
    for row in &rows {
        html.push_str("<tr>");
        for cell in row {
            html.push_str("<td>");
            html.push_str(&html_escape(cell));
            html.push_str("</td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    html
}

/// Parse a value's source form into rows of cells.
///
/// - For 1-D shapes (`[1, 2, 3]`), returns a single row of `["1", "2", "3"]`.
/// - For 2-D shapes (`[[1, 2], [3, 4]]`), returns two rows.
/// - For non-`[...]`-wrapped values, returns a single row with one cell
///   holding the entire value (graceful fallback — the cell text is
///   still HTML-escaped and visible in the output).
pub fn parse_rich_rows(value: &str) -> Vec<Vec<String>> {
    let v = value.trim();
    // Strip ONE level of outer brackets.
    let inner = match v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        Some(s) => s,
        None => return vec![vec![v.to_string()]],
    };
    // 2-D detection: inner string contains a `[` (start of a nested
    // row). Otherwise treat as 1-D.
    if inner.contains('[') {
        parse_nested_rows(inner)
    } else {
        vec![split_top_level_commas(inner)]
    }
}

/// Parse the inner content of a 2-D literal (e.g. `"[1, 2], [3, 4]"`)
/// into one row of cells per nested `[...]` block.
///
/// Walks the string tracking bracket depth. When depth returns to 0
/// after a closing `]`, the slice between the matching `[` and `]`
/// is split on top-level commas into a row. Non-bracketed content
/// between rows (whitespace, stray commas) is ignored.
pub fn parse_nested_rows(s: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut depth = 0i32;
    let mut current_start = None;
    for (i, c) in s.char_indices() {
        match c {
            '[' => {
                if depth == 0 {
                    current_start = Some(i + c.len_utf8());
                }
                depth += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = current_start {
                        let row_str = &s[start..i];
                        rows.push(split_top_level_commas(row_str));
                    }
                    current_start = None;
                }
            }
            _ => {}
        }
    }
    if rows.is_empty() {
        // Defensive: if the input looked 2-D-ish but didn't yield any
        // complete rows (malformed input), fall back to a single row
        // of the raw content so the test still gets a `<table>`.
        rows.push(split_top_level_commas(s));
    }
    rows
}

/// Split `s` on TOP-LEVEL commas (commas at bracket depth 0). Used by
/// [`parse_rich_rows`] and [`parse_nested_rows`] to break a row into
/// cells. Comma separators inside nested `[...]` / `{...}` / `(...)`
/// are preserved within their cell.
pub fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in s.chars() {
        match c {
            '[' | '{' | '(' => {
                depth += 1;
                current.push(c);
            }
            ']' | '}' | ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                let cell = current.trim().to_string();
                if !cell.is_empty() {
                    out.push(cell);
                }
                current.clear();
            }
            _ => {
                current.push(c);
            }
        }
    }
    let cell = current.trim().to_string();
    if !cell.is_empty() {
        out.push(cell);
    }
    if out.is_empty() {
        // Guarantee at least one cell so the resulting `<td>` is
        // always non-empty (matches Jupyter's expectation that
        // `execute_result` MIME bodies be non-empty strings).
        out.push(String::new());
    }
    out
}

/// HTML-escape `s` for safe injection into a `text/html` MIME body.
///
/// Escapes the five XML special chars: `&`, `<`, `>`, `"`, `'`. The
/// order (`&` first) prevents double-escaping the entities introduced
/// by later replacements.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// Return the current UTC timestamp as ISO-8601 with microsecond
/// precision (the shape Jupyter clients expect in the `date` field).
///
/// We deliberately do NOT pull `chrono` here (the workspace already
/// pins it but the buff-jupyter crate's dependency surface stays
/// minimal). Instead, we format from `SystemTime` directly — the
/// resulting string is correct enough for handshake purposes (real
/// kernels like ipykernel use full RFC 3339 with timezone, which our
/// format approximates by appending the `Z` UTC marker).
pub fn now_iso() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let micros = dur.subsec_micros();
    // Split secs into Y/M/D H:M:S via the civil-from-days algorithm
    // (Howard Hinnant, http://howardhinnant.github.io/date_algorithms.html).
    let days = (secs / 86_400) as i64;
    let remainder = secs % 86_400;
    let hour = (remainder / 3600) as u32;
    let minute = ((remainder % 3600) / 60) as u32;
    let second = (remainder % 60) as u32;
    // Days since 1970-01-01 -> civil date.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}.{micros:06}Z")
}
