//! `buff-fuzz` — Property-based fuzzing framework for the Buff language.
//!
//! Provides [`Strategy`] (a value describing how to generate random inputs)
//! and the [`run`] entry point (drive a closure with random inputs from a
//! strategy, recording failures). Plus a codegen-time helper
//! [`lower_fuzz_harness`] that emits a Buff `@fuzz func` body as a
//! `syn::Item` (the future `@fuzz`-attribute integration shape).
//!
//! # Why this exists
//!
//! Property-based testing catches edge cases unit tests miss by feeding
//! randomly-generated inputs to a property assertion. Every major language
//! ecosystem ships one (Rust: [`proptest`](https://docs.rs/proptest/) /
//! [`quickcheck`](https://docs.rs/quickcheck/), Python: `hypothesis`,
//! Haskell: `QuickCheck`). T22 (API compatibility spike), T23 (flagship
//! tests), and security-critical parsers (lexer/parser/hash/crypto)
//! all benefit from property tests.
//!
//! # Design (per T3 macro spike DEFER outcome)
//!
//! Rust has no way to auto-generate test harnesses without procedural
//! macros. The T3 spike deferred the macro system
//! (`.sisyphus/decisions/macro-system-v1x.md`) and recommended runtime
//! workarounds. `buff-fuzz` follows that recommendation — mirroring the
//! `buff-mock` (T25) pattern exactly:
//!
//! 1. The **runtime API** ([`Strategy`], [`run`], [`FuzzSummary`]) is a
//!    pure-Rust library usable directly from any test.
//! 2. The **codegen helper** [`lower_fuzz_harness`] emits the test
//!    harness as a `syn::Item` — `buff-lang-codegen-rust` can call it in
//!    the future to expand `@fuzz func name(input) { ... }` into the
//!    matching `buff_fuzz::run(...)` boilerplate. (Future integration;
//!    the MVP ships WITHOUT requiring parser/AST/codegen-rust changes.)
//!
//! # Why `proptest` (NOT `cargo-fuzz` / `afl.rs`)
//!
//! The plan spec (`.sisyphus/plans/buff-v1x-frameworks.md` task T27)
//! originally named libFuzzer. We deliberately substitute `proptest`
//! because libFuzzer links a C/C++ shim via `cc-rs` — the same class of
//! failure that pushed the hand-rolled lexer/parser per AGENTS.md.
//! `proptest` is pure-Rust, compiles cleanly on this Windows MSVC host,
//! and ships the same property-based surface (random input generation +
//! shrinking on failure).
//!
//! # Quick start
//!
//! ```ignore
//! use buff_fuzz::{run, Strategy};
//!
//! // Property: for every i64 in 0..=100, the value squared is non-negative.
//! let strategy = Strategy::int(0, 100);
//! let summary = run(&strategy, 256, |n| n * n >= 0);
//! assert_eq!(summary.failures.len(), 0);
//! ```
//!
//! # Strategy combinators (future work)
//!
//! The MVP ships primitive strategies only (`int` / `float` / `bool` /
//! `string` / `bytes`). A follow-up task will add `Strategy.vector(s)`,
//! `Strategy.one_of(vec)`, and `s.filter(pred)` / `s.map(f)` combinators
//! once a use case from T22 / T23 / T135 surfaces.
//!
//! # References
//!
//! - Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T27.
//! - Macro spike decision: `.sisyphus/decisions/macro-system-v1x.md`.
//! - `proptest` (Rust): <https://docs.rs/proptest/latest/proptest/>.
//! - `hypothesis` (Python): <https://hypothesis.readthedocs.io/>.

pub mod error;
pub mod lowering;
pub mod runner;
pub mod strategy;

pub use error::{FuzzError, FuzzResult};
pub use lowering::lower_fuzz_harness;
pub use runner::{run, FuzzSummary};
pub use strategy::{FuzzValue, Strategy};

/// Crate version (matches `Cargo.toml`).
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod smoke_tests {
    //! Smoke tests at the crate root — assert that the public API
    //! surface compiles and the basic shape works. Full behavioral
    //! coverage lives in `tests/api_tests.rs` + `tests/property_tests.rs`.

    use super::*;

    #[test]
    fn smoke_strategy_int_constructs() {
        let s = Strategy::int(0, 10);
        assert!(matches!(s, Strategy::Int { min: 0, max: 10 }));
    }

    #[test]
    fn smoke_run_passing_property() {
        let s = Strategy::int(0, 100);
        let summary = run(&s, 32, |n| n >= 0 && n <= 100);
        assert_eq!(summary.failures.len(), 0);
        assert_eq!(summary.iterations, 32);
    }

    #[test]
    fn smoke_run_failing_property_records_failure() {
        let s = Strategy::int(0, 100);
        let summary = run(&s, 256, |n| n < 50);
        assert!(summary.failures.len() > 0);
        for value in &summary.failures {
            assert!(*value >= 50);
        }
    }
}
