//! `buff build` — compile a `.buff` file or project into a native executable.
//!
//! Two modes:
//!
//! 1. **Single-file mode** (v0.1 behavior): `buff build <FILE> [--release]
//!    [--minimal] [--fast]` compiles a single `.buff` file via the Buff
//!    pipeline → rustc. The `--target` flag is IGNORED in this mode (single-file
//!    rustc does not cross-compile). The `--minimal` flag (T60) selects the
//!    size-minimized rustc flag set (`opt-level=z` + `panic=abort` +
//!    `strip=symbols` + `lto=true` + `codegen-units=1`). The `--fast` flag
//!    (T55) selects the no-optimization profile (`opt-level=0` +
//!    `debuginfo=0`) for the fastest possible compile.
//!
//! 2. **Project mode** (T120 / T1 multi-file linking): `buff build
//!    [--release] [--minimal] [--fast] [--target <TRIPLE>]` (no file
//!    argument) looks for `buff.toml` in the current directory, generates
//!    `Cargo.toml`, and shells out to `cargo build`. T1 wires this through
//!    [`project_pipeline::compile_project_to_cargo`] which walks every
//!    transitively-imported module, builds a module graph (cycle
//!    detection + visibility check), and flattens everything into a
//!    single Rust source. `--target list` prints the Buff-supported
//!    target set and exits without building. `--minimal` (T60) sets the
//!    `RUSTFLAGS` env var with the size-minimization flags before invoking
//!    cargo (so the same flag set applies whether the build goes through
//!    bare rustc or cargo-driven compilation).
//!
//! T55 compile-speed knobs (all opt-in):
//!
//! - `--no-cache` — bypass the generated-Rust cache (force full codegen).
//! - `--sccache` — wrap rustc in `sccache` for cross-project crate caching
//!   (also writes `.cargo/config.toml` so bare `cargo` picks it up).
//! - `--fast` — lowest-precedence profile; `--minimal`/`--release` override.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::compile_speed;
use crate::config::{self, BuffConfig};
use crate::pipeline;
use crate::project_pipeline::{
    self, cargo_build_project, compile_project_to_cargo, CargoMode, TARGET_LIST_KEYWORD,
};

/// Entry point for `buff build [<FILE>] [--output <PATH>] [--release]
/// [--minimal] [--fast] [--no-cache] [--sccache] [--target <TRIPLE>]
/// [--linker <auto|mold|lld|system>] [--debuginfo <line-tables-only|full|none>]
/// [--backend <llvm|cranelift>]`.
///
/// When `file` is `Some`, compiles that single `.buff` file (v0.1 behavior;
/// the `--target` flag is ignored in this mode).
/// When `file` is `None`, builds the project in the current directory via
/// `cargo build` (T120 project-level build). The `--target <TRIPLE>` flag
/// is forwarded to cargo; `--target list` prints the supported-target list
/// and exits without building.
///
/// Precedence: `minimal` (T60) > `release` (T56) > `fast` (T55) > debug —
/// mirrors cargo's `--profile` semantics (a more-specific profile wins).
///
/// T4: `--backend=cranelift` is honoured ONLY for Debug builds — the
/// pipeline gate enforces LLVM for release/minimal/fast regardless of
/// the flag. The project-mode path (cargo build) does NOT honour
/// `--backend` (cargo's own `[profile.dev] codegen-backend` controls
/// that; buff does not rewrite the user's `Cargo.toml`).
#[allow(clippy::too_many_arguments)]
pub fn run(
    file: Option<&Path>,
    output: Option<&Path>,
    release: bool,
    minimal: bool,
    fast: bool,
    no_cache: bool,
    sccache: bool,
    target: Option<&str>,
    linker: pipeline::LinkerChoice,
    debuginfo: pipeline::DebugInfoChoice,
    backend: pipeline::BackendChoice,
) -> Result<()> {
    // T55: when --sccache is requested, write the .cargo/config.toml
    // snippet so bare `cargo build`/`cargo test` also pick up sccache.
    // Best-effort — a write failure logs a warning but does not fail the
    // build (the `buff build` path still wraps rustc directly).
    if sccache {
        write_sccache_config_best_effort();
    }
    match file {
        Some(f) => {
            if let Some(t) = target {
                if t == TARGET_LIST_KEYWORD {
                    println!("{}", project_pipeline::target_list_str());
                    return Ok(());
                }
                eprintln!(
                    "warning: --target {t} is ignored in single-file mode \
                     (use project mode by omitting the FILE argument to cross-compile)"
                );
            }
            build_single_file(f, output, release, minimal, fast, no_cache, sccache, linker, debuginfo, backend)
        }
        None => build_project(release, minimal, fast, target),
    }
}

