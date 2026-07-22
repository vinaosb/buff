//! `buff-resilience` — resilience patterns for the Buff language.
//!
//! Pure-Rust MVP providing four composable resilience primitives
//! (Retry / CircuitBreaker / RateLimiter / Timeout) plus a [`Pipeline`]
//! that chains them. Hand-rolled on `std::time` + `std::thread` —
//! NO `tower`, NO `governor`, NO async runtime required (matches
//! Buff's "no `_async` suffix" rule §6 + "no function-coloring" rule).
//!
//! # Pipeline
//!
//! ```text
//!   RetryPolicy.new(max_attempts, initial_delay, backoff)
//!        │
//!        ▼
//!   policy.execute(handler)  ──▶  Result<T, ResilienceError>
//!
//!   CircuitBreaker.new(failure_threshold, reset_timeout)
//!        │
//!        ▼
//!   cb.execute(handler)  ──▶  Result<T, ResilienceError>
//!
//!   RateLimiter.new(requests_per_second)
//!        │
//!        ▼
//!   rl.execute(handler)  ──▶  Result<T, ResilienceError>
//!
//!   Timeout.new(duration)
//!        │
//!        ▼
//!   timeout.execute(handler)  ──▶  Result<T, ResilienceError>
//!
//!   Pipeline.new()
//!       .retry(policy)
//!       .circuit_breaker(cb)
//!       .rate_limiter(rl)
//!       .timeout(to)
//!       .execute(handler)  ──▶  Result<T, ResilienceError>
//!                               (composition: outermost = retry,
//!                                innermost = timeout — matches the
//!                                T36 spec ordering)
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `RetryPolicy`, `CircuitBreaker`, `RateLimiter`, `Timeout`, `Pipeline`, `BreakerState`, `ResilienceError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | All public functions consume owned inputs and return owned outputs. Stateful primitives (`CircuitBreaker`, `RateLimiter`, `Pipeline`) borrow `&mut self`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, ResilienceError>`. Handler errors coerced via `Display` into `Exhausted::last_error`. |
//! | R4 — Thread safety | All public types are `Send` (no `Rc`, no raw pointers). `RetryPolicy` + `Timeout` are `Sync + Copy`; stateful types are `Send` but NOT `Sync` (state mutation requires `&mut self`). `Pipeline` wraps stateful types in `Arc<Mutex<_>>` so the whole pipeline is `Send + Clone`. |
//! | R5 — Lifetime hiding | No public lifetime parameters. All borrowed inputs (`&str`, `&[u8]`) are converted to owned at the boundary. |
//! | R6 — Panic boundary | `Pipeline::execute` wraps its body in `catch_unwind` so a panic in the handler becomes `Err(ResilienceError::Panic)` instead of process abort. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. All fallible operations return `Result`.

pub mod error;

pub use error::ResilienceError;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Maximum backoff per single sleep (60s). Guards against runaway
/// exponential growth when `backoff_factor` is set very high.
const MAX_BACKOFF_CAP: Duration = Duration::from_secs(60);

/// 1ms polling cadence used by Timeout + RateLimiter busy-waits.
const POLL_CADENCE: Duration = Duration::from_millis(1);

// ===========================================================================
// Retry
// ===========================================================================

/// Retry policy with exponential backoff.
///
/// Constructed via [`RetryPolicy::new`] and invoked via
/// [`RetryPolicy::execute`]. Stateless between invocations —
/// each `execute` call starts fresh at attempt 1.
///
/// The backoff formula is:
/// ```text
///   delay(attempt n) = initial_delay * backoff_factor ^ (n - 2)
/// ```
/// for `n >= 2` (attempt 1 always has zero pre-delay), clamped to
/// a 60-second maximum per sleep. The sleep happens BEFORE the
/// next attempt (never AFTER the last failure).
///
/// `Clone + Copy` so the same policy can be reused across
/// multiple [`Pipeline`]s and the codegen layer can pass it by
/// value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetryPolicy {
    max_attempts: u32,
    initial_delay: Duration,
    backoff_factor: f64,
}

