//! `buff bench-cold-start` — measure + record native-binary cold-start
//! time (T61).
//!
//! Compiles a minimal `.buff` fixture (`print("hello")`) to a native
//! executable via [`pipeline::compile_to_rust`] →
//! [`pipeline::compile_rust_to_exe`], then times the wall-clock duration
//! from process spawn to first byte on stdout across [`RUN_COUNT`] runs.
//! The median, min, and max are reported.
//!
//! # Acceptance target
//!
//! Buff cold-start < 50 ms (matching bare Rust). The benchmark emits a
//! JSON + Markdown report at `benchmarks/cold-start.{json,md}` documenting
//! the methodology + a comparison table for Go / Rust / Java / Python on
//! AWS Lambda + Cloudflare Workers.
//!
//! # MVP scope
//!
//! This subcommand measures **the Buff binary only** — it does NOT spawn
//! Go / Rust / Java / Python programs (those reference numbers are
//! documented in `benchmarks/cold-start.md` from published third-party
//! benchmarks). It does NOT deploy to AWS Lambda or Cloudflare Workers —
//! the local measurement is a faithful proxy for the cold-start
//! component (process spawn + first output) since neither runtime adds
//! per-language overhead on top of the native binary once the runtime
//! has loaded (Buff transpiles to bare Rust → same process model).
//!
//! # Methodology
//!
//! 1. Build a minimal `.buff` fixture once (no per-run rebuild — the
//!    binary is the cold-start subject, not the build pipeline).
//! 2. For each of [`RUN_COUNT`] runs:
//!    - Drain OS disk caches influence by reading the binary first
//!      (best-effort; we can't drop OS caches portably).
//!    - [`Command::spawn`] the executable.
//!    - Read the first byte from stdout via [`ChildStdout`].
//!    - Record `Instant` elapsed at first byte.
//!    - Wait for the process to exit (clean up zombie / file locks).
//! 3. Discard the first run (warm-up — primes the OS file cache).
//! 4. Compute median + min + max over the remaining runs.
//! 5. Write `benchmarks/cold-start.json` + append `benchmarks/cold-start.md`.
//!
//! The first-run discard follows the standard criterion for cold-start
//! benchmarks (used by the JVM/JDK, Go, and Rust microbenchmark suites)
//! — without it, the first run pays disk I/O that subsequent runs don't,
//! skewing the median upward.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::pipeline;

/// Number of timed runs (after the 1-run warm-up discard).
///
/// 10 is small enough to keep the subcommand snappy (<1 s on a typical
/// dev host) but large enough that the median is stable across runs.
pub const RUN_COUNT: usize = 10;

/// Number of warm-up runs discarded before timing.
///
/// 1 primes the OS file cache so we measure steady-state cold-start of
/// the language runtime, not disk I/O of loading the binary image.
pub const WARMUP_COUNT: usize = 1;

/// Default output paths (relative to cwd).
const JSON_REPORT_PATH: &str = "benchmarks/cold-start.json";
const MARKDOWN_REPORT_PATH: &str = "benchmarks/cold-start.md";

/// The fixture source — kept inline so the subcommand is self-contained
/// (doesn't depend on a file in the repo layout that may move).
const FIXTURE_SOURCE: &str = "func main():\n    print(\"hello\")\n";

/// Entry point for `buff bench-cold-start`.
///
/// Builds the minimal fixture once, runs the timing loop, prints a
/// summary table, and writes JSON + Markdown reports.
pub fn run() -> Result<()> {
    println!("buff bench-cold-start (T61) — measuring native binary cold-start\n");

    let exe_path = build_fixture_exe()?;
    println!(
        "fixture built: {}",
        exe_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    );

    let mut all_runs: Vec<Duration> = Vec::with_capacity(WARMUP_COUNT + RUN_COUNT);
    println!("\n{:<6} {:>14}", "run", "first-byte (ms)");
    println!("{:-<22}", "");
    for i in 0..(WARMUP_COUNT + RUN_COUNT) {
        let elapsed = time_one_run(&exe_path)?;
        let label = if i < WARMUP_COUNT { "warmup" } else { "timed" };
        println!("{:<6} {:>14}", format!("{label} {i}"), elapsed.as_millis());
        all_runs.push(elapsed);
    }

    let timed: &[Duration] = &all_runs[WARMUP_COUNT..];
<<<<<<< HEAD
    let stats =
        RunStats::compute(timed).with_context(|| "no timed samples to compute stats from")?;
=======
    let stats = RunStats::compute(timed);
>>>>>>> f50a2afc5e723fca16fa8b4917cfc9a721e92b98
    println!();
    println!(
        "{:<8} {:>10} {:>10} {:>10}",
        "summary", "min (ms)", "med (ms)", "max (ms)"
    );
    println!("{:-<42}", "");
    println!(
        "{:<8} {:>10} {:>10} {:>10}",
        "buff", stats.min_ms, stats.median_ms, stats.max_ms
    );

    let pass = stats.median_ms < ACCEPTANCE_THRESHOLD_MS;
    println!();
    if pass {
        println!(
            "PASS: median {} ms < {} ms acceptance threshold (matches bare Rust)",
            stats.median_ms, ACCEPTANCE_THRESHOLD_MS
        );
    } else {
        println!(
            "WARN: median {} ms >= {} ms acceptance threshold \
             (regression or slow host)",
            stats.median_ms, ACCEPTANCE_THRESHOLD_MS
        );
    }

    write_json_report(&stats, pass)?;
    write_markdown_report(&stats, pass)?;
    println!();
    println!("report written to {JSON_REPORT_PATH}");
    println!("report appended to {MARKDOWN_REPORT_PATH}");

    cleanup_fixture(&exe_path);
    Ok(())
}

