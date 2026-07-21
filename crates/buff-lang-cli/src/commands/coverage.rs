//! `buff coverage [PATH] [--html] [--lcov] [--output <PATH>] [--release]`
//! — collect Rust coverage on the generated `.rs` + reverse-map hits
//! to `.buff` lines via the T60 [`SourceMap`] (T137).
//!
//! # Pipeline
//!
//! 1. Detect the installed coverage tool — `cargo-llvm-cov` (preferred)
//!    or `cargo-tarpaulin` (fallback). If neither is on `PATH`, print
//!    an install hint + return an error (USER ACTION).
//! 2. Re-run the front-end pipeline ([`pipeline::compile_to_rust`]) on
//!    the `.buff` file to write the generated `.rs` alongside the
//!    source. The `.rs` file is what `cargo llvm-cov` instruments.
//! 3. Invoke the detected coverage tool, capturing its JSON output
//!    (llvm-cov's `--json` flag, or tarpaulin's `--out json`).
//! 4. Parse the JSON into [`RustLineHit`]s via
//!    [`parse_llvm_cov_json`](crate::coverage::parse_llvm_cov_json).
//! 5. Build a [`SourceMap`] populated by walking the generated `.rs`
//!    for `//buff-map:rust_line:buff_line` markers... **GAP-1** (see
//!    evidence file): codegen does NOT currently emit source-map
//!    markers into the `.rs`, so the CLI falls back to an identity
//!    mapping (rust_line == buff_line) for single-file compiles. The
//!    full source-map wiring requires codegen changes — deferred to
//!    post-v1.10 (see `task-137-coverage.txt` GAP-1).
//! 6. Render the requested reports (stdout summary always; HTML when
//!    `--html`; LCOV when `--lcov`).
//!
//! # Errors
//!
//! All fallible operations return [`anyhow::Result`] with rich,
//! user-facing context. No `unwrap` / `expect` / `panic!`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use buff_lang_error::{SourceId, SourceMap};

use crate::coverage::{
    map_rust_to_buff, parse_llvm_cov_json, render_html, render_lcov, BuffCoverage,
};
use crate::pipeline::compile_to_rust;

/// Entry point for `buff coverage [...]`.
///
/// See the module docs for the pipeline. `path` selects the `.buff`
/// source to coverage-run; `None` is currently rejected with a helpful
/// message (project-wide coverage is post-v1.10 — see GAP-2 in
/// `task-137-coverage.txt`).
pub fn run(
    path: Option<&Path>,
    html: bool,
    lcov: bool,
    output: Option<&Path>,
    release: bool,
) -> Result<()> {
    let file = path.ok_or_else(|| {
        anyhow::anyhow!(
            "buff coverage currently requires a single .buff file. \
             Project-wide coverage is post-v1.10 work — see \
             .sisyphus/evidence/task-137-coverage.txt GAP-2."
        )
    })?;

    // 1. Detect the installed coverage tool. We always surface the
    //    install hint + a USER ACTION error when both are missing —
    //    the actual coverage collection is a hard dependency.
    let tool = detect_coverage_tool();
    let tool = match tool {
        Some(t) => t,
        None => {
            print_missing_tool_hint();
            bail!(
                "no coverage tool found on PATH — install `cargo-llvm-cov` \
                 (preferred) or `cargo-tarpaulin`. \
                 See .sisyphus/evidence/task-137-coverage-USER-ACTION.txt \
                 for the install recipe."
            );
        }
    };

    eprintln!("Using coverage tool: {tool}");

    // 2. Re-run the front-end so the .rs sits next to the .buff.
    //    compile_to_rust already does lex + parse + codegen + write.
    let compile_out = compile_to_rust(file)?;

    // 3. Run the coverage tool + capture its JSON output.
    let json = run_coverage_tool(&tool, &compile_out.rust_file_path, release)?;

    // 4. Parse the JSON into RustLineHits.
    let rust_hits = parse_llvm_cov_json(&json)
        .map_err(|e| anyhow::anyhow!("failed to parse {tool} JSON output: {e}"))?;

    // 5. Build the (GAP-1: identity) SourceMap + side-table for this
    //    .buff file. The real source-map wiring requires codegen to
    //    emit its mappings into a side-table or via inline markers.
    //    For v1.10 we ship the mapping MODULE + report emitters fully
    //    tested (with synthetic SourceMaps) — the CLI uses identity
    //    mapping so end-to-end runs still produce a sensible (if
    //    imprecise) report. Full fidelity is post-v1.10.
    let source_id = SourceId(0);
    let buff_source = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read buff source `{}` for source-map construction",
            file.display()
        )
    })?;
    let mut source_map = SourceMap::new();
    source_map.add_source(source_id, file.to_path_buf(), buff_source.clone());
    populate_identity_mapping(&mut source_map, source_id, &buff_source);
    let paths_side_table = vec![(source_id, file.to_path_buf())];

    // 6. Translate to .buff coverage.
    let buff_hits = map_rust_to_buff(&rust_hits, &source_map, &paths_side_table);
    let coverage = BuffCoverage::aggregate(&buff_hits);

    // 7. Print the summary.
    print_summary(&coverage);

    // 8. Render optional reports.
    if html {
        let out_path = html_output_path(output);
        write_parent_dirs(&out_path)?;
        let html_doc = render_html(&coverage);
        std::fs::write(&out_path, html_doc.as_bytes())
            .with_context(|| format!("failed to write HTML report `{}`", out_path.display()))?;
        eprintln!("Wrote HTML report to {}", out_path.display());
    }
    if lcov {
        let out_path = lcov_output_path(output);
        write_parent_dirs(&out_path)?;
        let lcov_text = render_lcov(&coverage);
        std::fs::write(&out_path, lcov_text.as_bytes())
            .with_context(|| format!("failed to write LCOV report `{}`", out_path.display()))?;
        eprintln!("Wrote LCOV report to {}", out_path.display());
    }

    Ok(())
}

