//! `buff bench` — benchmark harness + baseline capture (T22).
//!
//! Thin command-level wrapper around the [`crate::bench_harness`] module.
//! Measures the 6 canonical fixtures, builds a [`BenchReport`] with the
//! per-fixture metrics, and writes it as pretty-printed JSON to the
//! configured output path.
//!
//! # Output
//!
//! Prints a one-line-per-fixture summary table to stdout + writes the
//! full JSON report to [`bench_harness::DEFAULT_BASELINE_PATH`] (or the
//! `--output <PATH>` override). The parent dir is created on demand.
//!
//! # Exit codes
//!
//! Always exits 0 — fixture failures are recorded in the JSON `error`
//! field rather than propagated. The harness's job is to measure
//! everything, including failures (the deliverable is "all 6 fixtures
//! measured", not "all 6 fixtures compiled").
//!
//! # Why a wrapper module?
//!
//! Mirrors the pattern established by `commands/bench_compile.rs` +
//! `commands/bench_cold_start.rs`: a thin `run(...)` entry that delegates
//! to the harness library, so the harness logic stays unit-testable
//! without going through `clap::Parser`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use buff_lang_cli::bench_harness::{
    self, build_report, detect_host, git_short_sha, iso8601_now, measure_fixture, resolve_fixtures,
    BenchReport, DEFAULT_BASELINE_PATH, DEFAULT_FIXTURES_DIR, FIXTURE_NAMES,
};

/// Entry point for `buff bench`.
///
/// `output` overrides the JSON path (default:
/// [`DEFAULT_BASELINE_PATH`]). `fixtures_dir` overrides the fixtures
/// directory (default: [`DEFAULT_FIXTURES_DIR`]). `no_backend` skips
/// the rustc invocation entirely (front-end metrics only — useful on
/// hosts where the linker is known-broken).
pub fn run(output: Option<&Path>, fixtures_dir: Option<&Path>, no_backend: bool) -> Result<()> {
    let output_path: PathBuf = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_BASELINE_PATH));
    let fixtures_dir_path: PathBuf = fixtures_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FIXTURES_DIR));

    println!("buff bench (T22) — v1.25 baseline capture");
    println!("fixtures dir: {}", fixtures_dir_path.display());
    println!("output:       {}", output_path.display());
    println!(
        "backend:      {}",
        if no_backend {
            "skipped (--no-backend)"
        } else {
            "enabled"
        }
    );
    println!();

    // Resolve + measure each fixture.
    let fixtures = resolve_fixtures(&fixtures_dir_path, FIXTURE_NAMES);
    if fixtures.is_empty() {
        anyhow::bail!(
            "no fixtures found under `{}` matching {:?} — \
             run from the repo root or pass --fixtures-dir",
            fixtures_dir_path.display(),
            FIXTURE_NAMES
        );
    }

    println!(
        "{:<20} {:>8} {:>8} {:>10} {:>10} {:>14}",
        "fixture", "lex_ms", "parse_ms", "tc_ms", "cg_ms", "codegen_hash[..16]"
    );
    println!("{:-<80}", "");

    let mut measurements = Vec::with_capacity(fixtures.len());
    for (path, name) in &fixtures {
        let m = measure_fixture(path, !no_backend)
            .with_context(|| format!("measurement failed for `{}`", path.display()))?;
        let hash_tail = m
            .codegen_hash
            .as_ref()
            .and_then(|h| h.get(7..7 + 16))
            .unwrap_or("<none>");
        let err_suffix = m
            .error
            .as_ref()
            .map(|e| format!("  ERR: {e}"))
            .unwrap_or_default();
        println!(
            "{:<20} {:>8} {:>8} {:>10} {:>10} {:>14}{}",
            name, m.lex_ms, m.parse_ms, m.typecheck_ms, m.codegen_ms, hash_tail, err_suffix,
        );
        measurements.push(m);
    }

    // Build the aggregate report.
    let report: BenchReport =
        build_report(iso8601_now(), git_short_sha(), detect_host(), measurements);

    // Serialise + write.
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    let json =
        serde_json::to_string_pretty(&report).context("failed to serialise baseline JSON")?;
    std::fs::write(&output_path, format!("{json}\n"))
        .with_context(|| format!("failed to write `{}`", output_path.display()))?;

    println!();
    println!(
        "wrote {} fixtures → {} ({} bytes)",
        report.fixtures.len(),
        output_path.display(),
        json.len(),
    );

    // Aggregate dispatch-hint summary on stdout (informational).
    if !report.dispatch_decisions.is_empty() {
        let mut summary: Vec<String> = report
            .dispatch_decisions
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        summary.sort();
        println!("dispatch hints: {}", summary.join(", "));
    }

    Ok(())
}
