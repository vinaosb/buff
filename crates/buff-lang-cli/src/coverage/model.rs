//! Coverage data types shared across the T137 mapping pipeline.
//!
//! These are plain-old-data structures that flow between the parse →
//! map → render stages. They intentionally derive `Debug, Clone,
//! PartialEq` (per repo convention) and contain no fallible methods.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// One line of Rust-level coverage: a hit count observed for a
/// specific 1-based line number in a generated `.rs` file.
///
/// Emitted by [`parse_llvm_cov_json`](super::parse::parse_llvm_cov_json)
/// from `cargo llvm-cov --json` output. Lines with a `count > 0` were
/// executed at least once; `count == 0` indicates an uncovered but
/// instrumented line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustLineHit {
    /// Absolute or relative path to the generated `.rs` file (mirrors
    /// what llvm-cov reports in its `filename` field — usually the
    /// Cargo workspace-relative path).
    pub rust_file: PathBuf,
    /// 1-based line number in `rust_file`.
    pub rust_line: usize,
    /// Number of times the line was hit by the test suite. `0` =
    /// uncovered instrumented line.
    pub count: u64,
}

/// One line of Buff-level coverage: a hit count observed (or inferred)
/// for a specific 1-based line number in a `.buff` source file.
///
/// Produced by [`map_rust_to_buff`](super::map::map_rust_to_buff) from
/// [`RustLineHit`]s + a populated [`SourceMap`](buff_lang_error::SourceMap).
///
/// When llvm-cov reports multiple Rust lines that all map to the same
/// Buff line (e.g. a multi-line Buff expression lowered to several
/// Rust statements), the hit counts are SUMMED so a Buff line is
/// considered covered if any of its Rust translations ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuffLineHit {
    /// Path to the `.buff` source file (resolved from the
    /// [`SourceMap`](buff_lang_error::SourceMap)'s `SourceFile` entries).
    pub buff_file: PathBuf,
    /// 1-based line number in `buff_file`.
    pub buff_line: usize,
    /// Aggregated hit count (sum of all Rust lines that mapped here).
    pub count: u64,
}

/// Per-`.buff`-file aggregated coverage.
///
/// Produced by [`aggregate`](BuffCoverage::aggregate) from a flat list
/// of [`BuffLineHit`]s. The map is keyed by `.buff` file path and
/// sorted by line number inside each file (BTreeMap on `usize`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuffFileCoverage {
    /// `buff_line → hit count`. Sorted ascending by line number.
    pub lines: BTreeMap<usize, u64>,
}

impl BuffFileCoverage {
    /// Number of lines that were hit at least once.
    pub fn covered_lines(&self) -> usize {
        self.lines.values().filter(|c| **c > 0).count()
    }

    /// Total number of instrumented lines (covered + uncovered).
    pub fn total_lines(&self) -> usize {
        self.lines.len()
    }

    /// Percentage of covered lines, in `[0.0, 100.0]`.
    ///
    /// Returns `100.0` for an empty file to avoid a div-by-zero +
    /// to match `cargo-llvm-cov`'s convention of treating no-code files
    /// as fully covered (they have nothing to test).
    pub fn percent(&self) -> f64 {
        if self.total_lines() == 0 {
            return 100.0;
        }
        let covered = self.covered_lines() as f64;
        let total = self.total_lines() as f64;
        (covered / total) * 100.0
    }
}

/// Top-level aggregated Buff coverage: a `buff_file → BuffFileCoverage`
/// map.
///
/// Produces a stable iteration order via the outer BTreeMap (sorted by
/// file path), so report rendering (LCOV / HTML) is deterministic for
/// the same input — a hard repo rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuffCoverage {
    /// `buff_file → per-file coverage`. Sorted by file path.
    pub files: BTreeMap<PathBuf, BuffFileCoverage>,
}

impl BuffCoverage {
    /// Aggregate a flat list of [`BuffLineHit`]s into a [`BuffCoverage`]
    /// by summing hit counts per `(file, line)` pair.
    ///
    /// Deterministic: the BTreeMap key ordering guarantees stable
    /// iteration regardless of input order.
    pub fn aggregate(hits: &[BuffLineHit]) -> Self {
        let mut cov = Self::default();
        for hit in hits {
            let entry = cov.files.entry(hit.buff_file.clone()).or_default();
            *entry.lines.entry(hit.buff_line).or_insert(0) += hit.count;
        }
        cov
    }

