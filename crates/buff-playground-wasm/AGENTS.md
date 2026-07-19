# buff-playground-wasm

Wasm-transpile-only entry point for the Buff playground (T114).
Compiles the lexer + parser + codegen-rust into a `cdylib` that JS can
call via `wasm-bindgen`. NO runtime, NO GPU, NO rustc — those don't
target `wasm32-unknown-unknown`.

## STRUCTURE

```
src/
└── lib.rs          # 200+ lines — wasm-bindgen entry `transpile(src) -> String`
                    #   + wire-format helpers (success_json / error_json)
                    #   + 7 host-side unit tests on the wire shape
tests/
└── (none — keep wire-shape tests inline in src/lib.rs)
```

## WHERE TO LOOK

| Task | Location |
|---|---|
| Change the wire format | `src/lib.rs` — `success_json` / `error_json` / the doc comment at the top |
| Add a new compiler phase (e.g. typecheck) | `src/lib.rs::transpile` — add a phase between parse and codegen |
| Add a new exported fn | `src/lib.rs` — annotate with `#[wasm_bindgen]` |

## CONVENTIONS (this crate only)

- **CRATE-TYPE: `["cdylib", "rlib"]`** — `cdylib` for the wasm bundle,
  `rlib` so `cargo test` can link the unit tests on the host.
- **NO `unwrap`/`expect`/`panic!` at the wasm boundary.** Every compiler
  call is `match`ed; panics are caught by `console_error_panic_hook` and
  surfaced as `{ok:false,...}` JSON. This is the repo hard rule, applied
  extra-strictly here because a panic in wasm is a JS exception with no
  Rust stack info.
- **Wire format: JSON string, not a struct.** `transpile(src) -> String`
  returns a JS string (zero-copy across the wasm boundary). The string
  is JSON in one of two shapes:
  ```json
  {"ok":true, "rust":"fn main() { ... }"}
  {"ok":false,"error":"parse error: ...","line":3,"col":7}
  ```
  `line`/`col` are 1-based, character-counted (multi-byte UTF-8 = 1 col).
  See `tests/playground.spec.cjs` for the contract tests.
- **Phase prefix in error message**: `lex error: …` / `parse error: …`
  / `codegen error: …` — mirrors the host `pipeline.rs` so JS can
  pattern-match if desired.
- **Determinism**: same input → byte-identical output. The URL-share
  feature depends on this — the playground state is pure-derived from
  the source.
- **Tests**: 7 inline tests on the wire format (success shape, parse
  error shape, lex error shape, UTF-8 safety, determinism, internal
  error helper). Run on host via `cargo test -p buff-playground-wasm`.

## BUILD

```bash
# Host (smoke-build + run unit tests):
cargo test -p buff-playground-wasm

# Wasm bundle (writes playground/pkg/ via wasm-bindgen):
cargo build -p buff-playground-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir playground/pkg --out-name buff_playground \
    target/wasm32-unknown-unknown/release/buff_playground_wasm.wasm
```

See [`playground/README.md`](../../../playground/README.md) for the full
build + deploy + test workflow.

## NOTES

- **Why `console_error_panic_hook`?** wasm panics normally surface as
  opaque JS exceptions with no Rust stack info. The hook installs a
  global panic handler that prints the Rust backtrace to `console.error`
  BEFORE the wasm runtime traps. Called via `set_once()` on first
  `transpile()` call — idempotent, so safe to call from every entry
  point.
- **Why `serde_json` instead of `js_sys::Object`?** Three reasons:
  1. JSON is a stable wire format — easy to consume from any JS
     framework, no FFI type churn.
  2. The output is a `String` (cheap across the wasm boundary; one
     allocation), not a complex `js_sys` graph.
  3. Tests can call `transpile()` on the HOST (via the `rlib` target)
     and `serde_json::from_str` the result — no browser required.
- **`wasm-bindgen` version pinning**: the workspace dependency floats
  within `0.2.x`. The `wasm-bindgen-cli` version MUST byte-match the
  version Cargo resolved into the .wasm. See `playground/README.md` →
  "Pinning wasm-bindgen" for the procedure.
- **`wgpu` constraint**: `buff-lang-runtime` (not used here) pulls in
  `wgpu v26`, which requires `wasm-bindgen >= 0.2.100`. The workspace
  resolver therefore picks `0.2.126+`. If you try to pin the workspace
  dep below `0.2.100`, the resolver fails — install the matching CLI
  instead.
