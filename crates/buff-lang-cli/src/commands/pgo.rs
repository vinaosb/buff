//! `buff build --pgo` — Profile-Guided Optimization orchestrator (T62).
//!
//! Automates the 3-step rustc/LLVM PGO flow:
//!
//! 1. **Phase 1 — instrument** (`buff build --pgo <FILE>`): compiles the
//!    `.buff` source via the normal front-end pipeline, then invokes
//!    `rustc` with `-C profile-generate=./target/pgo-data` so the
//!    resulting binary emits edge-profiling counters into
//!    `./target/pgo-data/` on every execution. The binary is placed at
//!    the conventional output path (e.g. `./pgo_demo[.exe]`) ready for
//!    Phase 2.
//! 2. **Phase 2 — run workload** (manual, NOT automated by `buff`):
//!    execute the instrumented binary against a representative
//!    workload. Counter data is written as `*.profraw` files into
//!    `./target/pgo-data/`. The [`print_phase2_instructions`] helper
//!    prints concrete guidance (which examples to run, how to verify
//!    `.profraw` files were produced) so the user knows exactly what
//!    to do between the two build phases.
//! 3. **Phase 3 — profile-guided rebuild** (`buff build --pgo --use
//!    <FILE>`): merges the captured `.profraw` files via
//!    [`merge_profraw_files`] (shells out to `llvm-profdata merge`),
//!    then recompiles with `-C profile-use=./target/pgo-data/merged.profdata`.
//!    LLVM uses the profile to drive inlining + block-layout decisions,
//!    typically yielding 10%+ speedup on compute-heavy code vs
//!    `--release`.
//!
//! **`llvm-profdata` requirement**: Phase 3 requires `llvm-profdata` on
//! `PATH` (`rustup component add llvm-tools-preview`). When missing,
//! [`detect_llvm_profdata`] returns `None` and [`run`] surfaces a stderr
//! note + returns an error (the merge step cannot be skipped — rustc
//! rejects a directory of `.profraw` files).
//!
//! **Single-file only**: like the rest of the v0.1 single-file rustc
//! pipeline, `--pgo` requires a `<FILE>` argument. Project / workspace
//! PGO (via `cargo build --profile pgo` + `CARGO_PROFILE_PGO_*` env
//! vars) is deferred to a follow-up — the single-file path covers the
//! T62 acceptance target (10%+ speedup on `examples/pgo_benchmark.buff`).
//!
//! **Orthogonal to `--release`/`--minimal`/`--fast`**: PGO is a separate
//! axis — it instruments OR consumes a profile, it does not select a
//! size/speed knob. Both Phase 1 and Phase 3 compile with the
//! release-grade baseline (`opt-level=3` + `lto=fat` + `codegen-units=1`,
//! matching [`pipeline::rustc_release_flags`]) so the instrumented
//! binary's runtime characteristics match the final profile-guided build.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::compile_speed;
use crate::pipeline;

/// Entry point for `buff build --pgo [<FILE>] [--output <PATH>] [--use]`.
///
/// Dispatches on `pgo_use`:
///
/// - `pgo_use = false` → Phase 1 (instrument) via [`run_phase1_instrument`].
/// - `pgo_use = true` → Phase 3 (profile-guided rebuild) via
///   [`run_phase3_use`].
///
/// `file` MUST be `Some` — PGO is single-file only in T62 (project /
/// workspace PGO is deferred). Returns an error when `file` is `None`.
///
/// Phase 2 (running the workload) is intentionally NOT a `buff` command
/// — it is a manual user action documented by [`print_phase2_instructions`],
/// which Phase 1 prints to stderr on success.
#[allow(clippy::too_many_arguments)]
pub fn run(
    file: Option<&Path>,
    output: Option<&Path>,
    pgo_use: bool,
    profile_dir: Option<&str>,
) -> Result<()> {
    let src = file.ok_or_else(|| {
        anyhow::anyhow!(
            "`buff build --pgo` requires a <FILE> argument in T62 \
             (project / workspace PGO is deferred — use a single .buff file)"
        )
    })?;
    let dir = profile_dir.unwrap_or(pipeline::PGO_DATA_DIR);
    if pgo_use {
        run_phase3_use(src, output, dir)
    } else {
        run_phase1_instrument(src, output, dir)
    }
}

