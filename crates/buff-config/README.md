# buff-config

> Layered configuration for the **Buff** language (viper-equivalent).

`buff-config` wraps the mature [`figment`](https://docs.rs/figment/latest/figment/) crate behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses config via the `Config` prelude type:

```buff
let cfg = Config.new()
cfg.set_default("port", 8080)
cfg.load_file("app.toml")
cfg.load_env("BUFF")
cfg.load_args(["--host=0.0.0.0"])

let port = cfg.get_int("port")  # 8080 (from default, overridable)
```

**Status: experimental** (T30 v1.13 frameworks wave 1).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Config` prelude type.

For direct Rust use:

```bash
cargo add buff-config --path crates/buff-config
```

## Quick start

```rust
use buff_config::Config;

fn main() -> Result<(), buff_config::ConfigError> {
    let cfg = Config::new();
    cfg.set_default("port", 8080);
    cfg.load_file("app.toml")?;
    cfg.load_env("BUFF");
    cfg.load_args(&["--host=0.0.0.0".to_string()]);

    assert_eq!(cfg.get_int("port"), Some(8080));
    assert_eq!(cfg.get("host"), Some("0.0.0.0".to_string()));
    Ok(())
}
```

## Public API

### `Config` — layered configuration store

| Method | Signature | Notes |
|---|---|---|
| `Config::new` | `() -> Config` | Empty config, no providers. |
| `cfg.set_default` | `(key, value) -> ()` | Lowest precedence. |
| `cfg.load_file` | `(path) -> Result<(), ConfigError>` | TOML/YAML/JSON. `catch_unwind` boundary. |
| `cfg.load_env` | `(prefix) -> ()` | Strips prefix from keys. |
| `cfg.load_args` | `(&[String]) -> ()` | `--key=value` or `--key value`. |
| `cfg.get` | `(key) -> Option<String>` | String value. |
| `cfg.get_int` | `(key) -> Option<i64>` | Integer value. |
| `cfg.get_float` | `(key) -> Option<f64>` | Float value. |
| `cfg.get_bool` | `(key) -> Option<bool>` | Bool value. |
| `cfg.watch` | `(path, callback) -> Result<ConfigWatcher, ConfigError>` | Hot reload. |

## Layered precedence (last wins)

1. Defaults (lowest priority)
2. File (TOML/YAML/JSON)
3. Environment variables
4. CLI args (highest priority)

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Config`, `ConfigError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `Config` owns its `Figment`. All getters return owned `Option<T>`. |
| R3 — Error mapping | Every fallible op returns `Result<T, ConfigError>`. `figment::Error` auto-converts via `From`. |
| R4 — Thread safety | `Config` is `Send + Sync` (wraps `figment::Figment` which is `Send + Sync`). |
| R5 — Lifetime hiding | No public lifetime parameters. `Config` owns its `Figment`. |
| R6 — Panic boundary | `load_file` / `watch` wrap bodies in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-config
cargo clippy -p buff-config --all-targets -- -D warnings
cargo fmt -p buff-config --check
```

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
