# buff-i18n

Internationalization for the Buff language. Pure-Rust MVP wrapping Mozilla's [`fluent-bundle`](https://crates.io/crates/fluent-bundle) crate + [`unic-langid`](https://crates.io/crates/unic-langid) (BCP 47 language identifiers). Per T44 spec: NO machine translation, NO RTL layout helpers (UI concern).

**Status: experimental** (T44 v1.17 frameworks wave 6).

## STRUCTURE

```
buff-i18n/
├── Cargo.toml            # fluent-bundle + unic-langid + thiserror + insta deps
├── src/
│   ├── lib.rs            # I18n main surface (~430 LOC)
│   └── error.rs          # I18nError enum (~70 LOC)
├── examples/
│   ├── i18n_three_locales.rs    # three-locale (en/pt-BR/es) roundtrip
│   ├── i18n_plural.rs           # Fluent { $count -> [one] ... } select
│   ├── i18n_fallback.rs         # current-misses → fallback → warning
│   └── i18n/
│       └── i18n_three_locales.buff  # Buff-side forward-decl (matches .rs)
└── tests/
    └── core.rs           # 18 unit tests + 4 insta snapshots (~280 LOC)
```

Total: ~870 LOC (well under the 2500 LOC T44 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new translate variant / arg type | `src/lib.rs` (extend `build_args` or add a new `translate_with_*` method) + test in `tests/core.rs` |
| Add a new error variant | `src/error.rs` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (12 functions, ≤15 cap)

### `I18n` (12 functions)
- Constructors: `new(locale)`, `with_fallback(locale, fallback)`
- Mutation: `add_resource(locale, ftl)`, `load(locale)`, `set_fallback(locale)`
- Accessors: `available_locales()`, `current_locale()`, `fallback_locale()`
- Translation: `translate(key)`, `translate_with_args(key, args)`, `has_message(key)`
- Diagnostics: `warnings()`

## CONVENTIONS

- **Pure-Rust only**: `fluent-bundle` 0.16 + `unic-langid` 0.9 are both 100% pure-Rust (Mozilla maintains them; no cc-rs, no native C deps). Matches the "no C library, no Docker" hard rule.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. Missing keys return the key string + record a warning.
- **catch_unwind boundary**: every public entry point wraps its body in `catch_unwind` per FFI guide R6.
- **Send + Sync via `Arc<Mutex<I18nInner>>`**: `FluentBundle<FluentResource>` is `Send` but not `Sync` by default (certain API surfaces cache resolved messages). Wrapping in `Arc<Mutex<...>>` makes `I18n` safe to share across `spawn` boundaries per R4. Clone is cheap (Arc bump); lock held only for the duration of a single call.
- **Missing-key contract**: `translate` falls back current → fallback → key-string (NOT a panic, NOT an empty string). Each fall-through to the key string records a warning surfaced via `i18n.warnings()`. The contract mirrors how i18next / gettext / fluent.js handle missing keys.
- **String args surface**: `translate_with_args(key, &BTreeMap<String, String>)` treats every arg as a string. Fluent's plural/gender/ICU select rules expect typed args (NUMBER for plurals, DATE for dates); the MVP deliberately uses string-only to keep the Buff surface simple. A future `translate_with_typed_args` may surface `i64`/`f64`/`chrono::DateTime` for proper CLDR plural-rule matching.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `fluent-bundle` | Upstream Fluent engine. `buff-i18n` is a safe wrapper; never re-exports `fluent_bundle::*` types directly. |
| `unic-langid` | Upstream BCP 47 language identifier parser. Used for `current_locale` / `fallback_locale` validation. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::I18n` + `PreludeAssocFn::{New, WithFallback}` + `PreludeInstanceFn::{AddResource, Load, SetFallback, AvailableLocales, CurrentLocale, FallbackLocale, Translate, TranslateWithArgs, HasMessage, Warnings}`. `ty.rs` has the `Type::I18n` variant + `is_prelude_i18n()` predicate. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::buff_type_to_syn` has the `Type::I18n => "buff_i18n::I18n"` arm. `lower_prelude_type_assoc_fn` has the `(I18n, New)` / `(I18n, WithFallback)` arms. `lower_prelude_type_instance_fn` has all 10 instance-method arms. `program_uses_namespace("I18n")` records `buff-i18n` + `fluent-bundle` + `unic-langid` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **`fluent-bundle` vs `fluent` umbrella**: we pin `fluent-bundle` 0.16 directly (not the `fluent` 0.17 umbrella) because the umbrella adds `fluent-pseudo` + `fluent-langneg` + macros we do not need for the MVP. Mirrors the conservative pin philosophy.
- **`fluent-langneg` deferred**: the T44 spec mentions ICU MessageFormat; we use Fluent's built-in select syntax (`{ $count -> [one] ... }`) which handles plurals/gender natively. CLDR-aware locale fallback negotiation (`pt-BR → pt → en`) is a v1.18+ enhancement via the `fluent-langneg` crate.
- **`rust-i18n` deferred**: the T44 spec mentions both `fluent` AND `rust-i18n`. We ship only Fluent for the MVP (more expressive — handles plurals/gender/ICU select natively). `rust-i18n`'s Cargo-macro simpler surface may be added as a higher-level wrapper later.
- **No machine translation / No RTL helpers**: both explicitly forbidden by T44 spec. The crate is a pure translation-table lookup; bidi/RTL is a UI layout concern (deferred to a future buff-ui helper).
- **MSVC host note**: same family of host blockers as buff-image / buff-cache (the crate's lib + clippy pass clean on this Windows host; full `cargo test -p buff-i18n` requires the msvcrt.lib that this VS 18 Insiders host is missing — CI runs on the 3-OS matrix without issue).
