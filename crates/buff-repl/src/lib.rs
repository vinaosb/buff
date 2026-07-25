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
//!                │ parse_command            │  pure classifier (this
//!                │   (this crate)           │  crate) → ReplAction
//!                └────────────┬─────────────┘
//!                             │  ReplAction
//!                             ▼
//!                ┌──────────────────────────┐
//!                │ buff_eval::Evaluator     │  lex + parse + codegen +
//!                │   (T125-prep)            │  rustc + spawn-and-capture
//!                └────────────┬─────────────┘
//!                             │  EvalResult
//!                             ▼
//!                ┌──────────────────────────┐
//!                │ format_eval_result       │  pure formatting fn (this
//!                │   (this crate)           │  crate) → display string
//!                └──────────────────────────┘
//! ```
//!
//! The REPL adds NO new compilation logic — it consumes `buff-eval`
//! exclusively.
//!
//! # Meta-commands (T125b / T125c)
//!
//! Lines starting with `:` are intercepted by the dispatcher
//! ([`dispatch_line`] / [`parse_command`]) BEFORE reaching
//! [`Evaluator::eval_line`]:
//!
//! - `:help` — print the list of available commands.
//! - `:type <expr>` — print the inferred type of `<expr>` against the
//!   accumulated `let` / `func` environment. Calls
//!   [`Evaluator::type_of`] — a pure lex + parse + infer pass that does
//!   NOT spawn rustc.
//! - `:load <path>` — read a `.buff` file and feed its top-level
//!   declarations (`func` / `struct` / `enum` / top-level `let`) into
//!   the live session via [`Evaluator::eval_line`]. The file's
//!   `func main` is SKIPPED (the user owns `main` in a REPL session).
//! - `:quit` — exit the REPL cleanly (same as Ctrl-D).
//!
//! Unknown `:foo` is NOT intercepted — it falls through to the normal
//! path so future T125d+ commands can be added without churning the
//! dispatcher.
//!
//! # Multi-line input (T125c)
//!
//! A line whose last non-comment, non-whitespace character is `:`
//! (Buff's offside-rule block opener — `func ...:`, `if ...:`, etc.)
//! triggers a continuation prompt ([`CONTINUATION_PROMPT`]). Subsequent
//! indented lines are buffered; a blank line OR a dedent to column 0
//! closes the block. The assembled source is then fed to the evaluator
//! as ONE unit. The completeness check ([`needs_continuation`]) is a
//! pure function — unit-testable without a TTY.
//!
//! # History persistence (T125c)
//!
//! On [`Repl::new`], the REPL best-effort loads `~/.buff_history` (path
//! resolved via [`dirs::home_dir`]). On ANY loop exit (Ctrl-D, Ctrl-C,
//! `:quit`, I/O error), the in-memory history is appended back to the
//! same file. Missing home dir or unreadable file → silent skip. Tests
//! never touch the real `~/.buff_history`; they construct the REPL via
//! [`Repl::with_history_path`] with a temp path or skip history
//! entirely.
//!
//! # Resilience
//!
//! A diagnostic from any pipeline phase (lex / parse / codegen / rustc /
//! spawn) is rendered to the terminal and the loop CONTINUES — the user
//! sees the error and gets a fresh prompt. Only `ReadlineError::Eof`
//! (Ctrl-D), `ReadlineError::Interrupted` (Ctrl-C), `:quit`, or an
//! unrecoverable rustyline I/O error break the loop.
//!
//! # Testing
//!
//! [`rustyline`] requires a TTY, so the interactive loop itself is not
//! unit-tested. The pure surface — [`parse_command`],
//! [`needs_continuation`], [`dispatch_line`], [`evaluate_and_format`],
//! [`format_eval_result`] — IS the testable layer. Integration tests in
//! `tests/repl_tests.rs` exercise the eval path end-to-end (including
//! a rustc spawn for the happy path).
//!
//! # No panics
//!
//! There are no `unwrap` / `expect` / `panic!` / `unimplemented!` /
//! `todo!` calls outside `#[cfg(test)]`.

// Boxing error types would reshape the REPL's public surface (mirrors the
// buff-eval / buff-jupyter decision). Out of scope; allowed at crate level.
#![allow(clippy::result_large_err)]

use std::io::{self, Write};
use std::path::PathBuf;

use buff_eval::{EvalResult, Evaluator};
use buff_lang_error::Diagnostic;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

/// The default prompt string shown at the start of each top-level REPL
/// line.
pub const DEFAULT_PROMPT: &str = "buff> ";

/// The continuation prompt shown when the user is mid-block (the
/// previous line opened a block via a trailing `:` and we're collecting
/// the indented body).
pub const CONTINUATION_PROMPT: &str = ".... ";

/// The default continuation message emitted when the REPL session ends
/// (Ctrl-D / Ctrl-C / `:quit`).
const FAREWELL: &str = "bye.";

/// The default file name for REPL history, relative to the user's home
/// directory.
const HISTORY_FILENAME: &str = ".buff_history";

/// A Buff REPL session.
///
/// Owns the [`DefaultEditor`] (rustyline line editor with in-memory
/// history) and the [`Evaluator`] (accumulating `let` / `func` state).
/// Construct with [`Repl::new`] and launch with [`Repl::run`].
///
/// The REPL is intentionally NOT `Clone` — the evaluator owns mutable
/// session state that callers usually want to keep singular. Tests that
/// need to drive the formatting layer without a TTY should call the
/// free function [`dispatch_line`] / [`evaluate_and_format`] directly.
#[derive(Debug)]
pub struct Repl {
    editor: DefaultEditor,
    evaluator: Evaluator,
    prompt: String,
    /// Path to the REPL history file. `None` when the home dir could
    /// not be resolved (headless CI / unusual environments) — in that
    /// case history is in-memory only and NOT persisted on exit.
    /// Tests override via [`Repl::with_history_path`] to avoid touching
    /// the user's real `~/.buff_history`.
    history_path: Option<PathBuf>,
}

impl Repl {
    /// Construct a fresh REPL with an empty evaluator and the default
    /// [`DEFAULT_PROMPT`]. Best-effort loads `~/.buff_history` if it
    /// exists; missing-file / unreadable-home errors are silently
    /// skipped (first run = no history).
    ///
    /// # Errors
    ///
    /// Returns [`rustyline::error::ReadlineError`] iff the underlying
    /// [`DefaultEditor::new`] fails.
    pub fn new() -> Result<Self, ReadlineError> {
        Self::with_prompt(DEFAULT_PROMPT)
    }

    /// Construct a fresh REPL with a custom prompt string. History path
    /// is resolved from [`dirs::home_dir`]; if home is unavailable,
    /// history is disabled (in-memory only).
    ///
    /// # Errors
    ///
    /// See [`Repl::new`].
    pub fn with_prompt(prompt: &str) -> Result<Self, ReadlineError> {
        let history_path = default_history_path();
        let mut repl = Self {
            editor: DefaultEditor::new()?,
            evaluator: Evaluator::new(),
            prompt: prompt.to_string(),
            history_path,
        };
        repl.try_load_history();
        Ok(repl)
    }

    /// Override the history file path. Pass `None` to disable history
    /// persistence entirely (useful for tests). Consumes and returns
    /// `self` for chaining.
    #[must_use]
    pub fn with_history_path(mut self, path: Option<PathBuf>) -> Self {
        self.history_path = path;
        self
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

    /// Best-effort load the history file. Missing file / IO error →
    /// silent skip (the REPL still works with empty in-memory history).
    fn try_load_history(&mut self) {
        if let Some(path) = &self.history_path {
            // Ignore errors: missing-file on first run, unreadable on
            // permission issues, etc. The REPL still functions with an
            // empty in-memory history.
            let _ = self.editor.load_history(path);
        }
    }

    /// Best-effort save the history file. Called on every loop exit
    /// path. Errors are silently dropped — losing history persistence
    /// is preferable to crashing the REPL on exit.
    fn try_save_history(&mut self) {
        if let Some(path) = &self.history_path {
            let _ = self.editor.save_history(path);
        }
    }

    /// Run the REPL loop until the user exits via Ctrl-D / Ctrl-C /
    /// `:quit`, or a rustyline I/O error occurs.
    ///
    /// Each iteration:
    /// 1. Read a line via rustyline (using [`DEFAULT_PROMPT`] or
    ///    [`CONTINUATION_PROMPT`] depending on whether we're mid-block).
    /// 2. Buffer the line; if [`needs_continuation`] reports the block
    ///    is still open, keep reading.
    /// 3. Once a complete unit is assembled, parse it via
    ///    [`parse_command`] and dispatch to the appropriate handler.
    /// 4. Add the buffered input to the rustyline history.
    ///
    /// A diagnostic from the evaluator NEVER terminates the loop — it
    /// is printed and the prompt returns. The loop breaks on
    /// `ReadlineError::Eof` (Ctrl-D), `ReadlineError::Interrupted`
    /// (Ctrl-C), `ReplAction::Quit` (`:quit`), or an unrecoverable
    /// rustyline I/O error. On ANY break, history is best-effort
    /// saved to [`Self::history_path`].
    ///
    /// # Errors
    ///
    /// Returns the first non-Eof / non-Interrupted `ReadlineError` to
    /// escape rustyline.
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
            // Accumulate a complete input unit (handling multi-line
            // blocks via the continuation prompt).
            let input = match self.read_complete_unit(&mut out) {
                ReadOutcome::Line(s) => s,
                ReadOutcome::Eof => {
                    let _ = writeln!(out, "{FAREWELL}");
                    let _ = out.flush();
                    self.try_save_history();
                    break;
                }
                ReadOutcome::Interrupted => {
                    let _ = writeln!(out);
                    let _ = writeln!(out, "{FAREWELL}");
                    let _ = out.flush();
                    self.try_save_history();
                    break;
                }
                ReadOutcome::Err(e) => {
                    let _ = out.flush();
                    self.try_save_history();
                    return Err(e);
                }
            };

            // Best-effort history append for the WHOLE assembled unit
            // (multi-line blocks become one entry — the natural unit for
            // up-arrow recall).
            if !input.trim().is_empty() {
                let _ = self.editor.add_history_entry(input.as_str());
            }

            // Dispatch via the pure classifier so the loop sees Quit.
            let action = parse_command(&input);
            if matches!(action, ReplAction::Quit) {
                let _ = writeln!(out, "{FAREWELL}");
                let _ = out.flush();
                self.try_save_history();
                break;
            }
            let formatted = handle_action(&mut self.evaluator, &action);
            let _ = write!(out, "{formatted}");
            let _ = out.flush();
        }
        Ok(())
    }

    /// Read one complete input unit, prompting with [`DEFAULT_PROMPT`]
    /// for the first line and [`CONTINUATION_PROMPT`] for subsequent
    /// lines while [`needs_continuation`] reports the block is still
    /// open.
    ///
    /// Returns the assembled source (multiple lines joined by `\n`),
    /// or a [`ReadOutcome`] signaling loop-exit conditions.
    fn read_complete_unit<W: Write>(&mut self, out: &mut W) -> ReadOutcome {
        let mut buffer = String::new();
        let mut first_iter = true;
        loop {
            let prompt: &str = if first_iter || buffer.is_empty() {
                &self.prompt
            } else {
                CONTINUATION_PROMPT
            };
            first_iter = false;
            let line = match self.editor.readline(prompt) {
                Ok(l) => l,
                Err(ReadlineError::Interrupted) => return ReadOutcome::Interrupted,
                Err(ReadlineError::Eof) => return ReadOutcome::Eof,
                Err(e) => return ReadOutcome::Err(e),
            };
            // Append the line + newline to the buffer.
            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(&line);
            // Empty / whitespace-only input at top level is a no-op —
            // return immediately so the caller doesn't loop forever on
            // blank lines.
            if !needs_continuation(&buffer) {
                let _ = out.flush();
                return ReadOutcome::Line(buffer);
            }
        }
    }
}

