//! `buff-repl` — interactive read-eval-print loop for Buff.
//!
//! Wraps [`buff_eval::Evaluator`] with a [`rustyline`] line editor to
//! provide a `buff repl` shell: type a Buff expression or statement,
//! press Enter, see the result. State (let-bindings, func declarations)
//! accumulates in the evaluator across lines.
//!
//! # Layering
//!
//! ```text
//!                ┌──────────────────────────┐
//!   keystrokes   │ rustyline::DefaultEditor │  line editing, history,
//!             ─▶ │   (this crate)           │  Ctrl-D / Ctrl-C handling
//!                └────────────┬─────────────┘
//!                             │  &str
//!                             ▼
//!                ┌──────────────────────────┐
//!                │ buff_eval::Evaluator     │  lex + parse + codegen +
//!                │   (T125-prep)            │  rustc + spawn-and-capture
//!                └────────────┬─────────────┘
//!                             │  EvalResult
//!                             ▼
//!                ┌──────────────────────────┐
//!                │ evaluate_and_format      │  pure formatting fn (this
//!                │   (this crate)           │  crate) → display string
//!                └──────────────────────────┘
//! ```
//!
//! The REPL adds NO new compilation logic — it consumes `buff-eval`
//! exclusively.
//!
//! # Resilience
//!
//! A diagnostic from any pipeline phase (lex / parse / codegen / rustc /
//! spawn) is rendered to the terminal and the loop CONTINUES — the user
//! sees the error and gets a fresh prompt. Only `ReadlineError::Eof`
//! (Ctrl-D), `ReadlineError::Interrupted` (Ctrl-C), or an unrecoverable
//! rustyline I/O error break the loop.
//!
//! # Testing
//!
//! [`rustyline`] requires a TTY, so the interactive loop itself is not
//! unit-tested. The pure formatting helper [`evaluate_and_format`] is
//! the testable surface: it takes a mutable `Evaluator` reference and an
//! input string and returns the exact string the REPL would print. The
//! integration tests in `tests/repl_tests.rs` exercise this fn
//! end-to-end (including a rustc spawn for the happy path).
//!
//! # No panics
//!
//! There are no `unwrap` / `expect` / `panic!` / `unimplemented!` /
//! `todo!` calls outside `#[cfg(test)]`.

use std::io::{self, Write};

use buff_eval::{EvalResult, Evaluator};
use buff_lang_error::Diagnostic;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

/// The default prompt string shown at the start of each REPL line.
pub const DEFAULT_PROMPT: &str = "buff> ";

/// The default continuation message emitted when the REPL session ends
/// (Ctrl-D / Ctrl-C).
const FAREWELL: &str = "bye.";

/// A Buff REPL session.
///
/// Owns the [`DefaultEditor`] (rustyline line editor with in-memory
/// history) and the [`Evaluator`] (accumulating `let` / `func` state).
/// Construct with [`Repl::new`] and launch with [`Repl::run`].
///
/// The REPL is intentionally NOT `Clone` — the evaluator owns mutable
/// session state that callers usually want to keep singular. Tests that
/// need to drive the formatting layer without a TTY should call the
/// free function [`evaluate_and_format`] directly.
#[derive(Debug)]
pub struct Repl {
    editor: DefaultEditor,
    evaluator: Evaluator,
    prompt: String,
}

impl Repl {
    /// Construct a fresh REPL with an empty evaluator and the default
    /// [`DEFAULT_PROMPT`].
    ///
    /// Errors from rustyline initialization (rare — usually a missing
    /// TTY) are surfaced immediately rather than being deferred to the
    /// first `readline` call.
    ///
    /// # Errors
    ///
    /// Returns [`rustyline::error::ReadlineError`] iff the underlying
    /// [`DefaultEditor::new`] fails.
    pub fn new() -> Result<Self, ReadlineError> {
        Self::with_prompt(DEFAULT_PROMPT)
    }

    /// Construct a fresh REPL with a custom prompt string.
    ///
    /// # Errors
    ///
    /// See [`Repl::new`].
    pub fn with_prompt(prompt: &str) -> Result<Self, ReadlineError> {
        Ok(Self {
            editor: DefaultEditor::new()?,
            evaluator: Evaluator::new(),
            prompt: prompt.to_string(),
        })
    }

    /// Replace the inner evaluator with a fresh one. Useful for tests
    /// that want to inspect evaluator state after construction. Not
    /// used by the production `buff repl` flow.
    #[must_use]
    pub fn with_evaluator(mut self, evaluator: Evaluator) -> Self {
        self.evaluator = evaluator;
        self
    }

    /// Borrow the inner evaluator. Useful for assertions in tests.
    pub fn evaluator(&self) -> &Evaluator {
        &self.evaluator
    }

