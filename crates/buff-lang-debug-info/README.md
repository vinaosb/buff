# buff-lang-debug-info

Buff-span stack traces via `.buffmap` source-map sidecar + `std::panic` hook.

Part of the Buff language compiler — T24 from the v1.13 *Foundations* roadmap. Stability: **experimental** (Buff SDK 2.0).

## Why

Every scripting language that lowers to a lower-level runtime (TypeScript, Python, Dart, …) gives users stack traces in their own language. Buff lowers to Rust, so without a translation layer a panicked Buff program shows the user generated `<file>.rs:LINE:COL` paths and Rust-internal frames — useless for debugging the Buff source.

This crate ships that translation layer: capture Rust ↔ Buff span mappings at codegen time, write them to a `.buffmap` sidecar next to the compiled binary, then read the sidecar at runtime inside a `std::panic::set_hook` interceptor to remap each Rust backtrace frame to its Buff source location.

## Usage

### Codegen sidecar emission

```rust
use buff_lang_debug_info::{build_source_map, format};

let rust_source = buff_lang_codegen_rust::generate_rust(&decls)?;
let map = build_source_map(&decls, &rust_source, buff_path, &buff_source);
format::write_to_file(&map, &buffmap_path)?;
```

### Runtime panic hook

```rust
buff_lang_debug_info::install_panic_hook();
// On panic: reads <current_exe>.buffmap, walks the Rust backtrace,
// prints a Buff-source-mapped trace. RUST_BACKTRACE=1 still works
// as an escape hatch (full Rust trace printed AFTER the Buff trace).
```

### `.buffmap` JSON schema (v1)

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

## Pipeline

```text
buff-lang-codegen-rust::generate_rust(&[Decl]) -> String
    │
    ▼  build_source_map(decls, &rust, buff_path, &buff_source)
SourceMap
    │
    ▼  serialize_to_string(&map) -> JSON String
<binary>.buffmap   (sidecar file)
    │
    ▼  install_panic_hook()  (BUFF_MAP_PATH or <exe>.buffmap)
std::panic::set_hook(...)
    │
    ▼  on panic: remap_panic_backtrace() -> BuffTrace
stderr (Buff trace first; Rust trace second when RUST_BACKTRACE=1)
```

## Escape hatch

`RUST_BACKTRACE=1` is ALWAYS preserved. The Buff trace is additive; the full Rust backtrace is additionally printed AFTER the Buff trace when the env var is set, so advanced users can drill into Rust internals when debugging interop issues.

## Determinism

All map/set types are `BTreeMap` / `BTreeSet` (never `HashMap`/`HashSet`). The same Buff program produces the same `.buffmap` JSON byte-for-byte across runs — a project hard rule.

## License

MIT OR Apache-2.0, same as the rest of the Buff compiler.
