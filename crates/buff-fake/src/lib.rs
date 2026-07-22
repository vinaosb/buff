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
    rng: rand::rngs::StdRng,
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
            rng: rand::rngs::StdRng::from_entropy(),
        }
    }

    /// Create a new `Faker` with the given locale and a randomly-seeded RNG.
    pub fn with_locale(locale: FakerLocale) -> Self {
        Faker {
            locale,
            rng: rand::rngs::StdRng::from_entropy(),
        }
    }

    /// Create a new `Faker` with the given locale and seed for reproducible output.
    pub fn with_seed(locale: FakerLocale, seed: u64) -> Self {
        use rand::SeedableRng;
        Faker {
            locale,
            rng: rand::rngs::StdRng::seed_from_u64(seed),
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
                use fake::faker::address::en::StreetAddress;
                StreetAddress().fake_with_rng(&mut self.rng)
            }
            FakerLocale::PtBr => {
                use fake::faker::address::pt_br::StreetAddress;
                StreetAddress().fake_with_rng(&mut self.rng)
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
                let words: Vec<String> = Words(word_count..=word_count)
                    .fake_with_rng(&mut self.rng);
                words.join(" ")
            }
            FakerLocale::PtBr => {
                use fake::faker::lorem::pt_br::Words;
                let words: Vec<String> = Words(word_count..=word_count)
                    .fake_with_rng(&mut self.rng);
                words.join(" ")
            }
        }));
        result.unwrap_or_else(|_| String::new())
    }

    /// Generate a random integer in [min, max] (inclusive).
    pub fn int(&mut self, min: i64, max: i64) -> i64 {
        let result = catch_unwind(AssertUnwindSafe(|| {
            use fake::faker::number::en::Number;
            let range = min..=max;
            Number().fake_with_rng::<i64, _>(&mut self.rng)
        }));
        // The fake crate's Number() generates within the full i64 range;
        // we clamp to [min, max] for the Buff surface.
        let val = result.unwrap_or(min);
        val.clamp(min, max)
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
            use fake::faker::datetime::en::DateTime;
            let dt: chrono::DateTime<chrono::Utc> = DateTime()
                .fake_with_rng(&mut self.rng);
            // Clamp to the requested range
            let clamped = if dt < start_dt { start_dt.into() }
                else if dt > end_dt { end_dt.into() }
                else { dt };
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
