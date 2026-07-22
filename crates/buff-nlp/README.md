# buff-nlp

> Text processing / NLP for the **Buff** language. Pure-Rust MVP.

`buff-nlp` wraps three mature Rust crates ([`whatlang`](https://crates.io/crates/whatlang), [`rust-stemmers`](https://crates.io/crates/rust-stemmers), [`unicode-segmentation`](https://crates.io/crates/unicode-segmentation)) behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses text processing via the `Text` namespace:

```buff
let lang = Text.detect_language("A raposa marrom salta sobre o cachorro.")
match lang:
    some(l):
        print(l.name(), "(", l.code(), ")")
    none:
        print("(no language detected)")

let stem = Text.stem(word: "running", algorithm: .english)
print(stem)

let tokens = Text.tokenize("Hello, world! Foo bar baz.")
print(tokens)

let sents = Text.sentences("Hello world! How are you? I am fine.")
print(sents)
```

**Status: experimental** (T46 v1.18 frameworks wave 7).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Text` prelude type.

For direct Rust use:

```bash
cargo add buff-nlp --path crates/buff-nlp
```

## Quick start

```rust
use buff_nlp::{StemAlgorithm, Text};

fn main() {
    let lang = Text::detect_language("The quick brown fox jumps over the lazy dog.");
    println!("{:?}", lang.map(|l| (l.name(), l.code())));

    let stem = Text::stem("running", StemAlgorithm::English).unwrap();
    println!("running -> {stem}");

    let tokens = Text::tokenize("Hello, world! Foo bar baz.");
    println!("{tokens:?}");

    let sents = Text::sentences("Hello world! How are you? I am fine.");
    println!("{sents:?}");
}
```

## Public API

### `Text` — namespace-only API (4 entry points)

| Method | Signature | Notes |
|---|---|---|
| `Text::detect_language` | `(&str) -> Option<Language>` | whatlang trigram classifier. Empty input → `None`. |
| `Text::stem` | `(&str, StemAlgorithm) -> Result<String, NlpError>` | Snowball stemmer (18 langs). Empty input → `Err(EmptyInput)`. |
| `Text::tokenize` | `(&str) -> Vec<String>` | UAX #29 word segmentation. Drops punctuation + whitespace. |
| `Text::sentences` | `(&str) -> Vec<String>` | UAX #29 sentence segmentation. Preserves inner whitespace. |

### `Language` — detected natural language

| Method | Signature | Notes |
|---|---|---|
| `language.code` | `() -> String` | ISO 639-3 code (e.g. `"eng"`, `"por"`, `"fra"`). |
| `language.name` | `() -> String` | English name (e.g. `"English"`, `"Portuguese"`). |

### `StemAlgorithm` — 18 supported languages

`Arabic`, `Danish`, `Dutch`, `English`, `Finnish`, `French`, `German`, `Greek`, `Hungarian`, `Italian`, `Norwegian`, `Portuguese`, `Romanian`, `Russian`, `Spanish`, `Swedish`, `Tamil`, `Turkish`.

## Supported backends

| Concern | Backend crate | Notes |
|---|---|---|
| Language detection (69+ langs) | `whatlang` 0.16 | Trigram statistical classifier. Pure-Rust. |
| Snowball stemming (18 langs)   | `rust-stemmers` 1.2 | Pure-Rust Snowball reference. |
| Word + sentence segmentation  | `unicode-segmentation` 1.12 | UAX #29 default segmentation. |

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md):

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Text`, `Language`, `StemAlgorithm`, `NlpError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | All public fns return owned values (`String` / `Vec<String>` / `Option<Language>`). |
| R3 — Error mapping | Every fallible op returns `Result<T, NlpError>`. `rust_stemmers::Error` auto-converts via `From`. |
| R4 — Thread safety | `Text` / `Language` / `StemAlgorithm` are `Copy + Send + Sync`. |
| R5 — Lifetime hiding | No public lifetime parameters. |
| R6 — Panic boundary | `stem` wraps body in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-nlp
cargo clippy -p buff-nlp --all-targets -- -D warnings
cargo fmt -p buff-nlp --check
```

Tests are hermetic: no external network or fixture files. Snapshots via `insta`.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