impl RetryPolicy {
    /// Build a retry policy. `max_attempts` MUST be >= 1
    /// (saturates to 1 if 0 is passed — never panics).
    /// `initial_delay` is the delay BEFORE attempt #2.
    /// `backoff_factor` of `2.0` doubles the delay each retry.
    pub const fn new(max_attempts: u32, initial_delay: Duration, backoff_factor: f64) -> Self {
        RetryPolicy {
            max_attempts: if max_attempts == 0 { 1 } else { max_attempts },
            initial_delay,
            backoff_factor,
        }
    }

    /// Convenience: a no-delay policy that fires `max_attempts`
    /// times back-to-back (useful for tests + idempotent reads).
    pub const fn no_delay(max_attempts: u32) -> Self {
        RetryPolicy::new(max_attempts, Duration::ZERO, 1.0)
    }

    #[inline]
    pub const fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    #[inline]
    pub const fn initial_delay(&self) -> Duration {
        self.initial_delay
    }

    #[inline]
    pub const fn backoff_factor(&self) -> f64 {
        self.backoff_factor
    }

    /// Compute the delay BEFORE attempt `next_attempt` (1-indexed).
    /// Returns `Duration::ZERO` for the first attempt. Clamped to
    /// [`MAX_BACKOFF_CAP`] to prevent pathological backoffs.
    ///
    /// Public so users (and codegen) can preview the exact backoff
    /// schedule before invoking [`Self::execute`].
    pub fn delay_for_attempt(&self, next_attempt: u32) -> Duration {
        if next_attempt <= 1 || self.initial_delay.is_zero() {
            return Duration::ZERO;
        }
        let exponent = (next_attempt - 2) as f64;
        let multiplier = self.backoff_factor.powf(exponent);
        let nanos = self.initial_delay.as_nanos() as f64 * multiplier;
        if !nanos.is_finite() || nanos < 0.0 {
            return Duration::ZERO;
        }
        let capped = nanos.min(MAX_BACKOFF_CAP.as_nanos() as f64);
        Duration::from_nanos(capped as u64)
    }

    /// Run `handler` up to `max_attempts` times, sleeping
    /// `delay_for_attempt(n)` between attempts. Returns `Ok(T)`
    /// on the first success, or `Err(Exhausted{ attempts, last_error })`
    /// if every attempt fails.
    pub fn execute<F, T, E>(&self, handler: F) -> Result<T, ResilienceError>
    where
        F: Fn() -> Result<T, E>,
        E: std::fmt::Display,
    {
        let mut last_error: Option<String> = None;
        for attempt in 1..=self.max_attempts {
            if attempt > 1 {
                let delay = self.delay_for_attempt(attempt);
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
            }
            match handler() {
                Ok(value) => return Ok(value),
                Err(err) => last_error = Some(err.to_string()),
            }
        }
        Err(ResilienceError::Exhausted {
            attempts: self.max_attempts,
            last_error: last_error.unwrap_or_default(),
        })
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy::new(3, Duration::from_millis(50), 2.0)
    }
}

// ===========================================================================
// CircuitBreaker
// ===========================================================================

/// Circuit breaker state machine.
///
/// - `Closed`: requests flow through; failures increment `failure_count`.
///   When `failure_count >= failure_threshold`, transitions to `Open`.
/// - `Open`: requests fail-fast with `Err(CircuitOpen)`; no handler
///   call is made. After `reset_timeout` elapses since the last
///   failure, the NEXT `execute` call transitions to `HalfOpen`.
/// - `HalfOpen`: a single probe request is allowed through. If it
///   succeeds, the breaker resets to `Closed` (count cleared). If
///   it fails, the breaker re-opens with `last_failure_at` updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// A circuit breaker around a fallible handler.
///
/// Constructed via [`CircuitBreaker::new`] (NOT `.create()` /
/// `.build()` per Buff §7). Stateful: invoke `execute` on `&mut self`.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    failure_threshold: u32,
    reset_timeout: Duration,
    state: BreakerState,
    failure_count: u32,
    last_failure_at: Option<Instant>,
}

