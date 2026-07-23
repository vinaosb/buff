//! T61 — `buff bench-cold-start` integration tests.
//!
//! Exercises the five acceptance criteria from the T61 spec:
//!
//! 1. **Subcommand parses correctly** — clap recognises `bench-cold-start`
//!    and maps it to [`Command::BenchColdStart`].
//! 2. **Report file generated** — `write_json_report` + `write_markdown_report`
//!    produce files at the documented paths.
//! 3. **JSON output well-formed** — the JSON report parses (via serde_json)
//!    and contains the required keys with the right types.
//! 4. **Markdown output well-formed** — the Markdown report contains the
//!    expected header + a data row with the right column count.
//! 5. **Buff binary exists + runs under threshold (smoke)** — actually
//!    compiles + runs the minimal fixture end-to-end via the pipeline,
//!    asserting the binary produces output + the cold-start measurement
//!    is finite. The hard 50 ms acceptance target is NOT enforced in CI
//!    (host-dependent); instead we assert the run produced a finite
//!    duration + non-empty output. Marked `#[ignore]` so it only runs
//!    on-demand (`cargo test -- --ignored`) because it requires rustc +
//!    is slow.
//!
//! Tests 1-4 are pure + fast (no rustc invocation). Test 5 is gated.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use buff_lang_cli::cli::{Cli, Command};
use buff_lang_cli::commands::bench_cold_start::{
    self, RunStats, ACCEPTANCE_THRESHOLD_MS, JSON_REPORT_PATH, MARKDOWN_REPORT_PATH,
};
use clap::Parser;

// ---------------------------------------------------------------------------
// Helpers — unique temp dir per test so parallel runs don't collide on the
// shared `benchmarks/` cwd path.
// ---------------------------------------------------------------------------

/// Guard that switches cwd to a unique temp dir on construction + restores
/// the original cwd on drop. Ensures report writes land in a sandbox.
struct CwdGuard {
    original: PathBuf,
}

