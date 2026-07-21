//! Parser for `cargo llvm-cov --json` (and `llvm-cov export --format=text`)
//! output.
//!
//! Translates the llvm-cov JSON envelope into the flat
//! [`RustLineHit`](super::model::RustLineHit) list consumed by
//! [`map_rust_to_buff`](super::map::map_rust_to_buff).
//!
//! # Format reference
//!
//! `cargo llvm-cov --json --coverage-only` (which wraps
//! `llvm-profdata merge` + `llvm-cov export --format=text`) emits:
//!
//! ```json
//! {
//!   "data": [
//!     {
//!       "files": [
//!         {
//!           "filename": "src/lib.rs",
//!           "summary": { "lines": { "count": 5, "covered": 4, "percent": 80 } },
//!           "segments": [[0,0,3,true,true,false], ...],
//!           "lines": [
//!             { "line_number": 1, "count": 3, "uncovered": false },
//!             { "line_number": 2, "count": 0, "uncovered": true  }
//!           ]
//!         }
//!       ]
//!     }
//!   ],
//!   "type": "llvm.coverage.json.export",
//!   "version": "2.0.1"
//! }
//! ```
//!
//! The `data[*].files[*].lines[*]` array is the per-line hit list we
//! care about. Each entry's `count` is the number of times the line
//! was executed; `0` means uncovered (instrumented but never run).
//!
//! `segments` is an alternative compressed region representation. We
//! prefer `lines` (always present in `cargo-llvm-cov` output) and
//! fall back to deriving line hits from `segments` only if `lines` is
//! missing or empty.
//!
//! # Errors
//!
//! All fallible operations return [`LlvmCovError`]. The parser is
//! deliberately permissive: a missing optional field is treated as
//! `0`/empty rather than an error, so the CLI can still surface
//! partial coverage when llvm-cov's schema shifts slightly between
//! versions. Hard errors are reserved for malformed JSON or a
//! completely missing `data` array.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::model::RustLineHit;

/// Error raised by [`parse_llvm_cov_json`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LlvmCovError {
    /// The JSON could not be parsed by `serde_json`.
    #[error("invalid JSON: {0}")]
    InvalidJson(String),
    /// The `data` array is missing or has the wrong shape. This is
    /// the only structurally-required field — everything else is
    /// optional.
    #[error("missing or malformed `data` array in llvm-cov JSON")]
    MissingDataArray,
}

/// Parse an llvm-cov JSON envelope string into a flat list of
/// [`RustLineHit`]s.
///
/// See the [module docs](self) for the expected format + the permissive
/// parsing rules. The returned hits are sorted by
/// `(rust_file, rust_line)` for deterministic downstream processing.
///
/// # Errors
///
/// - [`LlvmCovError::InvalidJson`] — input is not valid JSON.
/// - [`LlvmCovError::MissingDataArray`] — input is valid JSON but does
///   not contain a `data` array.
pub fn parse_llvm_cov_json(input: &str) -> Result<Vec<RustLineHit>, LlvmCovError> {
    let root: Value =
        serde_json::from_str(input).map_err(|e| LlvmCovError::InvalidJson(e.to_string()))?;
    let data = root
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or(LlvmCovError::MissingDataArray)?;

    let mut hits: Vec<RustLineHit> = Vec::new();
    for entry in data {
        let Some(files) = entry.get("files").and_then(|v| v.as_array()) else {
            // An entry without `files` is unusual but not malformed —
            // skip rather than failing the whole parse.
            continue;
        };
        for file in files {
            collect_file_hits(file, &mut hits);
        }
    }

    // Deterministic output ordering: by file path, then by line number.
    hits.sort_by(|a, b| {
        a.rust_file
            .cmp(&b.rust_file)
            .then(a.rust_line.cmp(&b.rust_line))
    });
    Ok(hits)
}

/// Walk one `data[*].files[*]` object and push its per-line hits.
///
/// Prefers `file.lines[*]` (always emitted by `cargo llvm-cov --json`);
/// falls back to `file.segments[*]` (region encoding) when `lines` is
/// absent or empty.
fn collect_file_hits(file: &Value, out: &mut Vec<RustLineHit>) {
    let Some(filename) = file.get("filename").and_then(|v| v.as_str()) else {
        return;
    };
    let rust_file = PathBuf::from(filename);

    if let Some(lines) = file.get("lines").and_then(|v| v.as_array()) {
        for line in lines {
            if let Some(hit) = parse_line_entry(&rust_file, line) {
                out.push(hit);
            }
        }
        if !out.is_empty() {
            return;
        }
    }

    // Fallback: derive line coverage from the `segments` region table.
    // Each segment is [line, col, count, has_count, is_region_entry].
    // We treat the line as covered when `count > 0`.
    if let Some(segments) = file.get("segments").and_then(|v| v.as_array()) {
        for seg in segments {
            if let Some(hit) = parse_segment_entry(&rust_file, seg) {
                out.push(hit);
            }
        }
    }
}

/// Convert one `lines[*]` entry to a [`RustLineHit`] when both
/// `line_number` and `count` are present.
fn parse_line_entry(rust_file: &Path, line: &Value) -> Option<RustLineHit> {
    let line_number = line.get("line_number")?.as_u64()?;
    let count = line.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
    // llvm-cov emits `count` as a u64 internally; line numbers are 1-based.
    let rust_line = usize::try_from(line_number).ok()?;
    Some(RustLineHit {
        rust_file: rust_file.to_path_buf(),
        rust_line,
        count,
    })
}

