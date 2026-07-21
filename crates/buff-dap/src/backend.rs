//! Backend debugger detection + subprocess spawn.
//!
//! The Buff DAP server is a **translation proxy** — it does not
//! itself implement a debugger. Instead it launches a Rust-capable
//! DAP backend as a subprocess and forwards traffic both ways,
//! applying the source-map translation at the `setBreakpoints`
//! (buff → rust) and `stackTrace` (rust → buff) boundaries.
//!
//! # Backends (preference order)
//!
//! 1. **`lldb-dap`** (preferred) — ships with llvm. Available on
//!    most platforms via the `llvm` or `llvm-tools` package. The
//!    upstream project is <https://github.com/llvm/llvm-project/tree/main/lldb/tools/lldb-dap>.
//! 2. **`codelldb`** — a VSCode extension that bundles its own
//!    lldb. Available as a standalone binary when installed via
//!    `cargo install codelldb` or extracted from the `.vsix`.
//! 3. **`vscode-lldb`** — same family as codelldb (the extension
//!    is published as "CodeLLDB" on the marketplace).
//!
//! When none is on `PATH`, the DAP server prints an install hint
//! and exits non-zero (USER ACTION — see
//! `.sisyphus/evidence/task-136-debugger-USER-ACTION.txt`).

use std::process::{Command, Stdio};

use crate::error::{DapError, DapResult};

/// A Rust-capable DAP backend debugger.
///
/// Variants are in preference order (see module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// `lldb-dap` — ships with llvm. Preferred.
    LldbDap,
    /// `codelldb` — VSCode extension shipped as a standalone.
    Codelldb,
    /// `vscode-lldb` — CodeLLDB extension adapter.
    VscodeLldb,
}

impl Backend {
    /// The executable name to look for on `PATH`.
    pub fn executable(self) -> &'static str {
        match self {
            Backend::LldbDap => "lldb-dap",
            Backend::Codelldb => "codelldb",
            Backend::VscodeLldb => "vscode-lldb",
        }
    }

    /// User-facing display name (matches the marketplace / docs).
    pub fn display_name(self) -> &'static str {
        match self {
            Backend::LldbDap => "lldb-dap",
            Backend::Codelldb => "codelldb",
            Backend::VscodeLldb => "vscode-lldb",
        }
    }

    /// All known backends, in preference order.
    pub fn all() -> &'static [Backend] {
        &[Backend::LldbDap, Backend::Codelldb, Backend::VscodeLldb]
    }

    /// Parse the `--backend <name>` CLI flag into a [`Backend`].
    ///
    /// Returns `None` when `name` is not a recognized backend
    /// (the CLI surfaces a helpful error).
    pub fn from_name(name: &str) -> Option<Self> {
        name.parse().ok()
    }
}

impl std::str::FromStr for Backend {
    type Err = ();

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "lldb-dap" => Ok(Backend::LldbDap),
            "codelldb" => Ok(Backend::Codelldb),
            "vscode-lldb" => Ok(Backend::VscodeLldb),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Detect the best available backend debugger on `PATH`.
///
/// Probes each backend's executable with `--version` (stdio piped
/// to null — silent on failure). Returns the first one that exits
/// cleanly, or [`Option::None`] when none is installed.
pub fn detect_backend() -> Option<Backend> {
    Backend::all()
        .iter()
        .copied()
        .find(|&b| tool_available(b.executable()))
}

/// Detect a specific backend (used when `--backend <name>` is set).
pub fn detect_specific(backend: Backend) -> Option<Backend> {
    if tool_available(backend.executable()) {
        Some(backend)
    } else {
        None
    }
}

/// Run `<exe> --version` returning `true` on a clean exit.
fn tool_available(exe: &str) -> bool {
    Command::new(exe)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A handle to a spawned backend subprocess.
///
/// Owns the child's stdin / stdout (the DAP transport channels).
/// stderr is inherited so backend diagnostics surface to the user
/// (mirrors how `buff-registry` / `buff-jupyter` surface child logs).
pub struct BackendProcess {
    /// The [`Backend`] kind that was spawned.
    pub backend: Backend,
    /// stdin writer — DAP messages we send TO the backend.
    pub stdin: std::process::ChildStdin,
    /// stdout reader — DAP messages we read FROM the backend.
    pub stdout: std::process::ChildStdout,
    /// The underlying child handle (kept for `kill` on drop).
    pub child: std::process::Child,
}

/// Spawn the specified backend as a subprocess with stdio piped.
///
/// Returns a [`BackendProcess`] handle the caller uses to pump DAP
/// traffic. The caller is responsible for closing stdin + waiting
/// on the child when the session ends.
pub fn spawn(backend: Backend) -> DapResult<BackendProcess> {
    let mut cmd = Command::new(backend.executable());
    // CodeLLDB accepts a `--port` arg for multi-mode; for stdio
    // mode (which is what VSCode uses by default), no args needed.
    // lldb-dap is stdio-only.
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let mut child = cmd
        .spawn()
        .map_err(|e| DapError::Io(format!("failed to spawn {}: {e}", backend.display_name())))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| DapError::Io(format!("{} did not expose stdin", backend.display_name())))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| DapError::Io(format!("{} did not expose stdout", backend.display_name())))?;

