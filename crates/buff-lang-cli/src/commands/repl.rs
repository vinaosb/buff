//! `buff repl` — launch an interactive read-eval-print loop.
//!
//! Thin shim around [`buff_repl::Repl::run`]. The real logic lives in
//! `crates/buff-repl/` (T125a): rustyline for line editing, buff-eval
//! (T125-prep) for evaluation, pure `evaluate_and_format` for the
//! formatting layer that integration tests can drive without a TTY.
//!
//! State (let-bindings, func declarations) accumulates in-memory for the
//! duration of the session. Persistence across sessions is T125c
//! territory (no history file is read or written here).
//!
//! # Errors
//!
//! Returns [`anyhow::Error`] iff rustyline initialization fails (rare —
//! usually a missing TTY). All evaluation errors surface through the
//! REPL loop as diagnostics and never escape this function.

use anyhow::Result;

use buff_repl::Repl;

/// Entry point for `buff repl`.
///
/// Constructs a fresh [`Repl`] and runs its loop. The loop returns when
/// the user presses Ctrl-D or Ctrl-C, or when rustyline encounters an
/// unrecoverable I/O error.
pub fn run() -> Result<()> {
    let mut repl = Repl::new().map_err(|e| anyhow::Error::msg(e.to_string()))?;
    repl.run().map_err(|e| anyhow::Error::msg(e.to_string()))
}
