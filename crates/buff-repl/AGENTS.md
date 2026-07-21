# buff-repl

Interactive read-eval-print loop for Buff. Layers on `buff-eval` for evaluation, `rustyline` 15 for line editing.

## OVERVIEW

Powers the `buff repl` subcommand. Wraps `Evaluator` with a `DefaultEditor` (history, Ctrl-D/Ctrl-C, line editing). Adds meta-commands (`:help`, `:type`, `:load`, `:quit`), multi-line continuation, and result formatting. No new compilation logic.

## STRUCTURE

```
src/
└── lib.rs            # 1573 lines — Repl struct, run loop, parse_command,
                     #   needs_continuation, format_eval_result, :load handler,
                     #   split_top_level_decls, inline tests (~590 lines)
tests/
└── repl_tests.rs     # End-to-end acceptance: eval path, multi-line blocks,
                     #   :load, state accumulation
```

## PUBLIC API

| Symbol | Notes |
|---|---|
| `Repl::new() -> Result<Self, ReadlineError>` | Fresh REPL, loads `~/.buff_history` best-effort |
| `Repl::with_prompt(prompt) -> Result<Self, ReadlineError>` | Custom prompt |
| `Repl::with_history_path(path: Option<PathBuf>) -> Self` | Override/disable history (tests use this) |
| `Repl::run() -> Result<(), ReadlineError>` | Blocking interactive loop |
| `Repl::run_with_writer(W) -> Result<(), ReadlineError>` | Same, but captures output |
| `parse_command(input) -> ReplAction` | Pure classifier (testable without TTY) |
| `dispatch_line(ev, input) -> String` | Pure eval+format (testable without TTY) |
| `format_eval_result(result) -> String` | Pure formatter for `EvalResult` |
| `needs_continuation(buffered) -> bool` | Pure multi-line heuristic |
| `help_text() -> String` | Meta-command help text |
| `ReplAction` enum | `Eval`, `Type`, `Help`, `Load`, `Quit`, `Nop` |
| `DEFAULT_PROMPT` / `CONTINUATION_PROMPT` | `"buff> "` / `".... "` |

## WHERE TO LOOK

| Task | File |
|---|---|
| Change prompt or keybindings | `lib.rs` — `DEFAULT_PROMPT`, `CONTINUATION_PROMPT`, `Repl::new` |
| Change eval semantics | `buff-eval` crate, NOT here |
| Add meta-command | `parse_command` (classifier) + `handle_action` (handler) + `help_text` |
| Change multi-line heuristic | `needs_continuation` + `strip_trailing_line_comment` |
| Change result formatting | `format_eval_result` + `render_diagnostic_for_repl` |
| History persistence | `try_load_history` / `try_save_history`, `HISTORY_FILENAME` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test code. Diagnostics render to terminal and the loop continues.
- **History persists to `~/.buff_history`** via `dirs::home_dir`. Missing home dir → in-memory only (silent skip). Tests override via `with_history_path`.
- **Unknown `:foo` falls through to Eval** — NOT silently dropped. The Buff lexer surfaces a parse diagnostic.
- **Multi-line heuristic is pure** (`:`+dedent). Does NOT run the parser. Blank line or dedent to column 0 closes the block.
- **`:load` skips `func main`** — the REPL user owns `main`. The empty-main artifact diagnostic (`"indented block"`) is detected and suppressed.
- **`format_eval_result`**: forwards stdout verbatim; appends `= <value>` only when the value differs from trimmed stdout (avoids duplicating `print(...)` output).
- **`Repl` is NOT `Clone`** — owns mutable evaluator state. Tests use `dispatch_line` or `evaluate_and_format` for pure-format testing.

## DEPS

| Crate | Purpose |
|---|---|
| `buff-eval` | Evaluation engine (workspace path dep) |
| `buff-lang-error` | Diagnostic type |
| `rustyline` 15 | Line editor (conservative pin: 13/14/15/16/17/18 share the API surface) |
| `dirs` 5 | `home_dir` for `~/.buff_history` |
