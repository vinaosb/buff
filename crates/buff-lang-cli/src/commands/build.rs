//! `buff build` — compile a `.buff` file into a native executable.
//!
//! Pipeline: [`pipeline::compile_to_rust`] → [`pipeline::compile_rust_to_exe`].
//! The intermediate `.rs` file is left on disk alongside the source so users
//! can inspect the transpiled Rust (this is a documented v0.1 behavior —
//! `buff run` is the variant that cleans up).

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::pipeline;

/// Entry point for `buff build <FILE> [--output <PATH>]`.
///
/// - Compiles `file` to Rust source (writes `<file>.rs`).
/// - Invokes `rustc` to produce an executable at `output` (or
///   `<file-stem>` with the platform exe extension if `--output` was omitted).
/// - Prints a short confirmation to stderr on success.
///
/// # Errors
///
/// Propagates any pipeline error (file-not-found, lex/parse/codegen failure,
/// rustc invocation failure) with rich context.
pub fn run(file: &Path, output: Option<&Path>) -> Result<()> {
    let compile_out = pipeline::compile_to_rust(file)?;

    let stem_output: PathBuf = match output {
        Some(p) => pipeline::with_exe_extension(p),
        None => pipeline::with_exe_extension(&file.with_extension("")),
    };

    let exe_path = pipeline::compile_rust_to_exe(&compile_out.rust_file_path, &stem_output, file)?;

    eprintln!("Built {}", exe_path.display());
    eprintln!("  source: {}", file.display());
    eprintln!("  rust:   {}", compile_out.rust_file_path.display());
    Ok(())
}