/// Write `[build] rustc-wrapper = "sccache"` to `.cargo/config.toml` so
/// subsequent bare `cargo` invocations also use sccache (T55).
///
/// Best-effort: creates `.cargo/` on demand, overwrites `config.toml`
/// idempotently. A write failure logs a stderr note and returns `Ok(())`
/// — the `buff build` rustc-wrapper path still works regardless.
fn write_sccache_config_best_effort() {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => return,
    };
    let cargo_dir = cwd.join(".cargo");
    if let Err(e) = std::fs::create_dir_all(&cargo_dir) {
        eprintln!("note: could not create `{}` ({e})", cargo_dir.display());
        return;
    }
    let config_path = cargo_dir.join("config.toml");
    if let Err(e) = std::fs::write(&config_path, compile_speed::sccache_cargo_config_toml()) {
        eprintln!("note: could not write `{}` ({e})", config_path.display());
    }
}

/// Build a single `.buff` OR `.buffhtml` file via the Buff pipeline → rustc
/// (v0.1 behavior; T133 extends to `.buffhtml`).
///
/// Dispatches on file extension:
/// - `.buff` (default): [`pipeline::compile_to_rust_with_cache`] →
///   [`pipeline::compile_rust_to_exe_with_speed`].
/// - `.buffhtml` (T133): [`pipeline::compile_buffhtml_to_rust`] →
///   [`pipeline::compile_buffhtml_rust_to_exe`] with the post-format
///   [`SpanMap`] wired through for span-aware error mapping.
///
/// `minimal` (T60) selects [`pipeline::BuildMode::Minimal`]; `release`
/// (T56) selects [`pipeline::BuildMode::Release`]; `fast` (T55) selects
/// [`pipeline::BuildMode::Fast`]; default is
/// [`pipeline::BuildMode::Debug`]. Precedence: minimal > release > fast.
#[allow(clippy::too_many_arguments)]
fn build_single_file(
    file: &Path,
    output: Option<&Path>,
    release: bool,
    minimal: bool,
    fast: bool,
    no_cache: bool,
    sccache: bool,
    linker: pipeline::LinkerChoice,
    debuginfo: pipeline::DebugInfoChoice,
    backend: pipeline::BackendChoice,
) -> Result<()> {
    let stem_output: PathBuf = match output {
        Some(p) => pipeline::with_exe_extension(p),
        None => pipeline::with_exe_extension(&file.with_extension("")),
    };
    let mode = pipeline::BuildMode::from_flags_v2(release, minimal, fast);

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

    // T55: cache ON by default (--no-cache bypasses); sccache opt-in.
    let use_cache = !no_cache;
    let compile_out = pipeline::compile_to_rust_with_cache(file, use_cache)?;
    pipeline::compile_rust_to_exe_with_speed(
        &compile_out.rust_file_path,
        &stem_output,
        file,
        mode,
        sccache,
        linker,
        debuginfo,
        backend,
    )?;
    eprintln!("Built {} ({})", stem_output.display(), mode_label(mode));
    eprintln!("  source: {}", file.display());
    eprintln!("  rust:   {}", compile_out.rust_file_path.display());
    Ok(())
}

