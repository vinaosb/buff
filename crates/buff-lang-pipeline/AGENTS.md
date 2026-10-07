# buff-lang-pipeline

Pipeline orchestration crate: drives `.buff` / `.buffhtml` source through lex → parse → codegen → `rustc` to produce native executables. Exposed as a LIBRARY (dual-consumed by `buff-lang-cli` and `buff-eval`/REPL/Jupyter) — deliberately **clap-free and tokio-free** so downstream tooling crates can depend on it without pulling the CLI's argument/async stacks.

## STRUCTURE

```
src/
├── lib.rs             # Compile entry points (compile_to_rust, compile_rust_to_exe,
│                      #   compile_buffhtml_to_rust, compile_buffhtml_rust_to_exe),
│                      #   BuildMode/BackendChoice/LinkerChoice/DebugInfoChoice enums,
│                      #   rustc flag builders (rustc_fast/release/minimal/pgo flags),
│                      #   profile TOML emitters, with_exe_extension, profiling injection
├── rustc_invoke.rs    # rustc command construction + PATH probing
│                      #   (on_path, cranelift_available, target_is_installed,
│                      #   configure_rustc_command, configure_detect_races)
├── error_mapper.rs    # rustc stderr → Buff-span diagnostics translation
│                      #   (translate_rustc_errors, translate_panic, classify_rustc_error,
│                      #   annotate_with_buff_codes, buffhtml variant)
├── compile_speed.rs   # Compile-speed tooling: FastLinker enum, sccache probing,
│                      #   disk cache (read/write_cache), BenchTier synthetic programs
└── incremental.rs     # salsa-backed incremental queries (SourceFile, parse_file,
                       #   typecheck_file, BuffDatabase)
```

## WHERE TO LOOK

| Task | Location |
|---|---|
| Add a compile option | `lib.rs::BuildMode` (or a new choice enum + `*_from_str`) + the `compile_rust_to_exe*` flag assembly |
| Change rustc invocation | `rustc_invoke.rs::configure_rustc_command` |
| Map a rustc error to Buff spans | `error_mapper.rs::translate_rustc_errors` (+ `classify_rustc_error` for ErrorCode tagging) |
| Add a fast-linker/sccache behavior | `compile_speed.rs::FastLinker` + `lib.rs::LinkerChoice` |
| Add an incremental query | `incremental.rs` (salsa `#[salsa::tracked]` fns on `BuffDatabase`) |
| Add a profiling mode | `lib.rs::ProfileMode` + `inject_profiling` (syn/quote based) |
| Add a rustc flag preset | `lib.rs::rustc_*_flags` family |
| Map exe path per-OS | `lib.rs::with_exe_extension` (single source of truth) |

## CONVENTIONS (this crate only)

- **NO clap, NO tokio.** This is a hard design constraint: `buff-eval` (REPL/Jupyter) depends on this crate, and those crates must not pull the CLI's clap stack or tokio runtime transitively. Any new dependency must respect that.
- **Single source of truth for rustc arg/exe-path logic.** `with_exe_extension`, the linker resolution, and the flag builders live ONLY here. Tooling crates import them — do NOT re-inline copies.
- **Errors via `anyhow::Result`.** Pipeline functions return `anyhow::Result`; user-facing Buff diagnostics come from `error_mapper.rs`, not `unwrap` chains.
- **No raw-string Rust codegen.** Generated Rust comes from codegen-rust (`syn` → `prettyplease`); this crate only orchestrates and post-processes it (e.g. profiling injection via `syn`/`quote`, inline_script_block via `buff-lang-codegen-buffhtml`).

## DEPENDENCIES

- Buff compiler crates: `buff-lang-ast`, `buff-lang-lexer`, `buff-lang-parser`, `buff-lang-types`, `buff-lang-error`, `buff-lang-codegen-rust`, `buff-lang-codegen-buffhtml`, `buff-lang-buffhtml-parser`
- `syn` / `quote` / `proc-macro2` (profiling guard injection + inline_script_block transformation)
- `prettyplease` (formatting of post-codegen transformations)
- `anyhow` (pipeline error type)
- `sha2` (cache keys)
- `salsa` (incremental parse/typecheck queries)

Consumers: `buff-lang-cli` (thin re-export layer) and `buff-eval` (REPL/Jupyter compile path).
