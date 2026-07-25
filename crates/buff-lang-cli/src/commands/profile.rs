//! `buff profile <FILE> [--alloc] [--output <PATH>]` — CPU / allocation
//! profiler (T111).
//!
//! Compiles the `.buff` file WITH profiling instrumentation injected into
//! the generated `fn main()` via [`pipeline::inject_profiling`], then runs
//! the resulting binary. Two modes:
//!
//! - **CPU mode** (default): links `pprof-rs` (100 Hz SIGPROF sampling on
//!   Unix / thread-based sampling on Windows) into the user binary. On
//!   exit, the binary writes `profile.flamegraph.svg`. The CLI copies
//!   this to `--output` (default: `profile.flamegraph.svg`).
//! - **Allocation mode** (`--alloc`): links `dhat` (global-allocator
//!   replacement tracking every heap allocation) into the user binary.
//!   On exit, the binary writes `dhat-heap.json` (viewable in
//!   `dh_view.html`). The CLI parses the JSON + renders a human-readable
//!   `alloc-report.txt` (top allocation sites by bytes + count) + copies
//!   the raw JSON alongside.
//!
//! # Architecture
//!
//! The single-file `rustc` pipeline cannot resolve `extern crate` deps
//! (there is no Cargo.toml telling rustc where to find `pprof` /
//! `dhat`). The profiling crates MUST be linked via a Cargo project.
//! This subcommand therefore:
//!
//! 1. Runs the normal Buff front-end ([`pipeline::compile_to_rust`]).
//! 2. Injects profiling guards via [`pipeline::inject_profiling`] (syn
//!    transform — wraps `fn main()` body, optionally prepends
//!    `#[global_allocator]` for alloc mode).
//! 3. Generates a **temporary Cargo project** in `./target/buff-profile-<hash>/`
//!    with a `Cargo.toml` listing `pprof` + `flamegraph` (CPU) or
//!    `dhat` (alloc) as deps + `src/main.rs` = the instrumented source.
//! 4. Runs `cargo build --release` in the temp project.
//! 5. Runs the resulting binary in the temp project dir (so profile
//!    artifacts land there).
//! 6. Copies the artifacts to the user's `--output` path.
//!
//! # Zero overhead when off
//!
//! Normal `buff build` / `buff run` are completely unaffected — the
//! profiling instrumentation is injected ONLY by this subcommand. There
//! is no env-var gate; the binary is either instrumented (built via
//! `buff profile`) or not (built via `buff build` / `buff run`).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::pipeline;

/// Default output path for CPU mode.
const DEFAULT_CPU_OUTPUT: &str = "profile.flamegraph.svg";
/// Default output path for alloc mode (human-readable report).
const DEFAULT_ALLOC_OUTPUT: &str = "alloc-report.txt";
/// Default output path for alloc mode (raw dhat JSON sidecar).
const DEFAULT_ALLOC_JSON: &str = "dhat-heap.json";

/// Entry point for `buff profile <FILE> [--alloc] [--output <PATH>]`.
///
/// Dispatches to [`run_cpu`] (default) or [`run_alloc`] (`--alloc`).
pub fn run(file: &Path, alloc: bool, output: Option<&Path>) -> Result<()> {
    if alloc {
        run_alloc(file, output)
    } else {
        run_cpu(file, output)
    }
}

