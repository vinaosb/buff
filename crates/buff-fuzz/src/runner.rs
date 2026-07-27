//! The [`run`] entry point — drive a property closure with random inputs.
//!
//! [`run`] takes a [`Strategy`], an iteration count, and a `Fn(i64) -> bool`
//! closure. It generates `iterations` random inputs according to the
//! strategy, invokes the closure for each, and accumulates failures in
//! the returned [`FuzzSummary`].
//!
//! # Why a closure that takes `i64` (not a typed payload)
//!
//! The MVP lowers Buff closures (e.g. `{ x => x >= 0 }`) to Rust
//! closures. Buff's `Int` type lowers to `i64`, so the simplest closure
//! shape is `Fn(i64) -> bool`. The runner therefore projects every
//! strategy's payload onto `i64` before invoking the property:
//!
//! - `Strategy.int(min, max)` → the generated `i64` itself.
//! - `Strategy.float(min, max)` → the bit pattern of the `f64`
//!   reinterpreted as `i64`.
//! - `Strategy.bool()` → `0` or `1`.
//! - `Strategy.string(max_len)` → a length in `0..max_len`.
//! - `Strategy.bytes(max_len)` → a length in `0..max_len`.
//!
//! # Hard-rule compliance
//!
//! No `unwrap` / `expect` / `panic!` in the runner (project hard rule).
//! The `proptest::test_runner::TestRunner` is constructed via
//! `Config::default()` and proptest's `Strategy::new_tree` API is used
//! directly to generate values; failures are folded into the
//! [`FuzzSummary`] rather than propagated as panics.

use proptest::strategy::Strategy as PropStrategy;
use proptest::test_runner::{Config, TestRunner};

use crate::strategy::Strategy;
use crate::FuzzError;

const MAX_RECORDED_FAILURES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzSummary {
    pub iterations: u32,
    pub failures: Vec<i64>,
}

impl FuzzSummary {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    pub fn failed_count(&self) -> usize {
        self.failures.len()
    }
}

pub fn run<F>(strategy: &Strategy, iterations: u32, property: F) -> Result<FuzzSummary, FuzzError>
where
    F: Fn(i64) -> bool,
{
    if iterations == 0 {
        return Err(FuzzError::invalid_iterations(0));
    }
    strategy.validate()?;

    let mut runner = TestRunner::new(Config {
        cases: iterations,
        ..Config::default()
    });
    let prop_strategy = build_proptest_strategy(strategy);

    let mut failures: Vec<i64> = Vec::new();
    for _ in 0..iterations {
        // BUG FIX (P6.2): previously cloned the runner inside the loop, which
        // snapshot the RNG state on every iteration → every generated value
        // was identical → the failing-property assertion never fired. Use the
        // runner directly so its RNG advances naturally across iterations.
        let tree = match prop_strategy.new_tree(&mut runner) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let value = tree.current();
        if !property(value) && failures.len() < MAX_RECORDED_FAILURES {
            failures.push(value);
        }
    }

    Ok(FuzzSummary {
        iterations,
        failures,
    })
}

fn build_proptest_strategy(strategy: &Strategy) -> proptest::strategy::BoxedStrategy<i64> {
    match strategy {
        Strategy::Int { min, max } => (*min..=*max).boxed(),
        Strategy::Float { min, max } => {
            let min_b = min.to_bits() as i64;
            let max_b = max.to_bits() as i64;
            let lo = min_b.min(max_b);
            let hi = max_b.max(max_b);
            (lo..=hi).boxed()
        }
        Strategy::Bool => (0_i64..=1).boxed(),
        Strategy::String { max_len } => (0..(*max_len as i64)).boxed(),
        Strategy::Bytes { max_len } => (0..(*max_len as i64)).boxed(),
    }
}
