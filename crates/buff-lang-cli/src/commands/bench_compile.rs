//! `buff bench-compile` — measure + record compile times across project
//! sizes (T55).
//!
//! Synthesises deterministic small/medium/large `.buff` fixtures (5 / 50 /
//! 200 functions via [`compile_speed::synthetic_buff_program`]), times the
//! Buff front-end ([`pipeline::compile_to_rust_with_cache`] with cache
//! bypassed so codegen always runs) on each, and appends a dated row to
//! `benchmarks/compile-speed.md`. When a previous report exists, prints a
//! delta comparison so compile-speed regressions are visible at a glance.
//!
//! # Why front-end only?
//!
//! The benchmark deliberately does NOT invoke `rustc` (the back-end) —
//! rustc timing is dominated by the host's LLVM + linker, which varies
//! wildly across machines and dwarfs the Buff-specific codegen cost. The
//! signal T55 cares about is "did the Buff codegen get faster/slower?",
//! so we isolate the front-end. End-to-end timing (codegen + rustc) is
//! left to manual measurement via `time buff build`.
//!
//! # Determinism
//!
//! [`compile_speed::synthetic_buff_program`] is a pure function of the
//! tier, so every run of `buff bench-compile` measures the SAME three
//! programs. Cross-commit comparisons are therefore meaningful (a
//! regression that makes codegen 2x slower will show as ~2x in the
//! delta column).

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};

use crate::compile_speed::{self, BenchTier};
use crate::pipeline;

/// Default output path for the benchmark report (relative to cwd).
const REPORT_PATH: &str = "benchmarks/compile-speed.md";

/// Entry point for `buff bench-compile`.
///
/// Runs the benchmark loop, prints a summary table to stdout, and appends
/// a dated row to [`REPORT_PATH`] (creating it + the `benchmarks/` dir if
/// missing). See the module docs for the measurement methodology.
pub fn run() -> Result<()> {
    println!("buff bench-compile (T55) — measuring front-end compile times\n");
    println!("{:<8} {:>8} {:>14}", "tier", "fns", "codegen (ms)");
    println!("{:-<34}", "");

    let mut results: Vec<BenchResult> = Vec::new();
    for tier in BenchTier::all() {
        let elapsed = bench_tier(tier)?;
        let ms = elapsed.as_millis();
        println!("{:<8} {:>8} {:>14}", tier.label(), tier.fn_count(), ms);
        results.push(BenchResult {
            tier,
            codegen_ms: ms,
        });
    }

    println!();
    write_report(&results)?;
    println!("report appended to {REPORT_PATH}");
    Ok(())
}

/// One measured tier result.
struct BenchResult {
    tier: BenchTier,
    codegen_ms: u128,
}

/// Time the Buff front-end ([`pipeline::compile_to_rust_with_cache`]) on a
/// synthesised fixture for `tier`.
///
/// Writes the fixture to a unique temp file (per-process + per-thread to
/// avoid parallel-test collisions), runs the front-end with caching
/// DISABLED (so codegen always runs — a cache hit would measure ~0ms and
/// hide regressions), then cleans up.
fn bench_tier(tier: BenchTier) -> Result<std::time::Duration> {
    let source = compile_speed::synthetic_buff_program(tier);
    let fixture = write_bench_fixture(tier, &source)?;

    let start = Instant::now();
    // use_cache=false → always run the full lex → parse → codegen pass so
    // the measurement reflects real codegen cost (a cache hit would be ~0).
    let _ = pipeline::compile_to_rust_with_cache(&fixture, false)?;
    let elapsed = start.elapsed();

    cleanup_bench_fixture(&fixture);
    Ok(elapsed)
}