/// Available Rust coverage tools, in preference order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageTool {
    /// `cargo-llvm-cov` — preferred. Native LCOV + JSON + HTML
    /// emitter, fast, workspaces. Install:
    /// `cargo install cargo-llvm-cov`.
    LlvmCov,
    /// `cargo-tarpaulin` — fallback. Pure-Rust, Linux only. Install:
    /// `cargo install cargo-tarpaulin`.
    Tarpaulin,
}

impl CoverageTool {
    /// The literal string used in user-facing messages.
    pub fn as_str(self) -> &'static str {
        match self {
            CoverageTool::LlvmCov => "cargo-llvm-cov",
            CoverageTool::Tarpaulin => "cargo-tarpaulin",
        }
    }

    /// The `cargo` subcommand name to invoke.
    pub fn cargo_subcommand(self) -> &'static str {
        match self {
            CoverageTool::LlvmCov => "llvm-cov",
            CoverageTool::Tarpaulin => "tarpaulin",
        }
    }
}

impl std::fmt::Display for CoverageTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Detect the best installed coverage tool. Returns `None` when
/// neither `cargo-llvm-cov` nor `cargo-tarpaulin` is on `PATH`.
///
/// Each probe runs `<tool> --version` with stdio piped to `null` so
/// detection is silent on failure. A success exit counts as
/// "installed".
pub fn detect_coverage_tool() -> Option<CoverageTool> {
    if tool_available("cargo", &["llvm-cov", "--version"]) {
        return Some(CoverageTool::LlvmCov);
    }
    if tool_available("cargo", &["tarpaulin", "--version"]) {
        return Some(CoverageTool::Tarpaulin);
    }
    None
}

