# buff-lsp

Language Server Protocol server for Buff. Built on `lsp-server` (rust-analyzer's JSON-RPC scaffold) + `lsp-types`. Implements the v1.2 MVP capabilities: diagnostics, hover, completion, goto-def (single-file), document symbols, formatting.

## STRUCTURE

```
src/
├── lib.rs        # Module wiring + public API + re-exports
├── main.rs       # Thin binary entry — calls server::run_stdio
├── server.rs     # stdio transport + main loop + debounced diagnostics
├── handlers.rs   # Pure LSP request handlers (hover/completion/.../formatting)
├── state.rs      # DocumentState: text + LineIndex + cached DocumentAnalysis
├── analysis.rs   # tokenize → parse_recovering → TypeInferencer; builds DocumentAnalysis
├── position.rs   # UTF-16-aware byte ↔ LSP-position conversion (LineIndex)
└── symbol.rs     # SymbolIndex + TypeBindingIndex for hover/completion/goto-def
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new LSP capability | `handlers.rs` (add handler) + `server.rs` (register capability + dispatch arm) |
| Tune diagnostics debounce | `server.rs::DEBOUNCE_IDLE` constant (300ms per T117 spec) |
| Change byte↔position mapping | `position.rs::LineIndex` |
| Add a new type of symbol to the index | `symbol.rs::SymbolIndex::add_*` + `analysis.rs::infer_decl` |
| Modify how a Buff diagnostic maps to LSP | `handlers.rs::diagnostic_to_lsp` |
| Re-route formatting through a different impl | `handlers.rs::formatting` (currently calls `buff_lang_cli::fmt::format_source`) |

## CONVENTIONS (this crate only)

- **NO `unwrap`/`expect`/`panic!` in non-test code.** The whole codebase rule applies here too. Use `Result<_, LspError>` + `thiserror::Error`. The `server.rs` main loop returns `Result<(), LspError>`; `main.rs` surfaces errors on stderr and exits non-zero.
- **Pure handlers, side-effecting server.** All `handlers::*` are pure functions on `&DocumentState`. The only I/O lives in `server.rs`. This split keeps the handlers trivially testable (drive `analyze::analyze` → `DocumentState::new` → call handler → assert on response — no subprocess, no threads).
- **Full reparse only.** v1.2 reparses the entire file on every `didChange`. Incremental parsing is a v2.0 task (see plan T117 "Must NOT do"). The `TextDocumentSyncKind::FULL` declared in `server_capabilities` reflects this.
- **UTF-16-aware positions.** LSP columns are UTF-16 code units per the spec. `position::LineIndex` is the authoritative converter — do NOT use `buff_lang_error::SourceMap::lookup` (it returns character-based columns and would misalign for astral characters).
- **Reuse `buff fmt`, don't reimplement.** `handlers::formatting` calls `buff_lang_cli::fmt::format_source` so the LSP and `buff fmt` produce byte-identical output.
- **Typecheck-only mode.** `analysis::analyze` runs `TypeInferencer` directly — NO Rust codegen. Distinct from the CLI's `buff check` (which surfaces codegen-time warnings). This is exactly the standalone typecheck the T117 spec asks for.
- **Tests in `tests/`.** Unit tests live inline (`#[cfg(test)] mod tests`); integration / protocol-conformance tests live in `tests/` (drive the handlers + `LineIndex` directly, not via subprocess — mirrors how `buff-lang-cli` tests drive the pipeline).

## DEPENDENCIES

- `lsp-server` 0.10 (rust-analyzer's JSON-RPC scaffold)
- `lsp-types` 0.97 (LSP 3.17 type set)
- `crossbeam-channel` (used by `lsp-server::Connection` + our debounce select!)
- `url` (for `Url`)
- `serde` / `serde_json` / `thiserror`
- Buff compiler crates: `buff-lang-ast`, `buff-lang-lexer`, `buff-lang-parser`, `buff-lang-types`, `buff-lang-error`
- `buff-lang-cli` (for `fmt::format_source` reuse — pulls in clap+tokio+toml as transitive deps; accepted trade-off per T117 "reuse, don't reimplement")

## LAUNCH (for VSCode extension T118)

The T118 VSCode extension bundles this binary and launches it via:

```json
"languageServer": {
  "command": "buff-lsp",
  "transport": "stdio"
}
```

No flags, no TCP transport for v1.2.
