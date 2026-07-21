//! T137: integration tests for `buff coverage` CLI subcommand.
//!
//! Covers the public surface wired up in this task:
//!
//! - `commands::coverage::run` rejects missing path (USER ACTION) +
//!   missing-tool detection (USER ACTION).
//! - `commands::coverage::detect_coverage_tool` runs cleanly with or
//!   without tools installed.
//! - `commands::coverage::html_output_path` /
//!   `lcov_output_path` resolve `--output` correctly.
//! - `commands::coverage::populate_identity_mapping` produces a
//!   SourceMap that translates Rust lines back to the originating
//!   Buff line (the v1.10 identity-mapping stopgap, fully testable
//!   locally).
//! - The full coverage module pipeline (parse → map → render LCOV +
//!   HTML) works end-to-end with a synthetic fixture.
//! - `clap` parses the new `Coverage` variant with the expected flags.
//!
//! The actual `cargo llvm-cov` / `cargo-tarpaulin` invocation is NOT
//! exercised here — that's a USER ACTION (see
//! `.sisyphus/evidence/task-137-coverage-USER-ACTION.txt`). Keeping
//! this test suite tool-free lets it run under CI's fast unit-test
//! gate.

use std::path::PathBuf;

use buff_lang_cli::cli::{Cli, Command};
use buff_lang_cli::commands::coverage::{
    detect_coverage_tool, html_output_path, lcov_output_path, populate_identity_mapping,
    CoverageTool,
};
use buff_lang_error::{SourceId, SourceMap, Span};

use clap::Parser;

// ---------------------------------------------------------------------------
// CLI shape: clap parses `coverage` subcommand.
// ---------------------------------------------------------------------------

#[test]
fn t137_cli_parses_coverage_with_defaults() {
    // `buff coverage examples/ola.buff` — no flags.
    let args = Cli::parse_from(["buff", "coverage", "examples/ola.buff"]);
    match args.command {
        Command::Coverage {
            path,
            html,
            lcov,
            output,
            release,
        } => {
            assert_eq!(
                path.as_deref(),
                Some(std::path::Path::new("examples/ola.buff"))
            );
            assert!(!html);
            assert!(!lcov);
            assert!(output.is_none());
            assert!(!release);
        }
        other => panic!("expected Command::Coverage, got {other:?}"),
    }
}

