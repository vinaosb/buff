# buff-template

HTML templating for the Buff language. Pure-Rust MVP wrapping the [`handlebars`](https://docs.rs/handlebars) crate. Runtime-only (no compile-time macros). Per T19 spec: `Template.from_path(path)`, `Template.from_string(source)`, `template.render(context)` returns String.

**Status: experimental** (T19 v1.13 frameworks wave 4).

## STRUCTURE

```
buff-template/
├── Cargo.toml            # handlebars + serde_json + thiserror + insta deps
├── src/
│   ├── lib.rs            # Template (main surface, ~120 LOC)
│   └── error.rs          # TemplateError enum (~30 LOC)
├── examples/
│   ├── hello_template.rs       # basic variable substitution
│   └── loop_template.rs        # loop + conditional
└── tests/
    ├── api.rs            # 15 unit tests (constructors, render, errors)
    └── render.rs         # 10 render-focused tests (nested, escaping, helpers)
```

Total: ~400 LOC (well under the 1500 LOC T19 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new constructor | `src/lib.rs` (add `pub fn` on `Template`) + test in `tests/api.rs` |
| Add a new error variant | `src/error.rs` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (3 functions, ≤20 cap)

### `Template` (3 functions)
- Constructors: `from_string`, `from_path`
- Instance: `render`

## CONVENTIONS

- **Pure-Rust only**: handlebars is pure-Rust, no native deps, no cc-rs.
- **Runtime-only**: NO compile-time template compilation (deferred to v1.18+).
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code.
- **catch_unwind boundary**: `from_string` / `from_path` / `render` wrap their bodies in `catch_unwind` per FFI guide R6.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `handlebars` | Upstream templating engine. `buff-template` is a safe wrapper; never re-exports `handlebars::*` types directly. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Template` + `PreludeAssocFn::{FromString, FromPath}` + `PreludeInstanceFn::Render`. `ty.rs` has the `Type::Template` variant. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::buff_type_to_syn` has the `Type::Template => "buff_template::Template"` arm. `lower_prelude_type_assoc_fn` has the `(Template, FromString)` / `(Template, FromPath)` arms. `lower_prelude_type_instance_fn` has the `(Template, Render)` arm. `program_uses_namespace("Template")` records `buff-template` + `handlebars` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |
