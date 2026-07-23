//! `buff-nlp` — text processing / NLP for the Buff language.
//!
//! Pure-Rust MVP wrapping three mature Rust crates via a safe FFI
//! boundary per the [T4 FFI guide](../buff-lang-ffi-guide/GUIDE.md):
//!
//! | Concern | Backend crate |
//! |---------|---------------|
//! | Language detection (69+ langs) | `whatlang` 0.16 (trigram statistical classifier) |
//! | Snowball stemming (18 langs)   | `rust-stemmers` 1.2 (pure-Rust Snowball reference) |
//! | Word + sentence segmentation  | `unicode-segmentation` 1.12 (UAX #29) |
//!
//! # Pipeline
//!
//! ```text
//!   text ──┬──▶ Text.detect_language() ──▶ Option<Language>
//!         ├──▶ Text.stem(word, algorithm) ──▶ Result<String, NlpError>
//!         ├──▶ Text.tokenize() ──▶ Vec<String>   (UAX #29 word boundaries)
//!         └──▶ Text.sentences() ──▶ Vec<String>  (UAX #29 sentence boundaries)
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Text`, `Language`, `StemAlgorithm`, `NlpError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `detect_language` returns owned `Option<Language>`. `stem` returns owned `String`. `tokenize` / `sentences` return owned `Vec<String>`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, NlpError>`. `rust_stemmers::Error` mapped via `From`. |
//! | R4 — Thread safety | `Text` / `Language` / `StemAlgorithm` are `Copy + Send + Sync`. `NlpError` is `Send + Sync`. |
//! | R5 — Lifetime hiding | No public lifetime parameters. All inputs are `&str` borrowed; all outputs are owned. |
//! | R6 — Panic boundary | `stem` wraps its body in `catch_unwind`. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Every fallible op returns `Result`.

pub mod error;

pub use error::NlpError;

use std::panic::{catch_unwind, AssertUnwindSafe};
use unicode_segmentation::UnicodeSegmentation;

/// A detected natural language (wraps `whatlang::Lang`).
///
/// Constructed by [`Text::detect_language`]. Carries no allocation
/// overhead beyond the wrapped enum — `code()` / `name()` clone the
/// inner `&'static str` into an owned `String` at the boundary so the
/// Buff surface is uniformly owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Language(whatlang::Lang);

impl Language {
    /// The ISO 639-3 language code (three lowercase letters, e.g.
    /// `"eng"`, `"por"`, `"fra"`). Owned `String` per the FFI guide
    /// R5 (no lifetime leakage).
    pub fn code(&self) -> String {
        self.0.code().to_string()
    }

    /// The human-readable English name of the language (e.g.
    /// `"English"`, `"Portuguese"`, `"French"`). Owned `String`.
    pub fn name(&self) -> String {
        self.0.name().to_string()
    }

    /// pub(crate) constructor — only [`Text::detect_language`] builds
    /// `Language` values. Used internally by tests; not Buff-visible.
    pub(crate) fn from_whatlang(lang: whatlang::Lang) -> Self {
        Language(lang)
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.0.name(), self.0.code())
    }
}

/// A Snowball stemming algorithm selection (18 supported languages).
///
/// Maps 1:1 to `rust_stemmers::Algorithm`. The Snowball project
/// (https://snowballstem.org/) maintains the canonical reference
/// implementations; `rust-stemmers` is the pure-Rust port used here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StemAlgorithm {
    Arabic,
    Danish,
    Dutch,
    English,
    Finnish,
    French,
    German,
    Greek,
    Hungarian,
    Italian,
    Norwegian,
    Portuguese,
    Romanian,
    Russian,
    Spanish,
    Swedish,
    Tamil,
    Turkish,
}