/// Outcome of a single rustyline `readline` attempt inside the REPL
/// loop, including the multi-line accumulation step.
enum ReadOutcome {
    /// A complete input unit (one or more lines).
    Line(String),
    /// Ctrl-D / EOF on stdin.
    Eof,
    /// Ctrl-C.
    Interrupted,
    /// Any other rustyline I/O error.
    Err(ReadlineError),
}

/// Resolve the default history file path (`~/.buff_history`).
///
/// Returns `None` when the home directory cannot be resolved (headless
/// CI, unusual environments). In that case history persistence is
/// silently disabled.
fn default_history_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(HISTORY_FILENAME))
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

// ---------------------------------------------------------------------------
// T125c: pure command classifier + ReplAction model.
// ---------------------------------------------------------------------------

/// A classified REPL action produced by [`parse_command`].
///
/// The classifier is PURE — it touches no global state, no TTY, no
/// rustyline editor, no evaluator. It just looks at the input string
/// and decides what KIND of action the REPL should take. The caller
/// (`run_with_writer` / `dispatch_line`) then matches on the variant
/// to execute the action.
///
/// This split exists because some actions (`Quit`, `Load`) require
/// control-flow or file-IO that a `String` return type can't express
/// cleanly. The pure classifier is testable without a TTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplAction {
    /// A normal Buff expression or statement to evaluate via
    /// [`Evaluator::eval_line`]. Carries the (trimmed) source.
    Eval(String),
    /// `:type <expr>` — print the inferred type via
    /// [`Evaluator::type_of`]. Carries the expression source (may be
    /// empty if the user typed bare `:type`).
    Type(String),
    /// `:help` — print [`help_text`].
    Help,
    /// `:load <path>` — read the file and feed its top-level decls
    /// into the session. Carries the path (may be empty if the user
    /// typed bare `:load`).
    Load(String),
    /// `:quit` — exit the REPL cleanly (same as Ctrl-D).
    Quit,
    /// Empty / whitespace-only input — no-op, no output, prompt
    /// returns.
    Nop,
}

/// Classify a raw REPL input line into a [`ReplAction`].
///
/// Pure: no TTY, no evaluator mutation, no file IO. The interactive
/// [`Repl::run_with_writer`] loop calls this fn for every assembled
/// input unit (after multi-line accumulation) and matches on the
/// returned variant.
///
/// # Meta-command syntax
///
/// - Input is `trim`-ed first; whitespace-only → [`ReplAction::Nop`].
/// - Lines starting with `:` are meta-commands. The first whitespace-
///   delimited token after `:` is the command name; the rest (trimmed)
///   is the argument.
/// - Unknown `:foo` falls through to [`ReplAction::Eval`] so the Buff
///   lexer surfaces a parse diagnostic (preserves the T125b behavior
///   of NOT silently dropping unknown `:foo`).
/// - Lines NOT starting with `:` → [`ReplAction::Eval`] with the
///   trimmed source.
///
/// # Examples
///
/// ```
/// # use buff_repl::{parse_command, ReplAction};
/// assert_eq!(parse_command(":help"), ReplAction::Help);
/// assert_eq!(parse_command(":quit"), ReplAction::Quit);
/// assert_eq!(parse_command(":load foo.buff"), ReplAction::Load("foo.buff".to_string()));
/// assert_eq!(parse_command(":type x"), ReplAction::Type("x".to_string()));
/// assert_eq!(parse_command("2 + 3"), ReplAction::Eval("2 + 3".to_string()));
/// assert_eq!(parse_command("   "), ReplAction::Nop);
/// ```
#[must_use]
pub fn parse_command(input: &str) -> ReplAction {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return ReplAction::Nop;
    }
    let Some(rest) = trimmed.strip_prefix(':') else {
        return ReplAction::Eval(trimmed.to_string());
    };
    // Split on the first whitespace — everything up to it is the
    // command name, the rest is the (trimmed) argument.
    let (cmd, arg) = match rest.find(char::is_whitespace) {
        Some(idx) => (&rest[..idx], rest[idx..].trim()),
        None => (rest, ""),
    };
    match cmd {
        "help" => ReplAction::Help,
        "quit" | "exit" => ReplAction::Quit,
        "type" => ReplAction::Type(arg.to_string()),
        "load" => ReplAction::Load(arg.to_string()),
        // Unknown `:foo` — fall through to the Buff lexer, which will
        // surface a parse diagnostic. This is the documented T125b/T125c
        // behavior: unknown meta-commands are NOT silently dropped.
        _ => ReplAction::Eval(trimmed.to_string()),
    }
}

