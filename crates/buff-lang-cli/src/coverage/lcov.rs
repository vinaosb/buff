//! LCOV `.info` report emitter.
//!
//! Renders [`BuffCoverage`](super::model::BuffCoverage) into the
//! standard LCOV tracefile format consumed by `lcov`'s `genhtml`,
//! `coveralls`, `codecov`, and most CI coverage analysis tools.
//!
//! # Format reference
//!
//! LCOV `.info` is a line-oriented text format. Each source file is
//! a sequence of records delimited by `SF:<path>` ... `end_of_record`.
//! Per-line coverage uses `DA:<line>,<count>[,<checksum>]` entries.
//! The `LF` / `LH` lines summarise the total + covered line counts.
//!
//! ```text
//! SF:src/main.buff
//! DA:1,3
//! DA:2,0
//! DA:5,2
//! LF:3
//! LH:2
//! end_of_record
//! ```
//!
//! - `SF` — source file path (relative or absolute).
//! - `DA` — line hit: `<1-based line>,<execution count>`. `0` = uncovered.
//! - `LF` — total lines instrumented (after the `DA` entries).
//! - `LH` — lines hit (`count > 0`).
//! - `end_of_record` — closes the file block.
//!
//! The output is sorted by file path, then by line number — both via
//! [`BuffCoverage`]'s BTreeMap iteration order, which guarantees
//! deterministic output for the same input.

use super::model::BuffCoverage;

/// Render `coverage` as an LCOV `.info` string.
///
/// Empty coverage produces an empty string (no records).
pub fn render_lcov(coverage: &BuffCoverage) -> String {
    let mut out = String::new();
    for (path, file_cov) in &coverage.files {
        out.push_str("SF:");
        out.push_str(&path.display().to_string());
        out.push('\n');
        // Sort by line number — BTreeMap<usize, u64> already iterates
        // in ascending key order.
        for (line, count) in &file_cov.lines {
            out.push_str(&format!("DA:{line},{count}\n"));
        }
        let lf = file_cov.total_lines();
        let lh = file_cov.covered_lines();
        out.push_str(&format!("LF:{lf}\n"));
        out.push_str(&format!("LH:{lh}\n"));
        out.push_str("end_of_record\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn file_cov(entries: &[(usize, u64)]) -> super::super::model::BuffFileCoverage {
        let mut lines = BTreeMap::new();
        for (l, c) in entries {
            lines.insert(*l, *c);
        }
        super::super::model::BuffFileCoverage { lines }
    }

    #[test]
    fn render_empty_coverage_yields_empty_string() {
        let cov = BuffCoverage::default();
        assert_eq!(render_lcov(&cov), "");
    }

    #[test]
    fn render_single_file_single_line_hit() {
        let mut files = BTreeMap::new();
        files.insert(p("main.buff"), file_cov(&[(1, 3)]));
        let cov = BuffCoverage { files };
        let out = render_lcov(&cov);
        assert_eq!(out, "SF:main.buff\nDA:1,3\nLF:1\nLH:1\nend_of_record\n");
    }

    #[test]
    fn render_single_file_with_uncovered_line() {
        let mut files = BTreeMap::new();
        files.insert(p("main.buff"), file_cov(&[(1, 3), (2, 0), (5, 1)]));
        let cov = BuffCoverage { files };
        let out = render_lcov(&cov);
        let expected = "SF:main.buff\n\
            DA:1,3\n\
            DA:2,0\n\
            DA:5,1\n\
            LF:3\n\
            LH:2\n\
            end_of_record\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn render_multi_file_emits_separate_records() {
        let mut files = BTreeMap::new();
        files.insert(p("alpha.buff"), file_cov(&[(1, 1)]));
        files.insert(p("beta.buff"), file_cov(&[(10, 0)]));
        let cov = BuffCoverage { files };
        let out = render_lcov(&cov);
        // BTreeMap iteration is sorted by PathBuf — alpha before beta.
        let expected = "SF:alpha.buff\nDA:1,1\nLF:1\nLH:1\nend_of_record\n\
            SF:beta.buff\nDA:10,0\nLF:1\nLH:0\nend_of_record\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn render_lines_are_sorted_ascending() {
        // Even if a caller constructs the lines map out-of-order,
        // BTreeMap iteration sorts the keys.
        let mut files = BTreeMap::new();
        let mut lines = BTreeMap::new();
        lines.insert(5, 1u64);
        lines.insert(1, 2u64);
        lines.insert(3, 0u64);
        files.insert(
            p("main.buff"),
            super::super::model::BuffFileCoverage { lines },
        );
        let cov = BuffCoverage { files };
        let out = render_lcov(&cov);
        let expected = "SF:main.buff\nDA:1,2\nDA:3,0\nDA:5,1\nLF:3\nLH:2\nend_of_record\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn render_handles_absolute_paths() {
        // LCOV accepts both absolute + relative paths; we don't normalise.
        let mut files = BTreeMap::new();
        files.insert(p("/home/user/proj/main.buff"), file_cov(&[(1, 1)]));
        let cov = BuffCoverage { files };
        let out = render_lcov(&cov);
        assert!(out.starts_with("SF:/home/user/proj/main.buff\n"));
    }
}
