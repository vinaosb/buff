//! `buff build` — compile a `.buff` file or project into a native executable.
//!
//! Two modes:
//!
//! 1. **Single-file mode** (v0.1 behavior): `buff build <FILE> [--release]`
//!    compiles a single `.buff` file via the Buff pipeline → rustc.
//!
//! 2. **Project mode** (T120): `buff build [--release]` (no file argument)
//!    looks for `buff.toml` in the current directory, generates `Cargo.toml`,
//!    and shells out to `cargo build` / `cargo build --release`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::config::{self, BuffConfig};
use crate::pipeline;

/// Entry point for `buff build [<FILE>] [--output <PATH>] [--release]`.
///
/// When `file` is `Some`, compiles that single `.buff` file (v0.1 behavior).
/// When `file` is `None`, builds the project in the current directory via
/// `cargo build` (T120 project-level build).
pub fn run(file: Option<&Path>, output: Option<&Path>, release: bool) -> Result<()> {
    match file {
        Some(f) => build_single_file(f, output, release),
        None => build_project(release),
    }
}

/// Build a single `.buff` OR `.buffhtml` file via the Buff pipeline → rustc
/// (v0.1 behavior; T133 extends to `.buffhtml`).
///
/// Dispatches on file extension:
/// - `.buff` (default): [`pipeline::compile_to_rust`] →
///   [`pipeline::compile_rust_to_exe`].
/// - `.buffhtml` (T133): [`pipeline::compile_buffhtml_to_rust`] →
///   [`pipeline::compile_buffhtml_rust_to_exe`] with the post-format
///   [`SpanMap`] wired through for span-aware error mapping.
fn build_single_file(file: &Path, output: Option<&Path>, release: bool) -> Result<()> {
    let stem_output: PathBuf = match output {
        Some(p) => pipeline::with_exe_extension(p),
        None => pipeline::with_exe_extension(&file.with_extension("")),
    };
    let mode = pipeline::BuildMode::from_release_flag(release);

    let is_buffhtml = file
        .extension()
        .is_some_and(|e| e == pipeline::BUFFHTML_EXT);
    if is_buffhtml {
        let compile_out = pipeline::compile_buffhtml_to_rust(file)?;
        pipeline::compile_buffhtml_rust_to_exe(
            &compile_out.rust_file_path,
            &stem_output,
            file,
            mode,
            &compile_out.span_map,
            "", // source not retained on CompileOutput; error_mapper handles miss gracefully
        )?;
        eprintln!("Built {} ({})", stem_output.display(), mode_label(mode));
        eprintln!("  source: {}", file.display());
        eprintln!("  rust:   {}", compile_out.rust_file_path.display());
        return Ok(());
    }

    let compile_out = pipeline::compile_to_rust(file)?;
    pipeline::compile_rust_to_exe(&compile_out.rust_file_path, &stem_output, file, mode)?;
    eprintln!("Built {} ({})", stem_output.display(), mode_label(mode));
    eprintln!("  source: {}", file.display());
    eprintln!("  rust:   {}", compile_out.rust_file_path.display());
    Ok(())
}

/// Build the project in the current directory via `cargo build` (T120).
///
/// 1. Reads `buff.toml` from the current directory.
/// 2. Detects workspace mode (T123): if `[workspace]` is present,
///    delegates to [`build_workspace`] (emit virtual `Cargo.toml`,
///    transpile each member, `cargo build` at workspace root — cargo
///    fans out to members automatically).
/// 3. Otherwise: generates `Cargo.toml` from the manifest, transpiles
///    all `.buff` files in `src/`, invokes `cargo build`.
fn build_project(release: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;

    // Read buff.toml
    let manifest_path = cwd.join("buff.toml");
    let cfg = BuffConfig::load_from_file(&manifest_path)
        .with_context(|| format!("failed to load `{}`", manifest_path.display()))?;

    // Workspace mode (T123) — emit virtual Cargo.toml + transpile each member.
    if cfg.is_workspace() {
        return build_workspace(&cwd, &cfg, release);
    }

    // Single-package project mode (T120).
    let package = cfg
        .package
        .as_ref()
        .context("buff.toml has no [package] section (and no [workspace] — invalid manifest)")?;

    // Generate Cargo.toml
    let cargo_toml = config::generate_cargo_toml(&cfg);
    let cargo_path = cwd.join("Cargo.toml");
    std::fs::write(&cargo_path, &cargo_toml)
        .with_context(|| format!("failed to write `{}`", cargo_path.display()))?;

    // Transpile all source files in src/. T133: discover BOTH .buff and
    // .buffhtml files (the floor-grammar MUST-ship list, decision record
    // rsx-syntax-feasibility.md §6, requires `buff build` to recognise
    // .buffhtml alongside .buff in src/).
    let src_dir = cwd.join("src");
    if src_dir.is_dir() {
        for entry in std::fs::read_dir(&src_dir)
            .with_context(|| format!("failed to read `{}`", src_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "buff") {
                pipeline::compile_to_rust(&path)?;
            } else if path
                .extension()
                .is_some_and(|ext| ext == pipeline::BUFFHTML_EXT)
            {
                pipeline::compile_buffhtml_to_rust(&path)?;
            }
        }
    }

    // Invoke cargo build
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if release {
        cmd.arg("--release");
    }

    let result = cmd
        .output()
        .context("failed to invoke `cargo` — is it installed and on your PATH?")?;

    // Forward cargo's stderr (progress / warnings).
    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        eprint!("{stderr}");
    }

    if !result.status.success() {
        anyhow::bail!("cargo build exited with status {}", result.status);
    }

    let mode_str = if release { "release" } else { "debug" };
    eprintln!("Built project `{}` ({mode_str})", package.name);
    Ok(())
}