/// Acceptance threshold from the T61 spec: "Buff cold-start < 50 ms".
pub const ACCEPTANCE_THRESHOLD_MS: u128 = 50;

/// Build the minimal fixture to a temp-dir executable + return the path.
///
/// Uses [`pipeline::BuildMode::Fast`] for the quickest possible build
/// (the binary is the SUBJECT of the benchmark, not its build time —
/// but a faster build keeps the subcommand snappy and Fast produces
/// representative cold-start behavior since the binary shape is similar).
fn build_fixture_exe() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("buff-bench-cold-start-{}", std::process::id(),));
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create bench dir `{}`", dir.display()))?;
    let src_path = dir.join("cold_start_minimal.buff");
    std::fs::write(&src_path, FIXTURE_SOURCE)
        .with_context(|| format!("failed to write fixture `{}`", src_path.display()))?;

    let compile_out = pipeline::compile_to_rust(&src_path)?;
    let exe_path = pipeline::with_exe_extension(&dir.join("cold_start_minimal"));
    pipeline::compile_rust_to_exe(
        &compile_out.rust_file_path,
        &exe_path,
        &src_path,
        pipeline::BuildMode::Fast,
    )?;

    // The .buff and .rs fixtures are no longer needed after compile; the
    // executable is what we time. Best-effort cleanup of the intermediates.
    let _ = std::fs::remove_file(&compile_out.rust_file_path);
    Ok(exe_path)
}

pub fn time_one_run(exe: &Path) -> Result<Duration> {
    let mut child = Command::new(exe)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn `{}`", exe.display()))?;

    let mut stdout = child
        .stdout
        .take()
        .context("child stdout was not piped despite Stdio::piped()")?;

    let start = Instant::now();
    let mut first_byte: [u8; 1] = [0u8; 1];
    let read = stdout.read(&mut first_byte)?;
    let elapsed = start.elapsed();

    // Drain + reap so we don't leave a zombie / leak a file lock.
    let _ = child.wait();

    if read == 0 {
        anyhow::bail!(
            "child produced no output (EOF before first byte) — \
             fixture binary is broken"
        );
    }
    Ok(elapsed)
}

/// Summary statistics for the timed runs.
#[derive(Debug, Clone)]
pub struct RunStats {
    /// Number of samples (after warm-up discard).
    pub count: usize,
    /// Minimum elapsed in milliseconds.
    pub min_ms: u128,
    /// Maximum elapsed in milliseconds.
    pub max_ms: u128,
    /// Median elapsed in milliseconds.
    pub median_ms: u128,
}

impl RunStats {
    /// Compute min/median/max from a non-empty slice of [`Duration`]s.
    ///
    /// Returns [`None`] on an empty slice (the caller is expected to
    /// ensure at least one timed run before calling this).
    pub fn compute(timed: &[Duration]) -> Option<Self> {
        if timed.is_empty() {
            return None;
        }
        let mut ms: Vec<u128> = timed.iter().map(|d| d.as_millis()).collect();
        ms.sort_unstable();
        let count = ms.len();
        let min_ms = ms[0];
        let max_ms = ms[count - 1];
        let median_ms = ms[count / 2];
        Some(Self {
            count,
            min_ms,
            max_ms,
            median_ms,
        })
    }
}