impl CircuitBreaker {
    /// Build a new breaker. `failure_threshold` MUST be >= 1
    /// (saturates to 1 if 0 is passed — never panics).
    pub fn new(failure_threshold: u32, reset_timeout: Duration) -> Self {
        CircuitBreaker {
            failure_threshold: failure_threshold.max(1),
            reset_timeout,
            state: BreakerState::Closed,
            failure_count: 0,
            last_failure_at: None,
        }
    }

    #[inline]
    pub fn state(&self) -> BreakerState {
        self.state
    }

    #[inline]
    pub fn failure_count(&self) -> u32 {
        self.failure_count
    }

    #[inline]
    pub fn failure_threshold(&self) -> u32 {
        self.failure_threshold
    }

    #[inline]
    pub fn reset_timeout(&self) -> Duration {
        self.reset_timeout
    }

    fn maybe_transition_to_half_open(&mut self) {
        if self.state == BreakerState::Open {
            let ready = self
                .last_failure_at
                .map(|t| t.elapsed() >= self.reset_timeout)
                .unwrap_or(true);
            if ready {
                self.state = BreakerState::HalfOpen;
            }
        }
    }

    fn record_success(&mut self) {
        self.failure_count = 0;
        self.last_failure_at = None;
        self.state = BreakerState::Closed;
    }

    fn record_failure(&mut self) {
        self.failure_count = self.failure_count.saturating_add(1);
        self.last_failure_at = Some(Instant::now());
        if self.failure_count >= self.failure_threshold || self.state == BreakerState::HalfOpen {
            self.state = BreakerState::Open;
        }
    }

    /// Run `handler` if the breaker allows it. Returns:
    /// - `Ok(T)` when the handler succeeds (breaker may reset).
    /// - `Err(CircuitOpen)` when the breaker is open and not yet
    ///   ready to probe.
    /// - `Err(Exhausted{ attempts: 1, last_error })` when the handler
    ///   fails (the breaker records the failure; the user sees the
    ///   original error wrapped in a 1-attempt Exhausted to keep the
    ///   error type uniform across the crate).
    pub fn execute<F, T, E>(&mut self, handler: F) -> Result<T, ResilienceError>
    where
        F: FnOnce() -> Result<T, E>,
        E: std::fmt::Display,
    {
        self.maybe_transition_to_half_open();
        if self.state == BreakerState::Open {
            return Err(ResilienceError::CircuitOpen {
                failure_count: self.failure_count,
                threshold: self.failure_threshold,
            });
        }
        match handler() {
            Ok(value) => {
                self.record_success();
                Ok(value)
            }
            Err(err) => {
                let msg = err.to_string();
                self.record_failure();
                Err(ResilienceError::Exhausted {
                    attempts: 1,
                    last_error: msg,
                })
            }
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        CircuitBreaker::new(5, Duration::from_secs(30))
    }
}

// ===========================================================================
// RateLimiter
// ===========================================================================

/// Token-bucket rate limiter.
///
/// Constructed via [`RateLimiter::new(requests_per_second)`]. The
/// bucket holds at most `requests_per_second` tokens (one token
/// per request). Tokens refill continuously at the configured rate
/// based on elapsed wall-clock time since the last refill.
///
/// `execute` blocks the calling thread until a token is available
/// (the wait is bounded by `1 / requests_per_second`). Use
/// `try_execute` for a non-blocking variant that returns
/// `Err(RateLimited)` immediately when no token is available.
///
/// Interior state uses `Instant` for the refill clock — pure-Rust,
/// no `quanta`/cc-rs.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    requests_per_second: f64,
    tokens: f64,
    max_tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    /// Build a new limiter. `requests_per_second` MUST be > 0.0
    /// and finite (saturates to a small epsilon otherwise —
    /// never panics). The bucket starts full.
    pub fn new(requests_per_second: f64) -> Self {
        let rate = if !requests_per_second.is_finite() || requests_per_second <= 0.0 {
            0.000_001
        } else {
            requests_per_second
        };
        RateLimiter {
            requests_per_second: rate,
            tokens: rate,
            max_tokens: rate,
            last_refill: Instant::now(),
        }
    }

