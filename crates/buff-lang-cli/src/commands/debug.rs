//! `buff debug <FILE> [--backend <NAME>] [--source-map <PATH>]` —
//! Debug Adapter Protocol translation proxy (T136).
//!
//!

#![allow(clippy::items_after_test_module)]
//! Thin shim around the `buff-dap` crate. All real protocol /
//! translation / backend-spawn logic lives in
//! `crates/buff-dap/src/`.
//!
//! # Pipeline
//!
//! 1. Detect / validate the backend debugger (lldb-dap preferred,
//!    codelldb / vscode-lldb fallbacks). When `--backend` is set,
//!    use that one explicitly; otherwise auto-detect. When none is
//!    installed, print an install hint + return an error (USER
//!    ACTION).
//! 2. Re-run the front-end pipeline ([`pipeline::compile_to_rust`])
//!    on the `.buff` file to write the generated `.rs` alongside the
//!    source. The `.rs` file is what the backend debugs.
//! 3. Build a [`SourceMap`] populated with the **identity mapping**
//!    (rust_line == buff_line) as the v1.10 stopgap. The real
//!    source-map wiring requires codegen changes — deferred to
//!    post-v1.10 (see `task-136-debugger.txt` GAP-1, same gap as
//!    T137 coverage).
//! 4. Hand off to [`buff_dap::run_session`] which spawns the
//!    backend, pumps DAP traffic both directions, and applies the
//!    source-map translation at `setBreakpoints` + `stackTrace`
//!    boundaries.
//!
//! # Errors
//!
//! All fallible operations return [`anyhow::Result`] with rich,
//! user-facing context. No `unwrap` / `expect` / `panic!`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use buff_dap::{print_missing_backend_hint, Backend, ServerConfig};
use buff_lang_error::{SourceId, SourceMap};

use crate::commands::coverage::populate_identity_mapping;
use crate::pipeline::compile_to_rust;

/// Entry point for `buff debug [...]`.
///
/// See the module docs for the pipeline. `file` selects the `.buff`
/// source to debug; `backend` overrides auto-detection when set.
pub fn run(file: &Path, backend: Option<&str>, _source_map: Option<&Path>) -> Result<()> {
    // 1. Resolve the backend.
    let backend = resolve_backend(backend)?;

    // 2. Re-run the front-end so the .rs sits next to the .buff.
    //    compile_to_rust already does lex + parse + codegen + write.
    let compile_out = compile_to_rust(file)?;

    // 3. Read the buff source (we need it for line-start computation
    //    in the translation layer).
    let buff_source = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read buff source `{}` for source-map construction",
            file.display()
        )
    })?;

    // 4. Build the (GAP-1: identity) SourceMap. Reuses the T137
    //    coverage helper (same v1.10 stopgap pattern — codegen does
    //    not yet emit real source-map markers).
    let source_id = SourceId(0);
    let mut source_map = SourceMap::new();
    source_map.add_source(source_id, file.to_path_buf(), buff_source.clone());
    populate_identity_mapping(&mut source_map, source_id, &buff_source);

    // 5. Build the server config.
    let config = ServerConfig {
        backend,
        buff_file: file.to_path_buf(),
        rust_file: compile_out.rust_file_path.clone(),
        buff_source,
        source_map,
        buff_source_id: source_id,
    };

    // 6. Hand off to the DAP server. Blocks until editor disconnect
    //    or backend exits.
    buff_dap::run_session(&config).map_err(|e| anyhow::Error::msg(e.to_string()))?;

    Ok(())
}

/// Resolve the backend debugger to use.
///
/// When `name` is set, validates it's a known backend + checks
/// installation. When `None`, auto-detects the best installed
/// backend. Prints the install hint + bails when none is found.
fn resolve_backend(name: Option<&str>) -> Result<Backend> {
    if let Some(name) = name {
        let backend = Backend::from_name(name).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown backend `{name}` — expected one of: lldb-dap, codelldb, vscode-lldb"
            )
        })?;
        if buff_dap::detect_specific(backend).is_none() {
            print_missing_backend_hint();
            bail!(
                "backend `{backend}` is not installed. See \
                 .sisyphus/evidence/task-136-debugger-USER-ACTION.txt for the install recipe."
            );
        }
        Ok(backend)
    } else {
        match buff_dap::detect_backend() {
            Some(b) => Ok(b),
            None => {
                print_missing_backend_hint();
                bail!(
                    "no DAP backend debugger found on PATH. See \
                     .sisyphus/evidence/task-136-debugger-USER-ACTION.txt for the install recipe."
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_backend_rejects_unknown_name() {
        let err = run(Path::new("examples/ola.buff"), Some("gdb"), None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("unknown backend `gdb`"),
            "expected unknown-backend error, got: {msg}"
        );
    }

    #[test]
    fn resolve_backend_returns_error_for_missing_file() {
        // Even with a backend installed, we should fail fast when
        // the file doesn't exist (compile_to_rust surfaces the read
        // error first).
        if buff_dap::detect_backend().is_none() {
            eprintln!("skipping: no DAP backend installed on this host");
            return;
        }
        let err = run(Path::new("__nonexistent_buff_file__.buff"), None, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("failed to read") || msg.contains("source file"),
            "expected read error, got: {msg}"
        );
    }

    #[test]
    fn resolve_backend_auto_detects_when_no_backend_installed() {
        if buff_dap::detect_backend().is_some() {
            eprintln!("skipping: a DAP backend is installed on this host");
            return;
        }
        let err = run(Path::new("examples/ola.buff"), None, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no DAP backend debugger found") || msg.contains("backend"),
            "expected missing-backend error, got: {msg}"
        );
    }

    #[test]
    fn run_accepts_source_map_arg_without_panicking() {
        // Just verify the signature accepts the source-map arg; we
        // don't actually exercise the explicit-source-map path here
        // (deferred to a follow-up — the arg is accepted but ignored
        // in v1.10).
        if buff_dap::detect_backend().is_none() {
            eprintln!("skipping: no DAP backend installed");
            return;
        }
        // This will fail at backend spawn / compile, NOT at arg parse.
        let _ = run(
            Path::new("examples/ola.buff"),
            None,
            Some(Path::new("does-not-exist.json")),
        );
    }

    #[test]
    fn resolve_backend_helper_recognizes_known_names() {
        // Direct test of the from_name surface (does NOT probe PATH).
        assert_eq!(Backend::from_name("lldb-dap"), Some(Backend::LldbDap));
        assert_eq!(Backend::from_name("codelldb"), Some(Backend::Codelldb));
        assert_eq!(Backend::from_name("vscode-lldb"), Some(Backend::VscodeLldb));
        assert_eq!(Backend::from_name("unknown"), None);
    }
}

// Suppress unused-import warning for PathBuf when only Path is used
// at runtime (kept for forward-compat when --source-map is wired).
#[allow(dead_code)]
fn _ensure_pathbuf_in_scope() -> PathBuf {
    PathBuf::new()
}
