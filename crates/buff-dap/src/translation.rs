//! Source-map translation layer.
//!
//! The DAP server sits between the editor (which thinks in `.buff`
//! coordinates) and the backend debugger (which thinks in `.rs`
//! coordinates). This module is the pure translation surface that
//! bridges the two coordinate systems via the T60 [`SourceMap`].
//!
//! # Two directions
//!
//! - **`setBreakpoints` direction (buff → rust)**: the editor asks
//!   "set a breakpoint at `foo.buff:42`". We translate `42` to the
//!   corresponding `rust_line` and forward to the backend. See
//!   [`translate_breakpoints_buff_to_rust`].
//! - **`stackTrace` direction (rust → buff)**: the backend reports
//!   a stack frame at `<gen>.rs:87`. We translate `87` to the
//!   corresponding `buff_file:buff_line:buff_col` for display in the
//!   editor. See [`translate_stack_frame_rust_to_buff`] and
//!   [`translate_stack_trace_rust_to_buff`].
//!
//! # Limitations (documented; not v1.10 work)
//!
//! - **`scopes` / `variables` / `evaluate` are pass-through**: the
//!   backend reports locals in Rust terms (post-codegen names may
//!   differ from the user's `.buff` names). Buff-level variable
//!   name translation is future work — see
//!   [`task-136-debugger.txt`](../../../.sisyphus/evidence/task-136-debugger.txt).
//! - **GPU shader debugging**: WGSL shaders run on the GPU; they
//!   have no DAP representation. Documented as out of scope.
//! - **Multi-file `.buff` projects**: T60 SourceMap is single-file
//!   (one .buff → one .rs at a time). Multi-file support requires
//!   codegen changes (deferred; same GAP as T137 coverage).
//!
//! # T60 API surface consumed (READ-ONLY)
//!
//! [`SourceMap`] exposes (from `crates/buff-lang-error/src/source_map.rs`):
//!
//! - `add_source(id, path, content)` — register a source file.
//! - `lookup(id, offset) -> Option<(line, col)>` — byte offset →
//!   1-based (line, col).
//! - `add_mapping(buff_span, rust_line)` — record a mapping.
//! - `lookup_buff(rust_line) -> Option<Span>` — forward (exact +
//!   closest-below fallback).
//! - `lookup_rust(buff_span) -> Option<usize>` — reverse (exact
//!   span match only).
//!
//! Because `lookup_rust` requires an EXACT span match (not a
//! line-based lookup), the buff→rust direction builds the inverse
//! map by scanning candidate `rust_line`s in `[1..=upper_bound]`
//! and matching each returned span's start offset to the requested
//! buff_line. This is O(N) per breakpoint but N is bounded by the
//! source file size; breakpoints are set rarely (once per session).

use std::path::{Path, PathBuf};

use buff_lang_error::{SourceId, SourceMap};

/// Maximum number of `rust_line` candidates scanned when building
/// the buff→rust inverse map per breakpoint. Bounded by the
/// source's total line count + a safety margin to cover cases where
/// codegen expands a single buff statement into multiple rust lines.
const RUST_LINE_SCAN_LIMIT: usize = 10_000;

/// A single translated breakpoint (forward direction).
///
/// Maps 1:1 to a single entry in the `setBreakpointsRequest.body.
/// breakpoints` array the editor sent. The `rust_line` is what we
/// forward to the backend; `translated` is `false` when no source-map
/// entry exists for the requested `buff_line` (in which case we fall
/// back to the identity mapping — `rust_line == buff_line` — so the
/// backend still gets a best-effort location).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedBreakpoint {
    /// The original `.buff` line (1-based, from the editor request).
    pub buff_line: u32,
    /// The translated `.rs` line (1-based, forwarded to backend).
    pub rust_line: u32,
    /// `true` when an exact source-map match was found; `false`
    /// when we fell back to the identity mapping.
    pub translated: bool,
}

/// Translate a set of `.buff` breakpoints to `.rs` lines for the
/// backend's `setBreakpoints` request.
///
/// For each requested `buff_line`, scans candidate `rust_line`s via
/// [`SourceMap::lookup_buff`] and finds the one whose returned
/// [`Span`]'s start offset falls on `buff_line` (computed via
/// [`SourceMap::lookup`]). When no match is found, falls back to
/// the identity mapping (`rust_line = buff_line`) so the backend
/// still gets a location — best-effort for unmapped lines.
///
/// `upper_bound` defaults to [`RUST_LINE_SCAN_LIMIT`] but is
/// parameterized for testability.
pub fn translate_breakpoints_buff_to_rust(
    breakpoints: &[u32],
    source_map: &SourceMap,
    buff_source_id: SourceId,
    buff_source: &str,
) -> Vec<TranslatedBreakpoint> {
    let line_starts = compute_line_starts(buff_source);
    translate_breakpoints_with_scan_limit(
        breakpoints,
        source_map,
        buff_source_id,
        &line_starts,
        RUST_LINE_SCAN_LIMIT,
    )
}