/// Build a workspace by shelling out to `cargo build` at the workspace root
/// (T123).
///
/// Passthrough semantics — the Buff CLI does NOT reinvent workspace
/// dependency-dedup or shared-`target/`. Cargo handles fan-out to all
/// members automatically; this function's job is:
///
/// 1. Emit a virtual `Cargo.toml` at the workspace root (no `[package]`).
/// 2. Transpile every member's `src/*.buff` files to `.rs` (cargo itself
///    does NOT know about `.buff` — we must produce the `.rs` cargo expects).
/// 3. Invoke `cargo build` (or `cargo build --release`) at the root.
///
/// Shared `target/` + dep-dedup come FREE from cargo workspaces — that's
/// the entire reason this is a passthrough.
fn build_workspace(root: &Path, cfg: &BuffConfig, release: bool) -> Result<()> {
    // `build_workspace` is only reached after `cfg.is_workspace()` returned
    // true in `build_project`, so workspace is guaranteed Some. Use
    // `context` (not `expect`) to honour the no-panic repo rule — if a
    // future caller violates the precondition, the error surfaces loudly
    // via the normal error-mapper instead of crashing the process.
    let ws = cfg
        .workspace
        .as_ref()
        .context("internal: build_workspace called without [workspace] section")?;

    // 1. Emit virtual Cargo.toml at the workspace root.
    let cargo_toml = config::generate_cargo_toml(cfg);
    let cargo_path = root.join("Cargo.toml");
    std::fs::write(&cargo_path, &cargo_toml)
        .with_context(|| format!("failed to write `{}`", cargo_path.display()))?;

    // 2. Transpile each member's source files. Buff (not cargo) owns the
    //    `.buff`/`.buffhtml` → `.rs` step; cargo then compiles the `.rs`
    //    files into member binaries. We do NOT loop members at the cargo
    //    level (cargo fans out itself) — we only loop to run the Buff
    //    transpiler. T133: also pick up `.buffhtml` files in each member's
    //    src/ dir (decision record §6 MUST-ship list).
    for member in &ws.members {
        let member_src = root.join(member).join("src");
        if !member_src.is_dir() {
            // Member without src/ is malformed; let cargo surface the error
            // (it will report "missing Cargo.toml" or similar). Don't panic.
            continue;
        }
        for entry in std::fs::read_dir(&member_src)
            .with_context(|| format!("failed to read `{}`", member_src.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "buff") {
                pipeline::compile_to_rust(&path)?;
            } else if path
                .extension()
                .is_some_and(|ext| ext == pipeline::BUFFHTML_EXT)
            {
                pipeline::compile_buffhtml_to_rust(&path)?;
            }
        }
    }

    // 3. Shell out to cargo build at workspace root (fans out to members).
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if release {
        cmd.arg("--release");
    }

    let result = cmd
        .output()
        .context("failed to invoke `cargo` — is it installed and on your PATH?")?;

    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        eprint!("{stderr}");
    }

    if !result.status.success() {
        anyhow::bail!("cargo build exited with status {}", result.status);
    }

    let mode_str = if release { "release" } else { "debug" };
    eprintln!(
        "Built workspace with {} member(s) ({mode_str})",
        ws.members.len()
    );
    Ok(())
}

/// Render the [`pipeline::BuildMode`] as a user-facing lowercase label for
/// the success line. Kept here (not on the enum) so the pipeline module
/// stays free of presentation concerns — this is a CLI-output helper.
fn mode_label(mode: pipeline::BuildMode) -> &'static str {
    if mode.is_release() {
        "release"
    } else {
        "debug"
    }
}
