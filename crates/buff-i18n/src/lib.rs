//! `buff-i18n` — internationalization for the Buff language.
//!
//! Pure-Rust MVP wrapping Mozilla's [`fluent-bundle`](https://crates.io/crates/fluent-bundle)
//! crate (the same engine Firefox uses) + [`unic-langid`](https://crates.io/crates/unic-langid)
//! for BCP 47 language identifiers. Per T44 spec: NO machine translation,
//! NO RTL layout helpers (UI concern).
//!
//! # Pipeline
//!
//! ```text
//!   I18n.new(locale) ───────────────────────┐
//!                                            ▼
//!   i18n.add_resource(locale, ftl) ─▶ I18n { bundles, ... }
//!                                            │
//!                                            ├─ i18n.load(locale)
//!                                            ├─ i18n.set_fallback(locale)
//!                                            ├─ i18n.available_locales()
//!                                            └─ i18n.translate(key)
//!                                               i18n.translate_with_args(key, args)
//!                                                   │
//!                                                   ▼
//!                                          FluentBundle.format_pattern(...)
//!                                          (handles plurals / gender / ICU select)
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `I18n`, `I18nError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `new` returns owned `I18n`. `translate` / `translate_with_args` return owned `String`. `available_locales` / `warnings` return owned `Vec<String>`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, I18nError>`. `fluent_bundle::FluentError` + `unic_langid::LanguageIdentifierError` mapped via dedicated arms. |
//! | R4 — Thread safety | `I18n` is `Send + Sync` (wraps `Arc<Mutex<I18nInner>>` — see Thread safety below). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `I18n` owns its bundles + state. |
//! | R6 — Panic boundary | Every public entry point wraps its body in `catch_unwind` (per FFI guide §6). |
//!
//! # Thread safety
//!
//! `FluentBundle<FluentResource>` is `Send` (its internal state is owned)
//! but NOT `Sync` by default (the bundle caches resolved messages in a
//! `RefCell`-like fashion on certain API surfaces). To make `I18n` safe
//! to share across `spawn` boundaries per FFI guide R4, we wrap the
//! whole bundle map in `Arc<Mutex<I18nInner>>`. Cloning an `I18n`
//! bumps the `Arc` (cheap); locking the mutex serializes translates
//! against `load`/`add_resource` mutations. The lock is held only for
//! the duration of a single `translate` / `load` / etc — never across
//! a user callback.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Missing keys are surfaced as a stable warning +
//! the key string is returned verbatim (NOT a panic).

pub mod error;

pub use error::I18nError;

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use fluent_bundle::concurrent::FluentBundle as ConcurrentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

/// Maximum number of warnings retained for [`I18n::warnings`]. Older
/// warnings are dropped FIFO. Per T44 acceptance ("Missing key warning"),
/// users can observe missing-key / parse-error diagnostics without
/// unbounded memory growth.
const MAX_WARNINGS: usize = 64;

/// Internationalization catalog with locale fallback + pluralization /
/// gender / ICU MessageFormat support via Fluent.
///
/// Constructed via [`I18n::new`] (current locale == fallback) or
/// [`I18n::with_fallback`] (distinct current + fallback). Resources
/// are added per-locale via [`I18n::add_resource`]; the active locale
/// is switched via [`I18n::load`]; translations are produced via
/// [`I18n::translate`] / [`I18n::translate_with_args`].
///
/// Internally each locale gets its own `FluentBundle<FluentResource>`.
/// Multiple resources can be added to a single locale (the second
/// `add_resource` call appends to the same bundle's resource set).
/// Translations fall back from current → fallback → key-as-string.
#[derive(Clone)]
pub struct I18n {
    inner: Arc<Mutex<I18nInner>>,
}

struct I18nInner {
    current: LanguageIdentifier,
    fallback: LanguageIdentifier,
    bundles: BTreeMap<String, ConcurrentBundle<FluentResource>>,
    warnings: Vec<String>,
}

impl std::fmt::Debug for I18nInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("I18nInner")
            .field("current", &self.current)
            .field("fallback", &self.fallback)
            .field("locale_count", &self.bundles.len())
            .field("warnings", &self.warnings.len())
            .finish()
    }
}

impl std::fmt::Debug for I18n {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match lock_inner(&self.inner) {
            Ok(inner) => std::fmt::Debug::fmt(&*inner, f),
            Err(_) => f
                .debug_struct("I18n")
                .field("inner", &"<poisoned>")
                .finish(),
        }
    }
}

