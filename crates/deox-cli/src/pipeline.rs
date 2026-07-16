//! Shared compiler pipeline used by both `deox build` and `deox run`.
//!
//! The pipeline is split into two phases so callers can decide what to do
//! with the intermediate Rust source:
//!
//! - [`compile_to_rust`] — read a `.deox` file, lex, parse, and codegen into a
//!   Rust source string, writing it to `<file>.rs` alongside the input.
//! - [`compile_rust_to_exe`] — invoke `rustc --edition 2021` on a `.rs` file to
//!   produce a native executable.
//!
//! All fallible operations return [`anyhow::Result`] with rich, user-facing
//! context. No panics, no `unwrap`/`expect`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use deox_codegen_rust::generate_rust;
use deox_error::{SourceFile, SourceId};
use deox_lexer::tokenize;
use deox_parser::parse;

/// Output of the [`compile_to_rust`] phase: the generated Rust source plus the
/// path it was written to.
#[derive(Debug, Clone)]
pub struct CompileOutput {
    /// The generated Rust source code (already formatted via `prettyplease`).
    pub rust_source: String,
    /// Path of the `.rs` file that was written (alongside the input `.deox`).
    pub rust_file_path: PathBuf,
}

/// Run the front-end of the compiler: read → lex → parse → codegen → write.
///
/// Writes the generated Rust source to `file.with_extension("rs")` (i.e. the
/// `.rs` file sits next to the `.deox` source). The type-checking pass is
/// already integrated inside codegen (T12) and is non-fatal in v0.1.
///
/// # Errors
///
/// Returns an error if the file cannot be read, lexing fails, parsing fails,
/// codegen fails, or the `.rs` file cannot be written. Every error message
/// includes the source filename and (where possible) the line/column of the
/// offending span via [`SourceFile::lookup`].
pub fn compile_to_rust(file: &Path) -> Result<CompileOutput> {
    // 1. Read source.
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read source file `{}`", file.display()))?;

    // Build a SourceFile so we can map byte offsets to 1-based line/col for
    // diagnostic messages. SourceId(0) is fine — we only lex a single file.
    let source_id = SourceId(0);
    let source_file = SourceFile::new(file.to_path_buf(), source.clone());

    // 2. Lex. LexerError wraps the deox_error::LexError via `inner`.
    let tokens = tokenize(&source, source_id)
        .map_err(|e| format_diagnostic_error("lex", &e.inner.diagnostic, &source_file, file))?;

    // 3. Parse.
    let decls = parse(&tokens, source_id)
        .map_err(|e| format_diagnostic_error("parse", &e.diagnostic, &source_file, file))?;

    // 4. Codegen (type inference is integrated inside RustCodegen).
    let rust_source = generate_rust(&decls)
        .map_err(|e| format_diagnostic_error("codegen", &e.diagnostic, &source_file, file))?;

    // 5. Write the .rs file alongside the .deox source.
    let rust_file_path = file.with_extension("rs");
    std::fs::write(&rust_file_path, &rust_source)
        .with_context(|| format!("failed to write `{}`", rust_file_path.display()))?;

    Ok(CompileOutput {
        rust_source,
        rust_file_path,
    })
}

/// Compile a generated Rust source file into a native executable via `rustc`.
///
/// Invokes `rustc --edition 2021 -O <rust_file> -o <output>`. The `output`
/// path is passed verbatim to rustc — callers should pre-append the platform
/// executable extension (see [`with_exe_extension`]) if they want a
/// conventional name (e.g. `ola.exe` on Windows).
///
/// `deox_file` is the original `.deox` source path. When `rustc` emits
/// diagnostics referencing the intermediate `.rs` file, they are translated to
/// reference the `.deox` file instead via
/// [`error_mapper::translate_rustc_errors`].
///
/// # Errors
///
/// - Fails if `rustc` cannot be invoked (not installed / not in `PATH`).
/// - Fails if `rustc` exits with a non-zero status. Translated `rustc`
///   diagnostics are forwarded to the caller's stderr before bailing.
pub fn compile_rust_to_exe(rust_file: &Path, output: &Path, deox_file: &Path) -> Result<PathBuf> {
    let result = Command::new("rustc")
        .arg("--edition")
        .arg("2021")
        .arg("-O")
        .arg(rust_file)
        .arg("-o")
        .arg(output)
        // Capture output so we can translate rustc's file references.
        .output()
        .context("failed to invoke `rustc` — is it installed and on your PATH?")?;

    // Forward rustc's stderr (diagnostics / warnings), translating `.rs`
    // references to `.deox` so the user sees their original source location.
    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let translated = crate::error_mapper::translate_rustc_errors(&stderr, deox_file, rust_file);
        eprint!("{translated}");
    }

    if !result.status.success() {
        bail!("rustc exited with status {}", result.status);
    }

    // rustc writes exactly the path passed to `-o` (it does NOT append a
    // platform extension on its own on this toolchain), so the on-disk
    // artifact is precisely `output`.
    Ok(output.to_path_buf())
}

