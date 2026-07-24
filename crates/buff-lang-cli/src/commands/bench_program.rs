//! `buff bench-program <FILE>` — benchmark a Buff program's runtime (T99).
//!
//! Compiles the `.buff` file to a native binary once, then runs it N times
//! (default: 100) measuring wall-clock latency per run. Reports min, avg,
//! max, p50, p99, and standard deviation in a human-readable table.
//!
//! Warmup iterations (default: 10) run before measurement begins so OS
//! caches / JIT warmup don't skew the first measured runs.
//!
//! # Output format
//!
//! ```text
//! buff bench-program (T99) — 100 iterations + 10 warmup
//!
//! metric          value   unit
//! ------          -----   ----
//! min             1.23    ms
//! avg (mean)      1.45    ms
//! max             2.10    ms
//! p50             1.40    ms
//! p99             1.98    ms
//! stddev          0.12    ms
//! ```
//!
//! # Errors
//!
//! Propagates pipeline errors (lex/parse/codegen/rustc). If the compiled
//! binary exits non-zero on any iteration, the error is reported and the
//! benchmark aborts.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::pipeline;

/// Entry point for `buff bench-program <FILE> [--iterations <N>] [--warmup <N>]`.
pub fn run(file: &Path, iterations: u32, warmup: u32) -> Result<()> {
    let total = iterations.max(1);
    let warmup_count = warmup;

    println!("buff bench-program (T99) — {total} iterations + {warmup_count} warmup");
    println!();

    // --- Compile once ---
    let compile_out = pipeline::compile_to_rust(file)
        .with_context(|| format!("failed to compile `{}`", file.display()))?;

    let temp_dir = std::env::temp_dir().join("buff-bench-program");
    std::fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create temp dir `{}`", temp_dir.display()))?;

    let stem = file
        .file_stem()
        .map(|s| s.to_owned())
        .unwrap_or_else(|| std::ffi::OsString::from("buff_bench_program"));
    let exe_path = pipeline::with_exe_extension(&temp_dir.join(stem));

    pipeline::compile_rust_to_exe(
        &compile_out.rust_file_path,
        &exe_path,
        file,
        pipeline::BuildMode::Release,
    )
    .with_context(|| format!("failed to compile rust to exe for `{}`", file.display()))?;

    // --- Warmup ---
    if warmup_count > 0 {
        eprint!("warming up ... ");
        for _ in 0..warmup_count {
            let status = Command::new(&exe_path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .status()
                .with_context(|| format!("failed to run `{}` during warmup", exe_path.display()))?;
            if !status.success() {
                anyhow::bail!(
                    "benchmark binary `{}` exited with code {} during warmup",
                    exe_path.display(),
                    status.code().unwrap_or(-1)
                );
            }
        }
        eprintln!("done");
    }

    // --- Measure ---
    let mut times: Vec<Duration> = Vec::with_capacity(total as usize);

    for i in 0..total {
        let start = Instant::now();
        let status = Command::new(&exe_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .status()
            .with_context(|| {
                format!(
                    "failed to run `{}` on iteration {}",
                    exe_path.display(),
                    i + 1
                )
            })?;
        if !status.success() {
            anyhow::bail!(
                "benchmark binary `{}` exited with code {} on iteration {}",
                exe_path.display(),
                status.code().unwrap_or(-1),
                i + 1
            );
        }
        times.push(start.elapsed());
    }

    // --- Cleanup ---
    let _ = std::fs::remove_file(&exe_path);
    let _ = std::fs::remove_file(&compile_out.rust_file_path);

    // --- Compute statistics ---
    let n = times.len() as f64;
    times.sort();

    let min = times.first().copied().unwrap_or_default();
    let max = times.last().copied().unwrap_or_default();

    let sum: Duration = times.iter().copied().sum();
    let avg = sum.div_f64(n);

    let p50 = percentile(&times, 50.0);
    let p99 = percentile(&times, 99.0);

    let variance = times
        .iter()
        .map(|t| {
            let diff = t.as_secs_f64() - avg.as_secs_f64();
            diff * diff
        })
        .sum::<f64>()
        / n;
    let stddev = Duration::from_secs_f64(variance.sqrt());

    // --- Print table ---
    println!();
    println!("{:<18} {:>10}  {}", "metric", "value", "unit");
    println!("{:-<18} {:->10}  {:-<4}", "", "", "");
    print_row("min", min);
    print_row("avg (mean)", avg);
    print_row("max", max);
    print_row("p50", p50);
    print_row("p99", p99);
    print_row("stddev", stddev);

    Ok(())
}

/// Compute the P-th percentile from a sorted slice of durations.
fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p / 100.0).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Print a table row with the metric name and value in milliseconds.
fn print_row(label: &str, d: Duration) {
    let ms = d.as_secs_f64() * 1000.0;
    println!("{label:<18} {ms:>10.2}  ms");
}
