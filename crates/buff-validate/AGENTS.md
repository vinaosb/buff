# buff-validate

Declarative schema validation for the Buff language (pydantic-equivalent). Pure-Rust MVP wrapping the [`validator`](https://crates.io/crates/validator) crate's free-standing trait validators + exporting JSON Schema via [`serde_json`](https://crates.io/crates/serde_json). Safe FFI boundary per the [T4 FFI guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T29 v1.16 frameworks wave).

## STRUCTURE

```
buff-validate/
├── Cargo.toml            # validator + serde_json + regex + thiserror + insta deps
├── src/
│   ├── lib.rs            # Validator + Rule enum (main surface, ~290 LOC)
│   ├── error.rs          # ValidationError + ValidationErrors (~165 LOC)
│   └── schema.rs         # JSON Schema (Draft 2020-12) serializer (~115 LOC)
├── examples/
│   ├── signup_form.rs    # email + length + range rule chain
│   ├── regex_rules.rs    # regex pattern matching + bad-pattern fail-fast
│   ├── schema_export.rs  # JSON Schema export round-trip via serde_json
│   └── validate/
│       ├── signup_form.buff    # Buff-side forward-decl (matches .rs)
│       ├── regex_rules.buff    # Buff-side forward-decl (matches .rs)
│       └── schema_export.buff  # Buff-side forward-decl (matches .rs)
└── tests/
    └── core.rs           # 21 unit tests + 3 insta snapshots (~320 LOC)
```

Total: ~870 LOC (well under the 2000 LOC T29 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new rule kind | `src/lib.rs` (extend `Rule` enum + `Rule::apply` arm + `with_*` builder) + test in `tests/core.rs` |
| Add a new error variant | `src/error.rs` |
| Extend JSON Schema mapping | `src/schema.rs::serialize_schema` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (8 functions, ≤20 cap)

### `Validator` (8 functions)
- Constructors: `new`
- Builders (consume self, return Self for chaining): `with_email`, `with_url`, `with_length`, `with_range`, `with_regex`
- Actions: `validate` (returns `Result<(), ValidationErrors>`), `to_json_schema` (returns `String`)
- Accessor: `rule_count`

### `ValidationErrors` (5 methods)
- Constructors / mutation: `new`, `push`
- Accessors: `len`, `is_empty`, `iter`

### `ValidationError` (8 variants)
- `InvalidEmail`, `InvalidUrl`, `InvalidLength`, `InvalidRange`, `InvalidRegex` — single-rule failures
- `BadRegex`, `InvalidRuleConfig` — rule-registration-time failures (fail-fast)
- `MissingField`, `UncoercibleValue` — runtime/type-coercion failures
- `Panic` — `catch_unwind` boundary caught a panic

## CONVENTIONS

- **Pure-Rust only**: `validator` 0.20 with `default-features = false` + the `unicode` feature (proper char-count for length rules). The `derive` feature is OFF (T29 must-not #1: "no compile-time macro validation"). The `card` feature is OFF (credit-card validation deferred). NO native C deps — matches the "no C library, no Docker" hard rule.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. All fallible operations return `Result`.
- **catch_unwind boundary**: `validate` / `to_json_schema` / `with_regex` wrap their bodies in `catch_unwind` per FFI guide R6.
- **Fail-fast on bad config**: `with_length` / `with_range` surface `min > max` at registration; `with_regex` compiles the pattern at registration. Bad config NEVER waits until `validate` to surface.
- **No `&mut self` builders**: `with_*` methods consume `self` and return `Self` so the Buff-side lowering doesn't need to handle `&mut self` receivers (Buff's "no visible references" stance). Mirrors the axum `Router::route` pattern.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `validator` | Upstream validator. `buff-validate` calls `ValidateEmail` / `ValidateUrl` / `ValidateLength` / `ValidateRange` trait methods on `&str` / integer values; never re-exports `validator::*` types. The `derive` macro feature is OFF. |
| `serde_json` | JSON Schema export. Already pinned at the workspace level (T124a). |
| `regex` | Regex pattern compilation. Already pinned at the workspace level (T124d). |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Validator` + `Validator` instance fns (WithEmail / WithUrl / WithLength / WithRange / WithRegex / Validate / ToJsonSchema). `ty.rs` has the `Type::Validator` variant + `is_prelude_validator()` predicate. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::buff_type_to_syn` has the `Type::Validator => "buff_validate::Validator"` arm. `lower_prelude_type_assoc_fn` has the `(Validator, New)` arm. `lower_prelude_type_instance_fn` has all 7 instance-method arms. `program_uses_namespace("Validator")` records `buff-validate` + `validator` + `serde_json` + `regex` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **MSVC host blocker**: `cargo test -p buff-validate` fails on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue (same family that blocks `cargo check --workspace` here). CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. The crate's library `cargo check -p buff-validate --lib` and `cargo clippy -p buff-validate --all-targets -- -D warnings` both pass clean.
- **`with_*` builder pattern**: All builder methods consume `self` and return `Self` (or `Result<Self, ValidationError>`) — they do NOT take `&mut self`. This is Buff-friendly (Buff's surface hides references) and mirrors the `axum::Router::route` pattern. The `?` propagation in examples demonstrates the fail-fast `Result<Self>` shape.
- **Char-count vs byte-count for length**: `Rule::apply` for `Length` uses `value.chars().count()` (unicode-aware) and reports `actual` as the char count in the error. The underlying `validator::ValidateLength::validate_length` is also unicode-aware when the `unicode` cargo feature is enabled (it is — see Cargo.toml).
- **Regex compilation is eager**: `with_regex` compiles the pattern at registration time so a malformed pattern surfaces next to the `with_regex` call site, NOT deferred until the first `validate`. This is the workspace's fail-fast stance.
- **JSON Schema output is Draft 2020-12**: `$schema` field is `"https://json-schema.org/draft/2020-12/schema"`. Multiple rules on the same field merge into a single property entry (e.g. `email` + `length` constraints both go into `properties.email`). The `required` array is sorted alphabetically for deterministic output.
- **No async validators**: T29 must-not #2 — async validators are deferred to v1.22+. All `validate` calls are synchronous.
- **No custom closure callbacks**: Buff's no-closures-across-FFI stance means `with_custom` is not part of the MVP surface. A future `with_custom_name(predicate_name)` lowering could register named validators resolved at runtime.