    #[inline]
    pub fn requests_per_second(&self) -> f64 {
        self.requests_per_second
    }

    #[inline]
    pub fn available_tokens(&self) -> f64 {
        self.tokens
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let added = elapsed * self.requests_per_second;
        self.tokens = (self.tokens + added).min(self.max_tokens);
        self.last_refill = now;
    }

    fn consume_one(&mut self) -> bool {
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Non-blocking attempt: returns `Ok(T)` if a token was
    /// available (handler runs immediately), `Err(RateLimited)`
    /// otherwise. The bucket is refilled before checking.
    pub fn try_execute<F, T>(&mut self, handler: F) -> Result<T, ResilienceError>
    where
        F: FnOnce() -> T,
    {
        self.refill();
        if !self.consume_one() {
            return Err(ResilienceError::RateLimited {
                requests_per_second: self.requests_per_second,
            });
        }
        Ok(handler())
    }

    /// Blocking execute: refills, then waits (coarse sleep loop)
    /// until at least 1 token is available, then runs `handler`.
    pub fn execute<F, T>(&mut self, handler: F) -> Result<T, ResilienceError>
    where
        F: FnOnce() -> T,
    {
        loop {
            self.refill();
            if self.consume_one() {
                return Ok(handler());
            }
            let deficit = 1.0 - self.tokens;
            let wait_secs = deficit / self.requests_per_second;
            let wait = Duration::from_secs_f64(wait_secs.min(1.0));
            let step = wait.min(POLL_CADENCE);
            if !step.is_zero() {
                thread::sleep(step);
            }
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        RateLimiter::new(10.0)
    }
}

// ===========================================================================
// Timeout
// ===========================================================================

/// Timeout policy: runs a handler on a spawned thread and joins
/// with a deadline.
///
/// Constructed via [`Timeout::new(duration)`]. The handler must
/// be `Send + 'static` (it crosses a thread boundary); the
/// returned value must also be `Send + 'static`.
///
/// IMPORTANT: this is a best-effort SOFT timeout. The handler
/// runs in its own OS thread; Rust does not provide a safe way
/// to forcibly cancel a thread. If the handler does not finish
/// within `duration`, the join handle is dropped (worker thread
/// continues in the background until it returns or panics — it
/// does NOT block process exit). The caller sees
/// `Err(Timeout(duration))`. A future v1.18+ async variant could
/// use `tokio::select!` for true cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout {
    duration: Duration,
}

impl Timeout {
    pub const fn new(duration: Duration) -> Self {
        Timeout { duration }
    }

    #[inline]
    pub const fn duration(&self) -> Duration {
        self.duration
    }

    /// Run `handler` on a spawned thread and join with timeout.
    /// Returns:
    /// - `Ok(T)` if the handler finishes within `duration`.
    /// - `Err(Timeout(duration))` if the deadline elapses first.
    /// - `Err(Panic)` if the spawn fails OR the handler panics.
    pub fn execute<F, T>(&self, handler: F) -> Result<T, ResilienceError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let handle = thread::Builder::new()
            .name("buff-resilience-timeout".to_string())
            .spawn(handler)
            .map_err(|_| ResilienceError::Panic)?;
        let deadline = Instant::now() + self.duration;
        while Instant::now() < deadline {
            if handle.is_finished() {
                return handle.join().map_err(|_| ResilienceError::Panic);
            }
            thread::sleep(POLL_CADENCE);
        }
        if handle.is_finished() {
            return handle.join().map_err(|_| ResilienceError::Panic);
        }
        Err(ResilienceError::Timeout(self.duration))
    }
}

impl Default for Timeout {
    fn default() -> Self {
        Timeout::new(Duration::from_secs(30))
    }
}

// ===========================================================================
// Pipeline
// ===========================================================================