    /// Run the REPL loop until the user exits via Ctrl-D / Ctrl-C or a
    /// rustyline I/O error occurs.
    ///
    /// Each iteration:
    /// 1. Read a line via rustyline.
    /// 2. Pass it to [`Evaluator::eval_line`].
    /// 3. Format the result via [`evaluate_and_format`] and print it.
    /// 4. Add the line to the rustyline in-memory history.
    ///
    /// A diagnostic from the evaluator NEVER terminates the loop — it
    /// is printed and the prompt returns. Only `ReadlineError::Eof`
    /// (Ctrl-D), `ReadlineError::Interrupted` (Ctrl-C), or an
    /// unrecoverable rustyline I/O error break out.
    ///
    /// # Errors
    ///
    /// Returns the first non-Eof / non-Interrupted `ReadlineError` to
    /// escape rustyline, or an `io::Error` from stdout flushing.
    pub fn run(&mut self) -> Result<(), ReadlineError> {
        self.run_with_writer(io::stdout())
    }

    /// Same as [`Repl::run`] but writes output to a caller-supplied
    /// writer instead of stdout. Used by integration paths that want
    /// to capture REPL output.
    ///
    /// # Errors
    ///
    /// See [`Repl::run`].
    pub fn run_with_writer<W: Write>(&mut self, mut out: W) -> Result<(), ReadlineError> {
        loop {
            let line = match self.editor.readline(&self.prompt) {
                Ok(line) => line,
                Err(ReadlineError::Interrupted) => {
                    // Ctrl-C: print a newline so the prompt row is fresh,
                    // then break cleanly.
                    let _ = writeln!(out);
                    let _ = writeln!(out, "{FAREWELL}");
                    let _ = out.flush();
                    break;
                }
                Err(ReadlineError::Eof) => {
                    // Ctrl-D: break cleanly, no extra newline.
                    let _ = writeln!(out, "{FAREWELL}");
                    let _ = out.flush();
                    break;
                }
                Err(e) => {
                    let _ = out.flush();
                    return Err(e);
                }
            };
            // Best-effort history append. A failure here (rare) does not
            // break the loop — the user just loses this entry from
            // up-arrow recall.
            let _ = self.editor.add_history_entry(line.as_str());

            let formatted = evaluate_and_format(&mut self.evaluator, &line);
            // Write the formatted output. We never panic on a stdout
            // write failure — just continue to the next prompt.
            let _ = write!(out, "{formatted}");
            let _ = out.flush();
        }
        Ok(())
    }
}

/// Evaluate `input` against `evaluator` and return the exact string the
/// REPL would print for that result.
///
/// This is a PURE function — it touches no global state, no TTY, no
/// rustyline editor. It exists so the formatting layer can be tested
/// without spawning a terminal.
///
/// Formatting rules:
///
/// - If `result.diagnostic` is `Some`, render the diagnostic via its
///   `Display` impl (which yields `[Error] <message>` + `note: ...`
///   lines). The diagnostic is the LAST thing printed — partial stdout
///   / stderr from a runtime panic precedes it for debuggability.
/// - Else (clean run):
///   - Forward `result.stdout` verbatim (Buff's `print` output lands
///     here).
///   - Forward `result.stderr` verbatim when non-empty (rare for clean
///     runs, but Rust panic messages can leak through if the spawned
///     program wrote to stderr before a non-zero exit — those are
///     surfaced as diagnostics above, but we still keep the raw
///     output for transparency).
///   - If `result.value` is `Some(v)` AND `v` differs from the trimmed
///     stdout, append `= <v>` on its own line so bare expressions are
///     visually distinct (mirrors `rustc --explain` / `python -i`).
/// - Empty input → empty output (no-op).
///
/// # Errors
///
/// None — all failure paths from the evaluator are surfaced through
/// the returned string. This function never panics.
pub fn evaluate_and_format(ev: &mut Evaluator, input: &str) -> String {
    let result = ev.eval_line(input);
    format_eval_result(&result)
}

/// Format an [`EvalResult`] the way the REPL prints it.
///
/// Exposed separately from [`evaluate_and_format`] so tests can build
/// an `EvalResult` directly (e.g. to verify how a specific diagnostic
/// renders) without invoking the evaluator pipeline.
#[must_use]
pub fn format_eval_result(result: &EvalResult) -> String {
    let mut out = String::new();

    // Forward captured stdout verbatim (Buff's `print` output).
    if !result.stdout.is_empty() {
        out.push_str(&result.stdout);
        // Ensure stdout ends with a newline so the next section starts
        // on its own line. Buff's `print` always appends '\n', but the
        // runtime might emit raw bytes via FFI; guard either way.
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }

    // Forward captured stderr verbatim when non-empty. Usually empty on
    // clean runs; populated on runtime panics (which also produce a
    // diagnostic — surfaced below).
    if !result.stderr.is_empty() {
        out.push_str(&result.stderr);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }

    match &result.diagnostic {
        Some(d) => out.push_str(&render_diagnostic_for_repl(d)),
        None => {
            // Clean run. If there's an evaluated value, surface it.
            if let Some(value) = &result.value {
                // The wrapped `print(<expr>)` lowering already wrote the
                // value to stdout (which we forwarded above). Avoid
                // duplicating it as a separate `= <value>` line when the
                // stdout exactly matches — the user already saw it.
                //
                // Divergence happens when:
                // - the runtime padded the value (rare), OR
                // - stdout was empty (shouldn't happen for BareExpr but
                //   guard against future changes to the lowering).
                if value.trim() != result.stdout.trim() || result.stdout.trim().is_empty() {
                    out.push_str("= ");
                    out.push_str(value);
                    out.push('\n');
                }
            }
        }
    }

    out
}

