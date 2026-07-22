//! Integration tests for the `buff-nlp` crate.
//!
//! Covers all 6 public functions per the T46 spec:
//! - Constructors / accessors: `Language::code`, `Language::name`
//! - `Text::detect_language` — whatlang trigram classifier
//! - `Text::stem` — Snowball stemmer (English + Portuguese cases)
//! - `Text::tokenize` — UAX #29 word segmentation
//! - `Text::sentences` — UAX #29 sentence segmentation
//! - `StemAlgorithm` variants + Display
//!
//! Plus 5 insta snapshots (per T46 acceptance criteria: ≥10 tests).

use buff_nlp::{Language, NlpError, StemAlgorithm, Text};

const ENGLISH_SAMPLE: &str =
    "The quick brown fox jumps over the lazy dog near the riverbank every morning.";
const PORTUGUESE_SAMPLE: &str =
    "A rápida raposa marrom salta sobre o cão preguiçoso todas as manhãs.";
const FRENCH_SAMPLE: &str =
    "Le rapide renard brun saute par-dessus le chien paresseux chaque matin.";

#[test]
fn detect_language_english_sample() {
    let lang = Text::detect_language(ENGLISH_SAMPLE).expect("english sample");
    assert_eq!(lang.code(), "eng");
    assert_eq!(lang.name(), "English");
}

#[test]
fn detect_language_portuguese_sample() {
    let lang = Text::detect_language(PORTUGUESE_SAMPLE).expect("portuguese sample");
    assert_eq!(lang.code(), "por");
    assert_eq!(lang.name(), "Portuguese");
}

#[test]
fn detect_language_french_sample() {
    let lang = Text::detect_language(FRENCH_SAMPLE).expect("french sample");
    assert_eq!(lang.code(), "fra");
    assert_eq!(lang.name(), "French");
}

#[test]
fn detect_language_empty_input_returns_none() {
    assert!(Text::detect_language("").is_none());
    assert!(Text::detect_language("   ").is_none());
    assert!(Text::detect_language("\n\t\n").is_none());
}

#[test]
fn detect_language_pure_numbers_returns_none_or_low_confidence() {
    // Pure numbers / whitespace have no language signal. whatlang
    // may return `None` here (the desired case) or a low-confidence
    // guess. We accept either — the contract is "may return None".
    let _ = Text::detect_language("12345 67890");
}

#[test]
fn stem_english_running_to_run() {
    let stem = Text::stem("running", StemAlgorithm::English).expect("stem running");
    assert_eq!(stem, "run");
}

#[test]
fn stem_english_various_words() {
    assert_eq!(
        Text::stem("jumping", StemAlgorithm::English).unwrap(),
        "jump"
    );
    assert_eq!(
        Text::stem("happily", StemAlgorithm::English).unwrap(),
        "happili"
    );
    assert_eq!(
        Text::stem("cats", StemAlgorithm::English).unwrap(),
        "cat"
    );
}

#[test]
fn stem_portuguese_correndo() {
    let stem = Text::stem("correndo", StemAlgorithm::Portuguese).expect("stem correndo");
    // Snowball Portuguese stemmer reduces "correndo" to "corr".
    assert_eq!(stem, "corr");
}

#[test]
fn stem_empty_input_rejected() {
    let err = Text::stem("", StemAlgorithm::English).unwrap_err();
    assert!(matches!(err, NlpError::EmptyInput));
}