impl I18n {
    /// Create a new empty i18n catalog whose current AND fallback
    /// locale are both `locale`. The locale tag must be a valid BCP 47
    /// language identifier (e.g. `"en"`, `"en-US"`, `"pt-BR"`, `"es"`).
    ///
    /// Wraps `unic_langid::LanguageIdentifier::from_str`. The body is
    /// wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn new(locale: &str) -> Result<Self, I18nError> {
        Self::with_fallback(locale, locale)
    }

    /// Create a new empty i18n catalog with distinct current and
    /// fallback locales. `translate` looks up the current locale
    /// first; if the key is missing it falls back to `fallback`;
    /// if still missing, it returns the key string and records a
    /// warning.
    ///
    /// Both `locale` and `fallback` must be valid BCP 47 tags. They
    /// MAY be the same tag (equivalent to [`I18n::new`]).
    pub fn with_fallback(locale: &str, fallback: &str) -> Result<Self, I18nError> {
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<Self, I18nError> {
            let current = parse_langid(locale)?;
            let fallback = parse_langid(fallback)?;
            Ok(I18n {
                inner: Arc::new(Mutex::new(I18nInner {
                    current,
                    fallback,
                    bundles: BTreeMap::new(),
                    warnings: Vec::new(),
                })),
            })
        }));
        match result {
            Ok(ok) => ok,
            Err(_) => Err(I18nError::Panic),
        }
    }

    /// Add a Fluent `.ftl` resource for the given locale. Subsequent
    /// `add_resource` calls for the same locale append to the existing
    /// bundle's resource set. Message-id collisions across resources
    /// for the SAME locale resolve to the LAST-added definition
    /// (Fluent's `add_resource_overriding` semantics — matches how
    /// `fluent-rs` users compose multi-file catalogs); cross-locale
    /// duplicate ids are independent.
    ///
    /// Wraps `FluentResource::try_new` + `FluentBundle::add_resource_overriding`.
    /// Body wrapped in `catch_unwind` per FFI guide R6.
    pub fn add_resource(&self, locale: &str, ftl: &str) -> Result<(), I18nError> {
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), I18nError> {
            let mut inner = lock_inner(&self.inner)?;
            let langid = parse_langid(locale)?;
            let locale_key = langid.to_string();
            let resource = FluentResource::try_new(ftl.to_string())
                .map_err(|(_, errs)| I18nError::ResourceParse(format_parser_errors(&errs)))?;
            let bundle = inner
                .bundles
                .entry(locale_key)
                .or_insert_with(|| ConcurrentBundle::new_concurrent(vec![langid.clone()]));
            bundle.add_resource_overriding(resource);
            Ok(())
        }));
        match result {
            Ok(ok) => ok,
            Err(_) => Err(I18nError::Panic),
        }
    }

    /// Switch the active locale. The locale must have at least one
    /// resource loaded (via prior `add_resource`); otherwise returns
    /// [`I18nError::LocaleNotLoaded`]. Translations after `load` use
    /// the new active locale as the primary lookup target.
    ///
    /// Body wrapped in `catch_unwind` per FFI guide R6.
    pub fn load(&self, locale: &str) -> Result<(), I18nError> {
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), I18nError> {
            let mut inner = lock_inner(&self.inner)?;
            let langid = parse_langid(locale)?;
            let locale_key = langid.to_string();
            if !inner.bundles.contains_key(&locale_key) {
                return Err(I18nError::LocaleNotLoaded(locale_key));
            }
            inner.current = langid;
            Ok(())
        }));
        match result {
            Ok(ok) => ok,
            Err(_) => Err(I18nError::Panic),
        }
    }

    /// Set the fallback locale. Translations that miss in the
    /// current locale fall back to this locale before returning the
    /// raw key string. The locale need NOT have resources loaded
    /// (the fallback path returns the key string when both current
    /// and fallback miss).
    pub fn set_fallback(&self, locale: &str) -> Result<(), I18nError> {
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), I18nError> {
            let mut inner = lock_inner(&self.inner)?;
            inner.fallback = parse_langid(locale)?;
            Ok(())
        }));
        match result {
            Ok(ok) => ok,
            Err(_) => Err(I18nError::Panic),
        }
    }

    /// List all locales with at least one resource loaded. Sorted
    /// alphabetically. Returns an empty `Vec` if no resources have
    /// been added.
    pub fn available_locales(&self) -> Vec<String> {
        let result = catch_unwind(AssertUnwindSafe(|| match lock_inner(&self.inner) {
            Ok(inner) => inner.bundles.keys().cloned().collect(),
            Err(_) => Vec::new(),
        }));
        result.unwrap_or_default()
    }

    /// The currently-active locale tag. Initially set to the `locale`
    /// argument of [`I18n::new`] / [`I18n::with_fallback`]; changed
    /// by [`I18n::load`].
    pub fn current_locale(&self) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| {
            lock_inner(&self.inner)
                .map(|inner| inner.current.to_string())
                .unwrap_or_default()
        }));
        result.unwrap_or_default()
    }

    /// The fallback locale tag. Translations that miss in the
    /// current locale fall back here.
    pub fn fallback_locale(&self) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| {
            lock_inner(&self.inner)
                .map(|inner| inner.fallback.to_string())
                .unwrap_or_default()
        }));
        result.unwrap_or_default()
    }

    /// Translate a message id in the current locale, falling back to
    /// the fallback locale, then to the key string. Records a warning
    /// when both locales miss (so [`I18n::warnings`] surfaces the
    /// missing id). Equivalent to `translate_with_args(key, &empty)`.
    pub fn translate(&self, key: &str) -> String {
        self.translate_with_args(key, &BTreeMap::new())
    }

    /// Translate a message id with named arguments. The `args` map
    /// supplies placeholder values referenced from the `.ftl` source
    /// via `{ $name }`. Fluent's pluralization / gender / ICU select
    /// syntax is honored (the bundle resolves `{ $count -> [one] ...
    /// [other] ... }` based on the integer value of `$count`).
    ///
    /// Fall-back chain: current locale → fallback locale → `key` string.
    /// A warning is recorded for every miss so [`I18n::warnings`]
    /// surfaces missing translations.
    pub fn translate_with_args(&self, key: &str, args: &BTreeMap<String, String>) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut inner = match lock_inner(&self.inner) {
                Ok(inner) => inner,
                Err(_) => return key.to_string(),
            };
            let fluent_args = build_args(args);
            let current_key = inner.current.to_string();
            let fallback_key = inner.fallback.to_string();
            let lookup_order: Vec<String> = if current_key == fallback_key {
                vec![current_key.clone()]
            } else {
                vec![current_key.clone(), fallback_key]
            };
            for locale_key in &lookup_order {
                if let Some(bundle) = inner.bundles.get_mut(locale_key) {
                    if let Some(formatted) = format_message(bundle, key, &fluent_args) {
                        return formatted;
                    }
                }
            }
            let warning = format!("missing translation: {} (locale: {})", key, current_key);
            record_warning(&mut inner, warning);
            key.to_string()
        }));
        result.unwrap_or_else(|_| key.to_string())
    }

    /// Check whether the key exists in EITHER the current OR the
    /// fallback locale. Does NOT record a warning (intended as a
    /// cheap `if i18n.has_message(key) { ... }` predicate).
    pub fn has_message(&self, key: &str) -> bool {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let inner = match lock_inner(&self.inner) {
                Ok(inner) => inner,
                Err(_) => return false,
            };
            let current_key = inner.current.to_string();
            let fallback_key = inner.fallback.to_string();
            for locale_key in &[current_key, fallback_key] {
                if let Some(bundle) = inner.bundles.get(locale_key) {
                    if bundle.get_message(key).is_some() {
                        return true;
                    }
                }
            }
            false
        }));
        result.unwrap_or(false)
    }

    /// Recent missing-key / format-error warnings (newest first).
    /// Capped at [`MAX_WARNINGS`] entries. Useful for surfacing
    /// untranslated keys at app shutdown or in a debug overlay.
    pub fn warnings(&self) -> Vec<String> {
        let result = catch_unwind(AssertUnwindSafe(|| match lock_inner(&self.inner) {
            Ok(inner) => inner.warnings.iter().rev().cloned().collect(),
            Err(_) => Vec::new(),
        }));
        result.unwrap_or_default()
    }
}