/// Internal: same as [`translate_breakpoints_buff_to_rust`] but
/// takes precomputed line starts + a configurable scan limit
/// (mirrors how `buff-lang-cli::coverage` parameterizes the
/// identity-mapping builder).
fn translate_breakpoints_with_scan_limit(
    breakpoints: &[u32],
    source_map: &SourceMap,
    buff_source_id: SourceId,
    line_starts: &[usize],
    scan_limit: usize,
) -> Vec<TranslatedBreakpoint> {
    // Early-out: when the source map has no buff→rust mappings at
    // all, every breakpoint falls back to the identity mapping.
    // This is the v1.10 stopgap (codegen does not yet emit source-
    // map markers into the .rs — see task-136-debugger.txt GAP-1).
    if source_map.is_line_map_empty() {
        return breakpoints
            .iter()
            .map(|&bl| TranslatedBreakpoint {
                buff_line: bl,
                rust_line: bl,
                translated: false,
            })
            .collect();
    }

    breakpoints
        .iter()
        .map(|&buff_line| {
            translate_one_breakpoint(
                buff_line,
                source_map,
                buff_source_id,
                line_starts,
                scan_limit,
            )
        })
        .collect()
}

/// Translate a single `buff_line` → `rust_line`.
///
/// Strategy: scan `rust_line` in `[1..=scan_limit]` calling
/// `lookup_buff`; for each returned span, compute the buff line via
/// `lookup(span.source_id, span.start)`; the first match wins.
/// When no match, fall back to identity.
fn translate_one_breakpoint(
    buff_line: u32,
    source_map: &SourceMap,
    buff_source_id: SourceId,
    line_starts: &[usize],
    scan_limit: usize,
) -> TranslatedBreakpoint {
    let target_offset = line_start_offset(line_starts, buff_line as usize);

    for rust_line in 1..=scan_limit as u32 {
        if let Some(span) = source_map.lookup_buff(rust_line as usize) {
            // Only consider spans that belong to the requested
            // source file (the editor may have multiple buffers
            // open in the future; v1.10 is single-file).
            if span.source_id != buff_source_id {
                continue;
            }
            // Does this span start on the requested buff line?
            if span.start == target_offset {
                return TranslatedBreakpoint {
                    buff_line,
                    rust_line,
                    translated: true,
                };
            }
        }
    }

    // Fallback: identity mapping (rust_line = buff_line).
    TranslatedBreakpoint {
        buff_line,
        rust_line: buff_line,
        translated: false,
    }
}

/// A single translated stack frame (reverse direction).
///
/// Maps to the `stackTraceResponse.body.stackFrames[N]` entry the
/// editor renders. `buff_file` is the absolute path of the source
/// file; `buff_line` + `buff_col` are 1-based coordinates in that
/// file. `translated` is `false` when no source-map entry exists
/// for the backend's `rust_line` (the editor will see the raw
/// `.rs` location — diagnostic signal that the mapping is missing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedStackFrame {
    /// The backend-reported `.rs` line (1-based).
    pub rust_line: u32,
    /// The translated `.buff` source file path.
    pub buff_file: PathBuf,
    /// The translated `.buff` line (1-based).
    pub buff_line: u32,
    /// The translated `.buff` column (1-based, character-based).
    pub buff_col: u32,
    /// `true` when the source-map had a mapping; `false` when we
    /// passed the rust location through unchanged.
    pub translated: bool,
}

