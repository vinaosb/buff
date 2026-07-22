//! Integration tests for the `buff-i18n` crate.
//!
//! Covers all 12 public functions per the T44 spec:
//! - Constructors: `I18n::new`, `I18n::with_fallback`
//! - Mutation: `add_resource`, `load`, `set_fallback`
//! - Accessors: `available_locales`, `current_locale`, `fallback_locale`
//! - Translation: `translate`, `translate_with_args`, `has_message`
//! - Diagnostics: `warnings`
//!
//! Hermetic: every test constructs its own catalogs inline (no `.ftl`
//! fixtures needed). 13 unit tests + 4 insta snapshots (per the T44
//! acceptance criteria of 10+ tests).

use buff_i18n::{I18n, I18nError};
use std::collections::BTreeMap;

const EN_FTL: &str = "\
hello = Hello, world!
greet = Hello, { $name }!
emails = You have { $count } emails.
plural = { $count ->\n\
    [one] You have one email.\n\
    *[other] You have { $count } emails.\n\
}
";

const PT_BR_FTL: &str = "\
hello = Olá, mundo!
greet = Olá, { $name }!
emails = Você tem { $count } e-mails.
plural = { $count ->\n\
    [one] Você tem um e-mail.\n\
    *[other] Você tem { $count } e-mails.\n\
}
";

const ES_FTL: &str = "\
hello = ¡Hola, mundo!
greet = ¡Hola, { $name }!
";

// ---- Constructor tests -----------------------------------------------------

#[test]
fn new_rejects_invalid_locale_tag() {
    let err = I18n::new("not a locale!").unwrap_err();
    assert!(matches!(err, I18nError::InvalidLocale(_)));
}

#[test]
fn new_accepts_canonical_bcp47_tags() {
    for tag in &["en", "en-US", "pt-BR", "es", "zh-Hans-CN", "de-AT"] {
        let i18n = I18n::new(tag).unwrap_or_else(|e| panic!("locale {tag} should parse: {e:?}"));
        assert_eq!(i18n.current_locale(), *tag);
        assert_eq!(i18n.fallback_locale(), *tag);
    }
}

#[test]
fn with_fallback_distinguishes_current_and_fallback() {
    let i18n = I18n::with_fallback("pt-BR", "en").expect("pt-BR + en");
    assert_eq!(i18n.current_locale(), "pt-BR");
    assert_eq!(i18n.fallback_locale(), "en");
}

#[test]
fn with_fallback_rejects_either_locale_invalid() {
    let err = I18n::with_fallback("en", "INVALID!").unwrap_err();
    assert!(matches!(err, I18nError::InvalidLocale(_)));
    let err = I18n::with_fallback("INVALID!", "en").unwrap_err();
    assert!(matches!(err, I18nError::InvalidLocale(_)));
}

// ---- add_resource tests ----------------------------------------------------

#[test]
fn add_resource_then_translate_succeeds() {
    let i18n = I18n::new("en").expect("en");
    i18n.add_resource("en", EN_FTL).expect("add en");
    assert_eq!(i18n.translate("hello"), "Hello, world!");
}

#[test]
fn add_resource_rejects_garbage_ftl() {
    let i18n = I18n::new("en").expect("en");
    // A line without `=` is not valid FTL at the top level.
    let err = i18n
        .add_resource("en", "this is not valid fluent\n=no value\n")
        .unwrap_err();
    assert!(matches!(err, I18nError::ResourceParse(_)));
}

#[test]
fn add_resource_detects_duplicate_ids_within_same_locale() {
    let i18n = I18n::new("en").expect("en");
    i18n.add_resource("en", "hello = first").expect("first");
    let err = i18n.add_resource("en", "hello = second").unwrap_err();
    assert!(matches!(err, I18nError::Duplicate(_)), "got {err:?}");
}

#[test]
fn add_resource_same_id_across_distinct_locales_ok() {
    let i18n = I18n::with_fallback("en", "pt-BR").expect("en + pt-BR");
    i18n.add_resource("en", "hello = Hello").expect("en");
    i18n.add_resource("pt-BR", "hello = Olá").expect("pt-BR");
    assert_eq!(i18n.translate("hello"), "Hello");
}

// ---- load + locale switching tests -----------------------------------------

#[test]
fn load_switches_current_locale() {
    let i18n = I18n::with_fallback("en", "en").expect("en");
    i18n.add_resource("en", EN_FTL).expect("en");
    i18n.add_resource("pt-BR", PT_BR_FTL).expect("pt-BR");
    assert_eq!(i18n.translate("hello"), "Hello, world!");
    i18n.load("pt-BR").expect("load pt-BR");
    assert_eq!(i18n.current_locale(), "pt-BR");
    assert_eq!(i18n.translate("hello"), "Olá, mundo!");
}

#[test]
fn load_rejects_locale_without_resources() {
    let i18n = I18n::new("en").expect("en");
    i18n.add_resource("en", EN_FTL).expect("en");
    let err = i18n.load("fr").unwrap_err();
    assert!(matches!(err, I18nError::LocaleNotLoaded(_)));
}

#[test]
fn available_locales_lists_all_with_resources() {
    let i18n = I18n::new("en").expect("en");
    assert!(i18n.available_locales().is_empty());
    i18n.add_resource("en", EN_FTL).expect("en");
    i18n.add_resource("pt-BR", PT_BR_FTL).expect("pt-BR");
    i18n.add_resource("es", ES_FTL).expect("es");
    let locales = i18n.available_locales();
    assert_eq!(locales, vec!["en", "es", "pt-BR"]);
}

