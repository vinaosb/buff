# buff-lang-debug-info

T24 — Buff-span stack traces via `.buffmap` source-map sidecar + `std::panic` hook. Stability: **experimental** (Buff SDK 2.0).

## STRUCTURE

```
src/
├── lib.rs         # SourceMap + BuffLocation + FunctionAnchor public types
├── capture.rs     # build_source_map: AST + formatted Rust → SourceMap (post-format scan)
├── format.rs      # .buffmap JSON format (BuffMapFile + LineMapping + FunctionMapping + serialize/deserialize)
└── panic_hook.rs  # install_panic_hook + remap_panic_backtrace + RUST_BACKTRACE=1 escape hatch
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new anchor kind (struct, let-binding) | `capture.rs::build_source_map` (extend the `Decl` walk) |
| Change the `.buffmap` JSON schema | `format.rs` (bump `MAP_FORMAT_VERSION`, add field) |
| Tune the panic hook output format | `panic_hook.rs::buff_panic_handler` |
| Change `.buffmap` discovery (env var / exe path) | `panic_hook.rs::resolve_buff_map_path` |
| Add offline `buff backtrace` post-processing | `panic_hook.rs::remap_panic_backtrace` (already exposed; CLI wraps it) |

## CONVENTIONS

- **NO `unwrap`/`expect`/`panic!` in non-test code** (project hard rule; the panic hook itself runs INSIDE a panic, so any panic inside it would abort the process).
- **All maps `BTreeMap`, all sets `BTreeSet`** — never `HashMap`/`HashSet` (project hard rule, so `.buffmap` JSON is byte-identical across runs).
- **`RUST_BACKTRACE=1` always preserved** — the Buff trace is additive; users can always drill into Rust internals.
- **Sidecar file, NOT embedded in binary** — keeps codegen focused on program logic; debug info lives in its own file.
- **Forward + reverse lookup in one file** — both directions are projections of the same `SourceMap`; consumers compute the reverse on demand.
- **Stability: experimental** — `.buffmap` JSON schema may change in a minor bump. The `MAP_FORMAT_VERSION` constant gates consumer compatibility.
- **`SourceMap` is a NEW type** — distinct from `buff_lang_error::SourceMap` (which is the diagnostic-time map). This type carries `.buffmap`-serializable data only.
- **Tests**: 11+ unit tests across the 4 src files; integration tests live in `tests/`.

## PIPELINE WIRING

```text
buff-lang-codegen-rust::generate_rust(&[Decl]) -> String (formatted Rust)
    │
    ▼  build_source_map(decls, &rust, buff_path, &buff_source)
buff-lang-debug-info::SourceMap
    │
    ▼  serialize_to_string(&map) -> String
<binary>.buffmap   (written alongside <binary>)
    │
    ▼  install_panic_hook()  (called by buff-lang-runtime::init)
std::panic::set_hook(...)
    │
    ▼  on panic: remap_panic_backtrace(&map) -> BuffTrace
stderr (Buff trace first, Rust trace second when RUST_BACKTRACE=1)
```

## `.buffmap` JSON SCHEMA (v1)

```json
{
  "version": 1,
  "buff_file": "examples/debug/panic_demo.buff",
  "rust_file": "panic_demo.rs",
  "source_id": 0,
  "function_mappings": [
    {
      "buff_name": "helper",
      "buff_span_start": 30, "buff_span_end": 80,
      "buff_source_id": 0, "buff_line": 1, "buff_col": 1,
      "rust_start_line": 3, "rust_end_line": 5
    }
  ],
  "line_mappings": [
    { "rust_line": 3, "buff_line": 1, "buff_col": 1,
      "buff_span_start": 30, "buff_span_end": 80,
      "buff_source_id": 0, "buff_name": "helper" }
  ]
}
```

## DEPS

`serde`, `serde_json`, `thiserror`, `buff-lang-error`, `buff-lang-ast` (workspace). Pure-Rust, no native C — matches the "no C library, no Docker" hard rule.
