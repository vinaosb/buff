//! Shared rustc-invocation helpers used by both the CLI pipeline and
//! `buff-eval` (REPL / Jupyter / Bufflings).
//!
//! Extracted in T35 to eliminate the manual copy-paste between
//! `pipeline.rs` and `buff-eval/src/lib.rs`. The helpers here are
//! pure functions / Command-configuration — they do NOT pull in `clap`,
//! `tokio`, or any CLI-specific types, so `buff-eval` can depend on
//! this crate without inheriting the full CLI dependency tree.
//!
//! # What lives here
//!
//! - [`on_path`] — PATH probe (used by linker detection + sccache check).
//! - [`cranelift_available`] — probe whether the Cranelift codegen
//!   backend is installed (T4).
//! - [`target_is_installed`] — probe whether a rustc target triple is
//!   installed via `rustup target list --installed` (T112).
//! - [`configure_rustc_command`] — apply common flags + env vars to a
//!   `rustc` [`Command`] (edition, optimisation, linker, debuginfo,
//!   Cranelift, cross-compilation target).
//!
//! # What does NOT live here
//!
//! - [`pipeline::with_exe_extension`] — intentionally duplicated in
//!   `buff-eval` (see AGENTS.md: dependency isolation).
//! - Sccache wrapper logic — the CLI and eval use different approaches
//!   (CLI: `sccache rustc ...` via `compile_speed::rustc_command`; eval:
//!   `RUSTC_WRAPPER=sccache` env var). Each caller handles sccache
//!   before calling [`configure_rustc_command`].
//! - Linker-choice resolution — the CLI has 4 variants (Auto/Mold/Lld/
//!   System) while eval has 2 (Auto/System). Each caller resolves its
//!   own linker and passes the resulting flags.
//! - Build-mode flag selection — the CLI has 4 modes (Fast/Debug/Release/
//!   Minimal) while eval always uses Debug. Each caller passes its own
//!   `opt_flags` slice.

use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// PATH probe helper
// ---------------------------------------------------------------------------

/// Returns `true` when `name` (an executable basename) is found on `PATH`.
///
/// Walks `$PATH` entries and checks for an executable file matching `name`
/// (with the platform extension appended on Windows). No subprocess is
/// spawned — this is a pure filesystem probe, so it's cheap to call.
///
/// Mirrors the logic of `which`/`where` without shelling out. Returns
/// `false` when `PATH` is unset or empty.
pub fn on_path(name: &str) -> bool {
    let path_var = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    let candidates: Vec<PathBuf> = std::env::split_paths(&path_var).collect();
    for dir in candidates {
        let full = dir.join(name);
        if is_executable(&full) {
            return true;
        }
        // On Windows, also try with `.exe` (and `.bat`).
        if cfg!(windows) {
            if is_executable(&dir.join(format!("{name}.exe"))) {
                return true;
            }
        }
    }
    false
}

/// Cross-platform "is this path an executable file" check.
///
/// On Unix this checks the executable bit; on Windows it checks that the
/// file exists (Windows determines executability by extension, which the
/// caller already appended).
fn is_executable(path: &PathBuf) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(md) => md.is_file() && md.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(path)
            .map(|m| m.is_file())
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Cranelift backend probe (T4)
// ---------------------------------------------------------------------------