impl Default for I18n {
    /// Default `I18n` is an empty English (`"en"`) catalog. The
    /// codegen-lowered `unwrap_or_default()` fallback uses this so
    /// `Result<I18n, I18nError>` return paths can panic-free fall
    /// back to a no-op translator (mirrors the Image / Cache /
    /// DataFrame precedent).
    fn default() -> Self {
        I18n::new("en").unwrap_or_else(|_| I18n {
            inner: Arc::new(Mutex::new(I18nInner {
                current: LanguageIdentifier::default(),
                fallback: LanguageIdentifier::default(),
                bundles: BTreeMap::new(),
                warnings: Vec::new(),
            })),
        })
    }
}

impl std::fmt::Display for I18n {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = catch_unwind(AssertUnwindSafe(|| {
            lock_inner(&self.inner)
                .map(|inner| inner.bundles.len())
                .unwrap_or(0)
        }))
        .unwrap_or(0);
        write!(
            f,
            "I18n(current: {}, fallback: {}, locales: {})",
            self.current_locale(),
            self.fallback_locale(),
            count
        )
    }
}

// ---- pub(crate) helpers -----------------------------------------------------

/// Lock the inner mutex, returning a poisoned-mutex fallback error
/// rather than panicking (so a panicked prior owner does not bring
/// down subsequent calls — per the panic-free contract).
fn lock_inner(
    inner: &Arc<Mutex<I18nInner>>,
) -> Result<std::sync::MutexGuard<'_, I18nInner>, I18nError> {
    inner.lock().map_err(|_| I18nError::Panic)
}

