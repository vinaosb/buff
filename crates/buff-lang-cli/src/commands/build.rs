//! `buff build` — compile a `.buff` file into a native executable.
//!
//! Pipeline: [`pipeline::compile_to_rust`] → [`pipeline::compile_rust_to_exe`].
//! The intermediate `.rs` file is left on disk alongside the source so users
//! can inspect the transpiled Rust (this is a documented v0.1 behavior —
//! `buff run` is the variant that cleans up).
//!
//! T56: the `--release` flag selects [`pipeline::BuildMode::Release`] —
//! maximum optimization with `-C lto=fat -C opt-level=3 -C codegen-units=1`.
//! Default (`--release` omitted) preserves the v0.1 fast-debug profile.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::pipeline;

/// Entry point for `buff build <FILE> [--output <PATH>] [--release]`.
///
/// - Compiles `file` to Rust source (writes `<file>.rs`).
/// - Invokes `rustc` to produce an executable at `output` (or
///   `<file-stem>` with the platform exe extension if `--output` was omitted).
/// - When `release` is `true`, invokes rustc with release-grade optimization
///   (LTO + opt-level 3 + single codegen unit) via [`pipeline::BuildMode`].
/// - Prints a short confirmation to stderr on success (including the build
///   mode, so users can tell at a glance whether they got a debug or release
///   binary).
///
/// # Errors
///
/// Propagates any pipeline error (file-not-found, lex/parse/codegen failure,
/// rustc invocation failure) with rich context.
pub fn run(file: &Path, output: Option<&Path>, release: bool) -> Result<()> {
    let compile_out = pipeline::compile_to_rust(file)?;

    let stem_output: PathBuf = match output {
        Some(p) => pipeline::with_exe_extension(p),
        None => pipeline::with_exe_extension(&file.with_extension("")),
    };

    let mode = pipeline::BuildMode::from_release_flag(release);
    let exe_path =
        pipeline::compile_rust_to_exe(&compile_out.rust_file_path, &stem_output, file, mode)?;

    eprintln!("Built {} ({})", exe_path.display(), mode_label(mode));
    eprintln!("  source: {}", file.display());
    eprintln!("  rust:   {}", compile_out.rust_file_path.display());
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