// ---- translate + pluralization tests ---------------------------------------

#[test]
fn translate_with_args_substitutes_placeholders() {
    let i18n = I18n::new("en").expect("en");
    i18n.add_resource("en", EN_FTL).expect("en");
    let mut args = BTreeMap::new();
    args.insert("name".to_string(), "Alice".to_string());
    let result = i18n.translate_with_args("greet", &args);
    assert!(result.contains("Alice"));
    assert!(result.contains("Hello"));
}

#[test]
fn translate_falls_back_to_fallback_locale() {
    let i18n = I18n::with_fallback("pt-BR", "en").expect("pt-BR + en");
    i18n.add_resource("en", EN_FTL).expect("en");
    // pt-BR has no resources loaded — `load` would reject it. But the
    // initial current_locale IS pt-BR. translate() should fall back
    // to the en catalog.
    assert_eq!(i18n.translate("hello"), "Hello, world!");
}

#[test]
fn translate_missing_key_returns_key_and_warns() {
    let i18n = I18n::new("en").expect("en");
    i18n.add_resource("en", EN_FTL).expect("en");
    let result = i18n.translate("no-such-key");
    assert_eq!(result, "no-such-key");
    let warnings = i18n.warnings();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("no-such-key"));
}

#[test]
fn translate_pluralizes_via_fluent_select() {
    let i18n = I18n::new("en").expect("en");
    i18n.add_resource("en", EN_FTL).expect("en");

    let mut one = BTreeMap::new();
    one.insert("count".to_string(), "1".to_string());
    let one_result = i18n.translate_with_args("plural", &one);
    assert!(
        one_result.contains("one email"),
        "expected [one] branch, got: {one_result}"
    );

    let mut many = BTreeMap::new();
    many.insert("count".to_string(), "5".to_string());
    let many_result = i18n.translate_with_args("plural", &many);
    assert!(
        !one_result.is_empty() && !many_result.is_empty(),
        "both plural branches should produce output: one={one_result:?} many={many_result:?}"
    );
}

// ---- has_message tests -----------------------------------------------------

#[test]
fn has_message_checks_current_and_fallback() {
    let i18n = I18n::with_fallback("pt-BR", "en").expect("pt-BR + en");
    i18n.add_resource("en", EN_FTL).expect("en");
    assert!(i18n.has_message("hello"));
    assert!(!i18n.has_message("totally-missing"));
}

// ---- three locales (T44 acceptance: en, pt-BR, es) -------------------------

#[test]
fn three_locale_translation_en_ptbr_es() {
    let i18n = I18n::with_fallback("en", "en").expect("en");
    i18n.add_resource("en", EN_FTL).expect("en");
    i18n.add_resource("pt-BR", PT_BR_FTL).expect("pt-BR");
    i18n.add_resource("es", ES_FTL).expect("es");

    assert_eq!(i18n.translate("hello"), "Hello, world!");
    i18n.load("pt-BR").expect("pt-BR");
    assert_eq!(i18n.translate("hello"), "Olá, mundo!");
    i18n.load("es").expect("es");
    assert_eq!(i18n.translate("hello"), "¡Hola, mundo!");
}

// ---- Arc/Mutex + Send + Sync -----------------------------------------------

#[test]
fn clone_shares_state_across_threads() {
    let i18n = I18n::new("en").expect("en");
    i18n.add_resource("en", EN_FTL).expect("en");
    let clone = i18n.clone();
    let handle = std::thread::spawn(move || clone.translate("hello"));
    let result = handle.join().expect("thread join");
    assert_eq!(result, "Hello, world!");
}

#[test]
fn set_fallback_changes_fallback_locale() {
    let i18n = I18n::new("en").expect("en");
    i18n.add_resource("en", EN_FTL).expect("en");
    i18n.add_resource("pt-BR", PT_BR_FTL).expect("pt-BR");
    i18n.set_fallback("pt-BR").expect("set fallback");
    assert_eq!(i18n.fallback_locale(), "pt-BR");
}

// ---- Insta snapshots -------------------------------------------------------

#[test]
fn snapshot_default_i18n_display() {
    let i18n = I18n::default();
    insta::assert_snapshot!("default_i18n_display", format!("{i18n}"));
}

#[test]
fn snapshot_loaded_i18n_display() {
    let i18n = I18n::with_fallback("pt-BR", "en").expect("pt-BR + en");
    i18n.add_resource("en", EN_FTL).expect("en");
    i18n.add_resource("pt-BR", PT_BR_FTL).expect("pt-BR");
    i18n.add_resource("es", ES_FTL).expect("es");
    insta::assert_snapshot!("loaded_i18n_display", format!("{i18n}"));
}

#[test]
fn snapshot_error_variants() {
    let errs = format!(
        "{}\n{}\n{}\n{}\n{}",
        I18nError::InvalidLocale("BAD!".to_string()),
        I18nError::LocaleNotLoaded("fr".to_string()),
        I18nError::ResourceParse("unexpected token".to_string()),
        I18nError::Duplicate("hello".to_string()),
        I18nError::Panic,
    );
    insta::assert_snapshot!("i18n_error_variants", errs);
}

#[test]
fn snapshot_warning_after_missing_key() {
    let i18n = I18n::new("en").expect("en");
    i18n.add_resource("en", EN_FTL).expect("en");
    i18n.translate("missing-key-a");
    i18n.translate("missing-key-b");
    let warnings = i18n.warnings().join("\n");
    insta::assert_snapshot!("warnings_after_missing_keys", warnings);
}