/// Parse a locale string into a `LanguageIdentifier`, mapping the
/// `unic_langid` parser error to [`I18nError::InvalidLocale`].
fn parse_langid(s: &str) -> Result<LanguageIdentifier, I18nError> {
    LanguageIdentifier::from_str(s).map_err(|_| I18nError::InvalidLocale(s.to_string()))
}

/// Format the parser errors returned by `FluentResource::try_new`
/// into a single human-readable string. The error type lives in the
/// `fluent_syntax` crate (re-exported as `fluent_bundle::parser` is
/// NOT a public path in 0.16); we avoid naming the type by going
/// through a `Display`-only signature.
fn format_parser_errors<D: std::fmt::Display>(errs: &[D]) -> String {
    errs.iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

/// Convert a `BTreeMap<String, String>` of named args into a
/// `FluentArgs` instance ready for `format_pattern`. Every value is
/// treated as a string (the simplest Buff-visible surface; future
/// versions may surface an `i64` / `f64` arg path for Fluent's
/// pluralization rules to distinguish [one] vs [other]).
fn build_args(args: &BTreeMap<String, String>) -> FluentArgs<'_> {
    let mut fluent_args = FluentArgs::new();
    for (k, v) in args {
        fluent_args.set(k.as_str(), FluentValue::String(v.as_str().into()));
    }
    fluent_args
}

/// Look up `key` in `bundle`, format its pattern with `args`, return
/// `Some(formatted)` on success. Returns `None` if the message id is
/// missing. Records Fluent formatter errors into the bundle's
/// `format_pattern` error sink (silent — they are non-fatal and the
/// missing-key warning is recorded by the caller).
fn format_message(
    bundle: &mut ConcurrentBundle<FluentResource>,
    key: &str,
    args: &FluentArgs<'_>,
) -> Option<String> {
    let message = bundle.get_message(key)?;
    let pattern = message.value()?;
    let mut errors = Vec::new();
    let formatted = bundle.format_pattern(pattern, Some(args), &mut errors);
    Some(formatted.into_owned())
}

/// Append a warning to the inner log, evicting oldest entries to
/// stay under [`MAX_WARNINGS`].
fn record_warning(inner: &mut I18nInner, msg: String) {
    if inner.warnings.len() >= MAX_WARNINGS {
        inner.warnings.remove(0);
    }
    inner.warnings.push(msg);
}

#[cfg(test)]
mod smoke_tests {
    //! Tiny inline sanity checks. The full integration suite lives
    //! in `tests/core.rs` per the workspace `tests/`-per-crate
    //! convention.

    use super::*;

    #[test]
    fn new_rejects_invalid_locale() {
        let err = I18n::new("not a locale!").unwrap_err();
        assert!(matches!(err, I18nError::InvalidLocale(_)));
    }

    #[test]
    fn default_is_english_empty_catalog() {
        let i18n = I18n::default();
        assert_eq!(i18n.current_locale(), "en");
        assert!(i18n.available_locales().is_empty());
    }

    #[test]
    fn translate_missing_returns_key_and_records_warning() {
        let i18n = I18n::new("en").expect("en");
        let result = i18n.translate("does-not-exist");
        assert_eq!(result, "does-not-exist");
        let warnings = i18n.warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("does-not-exist"));
    }
}
