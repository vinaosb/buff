//! `deox run` — compile a `.deox` file and execute it immediately.
//!
//! Pipeline: [`pipeline::compile_to_rust`] → [`pipeline::compile_rust_to_exe`]
//! → spawn the executable. Both the executable and the intermediate `.rs` file
//! are removed afterwards (they live in a temp dir / next to the source
//! respectively, so leaving them would pollute the user's workspace).
//!
//! Runtime panics from the compiled binary reference the intermediate `.rs`
//! file. These are intercepted (T16) and translated back to the original
//! `.deox` location via [`crate::error_mapper::translate_panic`] before being
//! forwarded to the user's stderr.

use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::pipeline;

/// Entry point for `deox run <FILE> [-- ARGS]...`.
///
/// - Compiles `file` to Rust + executable (the executable goes into
///   `std::env::temp_dir().join("deox-run")` so it never pollutes the
///   user's source tree).
/// - Executes the compiled program with `args`, inheriting stdio.
/// - Cleans up both the executable and the `.rs` file.
/// - If the program exits non-zero, exits the `deox` process with the same
///   code (or 1 if no code is available).
///
/// # Errors
///
/// Propagates pipeline errors. A non-zero program exit code is *not* an
/// `Err` from this function — instead the process exits directly so the exit
/// code is preserved.
pub fn run(file: &Path, args: &[String]) -> Result<()> {
    let compile_out = pipeline::compile_to_rust(file)?;

    // Build a deterministic temp location for the executable.
    let temp_dir = std::env::temp_dir().join("deox-run");
    std::fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create temp dir `{}`", temp_dir.display()))?;

    let stem = file
        .file_stem()
        .map(|s| s.to_owned())
        .unwrap_or_else(|| std::ffi::OsString::from("deox_program"));
    let exe_stem = pipeline::with_exe_extension(&temp_dir.join(stem));

    let exe_path = pipeline::compile_rust_to_exe(&compile_out.rust_file_path, &exe_stem, file)?;

    // Execute, capturing output so runtime panics can be translated (T16).
    let output = Command::new(&exe_path)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute `{}`", exe_path.display()))?;

    // Forward the program's stdout (its normal output).
    if !output.stdout.is_empty() {
        let _ = std::io::stdout().write_all(&output.stdout);
        let _ = std::io::stdout().flush();
    }

    // Translate and forward stderr (handles runtime panics that reference
    // the intermediate .rs file — replaces with the original .deox path).
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    if !stderr_str.is_empty() {
        // v0.1: source map is empty (exact line tracking deferred); filename
        // translation is the primary win. See error_mapper::translate_panic.
        let source_map = deox_error::SourceMap::new();
        let translated = crate::error_mapper::translate_panic(
            &stderr_str,
            &compile_out.rust_file_path,
            file,
            &source_map,
        );
        eprint!("{translated}");
    }

    // Cleanup — best-effort, never propagates errors. On Windows the just-exited
    // executable may still be image-locked by the OS, so we retry briefly.
    let _ = remove_file_best_effort(&exe_path);
    let _ = remove_file_best_effort(&compile_out.rust_file_path);

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