/// Return `path` with the platform's executable extension applied.
///
/// - On Unix, `EXE_EXTENSION` is `""`, so the path is returned unchanged.
/// - On Windows, `EXE_EXTENSION` is `"exe"`: if `path` already ends with
///   `.exe` it is returned as-is, otherwise `.exe` is appended.
///
/// Callers should pass the result of this helper as the `-o` argument to
/// [`compile_rust_to_exe`] so that the produced executable has a conventional
/// name on the host platform.
pub fn with_exe_extension(path: &Path) -> PathBuf {
    let ext = std::env::consts::EXE_EXTENSION;
    if ext.is_empty() {
        return path.to_path_buf();
    }
    match path.extension() {
        Some(e) if e == ext => path.to_path_buf(),
        _ => {
            let mut p = path.to_path_buf();
            p.set_extension(ext);
            p
        }
    }
}

/// Format a phase-specific error (lex / parse / codegen) as a user-facing
/// anyhow error with line/column context when available.
fn format_diagnostic_error(
    phase: &str,
    diagnostic: &deox_error::Diagnostic,
    source_file: &SourceFile,
    file: &Path,
) -> anyhow::Error {
    let (line_col_note, source_line) = match source_file.lookup(diagnostic.span.start) {
        Some((line, col)) => (
            format!(" at {}:{}:{}\n  --> {}", file.display(), line, col, line),
            extract_source_line(source_file, line),
        ),
        None => (format!(" in {}", file.display()), String::new()),
    };

    let mut msg = format!("{phase} error: {}{line_col_note}", diagnostic.message);
    if !source_line.is_empty() {
        msg.push_str(&format!("\n      | {source_line}"));
    }
    for note in &diagnostic.notes {
        msg.push_str(&format!("\n  note: {note}"));
    }
    anyhow::anyhow!(msg)
}

/// Extract the 1-indexed `line_no` line of `source_file.content` (without the
/// trailing newline). Returns an empty string if the line number is out of
/// range.
fn extract_source_line(source_file: &SourceFile, line_no: usize) -> String {
    source_file
        .content
        .lines()
        .nth(line_no.saturating_sub(1))
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn with_exe_extension_unix_passthrough() {
        // On Unix, EXE_EXTENSION is "" so we always pass through.
        if std::env::consts::EXE_EXTENSION.is_empty() {
            assert_eq!(
                with_exe_extension(&PathBuf::from("/tmp/ola")),
                PathBuf::from("/tmp/ola")
            );
        }
    }

    #[test]
    fn with_exe_extension_appends_when_missing() {
        let p = with_exe_extension(&PathBuf::from("ola"));
        let ext = std::env::consts::EXE_EXTENSION;
        if ext.is_empty() {
            assert_eq!(p, PathBuf::from("ola"));
        } else {
            let mut expected = PathBuf::from("ola");
            expected.set_extension(ext);
            assert_eq!(p, expected);
        }
    }

    #[test]
    fn with_exe_extension_idempotent_when_already_present() {
        let ext = std::env::consts::EXE_EXTENSION;
        if ext.is_empty() {
            return;
        }
        let mut input = PathBuf::from("ola");
        input.set_extension(ext);
        let p = with_exe_extension(&input);
        assert_eq!(p, input, "should not double-append the extension");
    }

    #[test]
    fn extract_source_line_handles_utf8_and_multibyte() {
        let sf = SourceFile::new(
            PathBuf::from("test"),
            "first\nOlá, Deox!\nthird".to_string(),
        );
        assert_eq!(extract_source_line(&sf, 1), "first");
        assert_eq!(extract_source_line(&sf, 2), "Olá, Deox!");
        assert_eq!(extract_source_line(&sf, 3), "third");
        assert_eq!(extract_source_line(&sf, 99), "");
    }
}