impl StemAlgorithm {
    /// Parse a lowercase Snowball algorithm name (e.g. `"english"`,
    /// `"portuguese"`) into a [`StemAlgorithm`]. Returns `None` for
    /// unrecognised names. Case-insensitive.
    ///
    /// Used by the codegen lowering layer (Buff's `Text.stem(word,
    /// algorithm: "english")` surface accepts a String for the
    /// algorithm arg per the cross-language Snowball convention).
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_ascii_lowercase().as_str() {
            "arabic" => Some(StemAlgorithm::Arabic),
            "danish" => Some(StemAlgorithm::Danish),
            "dutch" => Some(StemAlgorithm::Dutch),
            "english" => Some(StemAlgorithm::English),
            "finnish" => Some(StemAlgorithm::Finnish),
            "french" => Some(StemAlgorithm::French),
            "german" => Some(StemAlgorithm::German),
            "greek" => Some(StemAlgorithm::Greek),
            "hungarian" => Some(StemAlgorithm::Hungarian),
            "italian" => Some(StemAlgorithm::Italian),
            "norwegian" => Some(StemAlgorithm::Norwegian),
            "portuguese" => Some(StemAlgorithm::Portuguese),
            "romanian" => Some(StemAlgorithm::Romanian),
            "russian" => Some(StemAlgorithm::Russian),
            "spanish" => Some(StemAlgorithm::Spanish),
            "swedish" => Some(StemAlgorithm::Swedish),
            "tamil" => Some(StemAlgorithm::Tamil),
            "turkish" => Some(StemAlgorithm::Turkish),
            _ => None,
        }
    }

    /// pub(crate) — map to the underlying `rust_stemmers::Algorithm`.
    /// Not Buff-visible (the codegen layer never crosses this bound
    /// directly; it goes through `Text::stem` which dispatches here).
    pub(crate) fn to_rust_stemmers(self) -> rust_stemmers::Algorithm {
        match self {
            StemAlgorithm::Arabic => rust_stemmers::Algorithm::Arabic,
            StemAlgorithm::Danish => rust_stemmers::Algorithm::Danish,
            StemAlgorithm::Dutch => rust_stemmers::Algorithm::Dutch,
            StemAlgorithm::English => rust_stemmers::Algorithm::English,
            StemAlgorithm::Finnish => rust_stemmers::Algorithm::Finnish,
            StemAlgorithm::French => rust_stemmers::Algorithm::French,
            StemAlgorithm::German => rust_stemmers::Algorithm::German,
            StemAlgorithm::Greek => rust_stemmers::Algorithm::Greek,
            StemAlgorithm::Hungarian => rust_stemmers::Algorithm::Hungarian,
            StemAlgorithm::Italian => rust_stemmers::Algorithm::Italian,
            StemAlgorithm::Norwegian => rust_stemmers::Algorithm::Norwegian,
            StemAlgorithm::Portuguese => rust_stemmers::Algorithm::Portuguese,
            StemAlgorithm::Romanian => rust_stemmers::Algorithm::Romanian,
            StemAlgorithm::Russian => rust_stemmers::Algorithm::Russian,
            StemAlgorithm::Spanish => rust_stemmers::Algorithm::Spanish,
            StemAlgorithm::Swedish => rust_stemmers::Algorithm::Swedish,
            StemAlgorithm::Tamil => rust_stemmers::Algorithm::Tamil,
            StemAlgorithm::Turkish => rust_stemmers::Algorithm::Turkish,
        }
    }
}

impl std::fmt::Display for StemAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Lowercase snake_case matches the Snowball convention and the
        // upstream `rust_stemmers::Algorithm` Debug form. Stable for
        // the Buff surface (cosmetic; used only by Display snapshots).
        let s = match self {
            StemAlgorithm::Arabic => "arabic",
            StemAlgorithm::Danish => "danish",
            StemAlgorithm::Dutch => "dutch",
            StemAlgorithm::English => "english",
            StemAlgorithm::Finnish => "finnish",
            StemAlgorithm::French => "french",
            StemAlgorithm::German => "german",
            StemAlgorithm::Greek => "greek",
            StemAlgorithm::Hungarian => "hungarian",
            StemAlgorithm::Italian => "italian",
            StemAlgorithm::Norwegian => "norwegian",
            StemAlgorithm::Portuguese => "portuguese",
            StemAlgorithm::Romanian => "romanian",
            StemAlgorithm::Russian => "russian",
            StemAlgorithm::Spanish => "spanish",
            StemAlgorithm::Swedish => "swedish",
            StemAlgorithm::Tamil => "tamil",
            StemAlgorithm::Turkish => "turkish",
        };
        f.write_str(s)
    }
}

/// Namespace marker for the text-processing API. The struct itself
/// carries no state; all functionality is exposed via associated
/// functions ([`Text::detect_language`] / [`Text::stem`] /
/// [`Text::tokenize`] / [`Text::sentences`]).
///
/// Mirrors the `Archive` / `Log` / `Toml` / `Config` namespace-only
/// prelude-type pattern — `Text` is never instantiated.
pub struct Text;

