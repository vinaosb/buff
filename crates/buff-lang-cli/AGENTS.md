# buff-lang-cli

Binary + library entry point. Composition root for the entire pipeline. 21 subcommands.

## STRUCTURE

```
src/
├── main.rs               # 87 lines — thin in responsibility (no pipeline logic), full match dispatch for 21 subcommands
├── lib.rs                # 44 lines — re-exports modules for integration tests
├── cli.rs                # 550 lines — clap::Parser + Command enum (21 variants) + JupyterCmd/UiCmd nested enums
├── pipeline.rs           # 867 lines — compile_to_rust + compile_rust_to_exe + compile_buffhtml_to_rust + BuildMode
├── fmt.rs                # 1839 lines — T57 Formatter with comment-preservation
├── check.rs              # 520 lines — T55 standalone typecheck (SHIPPED): lex→parse→TypeInferencer→naming_lint, no codegen
├── config.rs             # 1063 lines — BuffConfig (buff.toml serde), generate_cargo_toml, T122/T127 workspace+registry deps
├── error_mapper.rs       # 544 lines — translate_rustc_errors, translate_panic, filter_backtrace, .buffhtml-aware
├── scaffold.rs           # 382 lines — TemplateKind (Binary/Lib/Server/Gpu/Desktop/Workspace)
├── naming_lint.rs        # 336 lines — is_snake_case, is_pascal_case, lint_naming
├── test_runner.rs        # 391 lines — discover_test_names, parse_report
├── commands/             # 23 files: mod.rs + 22 subcommand modules (see subcommands below)
├── ui_dev/               # 7 files — T131 dev server: WebSocket live reload for `buff ui dev`
└── coverage/             # 6 files — T137 llvm-cov Rust-line → .buff source-line mapping
```

## 21 SUBCOMMANDS

`add` `build` `check` `clean` `deps` `fmt` `init` `install` `jupyter` `login` `new` `outdated` `publish` `registry` `repl` `run` `ssr` `test` `ui_build` `ui_dev` `ui_new` `update`

Note: `commands/ui_dev.rs` is a thin dispatch wrapper around `ui_dev/mod.rs` (which is both module root AND handler).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add subcommand | `cli.rs` (Command enum variant) + `commands/<name>.rs` + `main.rs` match arm |
| Change rustc invocation | `pipeline.rs::compile_rust_to_exe` |
| Change typecheck behavior | `check.rs` |
| Change formatting | `fmt.rs` |
| Inspect intermediate Rust before rustc | `pipeline.rs::compile_to_rust` (returns String) |
| Map an error variant to user message | `error_mapper.rs` |
| Modify project template | `scaffold.rs` |
| Change buff.toml parsing | `config.rs` |
| Change UI dev server | `ui_dev/mod.rs` |

## CONVENTIONS

- **NEVER put pipeline logic in `main.rs`** — it's thin in responsibility (no pipeline code), just dispatch. Tests drive `buff_lang_cli::commands::*` directly without subprocess.
- **Both bin + lib targets** in `Cargo.toml`. Lib is what tests import.
- **Standalone typecheck SHIPPED**: `buff check` (T55) runs TypeInferencer without codegen. The root AGENTS.md statement "standalone typecheck is post-v1.0" is OUTDATED.
- **Error path**: every `Result::Err` from pipeline must flow through `error_mapper` before reaching the user. Don't `eprintln!` raw errors.
- **Pipeline order**: `read_to_string` → `tokenize` → `parse` → `generate_rust` → `compile_rust_to_exe`. `.buffhtml` path runs in parallel with SpanMap.
- **Tests**: 23 files in `tests/`, 10 snapshots.
