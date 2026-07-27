//! `buff-fake` — fake data generation for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`fake`](https://docs.rs/fake/latest/fake/)
//! crate. Provides `Faker.name()`, `Faker.email()`, `Faker.address()`,
//! `Faker.phone()`, `Faker.uuid()`, `Faker.lorem(words)`,
//! `Faker.int(min, max)`, `Faker.datetime(range)`.
//! Locales: en-US, pt-BR.
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Faker`, `FakerError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | All methods return owned `String` / `i64` / `chrono::DateTime<chrono::Utc>`. |
//! | R3 — Error mapping | Fallible ops return `Result<T, FakerError>`. |
//! | R4 — Thread safety | `Faker` is `Send + Sync` (no interior mutability). |
//! | R5 — Lifetime hiding | No public lifetime parameters. All returns are owned. |
//! | R6 — Panic boundary | All public methods wrap bodies in `catch_unwind`. |

use std::panic::{catch_unwind, AssertUnwindSafe};

// fake 2.x API drift: `Fake` trait (provides `.fake_with_rng()`) must be
// imported at module scope; previously it was pulled in transitively.
use fake::Fake;
// rand 0.8 API drift: `SeedableRng` trait (provides `StdRng::from_entropy`
// and `StdRng::seed_from_u64`) hoisted to module scope; previously each
// constructor had a local `use rand::SeedableRng;`. We use the workspace
// alias `rand_08` (NOT `rand = "0.9"`) because fake 2.10's `Rng` trait
// aliasing ties to rand_core 0.6 — see Cargo.toml comment for full rationale.
use rand_08::SeedableRng;

pub mod error;

pub use error::FakerError;

/// A fake-data generator with locale support.
///
/// Constructed via [`Faker::new`] (defaults to en-US) or
/// [`Faker::with_locale`] (en-US or pt-BR). Each method call
/// produces a plausible random value in the configured locale.
///
/// Internally wraps a seeded `rand::rngs::StdRng` for reproducible
/// output when the same seed is used (via `Faker::with_seed`).
#[derive(Debug, Clone)]
pub struct Faker {
    locale: FakerLocale,
    rng: rand_08::rngs::StdRng,
}

/// Supported locales for fake data generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FakerLocale {
    EnUs,
    PtBr,
}

impl Faker {
    /// Create a new `Faker` with the default locale (en-US) and
    /// a randomly-seeded RNG.
    pub fn new() -> Self {
        Faker {
            locale: FakerLocale::EnUs,
            rng: rand_08::rngs::StdRng::from_entropy(),
        }
    }

    /// Create a new `Faker` with the given locale and a randomly-seeded RNG.
    pub fn with_locale(locale: FakerLocale) -> Self {
        Faker {
            locale,
            rng: rand_08::rngs::StdRng::from_entropy(),
        }
    }

    /// Create a new `Faker` with the given locale and seed for reproducible output.
    pub fn with_seed(locale: FakerLocale, seed: u64) -> Self {
        Faker {
            locale,
            rng: rand_08::rngs::StdRng::seed_from_u64(seed),
        }
    }