/// Run `<exe> <args> --version` returning `true` on a clean exit.
fn tool_available(exe: &str, args: &[&str]) -> bool {
    Command::new(exe)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run the detected coverage tool against `rust_file`, capturing its
/// JSON output as a String.
///
/// For `cargo-llvm-cov`: runs `cargo llvm-cov --json --nocapture
/// --covered-only` filtered to the file. We pass `--manifest-path` is
/// NOT applicable here (Buff projects don't have a Cargo.toml yet) —
/// instead we shell out to `cargo llvm-cov` directly on the standalone
/// `.rs`. **Pragmatic note**: cargo-llvm-cov expects a cargo project,
/// so this invocation may fail when run on a Buff-generated `.rs`
/// that's not part of a cargo workspace. In that case the user should
/// wrap the Buff project in a Cargo.toml (USER ACTION recipe in
/// `task-137-coverage-USER-ACTION.txt`).
///
/// For `cargo-tarpaulin`: runs `cargo tarpaulin --out json --skip-clean`
/// — same caveat re: cargo project requirement.
fn run_coverage_tool(tool: &CoverageTool, rust_file: &Path, release: bool) -> Result<String> {
    let parent = rust_file.parent().unwrap_or_else(|| Path::new("."));
    let mut cmd = Command::new("cargo");
    cmd.arg(tool.cargo_subcommand());
    match tool {
        CoverageTool::LlvmCov => {
            cmd.args(["--json", "--coverage-only"]);
            if release {
                cmd.arg("--release");
            }
        }
        CoverageTool::Tarpaulin => {
            cmd.args(["--out", "json", "--skip-clean"]);
            if release {
                cmd.arg("--release");
            }
        }
    }
    cmd.current_dir(parent);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    let output = cmd
        .output()
        .with_context(|| format!("failed to invoke `{tool}` in `{}`", parent.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{tool} exited with status {}: {}",
            output.status,
            stderr.trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(stdout)
}

/// Populate `source_map` with identity mappings: `rust_line N →
/// buff_span at line N` for every line in `buff_source`.
///
/// This is the v1.10 stopgap — full source-map wiring requires codegen
/// to emit its mappings into a side-table the CLI can read.
///
/// Computes the byte offset of each line start in `buff_source` (same
/// algorithm as [`buff_lang_error::SourceFile::new`]) so the spans
/// resolve to the correct line via `SourceMap::lookup`. An empty
/// source yields zero mappings (no lines to map).
pub fn populate_identity_mapping(
    source_map: &mut SourceMap,
    source_id: SourceId,
    buff_source: &str,
) {
    if buff_source.is_empty() {
        return;
    }
    for (line_idx, byte_offset) in line_starts(buff_source).into_iter().enumerate() {
        let rust_line = line_idx + 1; // 1-based
                                      // Use EOF as the span end so the span covers from this line
                                      // start to the end of the source — matches what real codegen
                                      // would emit (a statement's span extends to its terminator).
        let end = buff_source.len();
        let span = buff_lang_error::Span::new(byte_offset, end, source_id);
        source_map.add_mapping(span, rust_line);
    }
}

/// Compute the byte offset of each line start in `s` (mirrors
/// `buff_lang_error::SourceFile`'s internal algorithm).
fn line_starts(s: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, c) in s.char_indices() {
        if c == '\n' {
            starts.push(i + c.len_utf8());
        }
    }
    starts
}

/// Resolve `--output` to the HTML report path (default
/// `coverage/index.html`).
pub fn html_output_path(output: Option<&Path>) -> PathBuf {
    match output {
        Some(p) if p.extension().is_some_and(|e| e == "html") => p.to_path_buf(),
        Some(p) => p.join("index.html"),
        None => PathBuf::from("coverage").join("index.html"),
    }
}

/// Resolve `--output` to the LCOV report path (default
/// `coverage/lcov.info`).
pub fn lcov_output_path(output: Option<&Path>) -> PathBuf {
    match output {
        Some(p) if p.extension().is_some_and(|e| e == "info") => p.to_path_buf(),
        Some(p) => p.join("lcov.info"),
        None => PathBuf::from("coverage").join("lcov.info"),
    }
}

/// `mkdir -p` for the parent directory of `path`.
fn write_parent_dirs(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory `{}`", parent.display()))?;
        }
    }
    Ok(())
}

/// Print the per-file coverage summary to stderr (so it doesn't
/// pollute stdout when the user is piping a report).
fn print_summary(coverage: &BuffCoverage) {
    if coverage.files.is_empty() {
        eprintln!("No coverage data mapped to .buff files.");
        return;
    }
    eprintln!("Coverage summary:");
    for (path, file_cov) in &coverage.files {
        let covered = file_cov.covered_lines();
        let total = file_cov.total_lines();
        let pct = file_cov.percent();
        eprintln!(
            "  {} — {}/{} lines ({:.1}%)",
            path.display(),
            covered,
            total,
            pct
        );
    }
    eprintln!("Overall: {:.1}%", coverage.overall_percent());
}