/// Translate a single backend stack frame (rust → buff).
///
/// Returns the original rust_line in the `TranslatedStackFrame`
/// even when no mapping is found (caller can use the `translated`
/// flag to decide display: "show buff_file:buff_line" vs "show
/// <rust_file>:<rust_line>").
pub fn translate_stack_frame_rust_to_buff(
    rust_line: u32,
    source_map: &SourceMap,
    buff_source_id: SourceId,
    buff_file: &Path,
) -> TranslatedStackFrame {
    match source_map.lookup_buff(rust_line as usize) {
        Some(span) if span.source_id == buff_source_id => {
            match source_map.lookup(span.source_id, span.start) {
                Some((line, col)) => TranslatedStackFrame {
                    rust_line,
                    buff_file: buff_file.to_path_buf(),
                    buff_line: line as u32,
                    buff_col: col as u32,
                    translated: true,
                },
                // Span recorded but source file not registered —
                // shouldn't happen, but degrade gracefully.
                None => identity_frame(rust_line, buff_file),
            }
        }
        _ => identity_frame(rust_line, buff_file),
    }
}

/// Translate a batch of backend stack frames (rust → buff).
///
/// Convenience wrapper around [`translate_stack_frame_rust_to_buff`]
/// for the common case where the editor's `stackTrace` request
/// returns many frames at once.
pub fn translate_stack_trace_rust_to_buff(
    rust_lines: &[u32],
    source_map: &SourceMap,
    buff_source_id: SourceId,
    buff_file: &Path,
) -> Vec<TranslatedStackFrame> {
    rust_lines
        .iter()
        .map(|&rl| translate_stack_frame_rust_to_buff(rl, source_map, buff_source_id, buff_file))
        .collect()
}

/// Build an identity (untranslated) frame — used as a fallback.
fn identity_frame(rust_line: u32, buff_file: &Path) -> TranslatedStackFrame {
    TranslatedStackFrame {
        rust_line,
        buff_file: buff_file.to_path_buf(),
        buff_line: rust_line,
        buff_col: 1,
        translated: false,
    }
}

/// Compute the byte offset of each line start in `s` (mirrors
/// `buff_lang_error::SourceFile`'s internal algorithm).
fn compute_line_starts(s: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, c) in s.char_indices() {
        if c == '\n' {
            starts.push(i + c.len_utf8());
        }
    }
    starts
}