/// CPU profiling mode (pprof-rs SIGPROF sampling → flamegraph SVG).
fn run_cpu(file: &Path, output: Option<&Path>) -> Result<()> {
    let mode = pipeline::ProfileMode::Cpu;
    let instrumented = build_instrumented_source(file, mode)?;

    // Generate + build the temp Cargo project.
    let project_dir = write_profile_project(file, &instrumented.source, mode)?;
    eprintln!(
        "buff profile (CPU): built instrumented project at {}",
        project_dir.display()
    );

    // cargo build --release
    cargo_build_release(&project_dir)?;

    // Run the binary in the project dir so profile.flamegraph.svg lands
    // there (the injected code writes it to the current working dir).
    let exe_path = locate_target_exe(&project_dir, &instrumented.bin_name)?;
    eprintln!("buff profile (CPU): running instrumented binary ...");
    let status = Command::new(&exe_path)
        .current_dir(&project_dir)
        .status()
        .with_context(|| format!("failed to run `{}`", exe_path.display()))?;
    if !status.success() {
        // The injected catch_unwind re-exits with code 1 on user panic;
        // a non-panic non-zero exit is the user's own choice. The
        // flamegraph SVG is still written (the dump runs in the
        // catch_unwind cleanup path).
        eprintln!(
            "note: profiled binary exited with code {} — flamegraph \
             still written (profiling dump runs in the cleanup path)",
            status.code().unwrap_or(-1)
        );
    }

    // Copy the artifact to the user's --output path.
    let svg_src = project_dir.join(DEFAULT_CPU_OUTPUT);
    let svg_dst: PathBuf = match output {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from(DEFAULT_CPU_OUTPUT),
    };
    if svg_src.exists() {
        std::fs::copy(&svg_src, &svg_dst).with_context(|| {
            format!(
                "failed to copy `{}` → `{}`",
                svg_src.display(),
                svg_dst.display()
            )
        })?;
        println!("buff profile (CPU): wrote {}", svg_dst.display());
    } else {
        bail!(
            "expected `{}` in the instrumented binary's output dir but \
             it was not found — the pprof flamegraph write may have \
             failed (check stderr above for the dump error)",
            svg_src.display()
        );
    }

    cleanup_project(&project_dir);
    Ok(())
}

/// Allocation profiling mode (dhat global-allocator → dhat-heap.json +
/// human-readable alloc-report.txt).
fn run_alloc(file: &Path, output: Option<&Path>) -> Result<()> {
    let mode = pipeline::ProfileMode::Alloc;
    let instrumented = build_instrumented_source(file, mode)?;

    let project_dir = write_profile_project(file, &instrumented.source, mode)?;
    eprintln!(
        "buff profile (alloc): built instrumented project at {}",
        project_dir.display()
    );

    cargo_build_release(&project_dir)?;

    let exe_path = locate_target_exe(&project_dir, &instrumented.bin_name)?;
    eprintln!("buff profile (alloc): running instrumented binary ...");
    let status = Command::new(&exe_path)
        .current_dir(&project_dir)
        .status()
        .with_context(|| format!("failed to run `{}`", exe_path.display()))?;
    if !status.success() {
        eprintln!(
            "note: profiled binary exited with code {} — dhat report \
             still written (Drop runs in the cleanup path)",
            status.code().unwrap_or(-1)
        );
    }

    // Read the raw dhat-heap.json the binary wrote.
    let json_src = project_dir.join(DEFAULT_ALLOC_JSON);
    let report_dst: PathBuf = match output {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from(DEFAULT_ALLOC_OUTPUT),
    };

    if !json_src.exists() {
        bail!(
            "expected `{}` in the instrumented binary's output dir but \
             it was not found — the dhat Drop impl may have failed (check \
             stderr above)",
            json_src.display()
        );
    }

    // Copy the raw JSON alongside the report (the report path's parent
    // dir, or the current dir when --output is a bare filename).
    let json_dst = report_dst
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|d| d.join(DEFAULT_ALLOC_JSON))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ALLOC_JSON));
    std::fs::copy(&json_src, &json_dst).with_context(|| {
        format!(
            "failed to copy `{}` → `{}`",
            json_src.display(),
            json_dst.display()
        )
    })?;

    // Parse + render the human-readable report.
    let json_bytes = std::fs::read(&json_src)
        .with_context(|| format!("failed to read `{}`", json_src.display()))?;
    let report_text = render_dhat_report(&json_bytes, file);
    std::fs::write(&report_dst, &report_text)
        .with_context(|| format!("failed to write `{}`", report_dst.display()))?;
    println!("buff profile (alloc): wrote {}", report_dst.display());
    println!(
        "buff profile (alloc): wrote {} (raw dhat JSON)",
        json_dst.display()
    );

    cleanup_project(&project_dir);
    Ok(())
}