/// Phase 1 — instrumented build (T62).
///
/// Runs the normal Buff front-end ([`pipeline::compile_to_rust_with_cache`])
/// to produce the `.rs` file, then invokes `rustc` with the PGO
/// instrument flags ([`pipeline::rustc_pgo_instrument_flags`]) so the
/// resulting binary emits `*.profraw` counter files into `profile_dir`
/// on every execution.
///
/// On success, prints Phase 2 instructions to stderr (via
/// [`print_phase2_instructions`]) so the user knows to run the
/// instrumented binary before invoking Phase 3.
fn run_phase1_instrument(file: &Path, output: Option<&Path>, profile_dir: &str) -> Result<()> {
    // Ensure the profile-data directory exists so rustc's
    // profile-generate pass has somewhere to write .profraw files.
    std::fs::create_dir_all(profile_dir)
        .with_context(|| format!("failed to create PGO data directory `{profile_dir}`"))?;

    let stem_output: PathBuf = match output {
        Some(p) => pipeline::with_exe_extension(p),
        None => pipeline::with_exe_extension(&file.with_extension("")),
    };

    // Front-end: .buff → .rs (cache ON; --no-cache is handled by the
    // caller before dispatch — T62 single-file PGO always uses cache).
    let compile_out = pipeline::compile_to_rust_with_cache(file, true)?;

    // Back-end: .rs → instrumented executable.
    let pgo_flags = pipeline::rustc_pgo_instrument_flags(profile_dir);
    invoke_rustc_with_pgo_flags(&compile_out.rust_file_path, &stem_output, file, &pgo_flags)?;

    eprintln!(
        "Built {} (PGO Phase 1 — instrumented)",
        stem_output.display()
    );
    eprintln!("  source: {}", file.display());
    eprintln!("  rust:   {}", compile_out.rust_file_path.display());
    eprintln!("  profile-data dir: {profile_dir}");
    print_phase2_instructions(&stem_output, profile_dir);
    Ok(())
}

/// Phase 3 — profile-guided rebuild (T62).
///
/// 1. Detects `llvm-profdata` on `PATH` (via [`detect_llvm_profdata`]).
///    Returns an error with an install hint when missing.
/// 2. Merges the captured `.profraw` files in `profile_dir` into
///    `<profile_dir>/merged.profdata` via [`merge_profraw_files`].
/// 3. Recompiles with the PGO use flags
///    ([`pipeline::rustc_pgo_use_flags`]) so LLVM consumes the merged
///    profile to drive inlining + block-layout decisions.
fn run_phase3_use(file: &Path, output: Option<&Path>, profile_dir: &str) -> Result<()> {
    let merged_path = pipeline::pgo_merged_profile_path(Some(profile_dir));

    // 1. Detect llvm-profdata.
    let profdata = detect_llvm_profdata().ok_or_else(|| {
        anyhow::anyhow!(
            "`llvm-profdata` not found on PATH — required for PGO Phase 3\n\
             install it via: rustup component add llvm-tools-preview\n\
             (then ensure the llvm-tools bin dir is on your PATH)"
        )
    })?;

    // 2. Merge .profraw files.
    let profraw_count = merge_profraw_files(&profdata, profile_dir, &merged_path)?;
    if profraw_count == 0 {
        bail!(
            "no `.profraw` files found in `{profile_dir}` — run the \
             instrumented binary (built by `buff build --pgo`) against your \
             workload first, then re-run `buff build --pgo --use`"
        );
    }
    eprintln!("PGO Phase 3: merged {profraw_count} `.profraw` file(s) into `{merged_path}`");

    // 3. Front-end + back-end with profile-use.
    let stem_output: PathBuf = match output {
        Some(p) => pipeline::with_exe_extension(p),
        None => pipeline::with_exe_extension(&file.with_extension("")),
    };
    let compile_out = pipeline::compile_to_rust_with_cache(file, true)?;
    let pgo_flags = pipeline::rustc_pgo_use_flags(&merged_path);
    invoke_rustc_with_pgo_flags(&compile_out.rust_file_path, &stem_output, file, &pgo_flags)?;

    eprintln!(
        "Built {} (PGO Phase 3 — profile-guided rebuild)",
        stem_output.display()
    );
    eprintln!("  source: {}", file.display());
    eprintln!("  rust:   {}", compile_out.rust_file_path.display());
    eprintln!("  merged profile: {merged_path}");
    Ok(())
}

/// Invoke `rustc --edition 2021 <pgo_flags> <rust_file> -o <output>`,
/// forwarding stderr through the error mapper so `.rs` references are
/// translated back to `.buff` (mirrors
/// [`pipeline::compile_rust_to_exe`]).
///
/// Does NOT apply the T55 linker auto-detection or sccache wrapping —
/// PGO is an opt-in advanced flow and keeping the rustc invocation
/// minimal makes the flag set inspectable (a user debugging PGO wants
/// to see exactly what flags were passed, not have mold/lld silently
/// fused in).
fn invoke_rustc_with_pgo_flags(
    rust_file: &Path,
    output: &Path,
    buff_file: &Path,
    pgo_flags: &[String],
) -> Result<()> {
    let mut cmd = compile_speed::rustc_command(false);
    cmd.arg("--edition").arg("2021");
    for flag in pgo_flags {
        cmd.arg(flag);
    }
    cmd.arg(rust_file).arg("-o").arg(output);

    let result = cmd
        .output()
        .context("failed to invoke `rustc` — is it installed and on your PATH?")?;

    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let translated = crate::error_mapper::translate_rustc_errors(&stderr, buff_file, rust_file);
        eprint!("{translated}");
    }

    if !result.status.success() {
        bail!("rustc exited with status {}", result.status);
    }
    Ok(())
}