/// The `:help` text. Lists every meta-command the REPL supports.
///
/// Kept as a function (not a `const &str`) because it's composed from
/// the literal command prefixes ([`TYPE_CMD`] etc.) and we don't want
/// to duplicate them. The body is a single newline-terminated string
/// so the REPL can `write!` it without further formatting.
#[must_use]
pub fn help_text() -> String {
    let mut out = String::new();
    out.push_str("Buff REPL commands:\n");
    out.push_str("  :help              Show this help message\n");
    out.push_str("  :type <expr>       Print the inferred type of <expr>\n");
    out.push_str("  :load <path>       Load a .buff file's declarations into the session\n");
    out.push_str("  :quit              Exit the REPL (same as Ctrl-D)\n");
    out.push('\n');
    out.push_str("Anything else is evaluated as a Buff expression or statement.\n");
    out.push_str(
        "Press Ctrl-D or Ctrl-C to exit. Multi-line input: end a block with a blank line.\n",
    );
    out
}

// ---------------------------------------------------------------------------
// T125c: multi-line continuation detector.
// ---------------------------------------------------------------------------

/// Decide whether the REPL should keep reading more lines for the
/// current input unit.
///
/// The contract is the `:`+dedent heuristic from T125c: a Buff block
/// opens when a line's last non-comment, non-whitespace char is `:`
/// (the offside-rule block opener — `func ...:`, `if ...:`, etc.).
/// Subsequent indented lines extend the block. The block closes on:
///
/// 1. A blank line (the canonical terminator — type Enter on an empty
///    line at the continuation prompt), OR
/// 2. A dedent to column 0 (a non-indented line that doesn't itself
///    open a new block).
///
/// This is a PURE function — no TTY, no evaluator, no parser. It does
/// NOT do full incremental parsing; it's a heuristic. The task spec
/// explicitly rules out a parser-driven checker.
///
/// # Rules (checked in order)
///
/// 1. Empty / whitespace-only buffer → `false` (nothing to continue).
/// 2. Buffer's last raw line is blank (empty or whitespace-only) →
///    `false` (user ended the block with Enter).
/// 3. Buffer's last non-blank, non-comment line ends with `:` → `true`
///    (a block opener — need a body).
/// 4. Buffer's last raw line is indented (starts with whitespace) →
///    `true` (still inside a body).
/// 5. Otherwise → `false` (dedented to column 0).
///
/// # Examples
///
/// ```
/// # use buff_repl::needs_continuation;
/// assert!(!needs_continuation(""));
/// assert!(!needs_continuation("2 + 3"));
/// assert!(needs_continuation("func f():"));
/// assert!(needs_continuation("func f():\n    let x = 1"));
/// assert!(!needs_continuation("func f():\n    let x = 1\n"));
/// // Blank line ends the block:
/// assert!(!needs_continuation("func f():\n    let x = 1\n\n"));
/// // Dedent to column 0 also ends the block:
/// assert!(!needs_continuation("func f():\n    let x = 1\nprint(1)\n"));
/// ```
#[must_use]
pub fn needs_continuation(buffered: &str) -> bool {
    if buffered.trim().is_empty() {
        return false;
    }
    // The last raw line is the substring after the last '\n' (or the
    // whole buffer if no '\n').
    let last_raw = buffered.rsplit('\n').next().unwrap_or("");
    // Rule 2: blank trailing line ends the block.
    if last_raw.trim().is_empty() {
        return false;
    }
    // Rule 3: find the last significant (non-blank, non-comment) line.
    // For the ends_with(':') check, strip trailing `// ...` comments so
    // `func f():  // doc` still opens a block. This is a heuristic — it
    // doesn't handle `//` inside string literals, but Buff's lexer
    // surface makes that rare and the task says don't gold-plate.
    let last_sig = buffered
        .rsplit('\n')
        .find(|line| {
            let t = line.trim();
            !t.is_empty() && !t.starts_with("//")
        })
        .unwrap_or("");
    let last_sig_before_comment = strip_trailing_line_comment(last_sig);
    if last_sig_before_comment.trim_end().ends_with(':') {
        return true;
    }
    // Rule 4: still inside an indented body.
    last_raw.starts_with(char::is_whitespace)
}

