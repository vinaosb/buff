# buff-i18n

Internationalization for the **Buff** language. Pure-Rust MVP wrapping Mozilla's [`fluent-bundle`](https://crates.io/crates/fluent-bundle) crate (the same engine Firefox uses) + [`unic-langid`](https://crates.io/crates/unic-langid) for BCP 47 language identifiers.

`buff-i18n` exposes Fluent's full localization power (pluralization, gender, ICU MessageFormat) behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses translations via the `I18n` prelude type:

```buff
let i18n = I18n.new(locale: "en")
i18n.add_resource(locale: "en", ftl: "hello = Hello, { $name }!")
i18n.add_resource(locale: "pt-BR", ftl: "hello = Olá, { $name }!")

i18n.load(locale: "pt-BR")
print(i18n.translate(key: "hello", args: ["name": "Alice"]))
```

**Status: experimental** (T44 v1.17 frameworks wave 6).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `I18n` prelude type.

For direct Rust use:

```bash
cargo add buff-i18n --path crates/buff-i18n
```

## Quick start

```rust
use buff_i18n::I18n;
use std::collections::BTreeMap;

fn main() {
    let i18n = I18n::with_fallback("pt-BR", "en").expect("construct");

    i18n.add_resource("en", "hello = Hello!\n").expect("add en");
    i18n.add_resource("pt-BR", "hello = Olá!\n").expect("add pt-BR");

    i18n.load("pt-BR").expect("load pt-BR");
    assert_eq!(i18n.translate("hello"), "Olá!");

    let mut args = BTreeMap::new();
    args.insert("name".to_string(), "Alice".to_string());
    i18n.add_resource("en", "greet = Hi, { $name }!\n").expect("extend en");
    i18n.load("en").expect("load en");
    let greeting = i18n.translate_with_args("greet", &args);
    assert!(greeting.contains("Alice"));
}
```

## Public API

### `I18n` — localization catalog (12 functions, ≤15 cap)

| Method | Signature | Notes |
|---|---|---|
| `I18n::new` | `(locale) -> Result<I18n, I18nError>` | current AND fallback both `locale`. |
| `I18n::with_fallback` | `(locale, fallback) -> Result<I18n, I18nError>` | distinct current + fallback. |
| `i18n.add_resource` | `(&self, locale, ftl) -> Result<(), I18nError>` | append FTL catalog for locale. |
| `i18n.load` | `(&self, locale) -> Result<(), I18nError>` | switch active locale (must have resources). |
| `i18n.set_fallback` | `(&self, locale) -> Result<(), I18nError>` | change fallback locale. |
| `i18n.available_locales` | `(&self) -> Vec<String>` | locales with at least one resource. |
| `i18n.current_locale` | `(&self) -> String` | active locale tag. |
| `i18n.fallback_locale` | `(&self) -> String` | fallback locale tag. |
| `i18n.translate` | `(&self, key) -> String` | translate (no args); falls back to key. |
| `i18n.translate_with_args` | `(&self, key, &BTreeMap<String, String>) -> String` | translate with named args (plural/gender). |
| `i18n.has_message` | `(&self, key) -> bool` | checks current OR fallback; no warning. |
| `i18n.warnings` | `(&self) -> Vec<String>` | recent missing-key / parse errors (newest first). |

### `I18nError`

| Variant | When |
|---|---|
| `InvalidLocale(String)` | BCP 47 parse failure (e.g. `"en_US"`). |
| `LocaleNotLoaded(String)` | `load` called for locale without resources. |
| `ResourceParse(String)` | Fluent `.ftl` parse error. |
| `Duplicate(String)` | `add_resource` collides with existing message id (same locale). |
| `Panic` | Internal panic caught by `catch_unwind` (R6). |

## Fluent syntax primer

Catalogs use Mozilla's Fluent `.ftl` syntax. The full reference is at <https://projectfluent.org/fluent/guide/>. A minimal subset:

```ftl
# Simple key
hello = Hello, world!

# Parameterized key (placeholder via { $name })
greet = Hello, { $name }!

# Pluralization via select
emails =
    { $count ->
        [one] You have one email.
       *[other] You have { $count } emails.
    }

# Attribute (e.g. for accessibility labels)
login-button = Sign in
    .aria-label = Sign in to your account
```

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md):

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `I18n`, `I18nError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `new` returns owned `I18n`. `translate`/`translate_with_args`/`available_locales`/`warnings` return owned `String` / `Vec<String>`. |
| R3 — Error mapping | Every fallible op returns `Result<T, I18nError>`. `fluent_bundle::FluentError` + `unic_langid::LanguageIdentifierError` mapped via dedicated arms. |
| R4 — Thread safety | `I18n` is `Send + Sync` (wraps `Arc<Mutex<I18nInner>>`). Clone bumps the Arc (cheap). |
| R5 — Lifetime hiding | No public lifetime parameters. `I18n` owns its bundles + state. |
| R6 — Panic boundary | Every public entry point wraps body in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-i18n
cargo clippy -p buff-i18n --all-targets -- -D warnings
cargo fmt -p buff-i18n --check
```

Tests are hermetic: every test constructs its own catalogs inline via `add_resource` (no `.ftl` fixtures needed). 18 unit tests + 4 insta snapshots.

## Deferred to v1.18+

- **`rust-i18n` simpler workflow**: the T44 spec mentions both `fluent` AND `rust-i18n`; we ship only `fluent-bundle` for the MVP because Fluent is the more expressive system (handles plurals/gender/ICU select natively). `rust-i18n` is a Cargo-macro-based simpler surface that may be added as a higher-level wrapper in a future task.
- **Machine translation**: explicitly forbidden by T44 spec.
- **RTL layout helpers**: explicitly forbidden (UI concern — defer to a future buff-ui helper).
- **Locale fallback negotiation**: `fluent-langneg` crate for proper CLDR-aware fallback chains (e.g. `pt-BR` → `pt` → `en`). Currently the fallback is a single explicit locale.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