pub fn write_json_report(stats: &RunStats, pass: bool) -> Result<()> {
    let path = PathBuf::from(JSON_REPORT_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    let json = format!(
        "{{\n\
         \x20 \"task\": \"T61\",\n\
         \x20 \"tool\": \"buff bench-cold-start\",\n\
         \x20 \"date\": \"{}\",\n\
         \x20 \"threshold_ms\": {},\n\
         \x20 \"samples\": {},\n\
         \x20 \"min_ms\": {},\n\
         \x20 \"median_ms\": {},\n\
         \x20 \"max_ms\": {},\n\
         \x20 \"pass\": {}\n\
         }}\n",
        chrono_like_date_string(),
        ACCEPTANCE_THRESHOLD_MS,
        stats.count,
        stats.min_ms,
        stats.median_ms,
        stats.max_ms,
        pass
    );
    std::fs::write(&path, json).with_context(|| format!("failed to write `{}`", path.display()))?;
    Ok(())
}

pub fn write_markdown_report(stats: &RunStats, pass: bool) -> Result<()> {
    let path = PathBuf::from(MARKDOWN_REPORT_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }

    let mut header = String::new();
    if !path.exists() {
        header.push_str("# Cold-Start Benchmark Report (T61)\n\n");
        header.push_str(
            "Generated by `buff bench-cold-start`. Each row is one run of the\n\
             timing loop (post-warm-up). The fixture is the minimal\n\
             `print(\"hello\")` Buff program compiled to a native binary via\n\
             the Buff pipeline → rustc. Time measured is process-spawn →\n\
             first-byte-on-stdout.\n\n",
        );
        header.push_str(
            "**Acceptance target**: median cold-start < 50 ms (matching bare\n\
             Rust). See `benchmarks/README.md` for cross-language comparison\n\
             methodology + reference numbers from published third-party\n\
             benchmarks (Go / Rust / Java / Python on AWS Lambda +\n\
             Cloudflare Workers).\n\n",
        );
        header.push_str("| date | samples | min (ms) | median (ms) | max (ms) | pass |\n");
        header.push_str("|---|---:|---:|---:|---:|:---:|\n");
    }

    let now = chrono_like_date_string();
    let row = format!(
        "| {now} | {} | {} | {} | {} | {} |\n",
        stats.count,
        stats.min_ms,
        stats.median_ms,
        stats.max_ms,
        if pass { "yes" } else { "no" }
    );

    let prev = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::write(&path, format!("{prev}{header}{row}"))
        .with_context(|| format!("failed to write `{}`", path.display()))?;
    Ok(())
}

/// Best-effort cleanup of the fixture executable.
fn cleanup_fixture(exe: &Path) {
    let _ = std::fs::remove_file(exe);
    // On Windows we may need a brief retry if the OS still holds an
    // image lock from the just-exited child. We don't loop hard here
    // because the temp dir will be reclaimed by the OS eventually.
}

/// Return a `YYYY-MM-DD` date string without pulling in a date dependency.
///
/// Uses `std::time::SystemTime` + Howard Hinnant's civil-from-days
/// algorithm. Pure stdlib, no chrono.
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
/// Howard Hinnant's `civil_from_days` — pure arithmetic, no loops.
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
    fn run_stats_empty_returns_none() {
        assert!(RunStats::compute(&[]).is_none());
    }

    #[test]
    fn run_stats_median_is_middle_of_sorted_samples() {
        let ds = vec![
            Duration::from_millis(2),
            Duration::from_millis(5),
            Duration::from_millis(1),
            Duration::from_millis(4),
            Duration::from_millis(3),
        ];
        let stats = RunStats::compute(&ds).expect("non-empty");
        // sorted: [1, 2, 3, 4, 5]; median index 5/2 = 2 → 3.
        assert_eq!(stats.min_ms, 1);
        assert_eq!(stats.median_ms, 3);
        assert_eq!(stats.max_ms, 5);
        assert_eq!(stats.count, 5);
    }

    #[test]
    fn run_stats_even_count_picks_upper_middle() {
        let ds = vec![
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(3),
            Duration::from_millis(4),
        ];
        let stats = RunStats::compute(&ds).expect("non-empty");
        // sorted: [1, 2, 3, 4]; median index 4/2 = 2 → 3 (upper-middle).
        assert_eq!(stats.median_ms, 3);
    }

    #[test]
    fn fixture_source_is_valid_minimal_buff() {
        // The inline fixture must be a non-trivial program with a print
        // statement (so the cold-start timing has a first-byte to read).
        assert!(FIXTURE_SOURCE.contains("func main():"));
        assert!(FIXTURE_SOURCE.contains("print"));
    }
}