impl Text {
    /// Detect the dominant natural language of `input` via trigram
    /// statistical classification (wraps `whatlang::Detector::detect`).
    ///
    /// Returns `None` when the input is empty, too short, or contains
    /// no recognisable language signal (e.g. pure numbers / URLs /
    /// whitespace). whatlang's trigram classifier needs ~10+ word
    /// characters to reach >90% accuracy; shorter inputs may produce
    /// `None` or low-confidence detections.
    ///
    /// Wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn detect_language(input: &str) -> Option<Language> {
        if input.trim().is_empty() {
            return None;
        }
        let input_owned = input.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            whatlang::Detector::detect(&input_owned)
                .map(|info| Language::from_whatlang(info.lang()))
        }));
        match result {
            Ok(Some(lang)) => Some(lang),
            Ok(None) => None,
            Err(_) => None,
        }
    }

    /// Reduce `word` to its Snowball stem for `algorithm` (e.g.
    /// `"running"` → `"run"` for English; `"correndo"` → `"corr"`
    /// for Portuguese). Wraps `rust_stemmers::Stemmer::new(alg).stem(word)`.
    ///
    /// Returns [`NlpError::EmptyInput`] for empty input. Returns
    /// [`NlpError::StemmerInit`] if the underlying stemmer fails to
    /// construct (defensive — every public `StemAlgorithm` variant
    /// maps to a known-good algorithm).
    ///
    /// Wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn stem(word: &str, algorithm: StemAlgorithm) -> Result<String, NlpError> {
        if word.is_empty() {
            return Err(NlpError::EmptyInput);
        }
        let word_owned = word.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| stem_inner(&word_owned, algorithm)));
        match result {
            Ok(Ok(stemmed)) => Ok(stemmed),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(NlpError::Panic),
        }
    }

    /// Tokenize `input` into word tokens via UAX #29 Unicode word
    /// segmentation (wraps `unicode_segmentation::UnicodeSegmentation::
    /// unicode_words`). Punctuation, whitespace, and other non-word
    /// segments are dropped; the result is a `Vec<String>` of just
    /// the word tokens (suitable for downstream stemming, frequency
    /// analysis, or full-text indexing).
    ///
    /// Returns an empty `Vec` for empty / whitespace-only input.
    pub fn tokenize(input: &str) -> Vec<String> {
        UnicodeSegmentation::unicode_words(input)
            .map(str::to_string)
            .collect()
    }

    /// Segment `input` into sentences via UAX #29 Unicode sentence
    /// segmentation (wraps `unicode_segmentation::UnicodeSegmentation::
    /// unicode_sentences`). Sentence boundaries follow the Unicode
    /// Standard Annex #29 default segmentation rules — handles `.`,
    /// `!`, `?` terminators with proper exception handling for
    /// abbreviations, decimals, and other ambiguous terminators.
    ///
    /// Returns an empty `Vec` for empty / whitespace-only input.
    /// The returned strings preserve leading / trailing whitespace
    /// (caller may `.trim()` if desired).
    pub fn sentences(input: &str) -> Vec<String> {
        UnicodeSegmentation::unicode_sentences(input)
            .map(str::to_string)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Internal helpers (crate-private). None of these use catch_unwind — the
// public entry points wrap the call sites.
// ---------------------------------------------------------------------------

fn stem_inner(word: &str, algorithm: StemAlgorithm) -> Result<String, NlpError> {
    let stemmer = rust_stemmers::Stemmer::new(algorithm.to_rust_stemmers()).map_err(|e| {
        NlpError::StemmerInit {
            algorithm,
            message: e.to_string(),
        }
    })?;
    Ok(stemmer.stem(word).into_owned())
}

impl Default for Text {
    /// `Text` impls `Default` so the codegen lowering can use
    /// `unwrap_or_default()` panic-free on `Result<Text, NlpError>`
    /// returning methods (matches the DataFrame / Image precedent).
    /// The default `Text` is a no-op namespace marker — calling any
    /// method on it returns the empty case (`None` / `Err(EmptyInput)`
    /// / empty `Vec`).
    fn default() -> Self {
        Text
    }
}