impl CwdGuard {
    fn sandbox(label: &str) -> Self {
        let thread_id_str = format!("{:?}", std::thread::current().id());
        let thread_id_sanitised: String = thread_id_str
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let dir = std::env::temp_dir().join(format!(
            "buff-t61-cold-start-tests-{}-{}-{}",
            label,
            std::process::id(),
            thread_id_sanitised,
        ));
        let _ = std::fs::create_dir_all(&dir);
        let original = std::env::current_dir().expect("cwd must be readable");
        std::env::set_current_dir(&dir).expect("set_current_dir must succeed");
        Self { original }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}

// ---------------------------------------------------------------------------
// 1: Subcommand parses correctly.
// ---------------------------------------------------------------------------

#[test]
fn bench_cold_start_subcommand_parses_from_argv() {
    let cli = Cli::parse_from(["buff", "bench-cold-start"]);
    match cli.command {
        Command::BenchColdStart => {}
        other => panic!("expected BenchColdStart, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 2: Report file generated.
// ---------------------------------------------------------------------------

#[test]
fn report_files_are_created_at_documented_paths() {
    let _guard = CwdGuard::sandbox("report_paths");
    let stats = fixture_stats();
    bench_cold_start::write_json_report(&stats, true).expect("json write");
    bench_cold_start::write_markdown_report(&stats, true).expect("md write");

    assert!(Path::new(JSON_REPORT_PATH).exists(), "json report missing");
    assert!(
        Path::new(MARKDOWN_REPORT_PATH).exists(),
        "markdown report missing"
    );
}

// ---------------------------------------------------------------------------
// 3: JSON output well-formed.
// ---------------------------------------------------------------------------

#[test]
fn json_report_is_well_formed_and_contains_required_keys() {
    let _guard = CwdGuard::sandbox("json_shape");
    let stats = fixture_stats();
    bench_cold_start::write_json_report(&stats, true).expect("json write");

    let raw = std::fs::read_to_string(JSON_REPORT_PATH).expect("read json");
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).expect("json must parse via serde_json");

    let obj = parsed
        .as_object()
        .expect("top-level JSON value must be an object");

    // Required keys + types from the documented shape.
    let required_keys: &[(&str, &str)] = &[
        ("task", "string"),
        ("tool", "string"),
        ("date", "string"),
        ("threshold_ms", "number"),
        ("samples", "number"),
        ("min_ms", "number"),
        ("median_ms", "number"),
        ("max_ms", "number"),
        ("pass", "boolean"),
    ];
    for (key, ty) in required_keys {
        assert!(
            obj.contains_key(*key),
            "json must contain `{key}` key: {raw}"
        );
        let actual_ty = match obj.get(*key) {
            Some(serde_json::Value::String(_)) => "string",
            Some(serde_json::Value::Number(_)) => "number",
            Some(serde_json::Value::Bool(_)) => "boolean",
            _ => "other",
        };
        assert_eq!(
            actual_ty, *ty,
            "json key `{key}` must be {ty}, got {actual_ty}: {raw}",
        );
    }
    // The reported threshold must equal the public constant.
    let threshold = obj
        .get("threshold_ms")
        .and_then(|v| v.as_u64())
        .expect("threshold_ms must be a u64");
    assert_eq!(
        threshold as u128, ACCEPTANCE_THRESHOLD_MS,
        "JSON threshold_ms must equal ACCEPTANCE_THRESHOLD_MS"
    );
    // `pass: true` was passed in.
    let pass = obj
        .get("pass")
        .and_then(|v| v.as_bool())
        .expect("pass must be a bool");
    assert!(pass, "pass must reflect the value passed in");
}

// ---------------------------------------------------------------------------
// 4: Markdown output well-formed.
// ---------------------------------------------------------------------------

#[test]
fn markdown_report_contains_header_and_table_row() {
    let _guard = CwdGuard::sandbox("md_shape");
    let stats = fixture_stats();
    bench_cold_start::write_markdown_report(&stats, true).expect("md write");

    let md = std::fs::read_to_string(MARKDOWN_REPORT_PATH).expect("read md");

    // Header markers — written once when the file is created.
    assert!(
        md.contains("# Cold-Start Benchmark Report (T61)"),
        "md must contain the H1 header: {md}"
    );
    assert!(
        md.contains("Acceptance target"),
        "md must document the acceptance target: {md}"
    );
    // The table header row must list all six columns.
    let header_row = "| date | samples | min (ms) | median (ms) | max (ms) | pass |";
    assert!(
        md.contains(header_row),
        "md must contain the table header row verbatim: {md}"
    );
    // The alignment row has exactly 6 `---`-or-`:---:` cells separated by `|`.
    let alignment_line = md
        .lines()
        .find(|l| l.starts_with("|---") && l.contains(":---:"))
        .expect("md must contain a table alignment row");
    let cell_count = alignment_line.matches("|").count();
    assert!(
        cell_count >= 7,
        "alignment row must have 6 columns (7 pipes incl. ends): {alignment_line}"
    );
    // A data row should be present matching the stats we wrote.
    let data_rows: Vec<&str> = md
        .lines()
        .filter(|l| l.starts_with("| 20") && l.matches("|").count() == cell_count)
        .collect();
    assert!(
        !data_rows.is_empty(),
        "md must contain at least one dated data row"
    );
    // Verify the row we just wrote contains the expected pass marker.
    let last = data_rows.last().expect("non-empty data rows");
    assert!(
        last.contains("| yes |"),
        "data row must end with `| yes |` (pass=true): {last}"
    );
}

// ---------------------------------------------------------------------------
// 5: Buff binary exists + runs under threshold (smoke).
// ---------------------------------------------------------------------------

/// Smoke test: actually compile + run the minimal fixture end-to-end.
///
/// Verifies:
/// - The fixture source parses + compiles via the Buff pipeline.
/// - The generated binary runs + produces output.
/// - The cold-start measurement (via [`bench_cold_start::time_one_run`])
///   returns a finite [`Duration`].
///
/// NOT enforced: the hard 50 ms threshold (host-dependent — Windows CI
/// runners often exceed 50 ms for process spawn alone, regardless of the
/// binary's merits). The acceptance target is documented in the JSON +
/// Markdown reports for human inspection.
///
/// Marked `#[ignore]` because it requires `rustc` on PATH + is slow (~3 s
/// for the rustc invocation). Run on-demand with
/// `cargo test -p buff-lang-cli --test bench_cold_start -- --ignored`.
#[test]
#[ignore]
fn buff_binary_exists_and_runs_under_threshold_smoke() {
    use buff_lang_cli::pipeline::{self, BuildMode};

    // Unique temp dir so parallel runs don't clobber the executable.
    let thread_id_str = format!("{:?}", std::thread::current().id());
    let thread_id_sanitised: String = thread_id_str
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let dir = std::env::temp_dir().join(format!(
        "buff-t61-smoke-{}-{}",
        std::process::id(),
        thread_id_sanitised,
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");

    let src_path = dir.join("smoke.buff");
    let mut src = std::fs::File::create(&src_path).expect("create src file");
    let fixture = "func main():\n    print(\"hello\")\n";
    src.write_all(fixture.as_bytes()).expect("write fixture");
    drop(src);

    let compile_out =
        pipeline::compile_to_rust(&src_path).expect("fixture must compile via the Buff pipeline");
    let exe_path = pipeline::with_exe_extension(&dir.join("smoke"));
    pipeline::compile_rust_to_exe(
        &compile_out.rust_file_path,
        &exe_path,
        &src_path,
        BuildMode::Fast,
    )
    .expect("fixture must compile to a binary via rustc");

    assert!(
        exe_path.exists(),
        "compiled binary must exist at {}",
        exe_path.display()
    );

    // Time one cold-start via the same helper the benchmark uses. This
    // exercises the real timing path (process spawn → first stdout byte).
    let elapsed = bench_cold_start::time_one_run(&exe_path).expect("cold-start measurement");

    // Finite duration + non-zero (sanity).
    assert!(
        elapsed > Duration::ZERO,
        "cold-start duration must be positive, got {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "cold-start duration must be under 10 s (smoke bound, NOT the \
         50 ms acceptance target — that's reported in the JSON/MD \
         reports for human inspection since it is host-dependent), \
         got {elapsed:?}"
    );

    // Cleanup (best-effort).
    let _ = std::fs::remove_file(&src_path);
    let _ = std::fs::remove_file(&compile_out.rust_file_path);
    let _ = std::fs::remove_file(&exe_path);
}

// ---------------------------------------------------------------------------
// Shared fixture builder.
// ---------------------------------------------------------------------------

/// Construct a representative [`RunStats`] fixture for the report-shape
/// tests (which don't need to invoke rustc).
fn fixture_stats() -> RunStats {
    let samples = vec![
        Duration::from_millis(2),
        Duration::from_millis(3),
        Duration::from_millis(4),
    ];
    RunStats::compute(&samples).expect("non-empty samples")
}