/// Build the project in the current directory via `cargo build` (T120).
///
/// T1: now uses [`compile_project_to_cargo`] for multi-file linking
/// (cycle detection + cross-file type inference + Cargo project
/// emission). The Cargo project is emitted at
/// `<cwd>/buff_target_project/` and cargo is invoked there with the
/// appropriate flags.
///
/// `minimal` (T60) sets the `RUSTFLAGS` env var with the size-minimization
/// flags before invoking cargo (so the same flag set applies whether the
/// build goes through bare rustc or cargo-driven compilation). The
/// workspace-root `Cargo.toml` declares the matching `[profile.minimal]`
/// block — callers using `cargo build --profile minimal` directly get the
/// identical behavior.
fn build_project(release: bool, minimal: bool, fast: bool, target: Option<&str>) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;

    // --target list short-circuits BEFORE doing any project work so the
    // user can run it outside a project dir too.
    if target == Some(TARGET_LIST_KEYWORD) {
        println!("{}", project_pipeline::target_list_str());
        return Ok(());
    }

    // Read buff.toml (optional for project mode — compile_project_to_cargo
    // emits a minimal Cargo.toml when no buff.toml is found).
    let manifest_path = cwd.join("buff.toml");
    let cfg: Option<BuffConfig> = match BuffConfig::load_from_file(&manifest_path) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!(
                "warning: could not load `{}` ({e}); \
                 emitting minimal Cargo.toml without manifest metadata",
                manifest_path.display()
            );
            None
        }
    };

    // Workspace mode (T123) — delegate to build_workspace.
    if let Some(c) = &cfg {
        if c.is_workspace() {
            return build_workspace(&cwd, c, release, minimal, fast);
        }
    }

    // Single-package project mode (T1 multi-file linking).
    let entry = find_project_entry(&cwd)?;
    let project_dir = cwd.join("buff_target_project");
    let compile_out = compile_project_to_cargo(&entry, Some(&project_dir), cfg.as_ref())?;
    let build_mode = pipeline::BuildMode::from_flags_v2(release, minimal, fast);

    cargo_build_project(
        &compile_out.project_dir,
        CargoMode::Build,
        build_mode,
        target,
    )?;

    let mode_str = mode_label(build_mode);
    let target_str = target.map(|t| format!(" --target {t}")).unwrap_or_default();
    eprintln!(
        "Built project ({mode_str}{target_str}) — output in {}",
        compile_out.project_dir.display()
    );
    Ok(())
}

/// Locate the project entry-point `.buff` file. Prefers `src/main.buff`;
/// falls back to the first `.buff` file in `src/` (sorted).
fn find_project_entry(cwd: &Path) -> Result<PathBuf> {
    let src_dir = cwd.join("src");
    let canonical = src_dir.join("main.buff");
    if canonical.is_file() {
        return Ok(canonical);
    }
    if !src_dir.is_dir() {
        bail!(
            "no `src/` directory in `{}` (use `buff new <NAME>` to scaffold a project)",
            cwd.display()
        );
    }
    let mut buff_files: Vec<PathBuf> = std::fs::read_dir(&src_dir)
        .with_context(|| format!("failed to read `{}`", src_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "buff"))
        .collect();
    buff_files.sort();
    let first = buff_files
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no `.buff` files in `{}`", src_dir.display()))?;
    eprintln!(
        "warning: no `src/main.buff`; using `{}` as the entry point",
        first.display()
    );
    Ok(first)
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
///
/// `minimal` (T60) sets `RUSTFLAGS` with the size-minimization flags
/// before invoking cargo — same flag set the single-file path uses
/// via [`pipeline::rustc_minimal_flags`].
fn build_workspace(
    root: &Path,
    cfg: &BuffConfig,
    release: bool,
    minimal: bool,
    fast: bool,
) -> Result<()> {
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
    //    T60: when --minimal is set, propagate the size-minimization flags
    //    via RUSTFLAGS env var so the same flag set applies as the
    //    single-file rustc path. (We do NOT pass `--profile minimal`
    //    because the virtual workspace Cargo.toml we emit does not declare
    //    the profile — RUSTFLAGS is the universal carrier.)
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if release && !minimal {
        cmd.arg("--release");
    }
    if minimal {
        // T60: size-minimization flags via RUSTFLAGS env var. The joined
        // form is correct because `rustc_minimal_flags()` already emits
        // each token with the `-C` prefix (e.g. `-C opt-level=z`), so
        // `.join(" ")` produces a valid space-separated RUSTFLAGS string.
        cmd.env("RUSTFLAGS", pipeline::rustc_minimal_flags().join(" "));
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

    let mode = pipeline::BuildMode::from_flags_v2(release, minimal, fast);
    let mode_str = mode_label(mode);
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
    match mode {
        pipeline::BuildMode::Fast => "fast",
        pipeline::BuildMode::Debug => "debug",
        pipeline::BuildMode::Release => "release",
        pipeline::BuildMode::Minimal => "minimal",
    }
}
