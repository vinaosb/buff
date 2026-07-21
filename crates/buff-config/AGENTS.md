# buff-config

Layered configuration for the Buff language (viper-equivalent). Pure-Rust MVP wrapping the [`figment`](https://docs.rs/figment/latest/figment/) crate. Provides layered config: defaults → file (TOML/YAML/JSON) → env vars → CLI args. Hot reload via `notify` file watcher.

**Status: experimental** (T30 v1.13 frameworks wave 1).

## STRUCTURE

```
buff-config/
├── Cargo.toml            # figment + notify + thiserror + serde deps
├── src/
│   ├── lib.rs            # Config + ConfigWatcher (main surface, ~280 LOC)
│   └── error.rs          # ConfigError enum (~50 LOC)
├── examples/
│   ├── config_basic.rs        # defaults + file + env + args
│   ├── config_hot_reload.rs   # file watch + callback
│   └── config/
│       ├── config_basic.buff  # Buff-side forward-decl (matches .rs)
│       └── config_hot_reload.buff  # Buff-side forward-decl (matches .rs)
└── tests/
    └── core.rs           # 12 unit tests (~200 LOC)
```

Total: ~530 LOC (well under the 2000 LOC T30 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new config provider | `src/lib.rs` (add `pub fn load_*` on `Config`) + test in `tests/core.rs` |
| Add a new error variant | `src/error.rs` + `From` impl if it wraps an underlying error |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (12 functions, ≤20 cap)

### `Config` (12 functions)
- Constructors: `new`
- Providers: `set_default`, `load_file`, `load_env`, `load_args`
- Accessors: `get`, `get_int`, `get_float`, `get_bool`
- Lifecycle: `watch`

### `ConfigWatcher` (0 functions — drop handle)

## CONVENTIONS

- **Pure-Rust only**: figment is pure-Rust (no native deps). notify is pure-Rust on all platforms.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. All getters return `Option<T>`.
- **catch_unwind boundary**: `load_file` / `watch` wrap their bodies in `catch_unwind` per FFI guide R6.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `figment` | Upstream config provider. `buff-config` is a safe wrapper; never re-exports `figment::*` types directly. |
| `notify` | File watcher for hot reload. Already pinned at workspace level for T131. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Config` + `PreludeAssocFn` variants. `ty.rs` has the `Type::Config` variant. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::buff_type_to_syn` has the `Type::Config => "buff_config::Config"` arm. `lower_prelude_type_assoc_fn` has the Config arms. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **Layered precedence**: defaults (lowest) → file → env vars → CLI args (highest). Last provider to set a key wins.
- **Hot reload**: `Config::watch` spawns a background thread that reloads the file and invokes the callback on every `Modify` event. Drop the `ConfigWatcher` handle to stop.
- **CLI arg parsing**: supports `--key=value` and `--key value` forms. Only `--` prefixed args are parsed.