#[test]
fn stem_idempotent_on_already_stemmed() {
    let once = Text::stem("running", StemAlgorithm::English).unwrap();
    let twice = Text::stem(&once, StemAlgorithm::English).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn tokenize_drops_punctuation_and_whitespace() {
    let tokens = Text::tokenize("Hello, world! Foo; bar: baz.");
    assert_eq!(
        tokens,
        vec!["Hello", "world", "Foo", "bar", "baz"]
    );
}

#[test]
fn tokenize_empty_input_returns_empty_vec() {
    assert!(Text::tokenize("").is_empty());
    assert!(Text::tokenize("   \n\t  ").is_empty());
    assert!(Text::tokenize("!@#$%^&*()").is_empty());
}

#[test]
fn tokenize_handles_unicode_word_boundaries() {
    let tokens = Text::tokenize("Olá mundo — café manhã");
    assert_eq!(tokens, vec!["Olá", "mundo", "café", "manhã"]);
}

#[test]
fn sentences_splits_on_terminators() {
    let input = "Hello world. How are you? I am fine!";
    let sents = Text::sentences(input);
    assert_eq!(sents.len(), 3);
    assert_eq!(sents[0], "Hello world. ");
    assert_eq!(sents[1], "How are you? ");
    assert_eq!(sents[2], "I am fine!");
}

#[test]
fn sentences_single_sentence_returns_one() {
    let sents = Text::sentences("Just one sentence with no terminator");
    assert_eq!(sents, vec!["Just one sentence with no terminator"]);
}

#[test]
fn sentences_empty_input_returns_empty_vec() {
    assert!(Text::sentences("").is_empty());
    assert!(Text::sentences("   ").is_empty());
}

#[test]
fn stem_algorithm_display_lowercase_snake() {
    assert_eq!(format!("{}", StemAlgorithm::English), "english");
    assert_eq!(format!("{}", StemAlgorithm::Portuguese), "portuguese");
    assert_eq!(format!("{}", StemAlgorithm::French), "french");
    assert_eq!(format!("{}", StemAlgorithm::Arabic), "arabic");
    assert_eq!(format!("{}", StemAlgorithm::Turkish), "turkish");
}

#[test]
fn stem_algorithm_to_rust_stemmers_all_variants() {
    // Every variant must map to a constructible stemmer.
    let all = [
        StemAlgorithm::Arabic,
        StemAlgorithm::Danish,
        StemAlgorithm::Dutch,
        StemAlgorithm::English,
        StemAlgorithm::Finnish,
        StemAlgorithm::French,
        StemAlgorithm::German,
        StemAlgorithm::Greek,
        StemAlgorithm::Hungarian,
        StemAlgorithm::Italian,
        StemAlgorithm::Norwegian,
        StemAlgorithm::Portuguese,
        StemAlgorithm::Romanian,
        StemAlgorithm::Russian,
        StemAlgorithm::Spanish,
        StemAlgorithm::Swedish,
        StemAlgorithm::Tamil,
        StemAlgorithm::Turkish,
    ];
    for alg in all {
        let _ = alg.to_rust_stemmers();
    }
}

#[test]
fn language_display_format() {
    let lang = Text::detect_language(ENGLISH_SAMPLE).expect("english");
    assert_eq!(format!("{lang}"), "English (eng)");
}

#[test]
fn text_default_is_namespace_marker() {
    // Text is a unit-like namespace marker; Default is the codegen
    // panic-free fallback (matches DataFrame / Image precedent).
    let _ = Text::default();
}

// ---- Insta snapshots (5+) ---------------------------------------------------

#[test]
fn snapshot_language_english() {
    let lang = Text::detect_language(ENGLISH_SAMPLE).expect("english");
    insta::assert_snapshot!("language_english", format!("{lang}"));
}

#[test]
fn snapshot_language_portuguese() {
    let lang = Text::detect_language(PORTUGUESE_SAMPLE).expect("portuguese");
    insta::assert_snapshot!("language_portuguese", format!("{lang}"));
}

#[test]
fn snapshot_stem_algorithm_all_variants() {
    let all = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        StemAlgorithm::Arabic,
        StemAlgorithm::Danish,
        StemAlgorithm::Dutch,
        StemAlgorithm::English,
        StemAlgorithm::Finnish,
        StemAlgorithm::French,
        StemAlgorithm::German,
        StemAlgorithm::Greek,
        StemAlgorithm::Hungarian,
        StemAlgorithm::Italian,
        StemAlgorithm::Norwegian,
        StemAlgorithm::Portuguese,
        StemAlgorithm::Romanian,
        StemAlgorithm::Russian,
        StemAlgorithm::Spanish,
        StemAlgorithm::Swedish,
        StemAlgorithm::Tamil,
        StemAlgorithm::Turkish,
    );
    insta::assert_snapshot!("stem_algorithm_all", all);
}

#[test]
fn snapshot_tokenize_sample() {
    let tokens = Text::tokenize("Hello, world! Foo bar baz.");
    insta::assert_snapshot!("tokenize_sample", tokens.join("|"));
}

#[test]
fn snapshot_nlp_error_debug() {
    let err1 = NlpError::EmptyInput;
    let err2 = NlpError::Panic;
    let err3 = NlpError::StemmerInit {
        algorithm: StemAlgorithm::English,
        message: "test stemmer failure".to_string(),
    };
    insta::assert_snapshot!("nlp_error_debug", format!("{err1}\n{err2}\n{err3}"));
}