/// Probe whether the Cranelift codegen backend is available (T4).
///
/// Runs `rustc +nightly -C codegen-backend=cranelift --version` (silent
/// — output is discarded). Returns `true` when the probe succeeds (exit
/// 0), `false` on any failure (missing nightly, missing component,
/// rustc not on PATH, etc.).
///
/// This is the single source of truth for "is Cranelift usable on this
/// host?" — both the CLI pipeline and `buff-eval` consult it before
/// setting `CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift` on the spawned
/// rustc process. The probe is cheap (sub-second) and runs at most once
/// per compile invocation.
///
/// # Why `+nightly`
///
/// `rustc-codegen-cranelift-preview` is currently nightly-only on
/// stable rustup channels. The probe therefore uses `+nightly` to
/// exercise the actual toolchain that would be used. A future stable
/// promotion would simplify this to bare `rustc`.
pub fn cranelift_available() -> bool {
    let probe = Command::new("rustc")
        .arg("+nightly")
        .arg("-C")
        .arg("codegen-backend=cranelift")
        .arg("--version")
        .output();
    match probe {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Target-installed probe (T112)
// ---------------------------------------------------------------------------

/// Probe whether a rustc target triple is installed (T112).
///
/// Runs `rustup target list --installed` and checks if `<triple>` appears
/// in the output. Returns `true` when the target is listed, `false` on
/// any failure (rustup not on PATH, probe error, target not found).
///
/// The probe is cheap (sub-second) and runs at most once per compile
/// invocation when `--target` is set.
pub fn target_is_installed(triple: &str) -> bool {
    let probe = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    match probe {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().any(|line| line.trim() == triple)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Shared rustc Command configuration
// ---------------------------------------------------------------------------

/// Configure a `rustc` [`Command`] with common flags shared by the CLI
/// pipeline and `buff-eval`.
///
/// Applies (in order):
///
/// 1. `--edition 2021`
/// 2. `opt_flags` — optimisation / LTO flags (e.g. `["-O"]` for Debug,
///    `["-C", "opt-level=3", "-C", "lto=fat", ...]` for Release).
/// 3. `linker_flags` — fast-linker `-C link-arg=-fuse-ld=<name>` flags
///    (empty slice for system default).
/// 4. `CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift` env var when
///    `use_cranelift` is `true` (scoped to the child process).
/// 5. `-C <debuginfo_flag>` — e.g. `debuginfo=1` for line tables.
/// 6. `--target <triple>` when `target` is `Some` (with an installed
///    check via [`target_is_installed`] — returns `Err` if missing).
///
/// After calling this, the caller should add:
///
/// ```ignore
/// cmd.arg(rust_file).arg("-o").arg(output);
/// let result = cmd.output()?;
/// ```
///
/// # Errors
///
/// Returns `Err(msg)` when `target` is `Some` but the triple is not
/// installed (via [`target_is_installed`]).
///
/// # Sccache note
///
/// Sccache is NOT handled here because the CLI and eval use different
/// approaches (CLI: `sccache rustc ...` wrapper command; eval:
/// `RUSTC_WRAPPER=sccache` env var). Each caller should set up sccache
/// on the `Command` *before* calling this function.
pub fn configure_rustc_command(
    cmd: &mut Command,
    opt_flags: &[&str],
    linker_flags: &[&str],
    use_cranelift: bool,
    debuginfo_flag: &str,
    target: Option<&str>,
) -> Result<(), String> {
    cmd.arg("--edition").arg("2021");

    for flag in opt_flags {
        cmd.arg(flag);
    }

    for flag in linker_flags {
        cmd.arg(flag);
    }

    // T4: Cranelift dev backend env var (scoped to the child process).
    if use_cranelift {
        cmd.env("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "cranelift");
    }

    // T3: debug-info flag (e.g. "debuginfo=1").
    cmd.arg("-C");
    cmd.arg(debuginfo_flag);

    // T112: cross-compilation target.
    if let Some(triple) = target {
        if !target_is_installed(triple) {
            return Err(format!(
                "Target `{triple}` is not installed.\n\
                 Run: rustup target add {triple}\n\n\
                 Common targets:\n\
                   x86_64-unknown-linux-gnu   (Linux x86_64)\n\
                   aarch64-apple-darwin        (Apple Silicon macOS)\n\
                   x86_64-pc-windows-msvc     (Windows x86_64)\n\
                   wasm32-unknown-unknown      (WebAssembly)"
            ));
        }
        cmd.arg("--target");
        cmd.arg(triple);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_path_returns_false_for_nonexistent() {
        // A name that is vanishingly unlikely to be on any PATH.
        assert!(!on_path("zz_buff_test_nonexistent_zz"));
    }

    #[test]
    fn cranelift_available_does_not_panic() {
        // The probe may return true or false depending on the host; we
        // only assert it doesn't crash.
        let _ = cranelift_available();
    }

    #[test]
    fn target_is_installed_returns_false_for_bogus_triple() {
        assert!(!target_is_installed("nonexistent-target-triple-12345"));
    }

    #[test]
    fn configure_rustc_command_sets_edition_and_flags() {
        let mut cmd = Command::new("rustc");
        configure_rustc_command(
            &mut cmd,
            &["-O"],
            &[],
            false,
            "debuginfo=1",
            None,
        )
        .expect("configure should succeed");

        let args: Vec<&str> = cmd.get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(
            args.contains(&"--edition"),
            "expected --edition in args: {args:?}"
        );
        assert!(
            args.contains(&"2021"),
            "expected 2021 in args: {args:?}"
        );
        assert!(
            args.contains(&"-O"),
            "expected -O in args: {args:?}"
        );
        assert!(
            args.contains(&"debuginfo=1"),
            "expected debuginfo=1 in args: {args:?}"
        );
    }

    #[test]
    fn configure_rustc_command_rejects_missing_target() {
        let mut cmd = Command::new("rustc");
        let result = configure_rustc_command(
            &mut cmd,
            &["-O"],
            &[],
            false,
            "debuginfo=1",
            Some("nonexistent-target-triple-12345"),
        );
        assert!(result.is_err(), "expected error for missing target");
        assert!(
            result.unwrap_err().contains("not installed"),
            "error should mention 'not installed'"
        );
    }

    #[test]
    fn configure_rustc_command_sets_cranelift_env() {
        let mut cmd = Command::new("rustc");
        configure_rustc_command(
            &mut cmd,
            &["-O"],
            &[],
            true,
            "debuginfo=1",
            None,
        )
        .expect("configure should succeed");

        let env_val = cmd.get_envs().find_map(|(k, v)| {
            if k == "CARGO_PROFILE_DEV_CODEGEN_BACKEND" {
                v.map(|v| v.to_str().unwrap().to_string())
            } else {
                None
            }
        });
        assert_eq!(
            env_val.as_deref(),
            Some("cranelift"),
            "expected CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift"
        );
    }

    #[test]
    fn configure_rustc_command_sets_linker_flags() {
        let mut cmd = Command::new("rustc");
        configure_rustc_command(
            &mut cmd,
            &["-O"],
            &["-C", "link-arg=-fuse-ld=mold"],
            false,
            "debuginfo=1",
            None,
        )
        .expect("configure should succeed");

        let args: Vec<&str> = cmd.get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(
            args.contains(&"link-arg=-fuse-ld=mold"),
            "expected linker flag in args: {args:?}"
        );
    }
}
