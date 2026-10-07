# buff-eval

Thin evaluation engine over existing compiler primitives. Adds NO new compilation logic.

## OVERVIEW

Layers beneath `buff-repl` (T125a) and `buff-jupyter` (T129) so they resolve to the same in-tree version. Composes the existing `tokenize` → `parse` → `generate_rust` path, then spawns `rustc` + the compiled binary, capturing stdout/stderr. State (`let` bindings, `func` declarations) accumulates across calls via verbatim source buffering.

## STRUCTURE

```
src/
└── lib.rs            # Evaluator, EvalResult, SnippetKind, classify,
                      #   run_full_program (rustc invoke via buff-lang-pipeline)
tests/
└── eval_tests.rs     # Acceptance scenarios: expression eval, state accumulation,
                      #   type introspection, stdout capture, error handling
```

## PUBLIC API

| Symbol | Notes |
|---|---|
| `Evaluator::new()` | Fresh evaluator, empty state |
| `Evaluator::eval(source) -> EvalResult` | Full snippet evaluation; accumulates state |
| `Evaluator::eval_line(line) -> EvalResult` | Alias for `eval` (documents REPL intent) |
| `Evaluator::type_of(expr) -> Option<Type>` | Pure lex+parse+infer, no `rustc` spawn |
| `EvalResult` | `value`, `stdout`, `stderr`, `diagnostic`, `exit_code` |
| `EvalResult::is_ok()` | `diagnostic.is_none() && exit_code == Some(0)` |
| `ResolvedType` | Re-export of `buff_lang_types::Type` |

## WHERE TO LOOK

| Task | File |
|---|---|
| Change eval semantics (classification, composition, rustc flags) | `lib.rs::evaluate`, `run_full_program`, `classify` |
| Change type introspection | `lib.rs::type_of` |
| Change how stdout is captured | `lib.rs::run_full_program` (uses `Command::output`) |
| REPL-specific concerns | `buff-repl` crate, NOT here |

## CONVENTIONS (this crate only)

- **Single source of truth in `buff-lang-pipeline`.** `with_exe_extension`, the fast-linker detection (`linker_args(LinkerChoice::Auto)`), and the sccache probe (`compile_speed::sccache_available`) are IMPORTED from `buff_lang_pipeline` — the crate is clap/tokio-free, so the former inline mirrors were deleted (T22 dedup). Do NOT re-inline copies.
- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test code. Every fallible operation returns `EvalResult` with `diagnostic` set.
- **Temp files** go to `<tmp>/buff-eval/` with unique stems (`eval-<pid>-<n>`). Best-effort cleanup after capture.
- **Snippet classification** (`classify`): tries `parse_expression` first (bare expr), then `parse` (top-level decl or full program), then falls back to `BodyStmt`. Bare `print(...)` calls are detected so they aren't double-wrapped.
- **`type_of` is side-effect-free**: mutates no state, returns `None` on any error (no diagnostic surfaced).
- **Bare expressions** get auto-wrapped as `print(<expr>)` to capture the value into `EvalResult::value`.
