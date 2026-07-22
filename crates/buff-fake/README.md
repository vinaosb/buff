# buff-fake

Fake data generation for the Buff language. Pure-Rust MVP wrapping the [`fake`](https://docs.rs/fake/latest/fake/) crate. Provides `Faker.name()`, `Faker.email()`, `Faker.address()`, `Faker.phone()`, `Faker.uuid()`, `Faker.lorem(words)`, `Faker.int(min, max)`, `Faker.datetime(range)`. Locales: en-US, pt-BR.

**Status: experimental** (T37 v1.13 frameworks wave 2).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Faker` prelude type.

For direct Rust use:

```bash
cargo add buff-fake --path crates/buff-fake
```

## Quick start

```rust
use buff_fake::{Faker, FakerLocale};

fn main() {
    let mut faker = Faker::with_seed(FakerLocale::EnUs, 42);
    println!("Name:  {}", faker.name());
    println!("Email: {}", faker.email());
    println!("Int:   {}", faker.int(1, 100));
}
```

## Public API

### `Faker` — fake data generator

| Method | Signature | Notes |
|---|---|---|
| `Faker::new` | `() -> Faker` | Default locale (en-US), random seed |
| `Faker::with_locale` | `(FakerLocale) -> Faker` | en-US or pt-BR |
| `Faker::with_seed` | `(FakerLocale, Int) -> Faker` | Reproducible output |
| `faker.name` | `() -> String` | Random full name |
| `faker.email` | `() -> String` | Random email address |
| `faker.address` | `() -> String` | Random street address |
| `faker.phone` | `() -> String` | Random phone number |
| `faker.uuid` | `() -> String` | Random UUID v4 |
| `faker.lorem` | `(Int) -> String` | Lorem ipsum with N words |
| `faker.int` | `(Int, Int) -> Int` | Random int in [min, max] |
| `faker.datetime` | `(String, String) -> Result<String, FakerError>` | Random datetime in range (RFC 3339) |

### `FakerLocale`

| Variant | Description |
|---|---|
| `FakerLocale::EnUs` | English (United States) |
| `FakerLocale::PtBr` | Portuguese (Brazil) |

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Faker`, `FakerLocale`, `FakerError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | All methods return owned `String` / `i64`. |
| R3 — Error mapping | `datetime` returns `Result<String, FakerError>`. |
| R4 — Thread safety | `Faker` is `Send + Sync` (no interior mutability). |
| R5 — Lifetime hiding | No public lifetime parameters. All returns are owned. |
| R6 — Panic boundary | Every public method wraps body in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-fake
cargo clippy -p buff-fake --all-targets -- -D warnings
cargo fmt -p buff-fake --check
```

Tests are hermetic: seeded RNG produces deterministic output. Snapshots via `insta`.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
