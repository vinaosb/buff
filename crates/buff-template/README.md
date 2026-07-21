# buff-template

> HTML templating for the **Buff** language. Pure-Rust MVP (runtime-only).

`buff-template` wraps the mature [`handlebars`](https://docs.rs/handlebars) crate behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses templates via the `Template` prelude type:

```buff
let t = Template.from_string("Hello {{name}}!")
let out = t.render("{\"name\": \"Buff\"}")
print(out)  // "Hello Buff!"
```

**Status: experimental** (T19 v1.13 frameworks wave 4).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Template` prelude type.

For direct Rust use:

```bash
cargo add buff-template --path crates/buff-template
```

## Quick start

```rust
use buff_template::Template;

fn main() {
    let t = Template::from_string("Hello {{name}}!").expect("compile");
    let out = t.render(r#"{"name": "World"}"#).expect("render");
    assert_eq!(out, "Hello World!");
}
```

## Public API

### `Template` — compiled HTML template

| Method | Signature | Notes |
|---|---|---|
| `Template::from_string` | `(source) -> Result<Template, TemplateError>` | Compile a template source string. `catch_unwind` boundary. |
| `Template::from_path` | `(path) -> Result<Template, TemplateError>` | Load and compile a `.html` file. |
| `template.render` | `(context_json) -> Result<String, TemplateError>` | Render with JSON context. |

### Template syntax

Uses standard handlebars syntax:

- `{{ variable }}` — variable substitution
- `{% if cond %}...{% endif %}` — conditionals
- `{% for item in list %}...{% endfor %}` — loops
- `{{! comment }}` — comments
- `{{#if cond}}...{{/if}}` — block helpers

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Template`, `TemplateError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `from_string`/`from_path` return owned `Template`. `render` returns owned `String`. |
| R3 — Error mapping | Every fallible op returns `Result<T, TemplateError>`. handlebars errors mapped via `From`. |
| R4 — Thread safety | `Template` is `Send + Sync` (wraps `handlebars::Handlebars` which is itself `Send + Sync`). |
| R5 — Lifetime hiding | No public lifetime parameters. `Template` owns its `Handlebars` registry. |
| R6 — Panic boundary | `from_string` / `from_path` / `render` wrap bodies in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-template
cargo clippy -p buff-template --all-targets -- -D warnings
cargo fmt -p buff-template --check
```

Tests are hermetic: no external template fixtures needed. All templates are inline strings.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