/// Composable resilience pipeline.
///
/// Built via the builder pattern: start with [`Pipeline::new`], then
/// chain `.retry(policy)` / `.circuit_breaker(cb)` / `.rate_limiter(rl)`
/// / `.timeout(to)` in any order. Each layer wraps the next; the
/// OUTERMOST layer fires first on entry and last on exit.
///
/// The default ordering (when all 4 are set via the builder) is:
///   Retry → CircuitBreaker → RateLimiter → Timeout → handler
/// matching the T36 spec example
/// (`pipeline = Retry → CircuitBreaker → RateLimiter → handler`).
///
/// Stateful layers (`CircuitBreaker`, `RateLimiter`) are stored in
/// `Arc<Mutex<_>>` so the pipeline is `Send + Clone`. The `execute`
/// call locks each mutex briefly — never across the handler invocation.
///
/// The handler signature for [`Pipeline::execute`] is
/// `Fn() -> Result<T, String>`. Buff codegen lowers `?`-propagating
/// errors to `Result<T, String>` per the FFI guide §3, so this is the
/// natural error type at the boundary. The Pipeline wraps the String
/// error in [`ResilienceError::Exhausted`] for the public surface.
#[derive(Clone)]
pub struct Pipeline {
    retry: Option<RetryPolicy>,
    circuit_breaker: Option<Arc<Mutex<CircuitBreaker>>>,
    rate_limiter: Option<Arc<Mutex<RateLimiter>>>,
    timeout: Option<Timeout>,
}