/// Look up the byte offset of the start of `line` (1-based) in
/// `line_starts`. Returns `usize::MAX` for out-of-range lines so
/// the comparison `span.start == target_offset` never accidentally
/// matches (a real span start is always a valid byte offset).
fn line_start_offset(line_starts: &[usize], line: usize) -> usize {
    if line == 0 {
        return usize::MAX;
    }
    let idx = line.saturating_sub(1);
    line_starts.get(idx).copied().unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::Span;

    /// Build a synthetic SourceMap populated with a known mapping:
    /// `buff_span(start=B, end=E, source_id=ID)` ↔ `rust_line=R`.
    /// Used by every translation test.
    fn build_map(entries: &[(usize, usize, SourceId, usize)]) -> SourceMap {
        let mut sm = SourceMap::new();
        for &(start, end, source_id, rust_line) in entries {
            sm.add_mapping(Span::new(start, end, source_id), rust_line);
        }
        sm
    }

    /// Compute line starts for a fixed 3-line buff source so we can
    /// reason about byte offsets deterministically.
    ///
    /// `"aaa\nbbb\nccc\n"` → line_starts = [0, 4, 8, 12] (1-based:
    /// line 1 starts at 0, line 2 starts at 4, line 3 starts at 8).
    fn line_starts_for_aaa_bbb_ccc() -> Vec<usize> {
        compute_line_starts("aaa\nbbb\nccc\n")
    }

    #[test]
    fn compute_line_starts_handles_multiline() {
        let starts = line_starts_for_aaa_bbb_ccc();
        assert_eq!(starts, vec![0, 4, 8, 12]);
    }

    #[test]
    fn translate_breakpoint_exact_match_returns_translated() {
        // buff source: "aaa\nbbb\nccc\n"
        // buff line 2 ("bbb") starts at byte 4.
        // Map: span(4, 7, ID0) ↔ rust_line 10.
        let id = SourceId(0);
        let sm = build_map(&[(4, 7, id, 10)]);
        let starts = line_starts_for_aaa_bbb_ccc();
        let result = translate_breakpoints_with_scan_limit(&[2], &sm, id, &starts, 100);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            TranslatedBreakpoint {
                buff_line: 2,
                rust_line: 10,
                translated: true,
            }
        );
    }

    #[test]
    fn translate_breakpoint_no_match_falls_back_to_identity() {
        // Request buff line 99 — way past the source length.
        let id = SourceId(0);
        let sm = build_map(&[(0, 3, id, 1)]);
        let starts = line_starts_for_aaa_bbb_ccc();
        let result = translate_breakpoints_with_scan_limit(&[99], &sm, id, &starts, 100);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].buff_line, 99);
        assert_eq!(result[0].rust_line, 99);
        assert!(!result[0].translated);
    }

    #[test]
    fn translate_breakpoint_empty_source_map_returns_all_identity() {
        let id = SourceId(0);
        let sm = SourceMap::new(); // empty
        let starts = vec![0usize];
        let result = translate_breakpoints_with_scan_limit(&[1, 5, 10], &sm, id, &starts, 100);
        assert_eq!(result.len(), 3);
        for (i, &bl) in [1u32, 5, 10].iter().enumerate() {
            assert_eq!(result[i].buff_line, bl);
            assert_eq!(result[i].rust_line, bl);
            assert!(!result[i].translated);
        }
    }

    #[test]
    fn translate_breakpoint_multiple_breakpoints_mixed() {
        // buff source: "aaa\nbbb\nccc\n"
        // Line 1 → rust 5, Line 3 → rust 15. Line 2 unmapped.
        let id = SourceId(0);
        let sm = build_map(&[(0, 3, id, 5), (8, 11, id, 15)]);
        let starts = line_starts_for_aaa_bbb_ccc();
        let result = translate_breakpoints_with_scan_limit(&[1, 2, 3], &sm, id, &starts, 100);
        assert_eq!(result.len(), 3);
        // Line 1 → rust 5 (translated).
        assert_eq!(result[0].rust_line, 5);
        assert!(result[0].translated);
        // Line 2 → identity (rust 2).
        assert_eq!(result[1].rust_line, 2);
        assert!(!result[1].translated);
        // Line 3 → rust 15 (translated).
        assert_eq!(result[2].rust_line, 15);
        assert!(result[2].translated);
    }

    #[test]
    fn translate_breakpoint_wrong_source_id_skips_entry() {
        // Two source IDs — entry on ID1 should not match a request
        // for ID0.
        let id0 = SourceId(0);
        let id1 = SourceId(1);
        let sm = build_map(&[(0, 3, id1, 5)]);
        let starts = line_starts_for_aaa_bbb_ccc();
        let result = translate_breakpoints_with_scan_limit(&[1], &sm, id0, &starts, 100);
        assert!(!result[0].translated);
        assert_eq!(result[0].rust_line, 1); // identity
    }

    #[test]
    fn translate_breakpoint_scan_limit_respected() {
        // Mapping exists at rust_line 200 but scan limit is 100.
        let id = SourceId(0);
        let sm = build_map(&[(0, 3, id, 200)]);
        let starts = line_starts_for_aaa_bbb_ccc();
        let result = translate_breakpoints_with_scan_limit(&[1], &sm, id, &starts, 100);
        assert!(!result[0].translated);
    }

    #[test]
    fn translate_stack_frame_exact_match_returns_translated() {
        // Span (4, 7, ID0) ↔ rust_line 10. Buff source "aaa\nbbb\nccc\n"
        // → span start 4 → line 2 col 1.
        let id = SourceId(0);
        let mut sm = build_map(&[(4, 7, id, 10)]);
        sm.add_source(
            id,
            PathBuf::from("test.buff"),
            "aaa\nbbb\nccc\n".to_string(),
        );
        let result = translate_stack_frame_rust_to_buff(10, &sm, id, Path::new("test.buff"));
        assert!(result.translated);
        assert_eq!(result.rust_line, 10);
        assert_eq!(result.buff_line, 2);
        assert_eq!(result.buff_col, 1);
        assert_eq!(result.buff_file, Path::new("test.buff"));
    }

    #[test]
    fn translate_stack_frame_no_match_returns_identity() {
        // T60 lookup_buff has a closest-below fallback: when the map
        // is non-empty, ANY rust_line returns the nearest span at or
        // below it. So rust_line=999 with a mapping at line 1
        // returns the span for line 1 (translated=true, buff_line=1).
        //
        // The identity fallback only triggers when the map is EMPTY
        // for this source_id (verified in a separate test below).
        let id = SourceId(0);
        let mut sm = build_map(&[(0, 3, id, 1)]);
        sm.add_source(id, PathBuf::from("test.buff"), "aaa".to_string());
        let result = translate_stack_frame_rust_to_buff(999, &sm, id, Path::new("test.buff"));
        assert!(result.translated); // closest-below: line 1's span
        assert_eq!(result.buff_line, 1);
    }

    #[test]
    fn translate_stack_frame_empty_source_map_returns_identity() {
        // When the source map has no mappings, lookup_buff returns
        // None → identity fallback.
        let id = SourceId(0);
        let mut sm = SourceMap::new();
        sm.add_source(id, PathBuf::from("test.buff"), "aaa".to_string());
        let result = translate_stack_frame_rust_to_buff(999, &sm, id, Path::new("test.buff"));
        assert!(!result.translated);
        assert_eq!(result.buff_line, 999);
    }

    #[test]
    fn translate_stack_frame_wrong_source_id_returns_identity() {
        let id0 = SourceId(0);
        let id1 = SourceId(1);
        let mut sm = build_map(&[(4, 7, id1, 10)]);
        sm.add_source(id1, PathBuf::from("other.buff"), "xxx\nyyy".to_string());
        let result = translate_stack_frame_rust_to_buff(10, &sm, id0, Path::new("test.buff"));
        assert!(!result.translated);
    }

    #[test]
    fn translate_stack_trace_batch_translates_each_frame() {
        // T60 lookup_buff falls back to closest-below. With mappings
        // at 101/102/103, rust_line=9999 returns the span for 103
        // (the highest mapping at or below 9999). So all four frames
        // translate (none fall through to identity).
        let id = SourceId(0);
        let mut sm = build_map(&[(0, 3, id, 101), (4, 7, id, 102), (8, 11, id, 103)]);
        sm.add_source(id, PathBuf::from("test.buff"), "aaa\nbbb\nccc".to_string());
        let result = translate_stack_trace_rust_to_buff(
            &[101, 102, 103, 9999],
            &sm,
            id,
            Path::new("test.buff"),
        );
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].buff_line, 1);
        assert_eq!(result[1].buff_line, 2);
        assert_eq!(result[2].buff_line, 3);
        // 9999 → closest-below (103) → buff line 3 (translated=true).
        assert!(result[3].translated);
        assert_eq!(result[3].buff_line, 3);
    }

    #[test]
    fn translate_stack_trace_empty_map_returns_all_identity() {
        // Empty source map → identity fallback for every frame.
        let id = SourceId(0);
        let mut sm = SourceMap::new();
        sm.add_source(id, PathBuf::from("test.buff"), "aaa".to_string());
        let result =
            translate_stack_trace_rust_to_buff(&[1, 2, 3], &sm, id, Path::new("test.buff"));
        assert_eq!(result.len(), 3);
        for r in &result {
            assert!(!r.translated);
        }
    }

    #[test]
    fn translate_stack_frame_uses_closest_below_fallback() {
        // SourceMap::lookup_buff falls back to the closest rust_line
        // at or below the requested one. So requesting rust_line 12
        // when only rust_line 10 is mapped returns the span for 10.
        let id = SourceId(0);
        let mut sm = build_map(&[(4, 7, id, 10)]);
        sm.add_source(id, PathBuf::from("test.buff"), "aaa\nbbb\nccc".to_string());
        let result = translate_stack_frame_rust_to_buff(12, &sm, id, Path::new("test.buff"));
        // The fallback should translate to buff line 2 (the mapped
        // span's line) — closest-below is the documented T60 behavior.
        assert!(result.translated);
        assert_eq!(result.buff_line, 2);
    }

    #[test]
    fn line_start_offset_handles_zero_line_gracefully() {
        let starts = vec![0, 4, 8];
        assert_eq!(line_start_offset(&starts, 0), usize::MAX);
        assert_eq!(line_start_offset(&starts, 1), 0);
        assert_eq!(line_start_offset(&starts, 2), 4);
        assert_eq!(line_start_offset(&starts, 99), usize::MAX);
    }

    #[test]
    fn translated_breakpoint_is_debug_clone_partial_eq() {
        // Smoke test the derives — ensures the public type satisfies
        // the workspace derive-defaults rule.
        let bp = TranslatedBreakpoint {
            buff_line: 1,
            rust_line: 2,
            translated: true,
        };
        let cloned = bp.clone();
        assert_eq!(bp, cloned);
        let _ = format!("{bp:?}");
    }

    #[test]
    fn translated_stack_frame_is_debug_clone_partial_eq() {
        let frame = TranslatedStackFrame {
            rust_line: 10,
            buff_file: PathBuf::from("foo.buff"),
            buff_line: 5,
            buff_col: 3,
            translated: true,
        };
        let cloned = frame.clone();
        assert_eq!(frame, cloned);
        let _ = format!("{frame:?}");
    }
}
