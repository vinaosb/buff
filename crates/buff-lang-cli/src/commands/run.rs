//! `buff run` — compile a `.buff` file and execute it immediately.
//!
//! Pipeline: [`pipeline::compile_to_rust`] → [`pipeline::compile_rust_to_exe`]
//! → spawn the executable. Both the executable and the intermediate `.rs` file
//! are removed afterwards (they live in a temp dir / next to the source
//! respectively, so leaving them would pollute the user's workspace).
//!
//! Runtime panics from the compiled binary reference the intermediate `.rs`
//! file. These are intercepted (T16) and translated back to the original
//! `.buff` location via [`crate::error_mapper::translate_panic`] before being
//! forwarded to the user's stderr.

use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::pipeline;

/// Entry point for `buff run <FILE> [-- ARGS]... [--release]`.
///
/// - Compiles `file` to Rust + executable (the executable goes into
///   `std::env::temp_dir().join("buff-run")` so it never pollutes the
///   user's source tree).
/// - When `release` is `true`, compiles with release-grade optimization
///   (T56): `-C lto=fat -C opt-level=3 -C codegen-units=1`. Off by default
///   — `buff run`'s tight edit-run loop usually values compile speed over
///   runtime speed.
/// - Executes the compiled program with `args`, inheriting stdio.
/// - Cleans up both the executable and the `.rs` file.
/// - If the program exits non-zero, exits the `buff` process with the same
///   code (or 1 if no code is available).
///
/// # Errors
///
/// Propagates pipeline errors. A non-zero program exit code is *not* an
/// `Err` from this function — instead the process exits directly so the exit
/// code is preserved.
pub fn run(file: &Path, args: &[String], release: bool) -> Result<()> {
    // T133: dispatch on file extension. `.buffhtml` uses the span-aware
    // pipeline so runtime panics can be reverse-mapped to .buffhtml spans.
    let is_buffhtml = file
        .extension()
        .is_some_and(|e| e == pipeline::BUFFHTML_EXT);

    // Build a deterministic temp location for the executable.
    let temp_dir = std::env::temp_dir().join("buff-run");
    std::fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create temp dir `{}`", temp_dir.display()))?;

    let stem = file
        .file_stem()
        .map(|s| s.to_owned())
        .unwrap_or_else(|| std::ffi::OsString::from("buff_program"));
    let exe_stem = pipeline::with_exe_extension(&temp_dir.join(stem));

    let mode = pipeline::BuildMode::from_release_flag(release);

    // Track the .rs file path + (for .buffhtml) the SpanMap + source so we
    // can post-process runtime panics after execution.
    let (rust_file_path, span_map_opt, source_for_err): (
        std::path::PathBuf,
        Option<buff_lang_codegen_buffhtml::SpanMap>,
        String,
    ) = if is_buffhtml {
        // .buffhtml path: read source once for span-aware panic translation.
        let source = std::fs::read_to_string(file).unwrap_or_default();
        let compile_out = pipeline::compile_buffhtml_to_rust(file)?;
        pipeline::compile_buffhtml_rust_to_exe(
            &compile_out.rust_file_path,
            &exe_stem,
            file,
            mode,
            &compile_out.span_map,
            &source,
        )?;
        (
            compile_out.rust_file_path,
            Some(compile_out.span_map),
            source,
        )
    } else {
        let compile_out = pipeline::compile_to_rust(file)?;
        pipeline::compile_rust_to_exe(&compile_out.rust_file_path, &exe_stem, file, mode)?;
        (compile_out.rust_file_path, None, String::new())
    };

    // Execute, capturing output so runtime panics can be translated (T16).
    let output = Command::new(&exe_stem)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute `{}`", exe_stem.display()))?;

    // Forward the program's stdout (its normal output).
    if !output.stdout.is_empty() {
        let _ = std::io::stdout().write_all(&output.stdout);
        let _ = std::io::stdout().flush();
    }

    // Translate and forward stderr (handles runtime panics that reference
    // the intermediate .rs file — replaces with the original source path).
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    if !stderr_str.is_empty() {
        if let Some(sm) = &span_map_opt {
            // T133: span-aware translation for .buffhtml runtime panics.
            let translated = crate::error_mapper::translate_buffhtml_rustc_errors(
                &stderr_str,
                file,
                &rust_file_path,
                sm,
                &source_for_err,
            );
            eprint!("{translated}");
        } else {
            // v0.1: source map is empty (exact line tracking deferred);
            // filename translation is the primary win. See error_mapper.
            let source_map = buff_lang_error::SourceMap::new();
            let translated = crate::error_mapper::translate_panic(
                &stderr_str,
                &rust_file_path,
                file,
                &source_map,
            );
            eprint!("{translated}");
        }
    }

    // Cleanup — best-effort, never propagates errors. On Windows the just-exited
    // executable may still be image-locked by the OS, so we retry briefly.
    let _ = remove_file_best_effort(&exe_stem);
    let _ = remove_file_best_effort(&rust_file_path);

    if !output.status.success() {
        // Preserve the program's exit code (or fall back to 1).
        std::process::exit(output.status.code().unwrap_or(1));
    }
    Ok(())
}

/// Try to delete `path`, retrying a few times with a short sleep.
///
/// Windows often holds a brief lock on a `.exe` image after the owning
/// process exits; a single `remove_file` call can fail with `PermissionDenied`
/// even though the process is gone. We retry up to 5 times with 20 ms gaps
/// (total wait ≤ 100 ms). Missing-file errors are treated as success.
fn remove_file_best_effort(path: &Path) -> std::io::Result<()> {
    let mut last_err: Option<std::io::Error> = None;
    for _ in 0..5 {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("remove_file_best_effort exhausted")))
}