    /// Aggregate percentage across all files (covered lines / total
    /// instrumented lines). Empty coverage → `100.0` (see
    /// [`BuffFileCoverage::percent`] rationale).
    pub fn overall_percent(&self) -> f64 {
        let total: usize = self.files.values().map(|f| f.total_lines()).sum();
        if total == 0 {
            return 100.0;
        }
        let covered: usize = self.files.values().map(|f| f.covered_lines()).sum();
        (covered as f64 / total as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn buff_file_coverage_empty_is_100_percent() {
        let f = BuffFileCoverage::default();
        assert_eq!(f.percent(), 100.0);
        assert_eq!(f.covered_lines(), 0);
        assert_eq!(f.total_lines(), 0);
    }

    #[test]
    fn buff_file_coverage_mixed_lines() {
        let mut lines = BTreeMap::new();
        lines.insert(1, 3);
        lines.insert(2, 0);
        lines.insert(5, 1);
        let f = BuffFileCoverage { lines };
        assert_eq!(f.total_lines(), 3);
        assert_eq!(f.covered_lines(), 2);
        // 2/3 = 66.666...% — allow a small float tolerance.
        let pct = f.percent();
        assert!(pct > 66.0 && pct < 67.0, "expected ~66.67%, got {pct}");
    }

    #[test]
    fn buff_coverage_aggregate_sums_duplicate_lines() {
        let hits = vec![
            BuffLineHit {
                buff_file: p("a.buff"),
                buff_line: 1,
                count: 2,
            },
            BuffLineHit {
                buff_file: p("a.buff"),
                buff_line: 1,
                count: 3,
            },
            BuffLineHit {
                buff_file: p("a.buff"),
                buff_line: 4,
                count: 0,
            },
            BuffLineHit {
                buff_file: p("b.buff"),
                buff_line: 10,
                count: 1,
            },
        ];
        let cov = BuffCoverage::aggregate(&hits);
        assert_eq!(cov.files.len(), 2, "two .buff files");
        let a = cov.files.get(&p("a.buff")).expect("a.buff present");
        assert_eq!(a.lines.get(&1).copied(), Some(5), "line 1 sums to 5");
        assert_eq!(a.lines.get(&4).copied(), Some(0), "line 4 uncovered");
        let b = cov.files.get(&p("b.buff")).expect("b.buff present");
        assert_eq!(b.lines.get(&10).copied(), Some(1));
    }

    #[test]
    fn buff_coverage_overall_percent_handles_empty() {
        let cov = BuffCoverage::default();
        assert_eq!(cov.overall_percent(), 100.0);
    }

    #[test]
    fn buff_coverage_overall_percent_aggregates_across_files() {
        let hits = vec![
            BuffLineHit {
                buff_file: p("a.buff"),
                buff_line: 1,
                count: 1,
            },
            BuffLineHit {
                buff_file: p("a.buff"),
                buff_line: 2,
                count: 0,
            },
            BuffLineHit {
                buff_file: p("b.buff"),
                buff_line: 1,
                count: 0,
            },
        ];
        let cov = BuffCoverage::aggregate(&hits);
        // 1 covered of 3 total = 33.33%
        let pct = cov.overall_percent();
        assert!(pct > 33.0 && pct < 34.0, "expected ~33.33%, got {pct}");
    }

    #[test]
    fn buff_coverage_iteration_is_deterministic() {
        // BTreeMap guarantees path-sorted order regardless of input order.
        let hits = vec![
            BuffLineHit {
                buff_file: p("zeta.buff"),
                buff_line: 1,
                count: 1,
            },
            BuffLineHit {
                buff_file: p("alpha.buff"),
                buff_line: 1,
                count: 1,
            },
            BuffLineHit {
                buff_file: p("mid.buff"),
                buff_line: 1,
                count: 1,
            },
        ];
        let cov = BuffCoverage::aggregate(&hits);
        let names: Vec<_> = cov.files.keys().collect();
        assert_eq!(
            names,
            vec![&p("alpha.buff"), &p("mid.buff"), &p("zeta.buff")],
            "BTreeMap should sort file paths"
        );
    }
}