    Ok(BackendProcess {
        backend,
        stdin,
        stdout,
        child,
    })
}

/// Print the install hint shown when no backend is detected.
pub fn print_missing_backend_hint() {
    eprintln!("No DAP backend debugger detected on PATH.");
    eprintln!();
    eprintln!("Install one of (in preference order):");
    eprintln!("  lldb-dap       (preferred — ships with llvm / llvm-tools)");
    eprintln!("  codelldb       (VSCode extension; standalone binary available)");
    eprintln!("  vscode-lldb    (CodeLLDB extension adapter)");
    eprintln!();
    eprintln!(
        "See .sisyphus/evidence/task-136-debugger-USER-ACTION.txt for the full install recipe."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_from_name_recognizes_known() {
        assert_eq!(Backend::from_name("lldb-dap"), Some(Backend::LldbDap));
        assert_eq!(Backend::from_name("codelldb"), Some(Backend::Codelldb));
        assert_eq!(Backend::from_name("vscode-lldb"), Some(Backend::VscodeLldb));
    }

    #[test]
    fn backend_from_name_rejects_unknown() {
        assert_eq!(Backend::from_name("gdb"), None);
        assert_eq!(Backend::from_name(""), None);
        assert_eq!(Backend::from_name("LLDB-DAP"), None); // case-sensitive
    }

    #[test]
    fn backend_all_in_preference_order() {
        let all = Backend::all();
        assert_eq!(all[0], Backend::LldbDap);
        assert_eq!(all[1], Backend::Codelldb);
        assert_eq!(all[2], Backend::VscodeLldb);
    }

    #[test]
    fn backend_display_matches_executable() {
        // Display name == executable name (for these three).
        for &b in Backend::all() {
            assert_eq!(b.display_name(), b.executable());
        }
    }

    #[test]
    fn detect_backend_runs_cleanly() {
        // Smoke: detection probes three executables; on a host with
        // none installed, returns None. With lldb-dap installed,
        // returns Some(LldbDap). Either outcome is acceptable here.
        let _ = detect_backend();
    }

    #[test]
    fn detect_specific_returns_none_for_uninstalled() {
        // Probing an obviously-fake backend name should return None.
        // (We rely on the tool_available helper failing.)
        let fake = Backend::LldbDap;
        // Skip if lldb-dap IS installed (CI / dev hosts).
        if tool_available(fake.executable()) {
            eprintln!("skipping: lldb-dap is installed");
            return;
        }
        assert_eq!(detect_specific(fake), None);
    }

    #[test]
    fn backend_to_string_matches_display_name() {
        assert_eq!(Backend::LldbDap.to_string(), "lldb-dap");
        assert_eq!(Backend::Codelldb.to_string(), "codelldb");
        assert_eq!(Backend::VscodeLldb.to_string(), "vscode-lldb");
    }
}
