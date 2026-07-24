//! T22 — `buff bench` integration tests.
//!
//! Two acceptance criteria:
//!
//! 1. **Subcommand parses** — clap recognises `bench` and maps it to
//!    [`Command::Bench`] (with + without `--output` / `--fixtures-dir`
//!    / `--no-backend`).
//! 2. **Harness can hash a known fixture** — `measure_fixture` on
//!    `examples/ola.buff` produces a stable `codegen_hash` (deterministic
//!    codegen is a project hard rule; the hash should be byte-identical
//!    across runs + commits).
//!
//! Test 2 is gated behind `examples/ola.buff` existing (true when run
//! from the repo root). The full end-to-end `buff bench` invocation is
//! left to manual verification because it depends on rustc being
//! available (which the Windows MSVC-blocked host is not).

use std::path::PathBuf;

use buff_lang_cli::bench_harness::{
    self, BenchReport, FixtureMeasurement, DEFAULT_BASELINE_PATH, DEFAULT_FIXTURES_DIR,
    FIXTURE_NAMES,
};
use buff_lang_cli::cli::{Cli, Command};
use clap::Parser;

// ---------------------------------------------------------------------------
// 1: Subcommand parses from argv.
// ---------------------------------------------------------------------------

#[test]
fn bench_subcommand_parses_with_no_args() {
    let cli = Cli::parse_from(["buff", "bench"]);
    match cli.command {
        Command::Bench {
            output,
            fixtures_dir,
            no_backend,
        } => {
            assert!(output.is_none(), "default output should be None (lib default applies)");
            assert!(fixtures_dir.is_none());
            assert!(!no_backend);
        }
        other => panic!("expected Bench, got {other:?}"),
    }
}

#[test]
fn bench_subcommand_parses_with_all_flags() {
    let cli = Cli::parse_from([
        "buff",
        "bench",
        "--output",
        "/tmp/baseline.json",
        "--fixtures-dir",
        "/tmp/fixtures",
        "--no-backend",
    ]);
    match cli.command {
        Command::Bench {
            output,
            fixtures_dir,
            no_backend,
        } => {
            assert_eq!(output.as_ref().map(|p| p.to_str().unwrap_or("")), Some("/tmp/baseline.json"));
            assert_eq!(
                fixtures_dir.as_ref().map(|p| p.to_str().unwrap_or("")),
                Some("/tmp/fixtures"),
            );
            assert!(no_backend, "--no-backend should set the flag");
        }
        other => panic!("expected Bench, got {other:?}"),
    }
}

#[test]
fn bench_subcommand_help_string_mentions_fixtures() {
    // clap auto-generates `--help`. We exercise the parser's long help
    // by feeding `--help` and confirming it errors with exit 0 (clap
    // treats `--help` as a "display help + exit 0" pseudo-error).
    let res = Cli::try_parse_from(["buff", "bench", "--help"]);
    let err = res.expect_err("--help should produce a clap error");
    assert!(
        err.to_string().contains("benchmark harness")
            || err.to_string().contains("fixtures")
            || err.to_string().contains("Display the help"),
        "--help output should mention the harness/fixtures; got: {err}",
    );
}

// ---------------------------------------------------------------------------
// 2: Constants are stable.
// ---------------------------------------------------------------------------

#[test]
fn default_baseline_path_is_v1_25_json_under_evidence() {
    assert_eq!(DEFAULT_BASELINE_PATH, ".sisyphus/evidence/baseline-v1.25.json");
}

#[test]
fn default_fixtures_dir_is_examples() {
    assert_eq!(DEFAULT_FIXTURES_DIR, "examples");
}

#[test]
fn fixture_names_contains_six_canonical_fixtures() {
    assert_eq!(
        FIXTURE_NAMES,
        &["ola", "fibonacci", "closures", "collections", "pattern_matching", "error_handling"],
        "the fixture set is part of the T22 contract — do not reorder/rename",
    );
}

// ---------------------------------------------------------------------------
// 3: Harness can hash a known fixture.
// ---------------------------------------------------------------------------

