# buff-lang-cli

Binary + library entry point for the Buff transpiler. Composition root for the entire pipeline.

## STRUCTURE

```
src/
├── main.rs           # 20 lines, thin: parses clap Cli, dispatches to commands::*
├── lib.rs            # Library root — re-exports modules for integration tests
├── cli.rs            # clap::Parser + Command enum (Build, Run, New, Init)
├── pipeline.rs       # compile_to_rust + compile_rust_to_exe (rustc invocation)
├── error_mapper.rs   # Maps internal errors → user-facing diagnostics
├── scaffold.rs       # `buff new` / `buff init` project templating
└── commands/
    ├── mod.rs        # Module exports
    ├── build.rs      # `buff build` — full pipeline, emits .rs
    ├── run.rs        # `buff run` — full pipeline + rustc + exec
    ├── new.rs        # `buff new <NAME>` — scaffold new project
    └── init.rs       # `buff init` — scaffold in cwd
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add subcommand | `cli.rs` (Command enum) + `commands/<name>.rs` + `main.rs` dispatch arm |
| Change rustc invocation | `pipeline.rs::compile_rust_to_exe` |
| Inspect intermediate Rust before rustc | `pipeline.rs::compile_to_rust` (returns String) |
| Map an error variant to user message | `error_mapper.rs` |
| Modify project template | `scaffold.rs` |

## CONVENTIONS (this crate only)

- **NEVER put pipeline logic in `main.rs`** — it must stay thin (20 lines) so tests drive `buff_lang_cli::commands::*` directly without subprocess.
- **Both bin + lib targets** in `Cargo.toml` — keep them in sync. Lib is what tests import.
- **Integration tests** in `tests/` (6 files: `cli_build_tests`, `cli_run_tests`, `error_mapping_tests`, `integration_tests`, `milestone_tests`, `scaffold_tests`). **Milestone tests** (`test_example_ola`, `test_example_fibonacci`) are v0.1 acceptance gates — do not skip or weaken.
- **rustc invocation** uses `--edition 2021`. Match the workspace edition.
- **Type-checking is INSIDE codegen** for v0.1 — CLI does not run a separate typecheck pass. v0.5 will add it.
- **Error path**: every `Result::Err` from pipeline must flow through `error_mapper` before reaching the user. Don't `eprintln!` raw errors.
- **Pipeline order** (see `lib.rs` doc comment): `read_to_string` → `tokenize` → `parse` → `generate_rust` → `compile_rust_to_exe`.
