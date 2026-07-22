# buff-fake

Fake data generation for the Buff language. Pure-Rust MVP wrapping the [`fake`](https://docs.rs/fake/latest/fake/) crate. Provides `Faker.name()`, `Faker.email()`, `Faker.address()`, `Faker.phone()`, `Faker.uuid()`, `Faker.lorem(words)`, `Faker.int(min, max)`, `Faker.datetime(range)`. Locales: en-US, pt-BR.

**Status: experimental** (T37 v1.13 frameworks wave 2).

## STRUCTURE

```
buff-fake/
├── Cargo.toml            # fake + rand + chrono + insta deps
├── src/
│   ├── lib.rs            # Faker + FakerLocale (main surface, ~200 LOC)
│   └── error.rs          # FakerError enum (~15 LOC)
├── examples/
│   └── faker_demo.rs     # all 8 methods demo with seeded RNG
└── tests/
    └── core.rs           # 15 unit tests + 2 insta snapshots (~200 LOC)
```

Total: ~415 LOC (well under the 1500 LOC T37 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new fake method | `src/lib.rs` (add `pub fn` on `Faker`) + test in `tests/core.rs` |
| Add a new error variant | `src/error.rs` |
| Add a new locale | `src/lib.rs` (FakerLocale enum + match arms in each method) |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (11 functions, ≤15 cap)

### `Faker` (11 functions)
- Constructors: `new`, `with_locale`, `with_seed`
- Generators: `name`, `email`, `address`, `phone`, `uuid`, `lorem`, `int`, `datetime`

## CONVENTIONS

- **Pure-Rust only**: `fake` crate is pure-Rust. NO native deps.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code.
- **catch_unwind boundary**: every public method wraps its body in `catch_unwind` per FFI guide R6.
- **Seeded RNG**: `with_seed` uses `rand::rngs::StdRng::seed_from_u64` for reproducible output.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `fake` | Upstream fake data provider. `buff-fake` is a safe wrapper; never re-exports `fake::*` types directly. |
| `rand` | RNG backend. Already in workspace at 0.8. |
| `chrono` | Datetime parsing for range validation. Already in workspace at 0.4. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Faker` + `PreludeAssocFn::{New, WithLocale, WithSeed}` + 8 `PreludeInstanceFn` variants. `ty.rs` has the `Type::Faker` variant + `is_prelude_faker()` predicate. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::buff_type_to_syn` has the `Type::Faker => "buff_fake::Faker"` arm. `lower_prelude_type_assoc_fn` has the `(Faker, New)` / `(Faker, WithLocale)` / `(Faker, WithSeed)` arms. `lower_prelude_type_instance_fn` has all 8 instance-method arms. `program_uses_namespace("Faker")` records `buff-fake` + `fake` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **Faker is a namespace-only type**: `Faker.new()` returns a runtime `Faker` value; instance methods are called on the receiver. This mirrors the `Image` / `Regex` / `URL` pattern.
- **`int` method**: The `fake` crate's `Number()` generates within the full i64 range; we clamp to [min, max] for the Buff surface.
- **`datetime` method**: Accepts RFC 3339 strings for start/end range. Returns RFC 3339 string. Validates that end > start.
- **Locale switching**: en-US and pt-BR use the `fake` crate's locale-specific modules (`fake::faker::name::en::Name` vs `fake::faker::name::pt_br::Name`).
