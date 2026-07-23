# buff-dap

Debug Adapter Protocol server for the Buff language. Translates DAP
requests from editors (VSCode via CodeLLDB / lldb-dap / vscode-lldb) into
Rust-debugger sessions, applying Buff's `SourceMap` for `.buff` ↔ `.rs`
line translation. Shipped in v1.10.0 (T60/T136).

## STRUCTURE

```
src/
├── lib.rs          # Public API + architecture diagram + limitations.
├── error.rs        # DapError enum (thiserror).
├── protocol.rs     # JSON-RPC over stdio framing (Content-Length header).
├── server.rs       # run_session — main read/dispatch/write loop.
├── translation.rs  # setBreakpoints (buff→rust) + stackTrace (rust→buff) via SourceMap.
└── backend.rs      # BackendProcess — spawn lldb-dap/codelldb/vscode-lldb.
tests/
├── translation.rs
└── wire_tests.rs
```

## PUBLIC API

The crate is a binary + library (dual `bin`/`lib`). Entry point:
`buff debug <file>` (CLI subcommand in `buff-lang-cli`) spawns the DAP
session over stdio.

## WHERE TO LOOK

| Task | File |
|---|---|
| Change request/response framing | `src/protocol.rs` |
| Change session loop / dispatch | `src/server.rs` |
| Change `.buff`↔`.rs` line mapping | `src/translation.rs` |
| Change backend debugger spawning | `src/backend.rs` |
| Change error variants | `src/error.rs` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (project hard rule).
- **Hand-rolled protocol** (not the `dap` crate) — DAP is small and
  mirrors LSP closely (which `buff-lsp` consumes via `lsp-types`). Avoids
  a new workspace dep and the cc-rs transitive failure class that killed
  `chumsky`/`logos`.
- **Translation-only proxy:** only `setBreakpoints` (editor→backend) and
  `stackTrace` (backend→editor) are translated via `SourceMap`. All other
  requests (`initialize`/`launch`/`continue`/`next`/`stepIn`/…/`evaluate`)
  pass through verbatim.
- **BTreeMap/BTreeSet only** where collections are used.

## OUT OF SCOPE (deferred to v2.0+)

- GPU kernel debugging.
- Watch expressions / reverse debugging.
- Per the T60/T136 task spec these are explicitly out of scope.

## DEPS

- `buff-lang-error` (workspace) — `SourceMap` / `Span` for line mapping.
- `serde` / `serde_json` (workspace) — DAP JSON-RPC messages.
- Dev: `insta`.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T60 / T136.
- Evidence: `.sisyphus/evidence/task-136-debugger.txt`.