/// Output of [`build_instrumented_source`]: the instrumented Rust source
/// + the binary name derived from the `.buff` file stem.
struct InstrumentedSource {
    /// The Rust source with profiling guards injected into `fn main()`.
    source: String,
    /// Binary name (Cargo `[[bin]].name`) — the `.buff` file stem,
    /// sanitised for Rust identifier rules.
    bin_name: String,
}

/// Run the Buff front-end + inject profiling guards (T111).
///
/// Calls [`pipeline::compile_to_rust`] (the existing front-end) then
/// [`pipeline::inject_profiling`] (the T111 syn transform). The
/// intermediate `.rs` file written by the front-end is NOT used —
/// we pass the returned `rust_source` string directly to the injector.
fn build_instrumented_source(
    file: &Path,
    mode: pipeline::ProfileMode,
) -> Result<InstrumentedSource> {
    let compile_out = pipeline::compile_to_rust(file)
        .with_context(|| format!("failed to compile `{}`", file.display()))?;
    let instrumented = pipeline::inject_profiling(&compile_out.rust_source, mode)?;
    let bin_name = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("buff_profile_target")
        .replace(['-', '.'], "_");
    Ok(InstrumentedSource {
        source: instrumented,
        bin_name,
    })
}

/// Write the temporary Cargo project for profiling (T111).
///
/// Layout: `<project_dir>/Cargo.toml` + `<project_dir>/src/main.rs`.
/// `project_dir` is `./target/buff-profile-<bin_name>/`.
fn write_profile_project(
    file: &Path,
    instrumented_source: &str,
    mode: pipeline::ProfileMode,
) -> Result<PathBuf> {
    let bin_name = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("buff_profile_target")
        .replace(['-', '.'], "_");

    let project_dir = PathBuf::from("target").join(format!("buff-profile-{bin_name}"));
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir)
        .with_context(|| format!("failed to create `{}`", src_dir.display()))?;

    // Write src/main.rs.
    let main_rs = src_dir.join("main.rs");
    std::fs::write(&main_rs, instrumented_source)
        .with_context(|| format!("failed to write `{}`", main_rs.display()))?;

    // Write Cargo.toml. The dep set depends on the profiling mode:
    // - Cpu: pprof (with flamegraph feature for in-process SVG rendering).
    // - Alloc: dhat.
    let cargo_toml = render_profile_cargo_toml(&bin_name, mode);
    let cargo_toml_path = project_dir.join("Cargo.toml");
    std::fs::write(&cargo_toml_path, &cargo_toml)
        .with_context(|| format!("failed to write `{}`", cargo_toml_path.display()))?;

    Ok(project_dir)
}

/// Render the `Cargo.toml` for the temp profiling project (T111).
///
/// Pins the profiling crates to the SAME versions the workspace pins
/// (see root `Cargo.toml` [workspace.dependencies]). Hardcoded version
/// strings because the temp Cargo project is NOT part of the workspace —
/// it resolves deps from crates.io independently.
fn render_profile_cargo_toml(bin_name: &str, mode: pipeline::ProfileMode) -> String {
    // NOTE: keep these version strings in sync with the workspace pins
    // in the root Cargo.toml [workspace.dependencies] T111 block. The
    // temp Cargo project is generated OUTSIDE the workspace (in
    // target/buff-profile-<name>/), so it cannot use `.workspace = true`.
    let deps = match mode {
        pipeline::ProfileMode::Cpu => {
            // pprof with the `flamegraph` feature pulls in the flamegraph
            // crate transitively + exposes `Report::flamegraph()`.
            r#"pprof = { version = "0.15", features = ["flamegraph"] }
flamegraph = "0.6""#
        }
        pipeline::ProfileMode::Alloc => r#"dhat = "0.3""#,
    };
    format!(
        "[package]\n\
         name = \"{bin_name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n\
         \n\
         [[bin]]\n\
         name = \"{bin_name}\"\n\
         path = \"src/main.rs\"\n\
         \n\
         [profile.release]\n\
         debug = true\n\
         \n\
         [dependencies]\n\
         {deps}\n\
         "
    )
}

