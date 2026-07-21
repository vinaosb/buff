//! Error mapper — translates `rustc` diagnostics and runtime panics from the
//! intermediate `.rs` file back to the original `.buff` source location.
//!
//! ## v0.1 strategy
//!
//! Exact Buff line translation requires a populated
//! [`SourceMap`](buff_lang_error::SourceMap) (Buff span ↔ Rust line), which in turn
//! requires a post-`prettyplease` line scan during codegen. For v0.1 we use the
//! simpler **filename translation** approach:
//!
//! - [`translate_rustc_errors`] — replaces the intermediate `.rs` path with the
//!   original `.buff` path in `rustc`'s stderr.
//! - [`translate_panic`] — replaces the `.rs` path embedded in the panic
//!   location string (e.g. `thread 'main' panicked at …, foo.rs:3:5`) with the
//!   `.buff` path. If a [`SourceMap`](buff_lang_error::SourceMap) with line mappings
//!   is available, the Rust line number is also translated to the closest Buff
//!   line; otherwise the Rust line is preserved (still an improvement over
//!   showing the `.rs` filename).
//! - [`filter_backtrace`] — removes Rust-stdlib frames so the user only sees
//!   frames originating from their Buff program.

use std::path::Path;

use buff_lang_codegen_buffhtml::SpanMap;
use buff_lang_error::SourceMap;

/// Replace every occurrence of the `rust_file` path in `stderr` with the
/// `buff_file` path.
///
/// `rustc` diagnostics reference the `.rs` file we generated (e.g.
/// `error[E0384]: … --> temp.rs:3:5`). After translation the same message
/// points at the user's `.buff` source instead.
///
/// Both forward-slash and backslash variants are replaced so the function
/// works on both Unix and Windows `rustc` output.
pub fn translate_rustc_errors(stderr: &str, buff_file: &Path, rust_file: &Path) -> String {
    let rust_str = rust_file.to_string_lossy();
    let buff_str = buff_file.to_string_lossy();
    // Replace both the native path and any slash-variant rustc may emit.
    let mut result = stderr.replace(rust_str.as_ref(), buff_str.as_ref());
    // Also handle the case where rustc normalizes to forward slashes.
    let rust_fwd = rust_str.replace('\\', "/");
    let buff_fwd = buff_str.replace('\\', "/");
    if rust_fwd != rust_str.as_ref() {
        result = result.replace(rust_fwd.as_str(), buff_fwd.as_str());
    }
    result
}

/// Translate a runtime panic message, replacing the `.rs` file reference with
/// the `.buff` file reference.
///
/// If `source_map` contains Rust-line → Buff-span mappings, the Rust line
/// number in the panic location is additionally translated to the closest Buff
/// source line (extracted from the span via the map's front-end
/// [`SourceMap::lookup`]). When the map is empty (v0.1 default), only the
/// filename is translated and the line number is preserved.
///
/// **Example** (v0.1, empty map):
///
/// ```text
/// thread 'main' panicked at 'attempt to divide by zero', prog.rs:2:15
/// ```
/// becomes:
/// ```text
/// thread 'main' panicked at 'attempt to divide by zero', prog.buff:2:15
/// ```
pub fn translate_panic(
    panic_msg: &str,
    rust_file: &Path,
    buff_file: &Path,
    source_map: &SourceMap,
) -> String {
    // Step 1: filename replacement (always).
    let rust_str = rust_file.to_string_lossy();
    let buff_str = buff_file.to_string_lossy();
    let mut result = panic_msg.replace(rust_str.as_ref(), buff_str.as_ref());
    // Also handle forward-slash normalization.
    let rust_fwd = rust_str.replace('\\', "/");
    let buff_fwd = buff_str.replace('\\', "/");
    if rust_fwd != rust_str.as_ref() {
        result = result.replace(rust_fwd.as_str(), buff_fwd.as_str());
    }

    // Step 2: if the source map has line mappings, translate the line number.
    // We look for the pattern "<buff_file>:LINE:COL" that we just produced and
    // replace LINE with the best Buff line. This is best-effort.
    if !source_map.is_line_map_empty() {
        result = translate_panic_line_numbers(&result, buff_file, source_map);
    }

    result
}

