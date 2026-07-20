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
//! # Meta-commands (T125b)
//!
//! Lines starting with `:type` are intercepted by the dispatcher
//! ([`dispatch_line`]) BEFORE reaching [`Evaluator::eval_line`]. The
//! `:type <expr>` form consults [`Evaluator::type_of`] — a pure lex +
//! parse + infer pass that does NOT spawn rustc — and prints the
//! inferred type. State accumulated by prior `let` / `func` lines is
//! consulted (read-only) but not mutated, so a `:type` query has no
//! side effects on subsequent evaluations.
//!
//! Everything else (including unknown `:foo` meta-commands, which are
//! T125c territory) flows through the normal [`evaluate_and_format`]
//! path. The interactive [`Repl::run`] loop calls the same dispatcher,
//! so behavior is identical between TTY and test harness.
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
//! unit-tested. The pure dispatcher [`dispatch_line`] is the testable
//! surface: it takes a mutable `Evaluator` reference and an input
//! string and returns the exact string the REPL would print. The
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
    /// 2. Dispatch it via [`dispatch_line`] (handles `:type` meta-commands
    ///    and forwards everything else to [`evaluate_and_format`]).
    /// 3. Print the formatted result.
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

            let formatted = dispatch_line(&mut self.evaluator, &line);
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

/// Dispatch a single REPL input line.
///
/// This is the PURE entry point the interactive [`Repl::run`] loop calls
/// for every keystroke line. It routes meta-commands (lines starting
/// with `:`) to their handlers and forwards everything else to
/// [`evaluate_and_format`] (the normal eval+format path).
///
/// # Meta-commands (T125b)
///
/// - `:type <expr>` — print the inferred type of `<expr>` against the
///   accumulated `let` / `func` environment. Calls
///   [`Evaluator::type_of`], which is a pure lex + parse + infer pass
///   (NO rustc spawn). State is consulted read-only — a `:type` query
///   has no side effects on subsequent evaluations.
/// - `:type` (no argument) — short usage hint, no panic.
///
/// Unknown meta-commands (`:foo`) are NOT intercepted in T125b; they
/// fall through to the normal path, where Buff's lexer will reject them
/// with a parse diagnostic. `:help` / `:quit` / `:load` are deferred
/// to T125c.
///
/// # Errors
///
/// None — all failure paths from the evaluator or the type inferencer
/// are surfaced through the returned string. This function never
/// panics.
pub fn dispatch_line(ev: &mut Evaluator, input: &str) -> String {
    let stripped = input.trim_start();
    if let Some(rest) = stripped.strip_prefix(TYPE_CMD) {
        // `rest` is whatever follows the literal `:type`. Require either
        // end-of-line OR a whitespace separator so `:typex` (no space)
        // is NOT treated as the meta-command — it falls through to the
        // normal Buff lexer, which can reject it on its own terms.
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return handle_type_command(ev, rest);
        }
    }
    evaluate_and_format(ev, input)
}

/// Handle the `:type <expr>` meta-command.
///
/// `expr_arg` is the substring AFTER the `:type` prefix (so it may be
/// empty, all-whitespace, or start with whitespace then a real
/// expression). We trim leading whitespace before passing to
/// [`Evaluator::type_of`] so `:type   x` works the same as `:type x`.
///
/// Output contract:
///
/// - Empty expression → usage hint, no panic.
/// - Inference succeeds → `<Display of Type>\n` (e.g. `Int<64>\n`).
/// - Inference fails (`type_of` returns `None`) → a one-line diagnostic
///   noting the expr we could not infer. The REPL loop continues.
fn handle_type_command(ev: &Evaluator, expr_arg: &str) -> String {
    let expr = expr_arg.trim();
    if expr.is_empty() {
        return format!(
            "{TYPE_CMD} requires an expression, e.g. `{TYPE_CMD} x` or `{TYPE_CMD} 2 + 3`\n"
        );
    }
    match ev.type_of(expr) {
        Some(ty) => format!("{ty}\n"),
        None => format!("cannot infer type of `{expr}`\n"),
    }
}

