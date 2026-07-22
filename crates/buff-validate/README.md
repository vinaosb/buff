# buff-validate

> Declarative schema validation for the **Buff** language. Pure-Rust MVP.

`buff-validate` wraps the [`validator`](https://crates.io/crates/validator) crate's free-standing trait validators (email / url / length / range / regex) and exports JSON Schema via [`serde_json`](https://crates.io/crates/serde_json). Inspired by pydantic / Zod / Joi. Buff code accesses validators via the `Validator` prelude type:

```buff
validator = Validator.new()
    .with_email(field: "email")
    .with_length(field: "name", min: 1, max: 80)
    .with_range(field: "age", min: 0, max: 150)

result = validator.validate(input: form_data)
match result:
    ok:
        print("passed")
    err(errors):
        print("failed: ${errors}")

schema = validator.to_json_schema()
```

**Status: experimental** (T29 v1.16 frameworks wave).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Validator` prelude type.

For direct Rust use:

```bash
cargo add buff-validate --path crates/buff-validate
```

## Quick start

```rust
use buff_validate::Validator;
use std::collections::HashMap;

fn main() -> Result<(), buff_validate::ValidationErrors> {
    let validator = Validator::new()
        .with_email("email")
        .with_length("name", 1, 80)
        .with_range("age", 0, 150)?;

    let mut good: HashMap<String, String> = HashMap::new();
    good.insert("email".to_string(), "alice@example.com".to_string());
    good.insert("name".to_string(), "Alice".to_string());
    good.insert("age".to_string(), "30".to_string());
    validator.validate(&good)?;

    println!("JSON schema: {}", validator.to_json_schema());
    Ok(())
}
```

## Public API

### `Validator` — declarative rule registry

| Method | Signature | Notes |
|---|---|---|
| `Validator::new` | `() -> Validator` | Empty rule set. |
| `validator.with_email` | `(self, field) -> Self` | Adds an email-format rule. |
| `validator.with_url` | `(self, field) -> Self` | Adds a URL-format rule. |
| `validator.with_length` | `(self, field, min, max) -> Result<Self, ValidationError>` | Adds a string-length rule. Errors if `min > max`. |
| `validator.with_range` | `(self, field, min, max) -> Result<Self, ValidationError>` | Adds an integer-range rule. Errors if `min > max`. |
| `validator.with_regex` | `(self, field, pattern) -> Result<Self, ValidationError>` | Adds a regex-match rule. Errors at registration on bad pattern (fail-fast). |
| `validator.validate` | `(&self, &HashMap<String, String>) -> Result<(), ValidationErrors>` | Runs all rules. Aggregates every failure. |
| `validator.to_json_schema` | `(&self) -> String` | Serializes rules to JSON Schema (Draft 2020-12). |
| `validator.rule_count` | `(&self) -> usize` | Number of registered rules. |

### Error types

| Type | Use |
|---|---|
| `ValidationError` | Single-rule failure (email/url/length/range/regex/bad-regex/missing-field/invalid-config/uncoercible/panic). |
| `ValidationErrors` | Aggregate across all rules in one `validate` call. Iterable, indexed, has `Display`. |

## Rule kinds

| Rule | Validates | JSON Schema output |
|---|---|---|
| `email` | RFC 5322 (via `validator::ValidateEmail`) | `{ "type": "string", "format": "email" }` |
| `url` | Absolute URL (via `validator::ValidateUrl`) | `{ "type": "string", "format": "uri" }` |
| `length(min, max)` | Char count within `[min, max]` | `{ "type": "string", "minLength": min, "maxLength": max }` |
| `range(min, max)` | Integer parsed from string within `[min, max]` | `{ "type": "integer", "minimum": min, "maximum": max }` |
| `regex(pattern)` | Pattern compiled at registration; `is_match` at validate | `{ "type": "string", "pattern": pattern }` |

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Validator`, `ValidationError`, `ValidationErrors`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `validate` borrows `&HashMap`; `to_json_schema` returns owned `String`. |
| R3 — Error mapping | Every fallible op returns `Result<T, ValidationError>` or aggregates via `ValidationErrors`. |
| R4 — Thread safety | `Validator` is `Send + Sync` (rules own `String` + `regex::Regex` — both `Send + Sync`). |
| R5 — Lifetime hiding | No public lifetime parameters. `Validator` owns every rule. |
| R6 — Panic boundary | `validate` / `to_json_schema` / `with_regex` wrap bodies in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-validate
cargo clippy -p buff-validate --all-targets -- -D warnings
cargo fmt -p buff-validate --check
```

Tests are hermetic: rule fixtures are constructed inline (no network, no file fixtures). Snapshots via `insta`.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
