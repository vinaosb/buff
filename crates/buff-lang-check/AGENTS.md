# buff-lang-check

Standalone typecheck + lint crate behind `buff check`. Runs lex → parse → `TypeInferencer` → naming/style lints WITHOUT codegen — the fast "did my Buff program go wrong" surface. Also checks `.buffhtml` SFC script blocks and collects `@deprecated` call warnings.

## STRUCTURE

```
src/
├── lib.rs           # check_source (the standalone pipeline), CheckOutcome/CheckReport,
│                    #   ErrorFormat, run_check_file*, check_buffhtml_source,
│                    #   collect_deprecated_call_warnings
└── naming_lint.rs   # is_snake_case / is_pascal_case, lint_naming,
                     #   lint_common_mistakes, lint_tab_indentation
```

## WHERE TO LOOK

| Task | Location |
|---|---|
| Add a `buff check` lint | `naming_lint.rs` (new `lint_*` fn) + wire it into `lib.rs::check_source` |
| Change typecheck behavior | `lib.rs::check_source` (drives `buff_lang_types::TypeInferencer` directly — no codegen) |
| Add `.buffhtml` checking | `lib.rs::check_buffhtml_source` (buffhtml parser + codegen-buffhtml lowering, then same checks) |
| Change output format | `lib.rs::ErrorFormat` + `run_check_file_with_format` |
| Add a naming rule | `naming_lint.rs::lint_naming` / `lint_common_mistakes` |

## CONVENTIONS (this crate only)

- **No codegen.** This crate deliberately stops at type inference + lints. Anything that needs to see generated Rust belongs in codegen-rust or the pipeline crate.
- **Diagnostics, not errors.** Problems surface as `buff_lang_error::Diagnostic`s collected into `CheckReport`; the crate never panics on user input.
- **Recovering parse.** Uses `parse_recovering` (accumulating) so one syntax error doesn't hide the rest of the file's issues — mirrors the LSP's analysis path.
- **Depends on `buff-plugins`.** The plugins crate is shared with the CLI/LSP; this is a documented exception to the compiler-crate layering (see ARCHITECTURE direction rules).

## DEPENDENCIES

- Buff compiler crates: `buff-lang-ast`, `buff-lang-lexer`, `buff-lang-parser`, `buff-lang-types`, `buff-lang-error`, `buff-lang-buffhtml-parser`, `buff-lang-codegen-buffhtml`
- `buff-plugins` (shared plugin surface with CLI/LSP)
- `anyhow` (file-IO entry points)
