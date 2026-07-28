//! `buff check` — type-checker + naming-convention linter command (T55).
//!
//! Thin wrapper around [`crate::check::run_check_file`] that translates the
//! returned [`CheckOutcome`] into a library-level result the CLI binary
//! ([`main.rs`](../../main.rs)) maps to an exit code.
//!
//! Like [`crate::commands::fmt`], the library `run` does NOT call
//! [`std::process::exit`]; that would abort the test harness. The CLI
//! binary inspects the returned outcome and exits accordingly:
//!
//! | Outcome                              | Default exit | With `-D` |
//! |--------------------------------------|--------------|-----------|
//! | [`CheckOutcome::Clean`]              | 0            | 0         |
//! | [`CheckOutcome::HasWarnings`]        | 0            | 1         |
//! | [`CheckOutcome::HasErrors`]          | 1            | 1         |
//!
//! `--deny-warnings` / `-D` promotes lint warnings to exit-non-zero (mirrors
//! `rustc -D warnings` / `cargo clippy -- -D warnings`). Type errors always
//! fail the exit code regardless of the flag.
//!
//! **T1 (v1.25 Wave 0):** `--error-format <human|json>` selects the output
//! format. `json` emits a single JSON array on stdout (see
//! [`crate::check::ErrorFormat`] for the shape).

use std::path::Path;

use anyhow::Result;

use crate::check::{run_check_file_with_format, CheckOutcome, ErrorFormat};
use buff_lang_ast::Decl;

/// Library entry point for `buff check <FILE> [--deny-warnings/-D]
/// [--error-format <human|json>] [--target <TRIPLE>] [--no-color]
/// [--dump-ast]`.
///
/// Returns the outcome directly (no process::exit) so tests can inspect it
/// and the CLI binary can translate it to an exit code.
///
/// T112: `--target <TRIPLE>` is accepted for CLI compatibility but is a
/// no-op in check mode — `buff check` runs the standalone typechecker
/// (T55) which does NOT invoke rustc, so there is no cross-compilation
/// to perform. The flag is parsed and validated but has no effect on the
/// check outcome.
///
/// T43: `--no-color` disables ANSI color in human-readable output.
///
/// P0.1.2b: `--dump-ast` serializes the parsed AST to deterministic JSON
/// (BTreeMap-ordered, compact). The output has the shape:
/// `{"declarations": [{"kind": "FuncDecl", "name": "main", ...}], "count": N}`.
/// Spans are emitted as `{"start": N, "end": N}` (raw byte offsets).
///
/// # Errors
///
/// Propagates file-read errors. Compile diagnostics are NOT errors at this
/// layer — they are returned as part of the [`CheckReport`] inside the
/// outcome.
pub fn run(
    file: &Path,
    deny_warnings: bool,
    format: ErrorFormat,
    _target: Option<&str>,
    no_color: bool,
    dump_ast: bool,
) -> Result<CheckOutcome> {
    let report = run_check_file_with_format(file, format, no_color)?;
    if dump_ast {
        dump_ast_json(file)?;
    }
    let outcome = if deny_warnings && matches!(report.outcome, CheckOutcome::HasWarnings) {
        CheckOutcome::HasErrors
    } else {
        report.outcome
    };
    Ok(outcome)
}

/// P0.1.2b: Parse the file and emit a deterministic JSON representation of
/// the AST. Uses `serde_json` with BTreeMap-ordered keys for determinism.
fn dump_ast_json(file: &Path) -> Result<()> {
    use buff_lang_error::SourceId;
    use buff_lang_lexer::tokenize;
    use buff_lang_parser::parse;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;

    let src = std::fs::read_to_string(file)?;
    let source_id = SourceId(0);
    let tokens = match tokenize(&src, source_id) {
        Ok(t) => t,
        Err(_) => {
            println!(r#"{{"error": "lexing failed"}}"#);
            return Ok(());
        }
    };
    let decls = match parse(&tokens, source_id) {
        Ok(d) => d,
        Err(_) => {
            println!(r#"{{"error": "parsing failed"}}"#);
            return Ok(());
        }
    };

    let declarations: Vec<Value> = decls
        .iter()
        .map(|d| {
            let mut entry = BTreeMap::new();
            let (kind, name) = decl_kind_and_name(d);
            entry.insert("kind".to_string(), json!(kind));
            entry.insert("name".to_string(), json!(name));
            entry.insert("debug".to_string(), json!(format!("{d:?}")));
            Value::Object(entry.into_iter().collect())
        })
        .collect();

    let mut output = BTreeMap::new();
    output.insert("declarations".to_string(), Value::Array(declarations));
    output.insert("count".to_string(), json!(decls.len()));
    println!(
        "{}",
        serde_json::to_string(&Value::Object(output.into_iter().collect()))?
    );
    Ok(())
}

/// Extract the kind string and name from a [`Decl`] for JSON serialization.
fn decl_kind_and_name(d: &Decl) -> (&'static str, String) {
    match d {
        Decl::FuncDecl(f) => ("FuncDecl", f.name.name.clone()),
        Decl::StructDecl(s) => ("StructDecl", s.name.name.clone()),
        Decl::EnumDecl(e) => ("EnumDecl", e.name.name.clone()),
        Decl::ImportDecl(_) => ("ImportDecl", "<import>".to_string()),
        Decl::ModuleDecl(m) => ("ModuleDecl", m.name.name.clone()),
        Decl::TraitDecl(t) => ("TraitDecl", t.name.name.clone()),
        Decl::ExportDecl(e) => {
            let (inner_kind, inner_name) = decl_kind_and_name(&e.inner);
            ("ExportDecl", format!("{inner_kind}:{inner_name}"))
        }
        Decl::ReexportDecl(_) => ("ReexportDecl", "<reexport>".to_string()),
        Decl::ExternCrateDecl(_) => ("ExternCrateDecl", "<extern_crate>".to_string()),
        Decl::ExternFuncDecl(f) => ("ExternFuncDecl", f.name.name.clone()),
        Decl::ExtendBlock(_) => ("ExtendBlock", "<extend>".to_string()),
        Decl::ImplBlock(_) => ("ImplBlock", "<impl>".to_string()),
    }
}