/// Run `cargo build --release` in `project_dir` (T111).
fn cargo_build_release(project_dir: &Path) -> Result<()> {
    eprintln!(
        "buff profile: running `cargo build --release` in {}",
        project_dir.display()
    );
    let result = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .current_dir(project_dir)
        .output()
        .context("failed to invoke `cargo build` — is cargo on your PATH?")?;
    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        eprint!("{stderr}");
    }
    if !result.status.success() {
        bail!(
            "cargo build --release exited with status {} (see stderr above)",
            result.status
        );
    }
    Ok(())
}

/// Locate the compiled binary in `<project_dir>/target/release/<bin_name>[.exe]`.
fn locate_target_exe(project_dir: &Path, bin_name: &str) -> Result<PathBuf> {
    let exe_name = if cfg!(windows) {
        format!("{bin_name}.exe")
    } else {
        bin_name.to_string()
    };
    let exe_path = project_dir.join("target").join("release").join(&exe_name);
    if !exe_path.exists() {
        bail!(
            "expected compiled binary at `{}` but it was not found — \
             cargo build may have failed silently",
            exe_path.display()
        );
    }
    Ok(exe_path)
}

/// Best-effort cleanup of the temp profiling project.
///
/// Removes the `target/` subdir (cargo's build cache — typically 50-200
/// MB for a profiling project with pprof/dhat deps) but keeps the
/// `Cargo.toml` + `src/main.rs` so the user can inspect the
/// instrumented source + re-run manually if desired.
fn cleanup_project(project_dir: &Path) {
    let target_dir = project_dir.join("target");
    if target_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&target_dir) {
            eprintln!(
                "note: failed to clean up `{}` ({e}); remove manually \
                 to reclaim disk space",
                target_dir.display()
            );
        }
    }
}