/// Strip a trailing `// ...` line comment from `line`, returning the
/// portion before the `//` (or the whole line if no comment is present).
///
/// This is a HEURISTIC — it does NOT understand string literals, so a
/// `//` inside a string literal would be wrongly treated as a comment
/// start. Buff rarely uses `//` inside string literals in practice
/// (URLs are typically constructed via concatenation), and the task
/// spec explicitly rules out gold-plating the parser.
fn strip_trailing_line_comment(line: &str) -> &str {
    // Find the first `//` in the line. If found, return everything
    // before it. (We don't handle `//` inside strings — see doc above.)
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

// ---------------------------------------------------------------------------
// T125c: `:load` — file ingestion.
// ---------------------------------------------------------------------------

/// Handle the `:load <path>` meta-command.
///
/// Reads the file, splits it into top-level declarations, and feeds
/// each non-main decl through [`Evaluator::eval_line`] so it
/// accumulates into the live session. The file's `func main` is
/// SKIPPED (the REPL user owns `main`; auto-running it on load would
/// be surprising).
///
/// Returns a status string summarizing the load (count loaded, count
/// skipped, any per-decl errors). The REPL prints this verbatim.
///
/// # The empty-main artifact
///
/// When buff-eval accumulates a TopLevelDecl, it composes a program
/// with the accumulated top-level source followed by `func main():`
/// with an EMPTY body. Buff's parser rejects an empty body with
/// `error[E1104]: expected indented block after ':'`. The decl source
/// IS accumulated BEFORE the run attempt, so this error is HARMLESS —
/// the decl is in the session despite the diagnostic. We detect this
/// specific artifact (via the "indented block" substring in the
/// diagnostic message) and count the decl as loaded, suppressing the
/// spurious error.
///
/// # Error handling
///
/// - Missing path arg → usage hint, no panic.
/// - File read error (missing / unreadable) → diagnostic line, loop
///   continues.
/// - Per-decl lex/parse/codegen errors (EXCLUDING the empty-main
///   artifact above) → summarized in the status string; the load does
///   NOT abort on the first error.
fn handle_load_command(ev: &mut Evaluator, path: &str) -> String {
    if path.is_empty() {
        return ":load requires a file path, e.g. `:load examples/fibonacci.buff`\n".to_string();
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return format!(":load: failed to read `{path}`: {e}\n");
        }
    };
    let decls = split_top_level_decls(&content);
    let mut loaded = 0usize;
    let mut skipped_main = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for decl in &decls {
        if is_func_main_decl(decl) {
            skipped_main += 1;
            continue;
        }
        let result = ev.eval_line(decl);
        let is_empty_main_artifact = result
            .diagnostic
            .as_ref()
            .map(|d| d.message.contains("indented block"))
            .unwrap_or(false);
        if result.diagnostic.is_none() || is_empty_main_artifact {
            loaded += 1;
        } else if let Some(d) = &result.diagnostic {
            let preview = decl_preview(decl);
            errors.push(format!("  in `{preview}`: {d}"));
        }
    }
    let mut out = String::new();
    out.push_str(&format!(":load `{path}`: {loaded} decl(s) loaded"));
    if skipped_main > 0 {
        out.push_str(&format!(", {skipped_main} main(s) skipped"));
    }
    out.push('\n');
    if !errors.is_empty() {
        out.push_str("errors:\n");
        for e in &errors {
            out.push_str(e);
            out.push('\n');
        }
    }
    out
}