/// Render a [`Diagnostic`] for REPL display.
///
/// Uses the diagnostic's `Display` impl rather than `Diagnostic::render`
/// because the REPL has no source-text context (the user typed the line
/// interactively, and the diagnostic's span is likely a dummy span for
/// runtime / rustc errors). `Display` yields the canonical
/// `[Severity] message` form plus any notes, which is the right level
/// of detail for a one-line REPL exchange.
fn render_diagnostic_for_repl(d: &Diagnostic) -> String {
    let mut out = String::new();
    out.push_str(&d.to_string());
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// Smoke tests — the deep scenarios live in `tests/repl_tests.rs`. These
// unit tests exercise the formatting layer with hand-built EvalResults
// (no rustc spawn) so they're fast and hermetic.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::{Diagnostic, Span};

    fn diag(msg: &str) -> Diagnostic {
        Diagnostic::error(msg, Span::dummy())
    }

    #[test]
    fn format_clean_result_with_value_forwards_stdout_only() {
        // Mirrors what eval_line("2 + 3") produces: value=Some("5"),
        // stdout="5\n", no diagnostic.
        let r = EvalResult {
            value: Some(String::from("5")),
            stdout: String::from("5\n"),
            stderr: String::new(),
            diagnostic: None,
            exit_code: Some(0),
        };
        let out = format_eval_result(&r);
        // Value is identical to trimmed stdout, so no `= 5` duplication.
        assert_eq!(out, "5\n");
    }

    #[test]
    fn format_clean_result_with_diverging_value_shows_equals_line() {
        let r = EvalResult {
            value: Some(String::from("42")),
            stdout: String::from("the answer is 42\n"),
            stderr: String::new(),
            diagnostic: None,
            exit_code: Some(0),
        };
        let out = format_eval_result(&r);
        assert!(
            out.contains("= 42\n"),
            "expected `= 42` line in output, got: {out:?}"
        );
    }

    #[test]
    fn format_result_with_diagnostic_renders_diagnostic() {
        let r = EvalResult {
            value: None,
            stdout: String::new(),
            stderr: String::new(),
            diagnostic: Some(diag("parse error: unexpected EOF")),
            exit_code: None,
        };
        let out = format_eval_result(&r);
        assert!(
            out.contains("[Error] parse error: unexpected EOF"),
            "expected diagnostic rendering in output, got: {out:?}"
        );
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn format_result_with_partial_stdout_then_diagnostic_keeps_both() {
        // Mirrors a runtime panic: program prints "about to panic\n",
        // then panics → diagnostic. Both should be visible.
        let r = EvalResult {
            value: None,
            stdout: String::from("about to panic\n"),
            stderr: String::from("thread 'main' panicked at ...\n"),
            diagnostic: Some(diag("eval: program exited with code 101")),
            exit_code: Some(101),
        };
        let out = format_eval_result(&r);
        assert!(
            out.contains("about to panic"),
            "missing partial stdout: {out:?}"
        );
        assert!(
            out.contains("thread 'main' panicked"),
            "missing partial stderr: {out:?}"
        );
        assert!(
            out.contains("[Error] eval: program exited with code 101"),
            "missing diagnostic: {out:?}"
        );
    }

    #[test]
    fn render_diagnostic_for_repl_ends_with_newline() {
        let d = diag("boom");
        let s = render_diagnostic_for_repl(&d);
        assert!(s.ends_with('\n'));
        assert!(s.contains("[Error] boom"));
    }

    #[test]
    fn evaluate_and_format_empty_input_yields_empty_string() {
        // Empty input → evaluator short-circuits with an empty EvalResult.
        // No stdout, no value, no diagnostic → empty formatted output.
        let mut ev = Evaluator::new();
        let out = evaluate_and_format(&mut ev, "");
        assert!(out.is_empty(), "expected empty output, got: {out:?}");
    }

    #[test]
    fn repl_with_prompt_builds_with_custom_prompt() {
        // We can't actually run() in a unit test (no TTY), but we can
        // verify the constructor works.
        let repl = Repl::with_prompt(">>> ").expect("DefaultEditor::new failed");
        assert_eq!(repl.prompt, ">>> ");
    }
}