/// Render a human-readable allocation report from a dhat `dhat-heap.json`
/// file (T111).
///
/// dhat's JSON format (v1): a single object with `dhatFileVersion`,
/// `mode`, `verb`, `clocksPerMuSec`, `totalBytes`, `totalBlocks`,
/// `frames`, `events`. We extract `totalBytes` + `totalBlocks` for the
/// header + `events[].req` (bytes) + `events[].allocs` (count) +
/// `events[].fs` (frame stack) for the top-N allocation sites.
fn render_dhat_report(json_bytes: &[u8], source_file: &Path) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "buff profile (alloc) — dhat heap report for {}\n",
        source_file.display()
    ));
    out.push_str("generated by `buff profile --alloc` (T111)\n");
    out.push('\n');

    // Parse the JSON defensively — dhat's format is stable but we
    // degrade gracefully on any parse error so the user ALWAYS gets a
    // report (even if it's just "see dhat-heap.json in dh_view.html").
    let parsed: Option<serde_json::Value> = serde_json::from_slice(json_bytes).ok();

    if let Some(ref v) = parsed {
        let total_bytes = v.get("totalBytes").and_then(|b| b.as_u64()).unwrap_or(0);
        let total_blocks = v.get("totalBlocks").and_then(|b| b.as_u64()).unwrap_or(0);
        out.push_str(&format!("total heap allocated: {total_bytes} bytes\n"));
        out.push_str(&format!("total allocations:    {total_blocks} blocks\n"));
        out.push('\n');

        // Frame table: dhat stores `frames` as an array of strings
        // (indexed by `events[].fs[].f` IDs). `events` is an array of
        // { req, allocs, fs: [{ f: frame_id, .. }], .. }.
        let frames = v.get("frames").and_then(|f| f.as_array());
        let events = v.get("events").and_then(|e| e.as_array());

        if let (Some(frames), Some(events)) = (frames, events) {
            out.push_str("top allocation sites (by bytes at peak):\n");
            out.push_str(&format!("{:<12}  {:<10}  {}\n", "bytes", "blocks", "site"));
            out.push_str(&format!("{:-<12}  {:-<10}  {:-<40}\n", "", "", ""));

            // Collect + sort events by req (bytes) descending.
            let mut event_list: Vec<(u64, u64, String)> = events
                .iter()
                .map(|ev| {
                    let req = ev.get("req").and_then(|r| r.as_u64()).unwrap_or(0);
                    let allocs = ev.get("allocs").and_then(|a| a.as_u64()).unwrap_or(0);
                    // Top frame of the allocation stack (last entry in
                    // fs[] is the innermost / allocating frame in dhat's
                    // convention).
                    let top_frame = ev
                        .get("fs")
                        .and_then(|fs| fs.as_array())
                        .and_then(|arr| arr.last())
                        .and_then(|last| last.get("f"))
                        .and_then(|f| f.as_u64())
                        .and_then(|fid| frames.get(fid as usize).and_then(|s| s.as_str()))
                        .unwrap_or("??");
                    (req, allocs, top_frame.to_string())
                })
                .collect();
            event_list.sort_by_key(|b| std::cmp::Reverse(b.0));

            for (bytes, blocks, site) in event_list.iter().take(20) {
                out.push_str(&format!("{:<12}  {:<10}  {}\n", bytes, blocks, site));
            }
        }
    } else {
        out.push_str(
            "WARNING: failed to parse dhat-heap.json — the raw JSON is\n\
             preserved alongside this report. Open it in dh_view.html for\n\
             the full interactive view.\n",
        );
    }

    out.push('\n');
    out.push_str(
        "raw data: dhat-heap.json (open in dh_view.html from the dhat\n\
         repository for the full interactive flamegraph + site table)\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_dhat_report_handles_invalid_json_gracefully() {
        let report = render_dhat_report(b"not valid json", Path::new("test.buff"));
        assert!(
            report.contains("failed to parse"),
            "invalid JSON must surface the warning: {report}"
        );
    }

    #[test]
    fn render_dhat_report_renders_totals_when_present() {
        let json = r#"{
            "dhatFileVersion": 1,
            "mode": "heap",
            "verb": "Allocated",
            "clocksPerMuSec": 1000,
            "totalBytes": 4096,
            "totalBlocks": 16,
            "frames": ["main", "alloc::vec"],
            "events": [
                {"req": 2048, "allocs": 8, "fs": [{"f": 0}, {"f": 1}]}
            ]
        }"#;
        let report = render_dhat_report(json.as_bytes(), Path::new("test.buff"));
        assert!(
            report.contains("4096 bytes"),
            "report must include totalBytes: {report}"
        );
        assert!(
            report.contains("16 blocks"),
            "report must include totalBlocks: {report}"
        );
        assert!(
            report.contains("alloc::vec"),
            "report must include top frame: {report}"
        );
    }

    #[test]
    fn render_profile_cargo_toml_cpu_mode_includes_pprof() {
        let toml = render_profile_cargo_toml("test_bin", pipeline::ProfileMode::Cpu);
        assert!(toml.contains("pprof"));
        assert!(toml.contains("flamegraph"));
        assert!(toml.contains(r#"name = "test_bin""#));
    }

    #[test]
    fn render_profile_cargo_toml_alloc_mode_includes_dhat() {
        let toml = render_profile_cargo_toml("test_bin", pipeline::ProfileMode::Alloc);
        assert!(toml.contains("dhat"));
        assert!(toml.contains(r#"name = "test_bin""#));
        assert!(!toml.contains("pprof"), "alloc mode must NOT pull in pprof");
    }
}