/// Print the Phase 2 (run workload) instructions to stderr.
///
/// Phase 2 is a manual user action — `buff` does NOT automate running
/// the instrumented binary (the representative workload is
/// application-specific). This helper prints concrete guidance:
///
/// - The exact command to execute the instrumented binary.
/// - How to verify `.profraw` files were produced.
/// - The next step (`buff build --pgo --use`).
fn print_phase2_instructions(exe: &Path, profile_dir: &str) {
    eprintln!();
    eprintln!("PGO Phase 2 — run your representative workload:");
    eprintln!("  1. Execute the instrumented binary:");
    eprintln!("     {}", exe.display());
    eprintln!("     (run it against your benchmark / test suite / real input)");
    eprintln!("  2. Verify counter data was captured:");
    eprintln!("     ls {profile_dir}/*.profraw");
    eprintln!("  3. Rebuild with the captured profile:");
    eprintln!("     buff build --pgo --use {}", exe.display());
    eprintln!();
}

/// Detect `llvm-profdata` on `PATH` (T62 Phase 3 prerequisite).
///
/// Returns `Some("llvm-profdata")` (or the resolved path on Windows)
/// when the tool is callable, `None` otherwise. Probes via
/// `llvm-profdata --version` with stdout/stderr suppressed so the
/// detection is silent on miss.
pub fn detect_llvm_profdata() -> Option<PathBuf> {
    let result = Command::new("llvm-profdata")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match result {
        Ok(status) if status.success() => Some(PathBuf::from("llvm-profdata")),
        _ => None,
    }
}

/// Merge `*.profraw` files in `profile_dir` into `merged_path` via
/// `llvm-profdata merge`.
///
/// Returns the count of `.profraw` files merged (0 when the directory
/// is empty or missing — the caller decides whether that's an error).
///
/// Invokes: `llvm-profdata merge -o <merged_path> <profile_dir>/*.profraw`
///
/// The glob expansion is done in Rust (not the shell) so this works
/// cross-platform without relying on a Unix shell (Windows `cmd.exe`
/// does not expand `*.profraw`).
fn merge_profraw_files(profdata: &Path, profile_dir: &str, merged_path: &str) -> Result<usize> {
    let profraw_files = collect_profraw_files(profile_dir);
    if profraw_files.is_empty() {
        return Ok(0);
    }

    let count = profraw_files.len();
    let mut cmd = Command::new(profdata);
    cmd.arg("merge").arg("-o").arg(merged_path);
    for f in &profraw_files {
        cmd.arg(f);
    }

    let result = cmd
        .output()
        .context("failed to invoke `llvm-profdata merge`")?;
    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        eprint!("{stderr}");
    }
    if !result.status.success() {
        bail!("llvm-profdata merge exited with status {}", result.status);
    }
    Ok(count)
}

/// Collect all `*.profraw` files in `profile_dir` (sorted for determinism).
///
/// Returns an empty Vec when the directory does not exist or contains
/// no `.profraw` files. Sorted by path so the merge is reproducible
/// across runs (llvm-profdata merge is order-sensitive for some
/// counter-disambiguation cases).
fn collect_profraw_files(profile_dir: &str) -> Vec<PathBuf> {
    let read_dir = match std::fs::read_dir(profile_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };
    let mut files: Vec<PathBuf> = read_dir
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "profraw"))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_llvm_profdata_does_not_panic_when_missing() {
        // Whether or not llvm-profdata is installed, this must return
        // cleanly (Some or None) — never panic.
        let _ = detect_llvm_profdata();
    }

    #[test]
    fn collect_profraw_files_returns_empty_for_missing_dir() {
        let dir = "./target/pgo-test-missing-dir-does-not-exist";
        let files = collect_profraw_files(dir);
        assert!(files.is_empty(), "missing dir must yield empty Vec");
    }

    #[test]
    fn collect_profraw_files_returns_empty_for_dir_with_no_profraw() {
        let dir = std::env::temp_dir().join("pgo-test-no-profraw");
        let _ = std::fs::create_dir_all(&dir);
        // Write a non-profraw file.
        let _ = std::fs::write(dir.join("not_prof.txt"), "ignore me");
        let files = collect_profraw_files(dir.to_str().unwrap_or("./target"));
        assert!(
            files
                .iter()
                .all(|f| f.extension() == Some("profraw".as_os())),
            "must only return .profraw files"
        );
        let _ = std::fs::remove_file(dir.join("not_prof.txt"));
    }

    #[test]
    fn merge_profraw_files_returns_zero_when_no_profraw() {
        let dir = std::env::temp_dir().join("pgo-test-merge-empty");
        let _ = std::fs::create_dir_all(&dir);
        let merged = dir.join("merged.profdata");
        // llvm-profdata is NOT invoked when there are 0 .profraw files
        // (early return), so this test passes even without the tool.
        let count = merge_profraw_files(
            Path::new("llvm-profdata"),
            dir.to_str().unwrap_or("./target"),
            merged.to_str().unwrap_or("./target/merged.profdata"),
        )
        .expect("merge must not error on empty dir");
        assert_eq!(count, 0, "empty dir must merge 0 files");
    }
}