/// Convert one `segments[*]` entry to a [`RustLineHit`] (fallback path).
///
/// llvm-cov segment tuple shape: `[line, col, count, has_count, is_region_entry]`.
/// We only consume `line` + `count`; both must be present + numeric.
fn parse_segment_entry(rust_file: &Path, seg: &Value) -> Option<RustLineHit> {
    let arr = seg.as_array()?;
    let line_number = arr.first()?.as_u64()?;
    let count = arr.get(2).and_then(|v| v.as_u64()).unwrap_or(0);
    let rust_line = usize::try_from(line_number).ok()?;
    Some(RustLineHit {
        rust_file: rust_file.to_path_buf(),
        rust_line,
        count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rust_line(file: &str, line: usize, count: u64) -> RustLineHit {
        RustLineHit {
            rust_file: PathBuf::from(file),
            rust_line: line,
            count,
        }
    }

    #[test]
    fn parse_minimal_two_file_example() {
        let json = r#"{
            "type": "llvm.coverage.json.export",
            "version": "2.0.1",
            "data": [
                {
                    "files": [
                        {
                            "filename": "src/lib.rs",
                            "lines": [
                                { "line_number": 1, "count": 3 },
                                { "line_number": 2, "count": 0 },
                                { "line_number": 5, "count": 2 }
                            ]
                        },
                        {
                            "filename": "src/main.rs",
                            "lines": [
                                { "line_number": 10, "count": 1 }
                            ]
                        }
                    ]
                }
            ]
        }"#;
        let hits = parse_llvm_cov_json(json).expect("parses");
        assert_eq!(
            hits,
            vec![
                rust_line("src/lib.rs", 1, 3),
                rust_line("src/lib.rs", 2, 0),
                rust_line("src/lib.rs", 5, 2),
                rust_line("src/main.rs", 10, 1),
            ]
        );
    }

    #[test]
    fn parse_returns_sorted_output() {
        // Deliberately feed files in non-sorted order.
        let json = r#"{
            "data": [{
                "files": [
                    {
                        "filename": "zeta.rs",
                        "lines": [
                            { "line_number": 5, "count": 1 },
                            { "line_number": 2, "count": 1 }
                        ]
                    },
                    {
                        "filename": "alpha.rs",
                        "lines": [
                            { "line_number": 1, "count": 1 }
                        ]
                    }
                ]
            }]
        }"#;
        let hits = parse_llvm_cov_json(json).expect("parses");
        assert_eq!(
            hits,
            vec![
                rust_line("alpha.rs", 1, 1),
                rust_line("zeta.rs", 2, 1),
                rust_line("zeta.rs", 5, 1),
            ]
        );
    }

    #[test]
    fn parse_falls_back_to_segments_when_lines_absent() {
        // Some older llvm-cov versions or different flags omit `lines`
        // and only emit the compressed `segments` region table.
        let json = r#"{
            "data": [{
                "files": [{
                    "filename": "src/lib.rs",
                    "segments": [
                        [1, 0, 3, true, true],
                        [2, 0, 0, true, false],
                        [5, 0, 2, true, true]
                    ]
                }]
            }]
        }"#;
        let hits = parse_llvm_cov_json(json).expect("parses");
        assert_eq!(
            hits,
            vec![
                rust_line("src/lib.rs", 1, 3),
                rust_line("src/lib.rs", 2, 0),
                rust_line("src/lib.rs", 5, 2),
            ]
        );
    }

    #[test]
    fn parse_invalid_json_errors() {
        let err = parse_llvm_cov_json("not json").unwrap_err();
        assert!(matches!(err, LlvmCovError::InvalidJson(_)));
    }

    #[test]
    fn parse_missing_data_array_errors() {
        let err = parse_llvm_cov_json(r#"{"type": "llvm.coverage.json.export"}"#).unwrap_err();
        assert_eq!(err, LlvmCovError::MissingDataArray);
    }

    #[test]
    fn parse_skips_file_without_filename() {
        // Malformed entry — should be silently skipped, not abort the parse.
        let json = r#"{
            "data": [{
                "files": [
                    { "lines": [{ "line_number": 1, "count": 1 }] },
                    {
                        "filename": "ok.rs",
                        "lines": [{ "line_number": 1, "count": 1 }]
                    }
                ]
            }]
        }"#;
        let hits = parse_llvm_cov_json(json).expect("parses");
        assert_eq!(hits, vec![rust_line("ok.rs", 1, 1)]);
    }

    #[test]
    fn parse_tolerates_missing_count_field() {
        // Missing `count` should default to 0 (uncovered) rather than error.
        let json = r#"{
            "data": [{
                "files": [{
                    "filename": "src/lib.rs",
                    "lines": [
                        { "line_number": 1 },
                        { "line_number": 2, "count": 4 }
                    ]
                }]
            }]
        }"#;
        let hits = parse_llvm_cov_json(json).expect("parses");
        assert_eq!(
            hits,
            vec![rust_line("src/lib.rs", 1, 0), rust_line("src/lib.rs", 2, 4),]
        );
    }

    #[test]
    fn parse_empty_data_array_returns_empty_hits() {
        let json = r#"{ "data": [] }"#;
        let hits = parse_llvm_cov_json(json).expect("parses");
        assert!(hits.is_empty());
    }
}