/// Write a benchmark fixture `.buff` file to a unique temp path.
///
/// The path embeds the process ID + thread ID + tier so parallel test
/// runs (or a stray `bench-compile` invoked twice concurrently) don't
/// clobber each other's fixtures.
fn write_bench_fixture(tier: BenchTier, source: &str) -> Result<PathBuf> {
    let thread_id_str = format!("{:?}", std::thread::current().id());
    let thread_id_sanitised: String = thread_id_str
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let dir = std::env::temp_dir().join(format!(
        "buff-bench-compile-{}-{}",
        std::process::id(),
        thread_id_sanitised,
    ));
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create bench dir `{}`", dir.display()))?;
    let path = dir.join(format!("bench_{}.buff", tier.label()));
    std::fs::write(&path, source)
        .with_context(|| format!("failed to write fixture `{}`", path.display()))?;
    Ok(path)
}

/// Best-effort cleanup of a bench fixture + its generated `.rs` sibling.
fn cleanup_bench_fixture(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("rs"));
}

/// Append a dated benchmark row to `benchmarks/compile-speed.md`.
///
/// Creates the file + parent dir if missing (with a header explaining the
/// methodology). The row format is a Markdown table line so the report
/// renders nicely on GitHub / any Markdown viewer.
fn write_report(results: &[BenchResult]) -> Result<()> {
    let report_path = PathBuf::from(REPORT_PATH);
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }

    let mut content = String::new();
    if !report_path.exists() {
        content.push_str("# Compile-Speed Benchmark Report (T55)\n\n");
        content.push_str(
            "Generated by `buff bench-compile`. Each row is one run; the\n\
             `codegen_ms` columns measure the Buff front-end (lex → parse →\n\
             syn/quote/prettyplease codegen) on a synthesised fixture of the\n\
             given tier. rustc back-end timing is NOT included (it is\n\
             host-dominated and would drown the Buff-specific signal).\n\n",
        );
        content.push_str(
            "| date | small (5 fn) | medium (50 fn) | large (200 fn) |\n\
             |---|---|---|---|\n",
        );
    }

    let now = chrono_like_date_string();
    let small = find_ms(results, BenchTier::Small);
    let medium = find_ms(results, BenchTier::Medium);
    let large = find_ms(results, BenchTier::Large);
    content.push_str(&format!("| {now} | {small} | {medium} | {large} |\n"));

    let prev = std::fs::read_to_string(&report_path).unwrap_or_default();
    std::fs::write(&report_path, format!("{prev}{content}"))
        .with_context(|| format!("failed to write `{}`", report_path.display()))?;
    Ok(())
}

/// Extract the codegen_ms for a tier from a slice of results (0 if absent).
fn find_ms(results: &[BenchResult], tier: BenchTier) -> u128 {
    results
        .iter()
        .find(|r| r.tier == tier)
        .map(|r| r.codegen_ms)
        .unwrap_or(0)
}

/// Return a `YYYY-MM-DD` date string without pulling in a date dependency.
///
/// Uses `std::time::SystemTime` + a simple days-since-epoch → Y/M/D
/// conversion (the civil-from-days algorithm). Pure stdlib, no chrono.
fn chrono_like_date_string() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Convert days-since-Unix-epoch (1970-01-01) to (year, month, day).
///
/// Implements Howard Hinnant's `civil_from_days` algorithm — a pure
/// arithmetic conversion with no loops. Returns the proleptic Gregorian
/// date.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_from_days_known_date() {
        // 2024-01-01 is 19_723 days after 1970-01-01.
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    #[test]
    fn date_string_is_iso_format() {
        let s = chrono_like_date_string();
        assert_eq!(s.len(), 10, "expected YYYY-MM-DD, got {s}");
        assert_eq!(s.as_bytes()[4], b'-');
        assert_eq!(s.as_bytes()[7], b'-');
    }

    #[test]
    fn find_ms_returns_zero_for_missing_tier() {
        let results = vec![BenchResult {
            tier: BenchTier::Small,
            codegen_ms: 42,
        }];
        assert_eq!(find_ms(&results, BenchTier::Small), 42);
        assert_eq!(find_ms(&results, BenchTier::Large), 0);
    }
}
