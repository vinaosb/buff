# buff-resilience

> Resilience patterns for the **Buff** language. Pure-Rust MVP (retry + circuit breaker + rate limiter + timeout + composable pipeline).

`buff-resilience` ships four composable resilience primitives — Retry, CircuitBreaker, RateLimiter, Timeout — plus a `Pipeline` that chains them in the order Retry → CircuitBreaker → RateLimiter → Timeout. Hand-rolled on `std::time` + `std::thread` behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses the primitives via the `Pipeline` prelude type:

```buff
let retry = RetryPolicy.new(max_attempts: 4, initial_delay_ms: 10, backoff_factor: 2.0)
let breaker = CircuitBreaker.new(failure_threshold: 5, reset_timeout_ms: 30000)
let limiter = RateLimiter.new(requests_per_second: 50.0)
let deadline = Timeout.new(duration_ms: 1000)

let pipeline = Pipeline.new()
    .retry(policy: retry)
    .circuit_breaker(cb: breaker)
    .rate_limiter(rl: limiter)
    .timeout(t: deadline)

let result = pipeline.execute(handler: fetch_upstream)
```

**Status: experimental** (T36 v1.16 frameworks wave 5).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Pipeline` / `RetryPolicy` / `CircuitBreaker` / `RateLimiter` / `Timeout` prelude types.

For direct Rust use:

```bash
cargo add buff-resilience --path crates/buff-resilience
```

## Quick start

```rust
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use buff_resilience::{CircuitBreaker, Pipeline, RateLimiter, RetryPolicy, Timeout};

fn main() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let mut pipeline = Pipeline::new()
        .retry(RetryPolicy::no_delay(3))
        .circuit_breaker(CircuitBreaker::new(5, Duration::from_secs(30)))
        .rate_limiter(RateLimiter::new(50.0))
        .timeout(Timeout::new(Duration::from_secs(1)));

    let result = pipeline.execute(move || {
        let n = c.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Err(String::from("first attempt always fails"))
        } else {
            Ok(7u32)
        }
    });
    assert_eq!(result.unwrap(), 7);
}
```

## Public API

### `RetryPolicy` — exponential backoff

| Method | Signature | Notes |
|---|---|---|
| `RetryPolicy::new` | `(max_attempts, initial_delay, backoff_factor) -> Self` | Saturates `max_attempts` to 1 if 0. |
| `RetryPolicy::no_delay` | `(max_attempts) -> Self` | Convenience: back-to-back retries. |
| `policy.delay_for_attempt` | `(n) -> Duration` | `initial_delay * backoff_factor^(n-2)`, clamped to 60s. |
| `policy.execute` | `(Fn() -> Result<T,E>) -> Result<T, ResilienceError>` | `E: Display`. Returns `Exhausted{attempts, last_error}` if all attempts fail. |

### `CircuitBreaker` — state machine

| Method | Signature | Notes |
|---|---|---|
| `CircuitBreaker::new` | `(failure_threshold, reset_timeout) -> Self` | Starts in `Closed`. |
| `cb.state` | `() -> BreakerState` | `Closed` / `Open` / `HalfOpen`. |
| `cb.failure_count` / `cb.failure_threshold` / `cb.reset_timeout` | `() -> u32` / `u32` / `Duration` | Introspection. |
| `cb.execute` | `(FnOnce() -> Result<T,E>) -> Result<T, ResilienceError>` | Short-circuits with `CircuitOpen` when open. |

### `RateLimiter` — token bucket

| Method | Signature | Notes |
|---|---|---|
| `RateLimiter::new` | `(requests_per_second) -> Self` | Saturates to epsilon for invalid rates. |
| `rl.try_execute` | `(FnOnce() -> T) -> Result<T, ResilienceError>` | Non-blocking; returns `RateLimited` if no token. |
| `rl.execute` | `(FnOnce() -> T) -> Result<T, ResilienceError>` | Blocking; sleeps until a token refills. |

### `Timeout` — soft deadline

| Method | Signature | Notes |
|---|---|---|
| `Timeout::new` | `(duration) -> Self` | |
| `timeout.execute` | `(FnOnce() -> T) -> Result<T, ResilienceError>` | Spawns worker thread; joins with deadline. Returns `Timeout(duration)` if it elapses. |

### `Pipeline` — composition

| Method | Signature | Notes |
|---|---|---|
| `Pipeline::new` | `() -> Self` | Empty. |
| `.retry` / `.circuit_breaker` / `.rate_limiter` / `.timeout` | `(policy) -> Self` | Builder. |
| `pipeline.execute` | `(Fn() -> Result<T,String>) -> Result<T, ResilienceError>` | Wraps body in `catch_unwind` per FFI R6. |

## Why hand-rolled (no `tower`)?

The T36 spec lists `tower` and `governor` as candidate backends. We deliberately hand-roll on `std::time` + `std::thread` instead because:

1. **tower's `Service` trait is async-first** (`Response` is wrapped in `Future<Output = Result<T,E>>`). Adopting it would force the Buff surface to be `async` — violating Buff's "no `await`, async propagates invisibly" rule (Buff §6: no `_async` suffix).
2. **governor pulls `quanta`** (a high-performance clock) that itself pulls `cc-rs` on non-x86 targets. The workspace's "no cc-rs" rule (same family that killed chumsky/logos/zmq) makes pure `std::time::Instant` the simpler choice.
3. **The 4 primitives + composition are <400 LOC** of pure std logic — small enough to hand-roll correctly with full test coverage (27 tests per T36 acceptance).

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md):

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `RetryPolicy`, `CircuitBreaker`, `RateLimiter`, `Timeout`, `Pipeline`, `BreakerState`, `ResilienceError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | All public functions return owned values; stateful primitives borrow `&mut self`. |
| R3 — Error mapping | Every fallible op returns `Result<T, ResilienceError>`. Handler errors coerced via `Display`. |
| R4 — Thread safety | All public types are `Send` (no `Rc`). `RetryPolicy` + `Timeout` are `Copy + Sync`; stateful types use `&mut self`. `Pipeline` wraps stateful layers in `Arc<Mutex<_>>` so it is `Send + Clone`. |
| R5 — Lifetime hiding | No public lifetime parameters. |
| R6 — Panic boundary | `Pipeline::execute` wraps its body in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-resilience
cargo clippy -p buff-resilience --all-targets -- -D warnings
cargo fmt -p buff-resilience --check
```

Tests are hermetic: no network, no filesystem, no shared global state. Time-based tests use real `thread::sleep` with generous slack (50ms+) to avoid flakes on CI runners under load.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