/// Print the install hint when no coverage tool is detected.
///
/// Rendered to stderr so a `buff coverage --html` invocation still
/// produces no stdout output on failure (allowing scripted pipelines
/// to detect failure via exit code).
fn print_missing_tool_hint() {
    eprintln!("No Rust coverage tool detected on PATH.");
    eprintln!();
    eprintln!("Install one of:");
    eprintln!("  cargo install cargo-llvm-cov   (preferred — Windows + macOS + Linux)");
    eprintln!("  cargo install cargo-tarpaulin  (Linux only)");
    eprintln!();
    eprintln!(
        "See .sisyphus/evidence/task-137-coverage-USER-ACTION.txt for the full install + run recipe."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_coverage_tool_returns_some_or_none_without_panicking() {
        // We can't assume either tool is installed on CI, so just
        // verify the detection function runs cleanly.
        let _ = detect_coverage_tool();
    }

    #[test]
    fn detect_coverage_tool_returns_llvm_cov_when_installed() {
        // Skip on CI hosts without llvm-cov — just verify when it IS
        // installed we get the right enum value.
        if !tool_available("cargo", &["llvm-cov", "--version"]) {
            eprintln!("skipping: cargo-llvm-cov not installed");
            return;
        }
        assert_eq!(detect_coverage_tool(), Some(CoverageTool::LlvmCov));
    }

    #[test]
    fn detect_coverage_tool_prefers_llvm_cov_over_tarpaulin() {
        // When both are installed, llvm-cov wins (preference order).
        if !tool_available("cargo", &["llvm-cov", "--version"]) {
            eprintln!("skipping: cargo-llvm-cov not installed");
            return;
        }
        assert_eq!(detect_coverage_tool(), Some(CoverageTool::LlvmCov));
    }

    #[test]
    fn run_returns_error_when_path_is_none() {
        let err = run(None, false, false, None, false).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("single .buff file"),
            "expected single-file error, got: {msg}"
        );
    }

    #[test]
    fn run_returns_error_when_no_tool_installed() {
        if detect_coverage_tool().is_some() {
            eprintln!("skipping: a coverage tool is installed");
            return;
        }
        let err = run(
            Some(Path::new("examples/ola.buff")),
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

    #[test]
    fn run_returns_error_for_nonexistent_file() {
        // Even with a tool installed, we should fail fast when the
        // file doesn't exist.
        if detect_coverage_tool().is_none() {
            eprintln!("skipping: no coverage tool installed");
            return;
        }
        let err = run(
            Some(Path::new("__nonexistent_buff_file__.buff")),
            false,
            false,
            None,
            false,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        // compile_to_rust surfaces "failed to read source file".
        assert!(
            msg.contains("failed to read") || msg.contains("source file"),
            "expected read error, got: {msg}"
        );
    }

    #[test]
    fn html_output_path_defaults_to_coverage_dir() {
        let p = html_output_path(None);
        assert_eq!(p, PathBuf::from("coverage").join("index.html"));
    }

    #[test]
    fn html_output_path_with_explicit_html_file() {
        let p = html_output_path(Some(Path::new("report.html")));
        assert_eq!(p, PathBuf::from("report.html"));
    }

    #[test]
    fn html_output_path_with_directory_treats_as_dir() {
        let p = html_output_path(Some(Path::new("artifacts")));
        assert_eq!(p, PathBuf::from("artifacts").join("index.html"));
    }

    #[test]
    fn lcov_output_path_defaults_to_coverage_dir() {
        let p = lcov_output_path(None);
        assert_eq!(p, PathBuf::from("coverage").join("lcov.info"));
    }

    #[test]
    fn lcov_output_path_with_explicit_info_file() {
        let p = lcov_output_path(Some(Path::new("custom.info")));
        assert_eq!(p, PathBuf::from("custom.info"));
    }

    #[test]
    fn coverage_tool_display_matches_str() {
        assert_eq!(CoverageTool::LlvmCov.to_string(), "cargo-llvm-cov");
        assert_eq!(CoverageTool::Tarpaulin.to_string(), "cargo-tarpaulin");
    }

    #[test]
    fn coverage_tool_cargo_subcommand() {
        assert_eq!(CoverageTool::LlvmCov.cargo_subcommand(), "llvm-cov");
        assert_eq!(CoverageTool::Tarpaulin.cargo_subcommand(), "tarpaulin");
    }
}
