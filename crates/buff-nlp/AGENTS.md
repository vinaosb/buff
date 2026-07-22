# buff-nlp

Text processing / NLP for the Buff language. Pure-Rust MVP wrapping three mature Rust crates ([`whatlang`](https://crates.io/crates/whatlang), [`rust-stemmers`](https://crates.io/crates/rust-stemmers), [`unicode-segmentation`](https://crates.io/crates/unicode-segmentation)) via a safe FFI boundary per the [T4 FFI guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T46 v1.18 frameworks wave 7).

## STRUCTURE

```
buff-nlp/
├── Cargo.toml                       # whatlang + rust-stemmers + unicode-segmentation + thiserror + insta deps
├── src/
│   ├── lib.rs                       # Text namespace + Language + StemAlgorithm (~280 LOC)
│   └── error.rs                     # NlpError enum (~70 LOC)
├── examples/
│   ├── nlp_detect_language.rs       # detect_language across 5 languages
│   ├── nlp_stem_words.rs            # English + Portuguese stemming
│   ├── nlp_tokenize.rs              # tokenize + sentences
│   └── nlp/
│       ├── nlp_detect_language.buff # Buff-side forward-decls (matches .rs)
│       ├── nlp_stem_words.buff
│       └── nlp_tokenize.buff
└── tests/
    └── core.rs                      # 20 unit tests + 5 insta snapshots (~220 LOC)
```

Total: ~650 LOC (well under the 2000 LOC T46 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new stemmer language | `src/lib.rs::StemAlgorithm` (add variant + 2 match arms in `to_rust_stemmers` + `Display`) + test in `tests/core.rs` |
| Add a new segmentation op (e.g. paragraph / line) | `src/lib.rs::Text` (add `pub fn` + match arm in codegen `lower_prelude_type_assoc_fn`) |
| Add a new error variant | `src/error.rs` + `From` impl if it wraps an underlying error |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeAssocFn + `assoc_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` |

## PUBLIC API (10 functions, ≤15 cap)

### `Text` namespace (4 functions, namespace-only — never instantiated)
- `Text::detect_language(input) -> Option<Language>` — whatlang trigram classifier
- `Text::stem(word, algorithm) -> Result<String, NlpError>` — Snowball stemmer
- `Text::tokenize(input) -> Vec<String>` — UAX #29 word segmentation
- `Text::sentences(input) -> Vec<String>` — UAX #29 sentence segmentation

### `Language` (2 functions)
- Accessors: `code`, `name`
- (constructible only via `Text::detect_language`)

### `StemAlgorithm` (1 function — Display impl, plus enum variants)
- Variants: `Arabic`, `Danish`, `Dutch`, `English`, `Finnish`, `French`, `German`, `Greek`, `Hungarian`, `Italian`, `Norwegian`, `Portuguese`, `Romanian`, `Russian`, `Spanish`, `Swedish`, `Tamil`, `Turkish` (18 total)

### `NlpError` (3 variants)
- `EmptyInput`, `StemmerInit`, `Panic`

## CONVENTIONS

- **Pure-Rust only**: `whatlang` 0.16 (no native deps), `rust-stemmers` 1.2 (pure-Rust Snowball reference — NOT a C binding), `unicode-segmentation` 1.12 (already pinned for T124 String segmentation). Matches the "no C library, no Docker" hard rule from AGENTS.md.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in non-test code. Every fallible op returns `Result<_, NlpError>`.
- **`catch_unwind` boundary**: `stem` wraps its body in `catch_unwind` per FFI guide R6. `detect_language` uses `catch_unwind` defensively (whatlang is panic-free in practice; the wrapper is defense-in-depth). `tokenize` / `sentences` are pure iterators over `&str` (no allocation of unbounded structures, no panic vectors) so the `catch_unwind` wrapper is omitted for them — documented inline.
- **Buff §6 / §7 compliance**: NO `_async` suffix (synchronous surface); no `Type.create()` / `Type.build()` (the `Text` namespace is never instantiated; `Language` is constructed only via `Text.detect_language`).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `whatlang` | Upstream language detector. `buff-nlp` is a safe wrapper; never re-exports `whatlang::*` types directly. |
| `rust-stemmers` | Upstream Snowball stemmer. Wrapped via `Text::stem`. |
| `unicode-segmentation` | Upstream UAX #29 segmentation. Wrapped via `Text::tokenize` / `Text::sentences`. Already pinned at workspace level for T124 String segmentation. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Text` (namespace-only — `buff_type()` returns `Type::Void`) + 4 `PreludeAssocFn` variants (DetectLanguage / Stem / Tokenize / Sentences). `is_namespace_only()` includes `Text`. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::lower_prelude_type_assoc_fn` gains the `(Text, DetectLanguage)` / `(Text, Stem)` / `(Text, Tokenize)` / `(Text, Sentences)` arms. `program_uses_namespace("Text")` records `buff-nlp` + `whatlang` + `rust-stemmers` + `unicode-segmentation` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **No external corpus / model files**: whatlang ships its trigram tables compiled into the binary; rust-stemmers ships its Snowball algorithms compiled in. No runtime file loading, no network calls.
- **Detect-language accuracy**: whatlang's trigram classifier reaches >90% accuracy on 50+ character samples in the 69 supported languages. Shorter samples (single words, numbers, punctuation-only) may produce `None` or low-confidence guesses — the API returns `Option<Language>` to surface this.
- **Snowball stemmer is not a lemmatizer**: stemming reduces a word to a stem that may not be a real word (e.g. English "happily" → "happili"; Portuguese "correndo" → "corr"). Stemming is faster + simpler than lemmatization; for true lemma lookup, wrap a future `buff-lemmatizer` crate (deferred to v1.20+).
- **UAX #29 segmentation is Unicode-aware**: handles Unicode word/sentence boundaries across all scripts (Latin, CJK, Cyrillic, Arabic, etc.). Punctuation and whitespace are dropped from word tokens; sentence tokens preserve their trailing whitespace (caller may `.trim()`).
- **18 stemmer languages** match `rust-stemmers`'s supported set 1:1. Adding a new language requires both an upstream `rust-stemmers` release AND a `StemAlgorithm` variant here (defensive — never silently map an unknown algorithm to a fallback).
- **`Language` is `Copy + Eq + Hash`**: whatlang's `Lang` enum is `Copy`, so `Language` derives the same. Safe to use as a HashMap key for frequency counting.