/// Split a Buff source string into its top-level declarations.
///
/// A new top-level decl starts whenever a non-blank, non-comment line
/// at column 0 appears after we've already accumulated content. Blank
/// lines and indented lines (body lines) extend the current decl.
///
/// This is a HEURISTIC split — it does NOT run the parser. It correctly
/// handles:
///
/// - Blank lines INSIDE a body (preserved with the surrounding decl).
/// - `//` line comments at column 0 attached to the following decl
///   (Buff convention: doc-comments lead their decl).
/// - Multiple consecutive top-level decls separated by blank lines.
///
/// It does NOT handle:
///
/// - Block comments spanning multiple lines (rare in Buff; the offside
///   rule makes them awkward).
/// - Top-level expressions / statements (those don't accumulate the
///   way `func` / `struct` / `enum` do).
fn split_top_level_decls(source: &str) -> Vec<String> {
    let mut decls: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in source.lines() {
        let starts_new_decl = !line.is_empty()
            && !line.starts_with(char::is_whitespace)
            && !current.trim().is_empty()
            && !is_continuation_of_current(&current, line);
        if starts_new_decl {
            decls.push(current.trim_end().to_string());
            current.clear();
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        decls.push(current.trim_end().to_string());
    }
    decls
}

/// `true` if `line` should be appended to the current buffer rather
/// than starting a new decl.
///
/// Two cases trigger continuation:
///
/// 1. The current decl is entirely comments so far AND the new line is
///    ALSO a `//` comment at column 0 (multi-line doc-comment block).
/// 2. The current decl is entirely comments so far AND the new line is
///    NOT a comment — this is the actual decl the comments document
///    (Buff convention: doc-comments lead their decl). The decl line
///    extends the comment block so they're emitted as ONE chunk.
fn is_continuation_of_current(current: &str, line: &str) -> bool {
    let current_is_all_comments = current
        .lines()
        .all(|l| l.trim().is_empty() || l.trim_start().starts_with("//"));
    if !current_is_all_comments {
        return false;
    }
    // Current is all comments. Two sub-cases:
    // (a) New line is also a comment → continuation (multi-line doc).
    // (b) New line is NOT a comment → it's the decl the docs document.
    // Either way, extend the current buffer.
    // The only case we DON'T extend is when the new line is blank AND
    // the current is all comments — then we treat the blank as a
    // separator (rare; usually the doc-comments directly lead the decl).
    !line.trim().is_empty()
}

/// `true` if `decl_source` parses (textually) as a `func main` decl.
///
/// Detects `func main()` / `func main:` / `func main()` with leading
/// comments. We use a textual check (not the parser) because we want
/// to SKIP main BEFORE feeding it through the evaluator — the
/// evaluator would classify it as FullProgram and run it verbatim,
/// which is undesirable on `:load`.
fn is_func_main_decl(decl_source: &str) -> bool {
    // Walk lines, find the first non-comment, non-blank line, check if
    // it starts with `func main`.
    for line in decl_source.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        // Strip leading whitespace (already trimmed) and check prefix.
        return t.starts_with("func main") && {
            // After `func main`, the next char should be `(`, `:`, or
            // whitespace then one of those. This avoids matching
            // `func mainHelper` etc.
            let rest = &t["func main".len()..];
            rest.is_empty()
                || rest.starts_with('(')
                || rest.starts_with(':')
                || rest.starts_with(char::is_whitespace)
        };
    }
    false
}

/// Short single-line preview of a decl, for error messages.
///
/// Returns the first non-comment, non-blank line, truncated to ~60
/// chars. Used in `:load` error summaries.
fn decl_preview(decl: &str) -> String {
    for line in decl.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        let mut s = t.to_string();
        if s.len() > 60 {
            s.truncate(57);
            s.push_str("...");
        }
        return s;
    }
    "<empty>".to_string()
}

// ---------------------------------------------------------------------------
// Action execution — shared between the interactive loop and
// `dispatch_line` (the pure backward-compat surface).
// ---------------------------------------------------------------------------

/// Execute a [`ReplAction`] against `ev` and return the output string.
///
/// This is the SINGLE handler both [`Repl::run_with_writer`] and
/// [`dispatch_line`] route through, ensuring identical behavior
/// between TTY and test harness. The interactive loop intercepts
/// [`ReplAction::Quit`] BEFORE calling this fn (since Quit is
/// control-flow, not output).
fn handle_action(ev: &mut Evaluator, action: &ReplAction) -> String {
    match action {
        ReplAction::Eval(src) => {
            // Ensure the source ends with a newline. Buff's offside-rule
            // lexer emits Indent/Dedent tokens based on line structure;
            // a multi-line block input WITHOUT a trailing newline won't
            // emit the final Dedent, causing the parser to reject the
            // block. Single-line inputs are unaffected (the extra `\n`
            // is a no-op for expressions / simple statements).
            let mut normalized = src.clone();
            if !normalized.ends_with('\n') {
                normalized.push('\n');
            }
            evaluate_and_format(ev, &normalized)
        }
        ReplAction::Type(expr) => handle_type_command(ev, expr),
        ReplAction::Help => help_text(),
        ReplAction::Load(path) => handle_load_command(ev, path),
        ReplAction::Quit => {
            // Reached only when `dispatch_line` is called directly with
            // `:quit` (the interactive loop intercepts Quit before
            // reaching here). Surface the farewell so the caller sees
            // SOMETHING; they're responsible for actually exiting.
            format!("{FAREWELL}\n")
        }
        ReplAction::Nop => String::new(),
    }
}