impl Pipeline {
    pub fn new() -> Self {
        Pipeline {
            retry: None,
            circuit_breaker: None,
            rate_limiter: None,
            timeout: None,
        }
    }

    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = Some(policy);
        self
    }

    pub fn circuit_breaker(mut self, cb: CircuitBreaker) -> Self {
        self.circuit_breaker = Some(Arc::new(Mutex::new(cb)));
        self
    }

    pub fn rate_limiter(mut self, rl: RateLimiter) -> Self {
        self.rate_limiter = Some(Arc::new(Mutex::new(rl)));
        self
    }

    pub fn timeout(mut self, to: Timeout) -> Self {
        self.timeout = Some(to);
        self
    }

    /// Read-only introspection: the configured retry policy (if any).
    pub fn retry_policy(&self) -> Option<RetryPolicy> {
        self.retry
    }

    /// Read-only introspection: the configured timeout (if any).
    pub fn timeout_config(&self) -> Option<Timeout> {
        self.timeout
    }

    /// True iff at least one layer is configured.
    pub fn has_layers(&self) -> bool {
        self.retry.is_some()
            || self.circuit_breaker.is_some()
            || self.rate_limiter.is_some()
            || self.timeout.is_some()
    }

    /// Execute the pipeline around `handler`. The handler is invoked
    /// zero or more times depending on which layers are configured
    /// (Retry may invoke it multiple times; CircuitBreaker may
    /// refuse to invoke it at all).
    ///
    /// The whole pipeline body is wrapped in `catch_unwind` per
    /// FFI guide R6 — a panic anywhere becomes
    /// `Err(ResilienceError::Panic)`.
    ///
    /// Layer semantics:
    /// - **Retry**: re-invokes the downstream on `Exhausted` only;
    ///   does NOT retry `CircuitOpen`, `RateLimited`, `Timeout`, or
    ///   `Panic` (those are fail-fast signals).
    /// - **CircuitBreaker**: opens after `failure_threshold` handler
    ///   failures; probes half-open after `reset_timeout`.
    /// - **RateLimiter**: blocks the caller (coarse 1ms sleep loop)
    ///   until a token is available.
    /// - **Timeout**: spawns the handler on a worker thread; soft
    ///   timeout via `JoinHandle::is_finished` polling.
    pub fn execute<F, T>(&mut self, handler: F) -> Result<T, ResilienceError>
    where
        F: Fn() -> Result<T, String> + Send + Sync + 'static,
        T: Send + 'static,
    {
        let result = catch_unwind(AssertUnwindSafe(|| self.execute_inner(handler)));
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(ResilienceError::Panic),
        }
    }

    fn execute_inner<F, T>(&mut self, handler: F) -> Result<T, ResilienceError>
    where
        F: Fn() -> Result<T, String> + Send + Sync + 'static,
        T: Send + 'static,
    {
        let retry_policy = self.retry.unwrap_or_else(|| RetryPolicy::no_delay(1));
        let handler_arc: Arc<F> = Arc::new(handler);
        let mut last_error_msg: String = String::new();

        for attempt in 1..=retry_policy.max_attempts() {
            if attempt > 1 {
                let delay = retry_policy.delay_for_attempt(attempt);
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
            }

            // CircuitBreaker layer: short-circuit if open.
            if let Some(cb_arc) = &self.circuit_breaker {
                let cb_guard = cb_arc.lock();
                let mut cb = match cb_guard {
                    Ok(g) => g,
                    Err(_) => return Err(ResilienceError::Panic),
                };
                cb.maybe_transition_to_half_open();
                if cb.state() == BreakerState::Open {
                    return Err(ResilienceError::CircuitOpen {
                        failure_count: cb.failure_count(),
                        threshold: cb.failure_threshold(),
                    });
                }
            }

            // RateLimiter layer: block until a token is available.
            if let Some(rl_arc) = &self.rate_limiter {
                loop {
                    let acquired = {
                        let mut rl = match rl_arc.lock() {
                            Ok(g) => g,
                            Err(_) => return Err(ResilienceError::Panic),
                        };
                        rl.refill();
                        rl.consume_one()
                    };
                    if acquired {
                        break;
                    }
                    thread::sleep(POLL_CADENCE);
                }
            }

            // Timeout layer (innermost): spawn the handler on a worker thread.
            let handler_clone = Arc::clone(&handler_arc);
            let dispatch = move || -> Result<T, String> { (handler_clone)() };
            let outcome: Result<T, ResilienceError> = if let Some(to) = self.timeout {
                match to.execute(dispatch) {
                    Ok(Ok(value)) => Ok(value),
                    Ok(Err(msg)) => Err(ResilienceError::Exhausted {
                        attempts: 1,
                        last_error: msg,
                    }),
                    Err(other) => Err(other),
                }
            } else {
                match dispatch() {
                    Ok(value) => Ok(value),
                    Err(msg) => Err(ResilienceError::Exhausted {
                        attempts: 1,
                        last_error: msg,
                    }),
                }
            };

            match outcome {
                Ok(value) => {
                    if let Some(cb_arc) = &self.circuit_breaker {
                        if let Ok(mut cb) = cb_arc.lock() {
                            cb.record_success();
                        }
                    }
                    return Ok(value);
                }
                Err(ResilienceError::Exhausted { last_error, .. }) => {
                    // Handler failed — record circuit-breaker failure and retry.
                    if let Some(cb_arc) = &self.circuit_breaker {
                        if let Ok(mut cb) = cb_arc.lock() {
                            cb.record_failure();
                        }
                    }
                    last_error_msg = last_error;
                }
                Err(other) => {
                    // Timeout / Panic / CircuitOpen: don't retry, propagate up.
                    if let Some(cb_arc) = &self.circuit_breaker {
                        if let Ok(mut cb) = cb_arc.lock() {
                            cb.record_failure();
                        }
                    }
                    return Err(other);
                }
            }
        }

        Err(ResilienceError::Exhausted {
            attempts: retry_policy.max_attempts(),
            last_error: last_error_msg,
        })
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Pipeline::new()
    }
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cb_state = self
            .circuit_breaker
            .as_ref()
            .and_then(|arc| arc.lock().ok())
            .map(|cb| (cb.state(), cb.failure_count()));
        f.debug_struct("Pipeline")
            .field("retry", &self.retry)
            .field("circuit_breaker", &cb_state)
            .field("rate_limiter_set", &self.rate_limiter.is_some())
            .field("timeout", &self.timeout)
            .finish()
    }
}