/// The literal prefix for the `:type` meta-command. Kept as a const so
/// the dispatcher and the usage hint stay in sync if it ever changes.
const TYPE_CMD: &str = ":type";

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

    // -----------------------------------------------------------------------
    // T125b: `:type` meta-command + dispatcher fall-through behavior.
    // The type_of path is pure lex+parse+infer (no rustc spawn), so these
    // are FAST hermetic unit tests that belong here in src/lib.rs.
    // -----------------------------------------------------------------------

    #[test]
    fn type_command_after_let_prints_resolved_type() {
        // Seed the evaluator with `let x = 42`, then ask `:type x`.
        // Buff's TypeInferencer resolves `42` to `Int<64>` (default Int
        // width). The REPL surfaces the Type's Display form verbatim.
        let mut ev = Evaluator::new();
        // We can't run the let-binding here (no rustc available in unit
        // tests), but type_of walks the body_stmts_src directly through
        // the inferencer, so we need the let-binding's SOURCE in the
        // accumulated state. Dispatch `let x = 42` to populate state.
        // Even though eval_line will fail to spawn rustc, the BODY source
        // is accumulated BEFORE the spawn attempt (see buff-eval's
        // `SnippetKind::BodyStmt` arm).
        let _ = ev.eval_line("let x = 42");
        let out = dispatch_line(&mut ev, ":type x");
        assert!(
            out.contains("Int"),
            "expected `Int` in type output, got: {out:?}"
        );
        assert!(
            out.ends_with('\n'),
            "type output should end with newline, got: {out:?}"
        );
        // No diagnostic tag in a successful type query.
        assert!(
            !out.contains("[Error]"),
            "type query should not produce an error, got: {out:?}"
        );
    }

    #[test]
    fn type_command_handles_extra_whitespace() {
        // `:type   x` (multiple spaces) should work the same as `:type x`.
        let mut ev = Evaluator::new();
        let _ = ev.eval_line("let x = 42");
        let out = dispatch_line(&mut ev, ":type   x");
        assert!(
            out.contains("Int"),
            "expected `Int` with extra whitespace, got: {out:?}"
        );
    }

    #[test]
    fn type_command_with_no_arg_prints_usage_hint() {
        // `:type` alone (no expr) → short usage hint, no panic.
        let mut ev = Evaluator::new();
        let out = dispatch_line(&mut ev, ":type");
        assert!(
            out.contains(":type"),
            "usage hint should mention the command name, got: {out:?}"
        );
        assert!(
            out.contains("expression"),
            "usage hint should mention `expression`, got: {out:?}"
        );
        assert!(out.ends_with('\n'));
        // No diagnostic — this is a usage hint, not an error.
        assert!(
            !out.contains("[Error]"),
            "usage hint should not be a diagnostic, got: {out:?}"
        );
    }

    #[test]
    fn type_command_with_only_whitespace_arg_prints_usage_hint() {
        // `:type   ` (whitespace only after the command) → usage hint.
        let mut ev = Evaluator::new();
        let out = dispatch_line(&mut ev, ":type   ");
        assert!(
            out.contains(":type") && out.contains("expression"),
            "whitespace-only arg should yield usage hint, got: {out:?}"
        );
    }

    #[test]
    fn type_command_for_unknown_expr_prints_inference_failure() {
        // `:type undefined_var` — the inferencer can't resolve a name
        // that was never `let`-bound. type_of returns None; REPL prints
        // a friendly "cannot infer" line. No panic.
        let mut ev = Evaluator::new();
        let out = dispatch_line(&mut ev, ":type undefined_var");
        assert!(
            out.contains("cannot infer"),
            "expected `cannot infer` message for unknown expr, got: {out:?}"
        );
        assert!(
            out.contains("undefined_var"),
            "expected the offending expr echoed back, got: {out:?}"
        );
    }

    #[test]
    fn type_command_does_not_consume_a_rustc_invocation() {
        // `:type` must be PURE — no spawn. We assert by checking that
        // dispatch returns a value within a tiny budget; a rustc spawn
        // would take ~100ms+, so a sub-millisecond return proves no
        // spawn happened. (This is a smoke check, not a precise bench.)
        let mut ev = Evaluator::new();
        let _ = ev.eval_line("let x = 42");
        let start = std::time::Instant::now();
        let _ = dispatch_line(&mut ev, ":type x");
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 500,
            "type_of should be fast (no rustc spawn); took {elapsed:?}"
        );
    }

    #[test]
    fn type_command_with_leading_input_whitespace_still_dispatches() {
        // `   :type x` (leading spaces before the meta-command) should
        // still hit the dispatcher — we trim_start before checking the
        // prefix.
        let mut ev = Evaluator::new();
        let _ = ev.eval_line("let x = 42");
        let out = dispatch_line(&mut ev, "   :type x");
        assert!(
            out.contains("Int"),
            "leading whitespace should not prevent :type dispatch, got: {out:?}"
        );
    }

    #[test]
    fn dispatch_falls_through_for_non_meta_input() {
        // Anything NOT starting with `:type ` must flow through to
        // evaluate_and_format. Verify by checking that a normal
        // expression yields the same output via both entry points.
        let mut ev1 = Evaluator::new();
        let mut ev2 = Evaluator::new();
        let direct = evaluate_and_format(&mut ev1, "2 + 3");
        let via_dispatch = dispatch_line(&mut ev2, "2 + 3");
        assert_eq!(
            direct, via_dispatch,
            "dispatcher must not alter non-meta-command behavior"
        );
    }

    #[test]
    fn dispatch_does_not_intercept_typex_without_space() {
        // `:typex` (no whitespace separator) must NOT be treated as the
        // `:type` meta-command. It should fall through to the Buff lexer,
        // which will produce some kind of diagnostic (parse error). The
        // exact diagnostic wording is buff-eval's contract; we just
        // assert that we did NOT enter the type-command path.
        let mut ev = Evaluator::new();
        let out = dispatch_line(&mut ev, ":typex");
        assert!(
            !out.starts_with("Int") && !out.starts_with("Bool") && !out.starts_with("String"),
            "`:typex` should NOT be parsed as a successful :type query, got: {out:?}"
        );
        // And it should NOT echo the usage hint either.
        assert!(
            !out.contains("requires an expression"),
            "`:typex` should not be treated as `:type` with missing arg, got: {out:?}"
        );
    }

    #[test]
    fn type_command_sees_state_from_prior_let_via_dispatch() {
        // The pure dispatcher must thread state across calls when the
        // caller reuses the same Evaluator. Seed via dispatch, query
        // via dispatch.
        let mut ev = Evaluator::new();
        // Seed: `let x = 42` (rustc will fail in unit-test env without
        // MSVC libs, but the source IS accumulated before the spawn).
        let _ = dispatch_line(&mut ev, "let x = 42");
        // Query: `:type x` — should resolve to Int via the accumulated
        // body_stmts_src.
        let out = dispatch_line(&mut ev, ":type x");
        assert!(
            out.contains("Int"),
            "type query should see state from prior let, got: {out:?}"
        );
    }
}