/// Given a panic string that already has the `.buff` filename substituted in,
/// find `<buff_file>:RUST_LINE:COL` patterns and replace `RUST_LINE` with the
/// closest Buff line from the source map.
fn translate_panic_line_numbers(msg: &str, buff_file: &Path, source_map: &SourceMap) -> String {
    let buff_str = buff_file.to_string_lossy();
    let mut result = String::with_capacity(msg.len());
    let mut rest = msg;

    while let Some(pos) = rest.find(buff_str.as_ref()) {
        // Copy everything up to and including the filename.
        result.push_str(&rest[..pos + buff_str.len()]);
        rest = &rest[pos + buff_str.len()..];

        // Expect a ':' followed by digits (the line number).
        if let Some(colon_pos) = rest.find(':') {
            let after_colon = &rest[colon_pos + 1..];
            // Parse the run of ASCII digits.
            let digit_end = after_colon
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_digit())
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);

            if digit_end > 0 {
                let line_str = &after_colon[..digit_end];
                if let Ok(rust_line) = line_str.parse::<usize>() {
                    if let Some(buff_span) = source_map.lookup_buff(rust_line) {
                        // Look up the Buff line from the span's source_id.
                        if let Some((buff_line, _)) =
                            source_map.lookup(buff_span.source_id, buff_span.start)
                        {
                            result.push(':');
                            result.push_str(&buff_line.to_string());
                            // Skip past the original digits.
                            rest = &after_colon[digit_end..];
                            continue;
                        }
                    }
                }
            }
            // Line parse failed — copy the ':' verbatim and continue.
            result.push(':');
            rest = &rest[colon_pos + 1..];
        } else {
            // No ':' after filename — nothing more to translate.
            result.push_str(rest);
            return result;
        }
    }
    // Copy any remaining text after the last filename occurrence.
    result.push_str(rest);
    result
}