/// Dispatch a single REPL input line.
///
/// This is the PURE entry point — it touches no global state, no TTY,
/// no rustyline editor. It exists so the formatting layer can be tested
/// without spawning a terminal. It classifies `input` via
/// [`parse_command`] and forwards to [`handle_action`].
///
/// Note: [`ReplAction::Quit`] is handled here by returning the farewell
/// string. The interactive [`Repl::run_with_writer`] loop intercepts
/// Quit BEFORE reaching this fn (since Quit needs to break the loop,
/// not just produce output). For pure-formatting callers (tests) this
/// asymmetry is invisible.
///
/// # Errors
///
/// None — all failure paths are surfaced through the returned string.
pub fn dispatch_line(ev: &mut Evaluator, input: &str) -> String {
    let action = parse_command(input);
    handle_action(ev, &action)
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

    // -----------------------------------------------------------------------
    // T125c: parse_command + ReplAction classifier.
    // -----------------------------------------------------------------------

    #[test]
    fn parse_command_classifies_help() {
        assert_eq!(parse_command(":help"), ReplAction::Help);
        // With trailing whitespace.
        assert_eq!(parse_command(":help   "), ReplAction::Help);
    }

    #[test]
    fn parse_command_classifies_quit() {
        assert_eq!(parse_command(":quit"), ReplAction::Quit);
        assert_eq!(parse_command(":exit"), ReplAction::Quit);
    }

    #[test]
    fn parse_command_classifies_load_with_path() {
        assert_eq!(
            parse_command(":load foo.buff"),
            ReplAction::Load("foo.buff".to_string())
        );
        // Path with spaces is supported (single arg).
        assert_eq!(
            parse_command(":load examples/fibonacci.buff"),
            ReplAction::Load("examples/fibonacci.buff".to_string())
        );
    }

    #[test]
    fn parse_command_classifies_load_without_path_as_empty_string() {
        // `:load` alone → Load("") — the handler prints a usage hint.
        assert_eq!(parse_command(":load"), ReplAction::Load(String::new()));
        assert_eq!(parse_command(":load   "), ReplAction::Load(String::new()));
    }

    #[test]
    fn parse_command_classifies_type() {
        assert_eq!(parse_command(":type x"), ReplAction::Type("x".to_string()));
        assert_eq!(
            parse_command(":type 2 + 3"),
            ReplAction::Type("2 + 3".to_string())
        );
        // Bare `:type` → Type("").
        assert_eq!(parse_command(":type"), ReplAction::Type(String::new()));
    }

    #[test]
    fn parse_command_classifies_eval_for_non_meta_input() {
        assert_eq!(
            parse_command("2 + 3"),
            ReplAction::Eval("2 + 3".to_string())
        );
        assert_eq!(
            parse_command("let x = 42"),
            ReplAction::Eval("let x = 42".to_string())
        );
        // Leading/trailing whitespace is trimmed by the classifier.
        assert_eq!(
            parse_command("  2 + 3  "),
            ReplAction::Eval("2 + 3".to_string())
        );
    }

    #[test]
    fn parse_command_classifies_nop_for_empty_input() {
        assert_eq!(parse_command(""), ReplAction::Nop);
        assert_eq!(parse_command("   "), ReplAction::Nop);
        assert_eq!(parse_command("\t\n"), ReplAction::Nop);
    }

    #[test]
    fn parse_command_unknown_meta_falls_through_to_eval() {
        // Unknown `:foo` should NOT be silently dropped — it falls
        // through to Eval so the Buff lexer surfaces a parse
        // diagnostic (preserves T125b behavior).
        match parse_command(":unknown") {
            ReplAction::Eval(s) => assert_eq!(s, ":unknown"),
            other => panic!("expected Eval for unknown meta, got {other:?}"),
        }
        // `:typex` (no whitespace after `:type`) → NOT :type, falls through.
        match parse_command(":typex") {
            ReplAction::Eval(s) => assert_eq!(s, ":typex"),
            other => panic!("expected Eval for `:typex`, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // T125c: needs_continuation multi-line heuristic.
    // -----------------------------------------------------------------------

    #[test]
    fn needs_continuation_for_empty_buffer_is_false() {
        assert!(!needs_continuation(""));
        assert!(!needs_continuation("   "));
        assert!(!needs_continuation("\n\n"));
    }

    #[test]
    fn needs_continuation_for_single_line_is_false() {
        assert!(!needs_continuation("2 + 3"));
        assert!(!needs_continuation("let x = 42"));
        assert!(!needs_continuation("print(\"hi\")"));
    }

    #[test]
    fn needs_continuation_for_block_opener_is_true() {
        assert!(needs_continuation("func f():"));
        assert!(needs_continuation("if x:"));
        assert!(needs_continuation("for i in range(10):"));
        // Trailing whitespace after `:` still opens a block.
        assert!(needs_continuation("func f():   "));
    }

    #[test]
    fn needs_continuation_for_indented_body_line_is_true() {
        // After `func f():\n`, an indented body line keeps the block open.
        assert!(needs_continuation("func f():\n    let x = 1"));
        assert!(needs_continuation("func f():\n    let x = 1\n    print(x)"));
    }

    #[test]
    fn needs_continuation_for_blank_line_terminates_block() {
        // A blank line at the end of the buffer ends the block.
        assert!(!needs_continuation("func f():\n    let x = 1\n"));
        assert!(!needs_continuation("func f():\n    let x = 1\n\n"));
        assert!(!needs_continuation("func f():\n    let x = 1\n    \n"));
    }

    #[test]
    fn needs_continuation_for_dedent_terminates_block() {
        // A non-indented line (after we've started a body) ends the
        // block — it's a new top-level statement.
        assert!(!needs_continuation("func f():\n    let x = 1\nprint(1)\n"));
    }

    #[test]
    fn needs_continuation_nested_block_openers_stay_open() {
        // A nested block opener (`if x:` inside `func f():`) keeps
        // the block open.
        assert!(needs_continuation("func f():\n    if x:"));
        assert!(needs_continuation("func f():\n    if x:\n        print(x)"));
    }

    #[test]
    fn needs_continuation_ignores_trailing_comments() {
        // A line whose trailing significant content ends with `:` but
        // is followed by a `// comment` should still open a block.
        // The `:` check inspects the last NON-COMMENT line, with
        // trailing `// ...` stripped.
        assert!(needs_continuation("func f():  // header comment"));
        // A body line followed by a comment does NOT re-open the block
        // via the `:` rule, BUT it's still indented so rule 4 keeps
        // the block open (we expect more body lines). This is correct
        // behavior — the comment is just a comment.
        assert!(needs_continuation("func f():\n    let x = 1  // set x"));
        // What terminates the block is a blank line:
        assert!(!needs_continuation("func f():\n    let x = 1  // set x\n"));
    }

    // -----------------------------------------------------------------------
    // T125c: help_text content.
    // -----------------------------------------------------------------------

    #[test]
    fn help_text_lists_every_meta_command() {
        let h = help_text();
        for cmd in &[":help", ":type", ":load", ":quit"] {
            assert!(
                h.contains(cmd),
                "help_text should mention `{cmd}`, got: {h:?}"
            );
        }
        // Each command has its own line.
        assert!(
            h.lines().filter(|l| l.starts_with("  :")).count() >= 4,
            "expected at least 4 command entries, got: {h:?}"
        );
        assert!(h.ends_with('\n'));
    }

    // -----------------------------------------------------------------------
    // T125c: dispatch_line routes :help and :quit (backward-compat layer).
    // -----------------------------------------------------------------------

    #[test]
    fn dispatch_help_returns_help_text() {
        let mut ev = Evaluator::new();
        let out = dispatch_line(&mut ev, ":help");
        assert!(
            out.contains(":help") && out.contains(":quit"),
            "expected help text from :help, got: {out:?}"
        );
    }

    #[test]
    fn dispatch_quit_returns_farewell() {
        // dispatch_line is the PURE layer — it can't actually break
        // the loop. It surfaces the farewell string; the interactive
        // loop intercepts Quit via parse_command before reaching here.
        let mut ev = Evaluator::new();
        let out = dispatch_line(&mut ev, ":quit");
        assert!(
            out.contains(FAREWELL),
            "expected farewell `{FAREWELL}` from :quit, got: {out:?}"
        );
    }

    #[test]
    fn dispatch_load_with_no_path_prints_usage_hint() {
        let mut ev = Evaluator::new();
        let out = dispatch_line(&mut ev, ":load");
        assert!(
            out.contains(":load") && out.contains("file path"),
            "expected usage hint from `:load` with no arg, got: {out:?}"
        );
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn dispatch_load_missing_file_prints_diagnostic_no_panic() {
        let mut ev = Evaluator::new();
        let out = dispatch_line(&mut ev, ":load /this/path/does/not/exist.buff");
        assert!(
            out.contains("failed to read"),
            "expected diagnostic for missing file, got: {out:?}"
        );
        assert!(
            out.contains("/this/path/does/not/exist.buff"),
            "expected path echoed in diagnostic, got: {out:?}"
        );
    }

    // -----------------------------------------------------------------------
    // T125c: split_top_level_decls + is_func_main_decl helpers.
    // -----------------------------------------------------------------------

    #[test]
    fn split_top_level_decls_separates_funcs() {
        let src = "func a():\n    return 1\n\nfunc b():\n    return 2\n";
        let decls = split_top_level_decls(src);
        assert_eq!(
            decls.len(),
            2,
            "expected 2 decls from 2-func source, got {decls:?}"
        );
        assert!(decls[0].contains("func a"));
        assert!(decls[1].contains("func b"));
    }

    #[test]
    fn split_top_level_decls_preserves_internal_blank_lines() {
        // A blank line INSIDE a body should stay with that decl.
        let src = "func a():\n    let x = 1\n\n    let y = 2\n";
        let decls = split_top_level_decls(src);
        assert_eq!(
            decls.len(),
            1,
            "expected 1 decl (blank line in body preserved), got {decls:?}"
        );
        assert!(decls[0].contains("let y = 2"));
    }

    #[test]
    fn split_top_level_decls_attaches_leading_comments() {
        let src = "// doc comment for a\nfunc a():\n    return 1\n";
        let decls = split_top_level_decls(src);
        assert_eq!(decls.len(), 1);
        assert!(decls[0].contains("doc comment"));
        assert!(decls[0].contains("func a"));
    }

    #[test]
    fn is_func_main_decl_detects_main() {
        assert!(is_func_main_decl("func main():\n    print(\"hi\")\n"));
        assert!(is_func_main_decl("func main():\n    print(\"hi\")"));
        assert!(is_func_main_decl(
            "// comment\nfunc main():\n    print(\"hi\")\n"
        ));
        // `func mainHelper` is NOT main.
        assert!(!is_func_main_decl("func mainHelper():\n    return 1\n"));
        // `func fib` is NOT main.
        assert!(!is_func_main_decl(
            "func fib(n: Int) -> Int:\n    return n\n"
        ));
    }

    #[test]
    fn split_and_skip_main_works_for_fibonacci_shape() {
        // Mirrors examples/fibonacci.buff structure: comments + fib +
        // main.
        let src = "// example comment\nfunc fib(n: Int) -> Int:\n    return n\n\nfunc main():\n    print(fib(10))\n";
        let decls = split_top_level_decls(src);
        assert_eq!(decls.len(), 2);
        assert!(!is_func_main_decl(&decls[0])); // fib
        assert!(is_func_main_decl(&decls[1])); // main
    }
}