    /// Generate a random full name.
    pub fn name(&mut self) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| match self.locale {
            FakerLocale::EnUs => {
                use fake::faker::name::en::Name;
                Name().fake_with_rng(&mut self.rng)
            }
            FakerLocale::PtBr => {
                use fake::faker::name::pt_br::Name;
                Name().fake_with_rng(&mut self.rng)
            }
        }));
        result.unwrap_or_else(|_| String::new())
    }

    /// Generate a random email address.
    pub fn email(&mut self) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| match self.locale {
            FakerLocale::EnUs => {
                use fake::faker::internet::en::SafeEmail;
                SafeEmail().fake_with_rng(&mut self.rng)
            }
            FakerLocale::PtBr => {
                use fake::faker::internet::pt_br::SafeEmail;
                SafeEmail().fake_with_rng(&mut self.rng)
            }
        }));
        result.unwrap_or_else(|_| String::new())
    }

    /// Generate a random street address.
    pub fn address(&mut self) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| match self.locale {
            FakerLocale::EnUs => {
                use fake::faker::address::en::{BuildingNumber, StreetName, StreetSuffix};
                format!(
                    "{} {} {}",
                    BuildingNumber().fake_with_rng::<String, _>(&mut self.rng),
                    StreetName().fake_with_rng::<String, _>(&mut self.rng),
                    StreetSuffix().fake_with_rng::<String, _>(&mut self.rng)
                )
            }
            FakerLocale::PtBr => {
                use fake::faker::address::pt_br::{BuildingNumber, StreetName, StreetSuffix};
                format!(
                    "{} {} {}",
                    StreetName().fake_with_rng::<String, _>(&mut self.rng),
                    BuildingNumber().fake_with_rng::<String, _>(&mut self.rng),
                    StreetSuffix().fake_with_rng::<String, _>(&mut self.rng)
                )
            }
        }));
        result.unwrap_or_else(|_| String::new())
    }

    /// Generate a random phone number.
    pub fn phone(&mut self) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| match self.locale {
            FakerLocale::EnUs => {
                use fake::faker::phone_number::en::PhoneNumber;
                PhoneNumber().fake_with_rng(&mut self.rng)
            }
            FakerLocale::PtBr => {
                use fake::faker::phone_number::pt_br::PhoneNumber;
                PhoneNumber().fake_with_rng(&mut self.rng)
            }
        }));
        result.unwrap_or_else(|_| String::new())
    }

    /// Generate a random UUID v4 string.
    pub fn uuid(&mut self) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| {
            use fake::uuid::UUIDv4;
            UUIDv4.fake_with_rng(&mut self.rng)
        }));
        result.unwrap_or_else(|_| String::new())
    }

    /// Generate a random lorem-ipsum text with the given number of words.
    pub fn lorem(&mut self, word_count: usize) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| match self.locale {
            FakerLocale::EnUs => {
                use fake::faker::lorem::en::Words;
                let words: Vec<String> =
                    Words(word_count..(word_count + 1)).fake_with_rng(&mut self.rng);
                words.join(" ")
            }
            FakerLocale::PtBr => {
                use fake::faker::lorem::pt_br::Words;
                let words: Vec<String> =
                    Words(word_count..(word_count + 1)).fake_with_rng(&mut self.rng);
                words.join(" ")
            }
        }));
        result.unwrap_or_else(|_| String::new())
    }

    /// Generate a random integer in [min, max] (inclusive).
    pub fn int(&mut self, min: i64, max: i64) -> i64 {
        // fake 2.x API drift: `fake::faker::number::en::Number` was removed
        // (the `number` module now exposes only `Digit` and
        // `NumberWithFormat`). The Buff surface only ever needed a uniform
        // integer in [min, max], so we go straight to `rand`'s `gen_range`,
        // which subsumes the old generate-then-clamp dance.
        use rand_08::Rng;
        let result = catch_unwind(AssertUnwindSafe(|| self.rng.gen_range(min..=max)));
        result.unwrap_or(min)
    }

    /// Generate a random datetime within the given range.
    /// `start` and `end` are RFC 3339 strings.
    pub fn datetime(&mut self, start: &str, end: &str) -> Result<String, FakerError> {
        let start_owned = start.to_string();
        let end_owned = end.to_string();
        let result = catch_unwind(AssertUnwindSafe(move || {
            let start_dt = chrono::DateTime::parse_from_rfc3339(&start_owned)
                .map_err(|e| FakerError::InvalidDateRange(format!("invalid start: {e}")))?;
            let end_dt = chrono::DateTime::parse_from_rfc3339(&end_owned)
                .map_err(|e| FakerError::InvalidDateRange(format!("invalid end: {e}")))?;
            if end_dt <= start_dt {
                return Err(FakerError::InvalidDateRange(
                    "end must be after start".to_string(),
                ));
            }
            use fake::faker::chrono::en::DateTime;
            let dt: chrono::DateTime<chrono::Utc> = DateTime().fake_with_rng(&mut self.rng);
            // Clamp to the requested range
            let clamped = if dt < start_dt {
                start_dt.into()
            } else if dt > end_dt {
                end_dt.into()
            } else {
                dt
            };
            Ok(clamped.to_rfc3339())
        }));
        match result {
            Ok(Ok(s)) => Ok(s),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(FakerError::Panic),
        }
    }
}

impl Default for Faker {
    fn default() -> Self {
        Faker::new()
    }
}