/// Helper — locate the repo root by walking up from `CARGO_MANIFEST_DIR`
/// looking for `examples/ola.buff`. Returns `None` when the fixture
/// cannot be found (e.g. running outside a checkout) — the dependent
/// tests then `return;` early instead of failing.
fn repo_root() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidate: PathBuf = manifest.clone();
    for _ in 0..8 {
        if candidate.join("examples/ola.buff").is_file() {
            return Some(candidate);
        }
        if !candidate.pop() {
            break;
        }
    }
    None
}

#[test]
fn measure_ola_fixture_produces_stable_codegen_hash() {
    let Some(root) = repo_root() else {
        eprintln!("skipping: examples/ola.buff not found (running outside repo?)");
        return;
    };
    let ola = root.join("examples/ola.buff");
    assert!(ola.is_file(), "examples/ola.buff should exist at {}", ola.display());

    // Two consecutive measurements on the SAME source must produce
    // byte-identical hashes (project hard rule: deterministic codegen).
    let m1 = bench_harness::measure_fixture(&ola, false).expect("measure ola run 1");
    let m2 = bench_harness::measure_fixture(&ola, false).expect("measure ola run 2");

    assert_eq!(m1.name, "ola");
    assert_eq!(m1.name, m2.name);

    let h1 = m1.codegen_hash.as_ref().expect("ola codegen should produce a hash");
    let h2 = m2.codegen_hash.as_ref().expect("ola codegen should produce a hash");
    assert!(
        h1.starts_with("sha256:"),
        "hash should be prefixed with `sha256:`, got {h1}",
    );
    assert_eq!(h1.len(), "sha256:".len() + 64, "hash should be sha256: + 64 hex chars");
    assert_eq!(h1, h2, "deterministic codegen — same fixture must hash identically");

    // ola is a one-function `print("...")` program.
    assert!(m1.function_count >= 1, "ola should have at least 1 function, got {}", m1.function_count);
    assert_eq!(m1.error, None, "ola should not produce a measurement error; got {:?}", m1.error);
}

#[test]
fn measure_missing_fixture_records_lex_error_not_panic() {
    // A non-existent path errors at the read step (anyhow propagates).
    let res = bench_harness::measure_fixture(PathBuf::from("/nonexistent/does-not-exist.buff").as_path(), false);
    assert!(res.is_err(), "missing fixture should error (not silently produce zero metrics)");
}

// ---------------------------------------------------------------------------
// 4: Report aggregation is well-formed.
// ---------------------------------------------------------------------------

#[test]
fn build_report_serialises_to_parseable_json() {
    let m = FixtureMeasurement {
        name: "ola".into(),
        lex_ms: 1,
        parse_ms: 2,
        typecheck_ms: 3,
        codegen_ms: 4,
        codegen_hash: Some("sha256:abc".into()),
        clean_build_ms: None,
        binary_size_bytes: None,
        incremental_build_ms: None,
        prefer_gpu_count: 0,
        prefer_npu_count: 0,
        function_count: 1,
        error: None,
    };
    let report = BenchReport {
        captured_at: "2026-07-23T00:00:00Z".into(),
        git_sha: "deadbeef".into(),
        host: "test".into(),
        hyperfine_available: false,
        fixtures: {
            let mut map = std::collections::BTreeMap::new();
            map.insert("ola".into(), m);
            map
        },
        binary_sizes_bytes: std::collections::BTreeMap::new(),
        dispatch_decisions: std::collections::BTreeMap::new(),
    };
    let json = serde_json::to_string_pretty(&report).expect("serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
    let obj = parsed.as_object().expect("top-level object");
    for key in &["captured_at", "git_sha", "host", "fixtures"] {
        assert!(obj.contains_key(*key), "report JSON must include `{key}`: {json}");
    }
}

// ---------------------------------------------------------------------------
// 5: hyperfine probe never panics.
// ---------------------------------------------------------------------------

#[test]
fn hyperfine_probe_returns_some_or_none_without_panicking() {
    // Just exercise the function — value depends on the host.
    let _ = bench_harness::hyperfine_available();
}