/// Filter a Rust backtrace to hide stdlib frames.
///
/// Removes lines that reference Rust's standard library or toolchain paths,
/// which are noise from the Buff user's perspective. The patterns filtered:
///
/// - `rustc/` (toolchain root, e.g. `…/rust/toolchains/…/rustc/…`)
/// - `rustlib` (rustlib sysroot)
/// - `/std/` and `\std\` (standard library source/modules)
/// - `/core/` and `\core\` (core library)
/// - `/alloc/` and `\alloc\` (alloc library)
///
/// Frames from the user's code (or the generated `.rs` file) are preserved.
pub fn filter_backtrace(backtrace: &str) -> String {
    backtrace
        .lines()
        .filter(|line| {
            !line.contains("rustc")
                && !line.contains("rustlib")
                && !line.contains("/std/")
                && !line.contains("\\std\\")
                && !line.contains("/core/")
                && !line.contains("\\core\\")
                && !line.contains("/alloc/")
                && !line.contains("\\alloc\\")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// T133: `.buffhtml` SFC error translation (filename + SpanMap).
// ---------------------------------------------------------------------------

/// T133: Translate `rustc` diagnostics from a `.buffhtml`-generated `.rs`
/// file back to the originating `.buffhtml` source location.
///
/// This is the span-aware sibling of [`translate_rustc_errors`]: in
/// addition to the filename translation (always applied), it consumes the
/// post-format [`SpanMap`] produced by
/// [`buff_lang_codegen_buffhtml::generate`] to reverse-map each `--> foo.rs:LINE:COL`
/// diagnostic reference to the `.buffhtml` line:col.
///
/// # Algorithm (per the span-mapping spike VERDICT PASS,
/// `.sisyphus/evidence/task-133-span-mapping-spike.txt`)
///
/// 1. Replace every occurrence of the `rust_file` path with the
///    `buffhtml_file` path (filename-only fallback — same as
///    [`translate_rustc_errors`] does for `.buff`).
/// 2. For each `<buffhtml_file>:<line>:<col>` triple in the translated
///    stderr (i.e. the lines rustc emitted against the `.rs` file, now
///    pointing at the `.buffhtml` filename): query
///    [`SpanMap::map_span`]\`(<line>, <col>)` to find the nearest preceding
///    anchor's `.buffhtml` byte span. If found, the rustc line:col is
///    replaced with the `.buffhtml` source line:col derived from
///    `buffhtml_source` byte offset lookup. If not found, the rustc
///    line:col is preserved (still pointing at the `.buffhtml` filename
///    — strictly better than `.rs`).
///
/// When `span_map` is empty, behaviour degrades gracefully to
/// filename-only translation (the T121b mitigation #1 baseline, marked
/// TODO(buffhtml-span) per the spike's mitigation #3).
pub fn translate_buffhtml_rustc_errors(
    stderr: &str,
    buffhtml_file: &Path,
    rust_file: &Path,
    span_map: &SpanMap,
    buffhtml_source: &str,
) -> String {
    // Step 1: filename replacement (always).
    let after_filename = translate_rustc_errors(stderr, buffhtml_file, rust_file);

    // Step 2: per-line span reverse-mapping.
    if span_map.is_empty() {
        // SpanMap empty — filename-only translation is the best we can do.
        // (T121b mitigation #1 baseline; spike recommendation recorded as
        // TODO(buffhtml-span) for the empty-map case.)
        return after_filename;
    }
    translate_buffhtml_lines(&after_filename, buffhtml_file, span_map, buffhtml_source)
}

/// Walk a (filename-translated) stderr buffer, find every
/// `<buffhtml_file>:RUST_LINE:RUST_COL` triple, and replace
/// (RUST_LINE, RUST_COL) with the corresponding `.buffhtml` (line, col)
/// from the [`SpanMap`].
fn translate_buffhtml_lines(
    msg: &str,
    buffhtml_file: &Path,
    span_map: &SpanMap,
    buffhtml_source: &str,
) -> String {
    let buffhtml_str = buffhtml_file.to_string_lossy();
    // Pre-compute the byte-offset -> (line, col) lookup table for the
    // .buffhtml source. The SpanMap gives us byte spans; we resolve them
    // to 1-based line:col here.
    let line_starts: Vec<usize> = byte_line_starts(buffhtml_source);

    let mut out = String::with_capacity(msg.len());
    let mut rest = msg;
    while let Some(pos) = rest.find(buffhtml_str.as_ref()) {
        out.push_str(&rest[..pos + buffhtml_str.len()]);
        rest = &rest[pos + buffhtml_str.len()..];

        // Expect `:LINE:COL` after the filename.
        if let Some(after_colon) = rest.strip_prefix(':') {
            // Parse two runs of digits separated by ':'.
            let (l1, l1_end) = match parse_digits(after_colon) {
                Some(t) => t,
                None => {
                    out.push(':');
                    out.push_str(after_colon);
                    return out;
                }
            };
            let after_l1 = &after_colon[l1_end..];
            let rest2 = match after_l1.strip_prefix(':') {
                Some(r) => r,
                None => {
                    out.push(':');
                    out.push_str(after_colon);
                    return out;
                }
            };
            let (l2, l2_end) = match parse_digits(rest2) {
                Some(t) => t,
                None => {
                    out.push(':');
                    out.push_str(after_colon);
                    return out;
                }
            };
            // Map via SpanMap; on miss keep the original line:col.
            let translated = span_map
                .map_span(l1, l2)
                .and_then(|span| byte_offset_to_line_col(span.start, &line_starts));
            match translated {
                Some((bh_line, bh_col)) => {
                    out.push(':');
                    out.push_str(&bh_line.to_string());
                    out.push(':');
                    out.push_str(&bh_col.to_string());
                }
                None => {
                    // Preserve original rustc line:col.
                    out.push(':');
                    out.push_str(&l1.to_string());
                    out.push(':');
                    out.push_str(&l2.to_string());
                }
            }
            rest = &rest2[l2_end..];
        } else {
            // No ':' after filename — nothing more to translate.
            out.push_str(rest);
            return out;
        }
    }
    out.push_str(rest);
    out
}

/// Build a sorted `Vec` of byte offsets where each line starts (0-based
/// offsets; line 1 starts at offset 0, line N starts at the offset of the
/// Nth `\n` + 1). Used by [`byte_offset_to_line_col`].
fn byte_line_starts(src: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Resolve a byte offset to 1-based (line, col) using a precomputed
/// `line_starts` table from [`byte_line_starts`]. Returns `None` if the
/// offset is out of range.
fn byte_offset_to_line_col(offset: usize, line_starts: &[usize]) -> Option<(usize, usize)> {
    let line_idx = line_starts
        .partition_point(|&s| s <= offset)
        .checked_sub(1)?;
    let line_start = *line_starts.get(line_idx)?;
    // offset >= line_start is guaranteed by partition_point semantics; use
    // saturating_sub for safety in case of unexpected input.
    let col = offset.saturating_sub(line_start) + 1;
    Some((line_idx + 1, col))
}

/// Parse a run of ASCII digits at the start of `s`, returning the parsed
/// `usize` value and the byte length consumed.
fn parse_digits(s: &str) -> Option<(usize, usize)> {
    let end = s
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    if end == 0 {
        return None;
    }
    let val = s[..end].parse::<usize>().ok()?;
    Some((val, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_codegen_buffhtml::SpanMapBuilder;
    use buff_lang_error::{SourceId, Span};
    use std::path::{Path, PathBuf};

    fn rust_path() -> PathBuf {
        PathBuf::from("/tmp/prog.rs")
    }

    fn buff_path() -> PathBuf {
        PathBuf::from("/tmp/prog.buff")
    }

    fn buffhtml_path() -> PathBuf {
        PathBuf::from("/tmp/counter.buffhtml")
    }

    fn buffhtml_rust_path() -> PathBuf {
        // The generated .rs path for `/tmp/counter.buffhtml` (matches
        // the codegen convention: same stem + .rs extension, alongside).
        PathBuf::from("/tmp/counter.rs")
    }

    #[test]
    fn translate_rustc_errors_replaces_path() {
        let stderr = "error[E0384]: cannot assign\n  --> /tmp/prog.rs:15:5\n   |\n15 |     x = 2;\n   |     ^^^^^";
        let result = translate_rustc_errors(stderr, &buff_path(), &rust_path());
        assert!(
            result.contains("prog.buff:15:5"),
            "expected buff path in result, got: {result}"
        );
        assert!(
            !result.contains("prog.rs:"),
            "should not contain .rs path, got: {result}"
        );
    }

    #[test]
    fn translate_panic_replaces_path_empty_map() {
        let sm = SourceMap::new();
        let panic = "thread 'main' panicked at 'boom', /tmp/prog.rs:2:15\nnote: run with `RUST_BACKTRACE=1`";
        let result = translate_panic(panic, &rust_path(), &buff_path(), &sm);
        assert!(
            result.contains("prog.buff:2:15"),
            "expected buff path + original line, got: {result}"
        );
        assert!(
            !result.contains("prog.rs:"),
            "should not contain .rs path, got: {result}"
        );
    }

    #[test]
    fn filter_backtrace_hides_stdlib() {
        let bt = "  0: std::panicking::begin_panic\n  1: prog::main\n             at /rustc/abc/lib/std/panicking.rs:5\n  2: core::fmt::write\n             at C:\\rust\\toolchains\\stable\\rustlib\\src\\core\\fmt.rs:10\n  3: prog::helper\n             at /tmp/prog.rs:5";
        let filtered = filter_backtrace(bt);
        assert!(
            !filtered.contains("rustc"),
            "should filter rustc frames: {filtered}"
        );
        assert!(
            !filtered.contains("rustlib"),
            "should filter rustlib frames: {filtered}"
        );
        assert!(
            filtered.contains("prog::main"),
            "should keep user frame prog::main: {filtered}"
        );
        assert!(
            filtered.contains("prog::helper"),
            "should keep user frame prog::helper: {filtered}"
        );
    }

    #[test]
    fn translate_rustc_errors_no_match_returns_unchanged() {
        let stderr = "some message without a path";
        let result = translate_rustc_errors(stderr, &buff_path(), &rust_path());
        assert_eq!(result, stderr, "unchanged when no path present");
    }

    #[test]
    fn translate_panic_translates_line_with_map() {
        // Build a source map: rust line 5 → buff span at byte 10 (line 2).
        let mut sm = SourceMap::new();
        let sid = buff_lang_error::SourceId(0);
        sm.add_source(
            sid,
            PathBuf::from("prog.buff"),
            "line1\nline2\nline3\nline4\nline5\n".to_string(),
        );
        // Byte offset 6 = start of "line2" = line 2.
        let buff_span = buff_lang_error::Span::new(6, 11, sid);
        sm.add_mapping(buff_span, 5);

        let panic = "thread 'main' panicked at 'boom', prog.rs:5:10";
        let result = translate_panic(panic, Path::new("prog.rs"), Path::new("prog.buff"), &sm);
        // The rust line 5 should map to buff line 2.
        assert!(
            result.contains("prog.buff:2:"),
            "expected line 2 (translated from rust 5), got: {result}"
        );
    }

    #[test]
    fn filter_backtrace_empty_input() {
        let filtered = filter_backtrace("");
        assert_eq!(filtered, "");
    }

    #[test]
    fn filter_backtrace_preserves_all_user_frames() {
        let bt = "  0: myapp::main\n  1: myapp::helper\n  2: myapp::foo";
        let filtered = filter_backtrace(bt);
        assert_eq!(filtered, bt, "all user frames preserved");
    }

    // -------------------------------------------------------------------------
    // T133: .buffhtml span-aware translation.
    // -------------------------------------------------------------------------

    fn span(start: usize, end: usize) -> Span {
        Span::new(start, end, SourceId(0))
    }

    #[test]
    fn translate_buffhtml_filename_only_when_spanmap_empty() {
        let sm = SpanMap::default();
        let stderr = "error[E0425]: cannot find `count`\n  --> /tmp/counter.rs:7:15";
        let result = translate_buffhtml_rustc_errors(
            stderr,
            &buffhtml_path(),
            &buffhtml_rust_path(),
            &sm,
            "ignored", // source not used when span_map is empty
        );
        assert!(
            result.contains("counter.buffhtml:7:15"),
            "expected buffhtml filename + original rustc line:col, got: {result}"
        );
        assert!(
            !result.contains("counter.rs:"),
            "should not contain .rs path, got: {result}"
        );
    }

    #[test]
    fn translate_buffhtml_span_translates_linecol() {
        // buffhtml source where `count` lives at line 4.
        let buffhtml_source = "line1\nline2\nline3\n<div>{count}</div>\n";
        let count_offset = buffhtml_source.find("count").unwrap_or(0);
        let mut builder = SpanMapBuilder::default();
        builder.add_anchor("count", span(count_offset, count_offset + 5));
        // The .rs source the SpanMap scans for anchors:
        let rs_source = "fn foo() {\n    let x = count;\n}\n"; // count on line 2 col 12
        let sm = builder.finalize(rs_source);

        let stderr = "error[E0425]: cannot find `count` in this scope\n  --> /tmp/counter.rs:2:13";
        let result = translate_buffhtml_rustc_errors(
            stderr,
            &buffhtml_path(),
            &buffhtml_rust_path(),
            &sm,
            buffhtml_source,
        );
        assert!(
            !result.contains("counter.rs:"),
            "should not contain .rs path: {result}"
        );
        assert!(
            result.contains("counter.buffhtml:"),
            "should contain .buffhtml filename: {result}"
        );
        // The translated line:col should be remapped to .buffhtml source
        // line 4 (where `count` lives).
        assert!(
            result.contains("counter.buffhtml:4:"),
            "expected line 4 (buffhtml), got: {result}"
        );
    }

    #[test]
    fn translate_buffhtml_span_miss_keeps_original_linecol() {
        // span_map has an anchor at (1, 5) (where zzz_present... is in the
        // rs source). The SpanMap's nearest-preceding-anchor semantics mean
        // any query AFTER the first anchor gets mapped to *some* buffhtml
        // span. To genuinely test the "miss" path, we query a position
        // BEFORE the first anchor — that returns None and preserves the
        // original rustc line:col.
        let buffhtml_source = "x\n";
        let mut builder = SpanMapBuilder::default();
        builder.add_anchor("zzz_present_in_rs_only", span(0, 1));
        let sm = builder.finalize("let zzz_present_in_rs_only = 1;\n");

        // rustc reports an error at line 1 col 1 (before the anchor at line 1 col 5).
        let stderr = "error: something --> /tmp/counter.rs:1:1";
        let result = translate_buffhtml_rustc_errors(
            stderr,
            &buffhtml_path(),
            &buffhtml_rust_path(),
            &sm,
            buffhtml_source,
        );
        assert!(
            result.contains("counter.buffhtml:1:1"),
            "miss (before first anchor) should preserve rustc line:col: {result}"
        );
    }

    #[test]
    fn byte_line_starts_handles_simple_text() {
        let starts = byte_line_starts("a\nbc\nd");
        assert_eq!(starts, vec![0, 2, 5]);
    }

    #[test]
    fn byte_offset_to_line_col_resolves_correctly() {
        let starts = vec![0, 2, 5]; // "a\nbc\nd"
        assert_eq!(byte_offset_to_line_col(0, &starts), Some((1, 1))); // 'a'
        assert_eq!(byte_offset_to_line_col(1, &starts), Some((1, 2))); // '\n'
        assert_eq!(byte_offset_to_line_col(2, &starts), Some((2, 1))); // 'b'
        assert_eq!(byte_offset_to_line_col(3, &starts), Some((2, 2))); // 'c'
        assert_eq!(byte_offset_to_line_col(5, &starts), Some((3, 1))); // 'd'
    }

    #[test]
    fn parse_digits_basic() {
        assert_eq!(parse_digits("123abc"), Some((123, 3)));
        assert_eq!(parse_digits("0"), Some((0, 1)));
        assert_eq!(parse_digits("abc"), None);
    }
}