#[test]
fn t137_cli_parses_coverage_with_all_flags() {
    let args = Cli::parse_from([
        "buff",
        "coverage",
        "examples/ola.buff",
        "--html",
        "--lcov",
        "--output",
        "artifacts/cov",
        "--release",
    ]);
    match args.command {
        Command::Coverage {
            path,
            html,
            lcov,
            output,
            release,
        } => {
            assert_eq!(
                path.as_deref(),
                Some(std::path::Path::new("examples/ola.buff"))
            );
            assert!(html);
            assert!(lcov);
            assert_eq!(
                output.as_deref(),
                Some(std::path::Path::new("artifacts/cov"))
            );
            assert!(release);
        }
        other => panic!("expected Command::Coverage, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Tool detection.
// ---------------------------------------------------------------------------

#[test]
fn t137_detect_coverage_tool_does_not_panic() {
    // We can't assert specific output since CI/local hosts vary —
    // just verify the function is callable + returns Option.
    let _ = detect_coverage_tool();
}

#[test]
fn t137_coverage_tool_as_str_round_trips() {
    assert_eq!(CoverageTool::LlvmCov.as_str(), "cargo-llvm-cov");
    assert_eq!(CoverageTool::Tarpaulin.as_str(), "cargo-tarpaulin");
}

// ---------------------------------------------------------------------------
// Output-path resolution.
// ---------------------------------------------------------------------------

#[test]
fn t137_html_output_path_default() {
    let p = html_output_path(None);
    assert_eq!(p, PathBuf::from("coverage").join("index.html"));
}

#[test]
fn t137_html_output_path_with_html_extension() {
    let p = html_output_path(Some(std::path::Path::new("custom_report.html")));
    assert_eq!(p, PathBuf::from("custom_report.html"));
}

#[test]
fn t137_html_output_path_with_directory() {
    let p = html_output_path(Some(std::path::Path::new("artifacts/cov")));
    assert_eq!(p, PathBuf::from("artifacts/cov").join("index.html"));
}

#[test]
fn t137_lcov_output_path_default() {
    let p = lcov_output_path(None);
    assert_eq!(p, PathBuf::from("coverage").join("lcov.info"));
}

#[test]
fn t137_lcov_output_path_with_info_extension() {
    let p = lcov_output_path(Some(std::path::Path::new("custom.info")));
    assert_eq!(p, PathBuf::from("custom.info"));
}

// ---------------------------------------------------------------------------
// Identity-mapping stopgap.
// ---------------------------------------------------------------------------

#[test]
fn t137_populate_identity_mapping_resolves_each_line() {
    // The identity mapping should produce a SourceMap that translates
    // any Rust line N → Buff line N, when both source files have the
    // same line count.
    let mut map = SourceMap::new();
    let id = SourceId(0);
    let buff_source = "line1\nline2\nline3\nline4\n";
    map.add_source(id, PathBuf::from("test.buff"), buff_source.to_string());
    populate_identity_mapping(&mut map, id, buff_source);

    // Verify each rust line maps back to the same buff line.
    for line in 1..=4 {
        let span = map.lookup_buff(line).expect("line should map");
        let (buff_line, _col) = map
            .lookup(span.source_id, span.start)
            .expect("line resolvable");
        assert_eq!(
            buff_line, line,
            "identity: rust line {line} → buff line {buff_line}"
        );
    }
}

#[test]
fn t137_populate_identity_mapping_handles_empty_source() {
    let mut map = SourceMap::new();
    let id = SourceId(0);
    map.add_source(id, PathBuf::from("empty.buff"), String::new());
    populate_identity_mapping(&mut map, id, "");
    assert!(map.is_line_map_empty(), "empty source → no mappings");
}

#[test]
fn t137_populate_identity_mapping_handles_no_trailing_newline() {
    let mut map = SourceMap::new();
    let id = SourceId(0);
    let buff_source = "only one line";
    map.add_source(id, PathBuf::from("test.buff"), buff_source.to_string());
    populate_identity_mapping(&mut map, id, buff_source);
    // A source with no trailing newline still has exactly one line.
    let span = map.lookup_buff(1).expect("line 1 should map");
    let (buff_line, _) = map.lookup(span.source_id, span.start).expect("resolvable");
    assert_eq!(buff_line, 1);
}

// ---------------------------------------------------------------------------
// End-to-end module pipeline (parse → map → render).
// ---------------------------------------------------------------------------

#[test]
fn t137_end_to_end_pipeline_parses_maps_and_renders() {
    use buff_lang_cli::coverage::{
        map_rust_to_buff, parse_llvm_cov_json, render_html, render_lcov, BuffCoverage,
    };

    // 1. Sample llvm-cov JSON output (3 Rust lines, 2 covered).
    let json = r#"{
        "data": [{
            "files": [{
                "filename": "out.rs",
                "lines": [
                    { "line_number": 1, "count": 3 },
                    { "line_number": 2, "count": 0 },
                    { "line_number": 3, "count": 5 }
                ]
            }]
        }]
    }"#;
    let rust_hits = parse_llvm_cov_json(json).expect("parse ok");
    assert_eq!(rust_hits.len(), 3);

    // 2. Build a SourceMap with identity-like mappings covering all 3
    //    lines (the CLI's stopgap behavior).
    let mut map = SourceMap::new();
    let id = SourceId(0);
    let buff_source = "line1\nline2\nline3\n";
    map.add_source(id, PathBuf::from("test.buff"), buff_source.to_string());
    populate_identity_mapping(&mut map, id, buff_source);
    let paths_side_table = vec![(id, PathBuf::from("test.buff"))];

    // 3. Map Rust hits → Buff hits.
    let buff_hits = map_rust_to_buff(&rust_hits, &map, &paths_side_table);
    assert_eq!(buff_hits.len(), 3);

    // 4. Aggregate.
    let coverage = BuffCoverage::aggregate(&buff_hits);
    let file_cov = coverage
        .files
        .get(&PathBuf::from("test.buff"))
        .expect("file present");
    assert_eq!(file_cov.total_lines(), 3);
    assert_eq!(file_cov.covered_lines(), 2);
    let pct = file_cov.percent();
    assert!(pct > 66.0 && pct < 67.0, "expected ~66.67%, got {pct}");

    // 5. Render LCOV — should contain SF, DA entries, LF/LH, end_of_record.
    let lcov = render_lcov(&coverage);
    assert!(lcov.contains("SF:test.buff"));
    assert!(lcov.contains("DA:1,3"));
    assert!(lcov.contains("DA:2,0"));
    assert!(lcov.contains("DA:3,5"));
    assert!(lcov.contains("LF:3"));
    assert!(lcov.contains("LH:2"));
    assert!(lcov.contains("end_of_record"));

    // 6. Render HTML — should contain DOCTYPE + the file path + line hits.
    let html = render_html(&coverage);
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("test.buff"));
    assert!(html.contains("count=3"));
    assert!(html.contains("count=0"));
    assert!(html.contains("count=5"));
}

// ---------------------------------------------------------------------------
// Missing-path + missing-tool USER ACTION error paths.
// ---------------------------------------------------------------------------

#[test]
fn t137_run_returns_error_when_path_is_none() {
    use buff_lang_cli::commands::coverage::run;
    let err = run(None, false, false, None, false).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("single .buff file"),
        "expected single-file error, got: {msg}"
    );
}

#[test]
fn t137_run_returns_error_when_no_tool_installed() {
    use buff_lang_cli::commands::coverage::run;
    if detect_coverage_tool().is_some() {
        eprintln!("skipping: a coverage tool is installed on this host");
        return;
    }
    let err = run(
        Some(std::path::Path::new("examples/ola.buff")),
        false,
        false,
        None,
        false,
    )
    .unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("no coverage tool found"),
        "expected missing-tool error, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Sanity: ensure Span + SourceId are still re-exported from buff-lang-error
// (we depend on the T60 API surface).
// ---------------------------------------------------------------------------

#[test]
fn t60_source_map_round_trip_via_public_api() {
    // Sanity-check the T60 SourceMap API the coverage module relies
    // on. If this test fails, T137 is BLOCKED on T60 source-map
    // changes.
    let mut map = SourceMap::new();
    let id = SourceId(42);
    map.add_source(id, PathBuf::from("fake.buff"), "hello\nworld\n".to_string());
    let span = Span::new(0, 5, id);
    map.add_mapping(span, 7);

    // rust_line → buff_span → buff (line, col).
    let recovered_span = map.lookup_buff(7).expect("rust_line 7 → span");
    assert_eq!(recovered_span, span);
    let (line, col) = map.lookup(id, span.start).expect("offset 0 → (1, 1)");
    assert_eq!((line, col), (1, 1));

    // Reverse: buff_span → rust_line.
    assert_eq!(map.lookup_rust(span), Some(7));
}
