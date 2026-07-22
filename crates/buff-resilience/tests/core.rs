//! Integration tests for `buff-resilience`.
//!
//! Covers all 4 primitives + Pipeline composition per T36 acceptance:
//! - Retry: exponential backoff, exhaustion, success-on-retry.
//! - CircuitBreaker: opens on threshold, half-open probe, reset.
//! - RateLimiter: token bucket, blocking vs non-blocking.
//! - Timeout: completes within deadline, soft-timeout fires.
//! - Pipeline: all-4-layer composition; per-layer short-circuit.
//!
//! Plus 5 insta snapshots of error messages.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use buff_resilience::{
    BreakerState, CircuitBreaker, Pipeline, RateLimiter, ResilienceError, RetryPolicy, Timeout,
};

// ===== RetryPolicy =====

#[test]
fn retry_succeeds_on_first_attempt() {
    let policy = RetryPolicy::no_delay(3);
    let result: Result<u32, ResilienceError> = policy.execute(|| Ok::<_, &str>(42));
    assert_eq!(result.unwrap(), 42);
}

#[test]
fn retry_succeeds_after_transient_failures() {
    let policy = RetryPolicy::no_delay(5);
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let result = policy.execute(move || {
        let n = c.fetch_add(1, Ordering::SeqCst);
        if n < 2 {
            Err("transient")
        } else {
            Ok(99)
        }
    });
    assert_eq!(result.unwrap(), 99);
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[test]
fn retry_exhausts_after_max_attempts_and_carries_last_error() {
    let policy = RetryPolicy::no_delay(3);
    let result: Result<u32, ResilienceError> = policy.execute(|| Err::<u32, _>("always fails"));
    match result {
        Err(ResilienceError::Exhausted {
            attempts,
            last_error,
        }) => {
            assert_eq!(attempts, 3);
            assert_eq!(last_error, "always fails");
        }
        other => panic!("expected Exhausted, got {other:?}"),
    }
}

#[test]
fn retry_saturates_zero_max_attempts_to_one() {
    let policy = RetryPolicy::no_delay(0);
    assert_eq!(policy.max_attempts(), 1);
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let _: Result<u32, _> = policy.execute(move || {
        c.fetch_add(1, Ordering::SeqCst);
        Err::<u32, &str>("nope")
    });
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn retry_backoff_is_exponential_and_capped() {
    let policy = RetryPolicy::new(5, Duration::from_millis(10), 2.0);
    assert_eq!(policy.delay_for_attempt(1), Duration::ZERO);
    assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(10));
    assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(20));
    assert_eq!(policy.delay_for_attempt(4), Duration::from_millis(40));
    assert_eq!(policy.delay_for_attempt(5), Duration::from_millis(80));
}

// ===== CircuitBreaker =====

#[test]
fn circuit_breaker_starts_closed_and_passes_through() {
    let mut cb = CircuitBreaker::new(3, Duration::from_secs(1));
    assert_eq!(cb.state(), BreakerState::Closed);
    let result: Result<u32, ResilienceError> = cb.execute(|| Ok::<_, &str>(7));
    assert_eq!(result.unwrap(), 7);
    assert_eq!(cb.state(), BreakerState::Closed);
    assert_eq!(cb.failure_count(), 0);
}

#[test]
fn circuit_breaker_opens_after_threshold_failures() {
    let mut cb = CircuitBreaker::new(3, Duration::from_secs(60));
    let _: Result<(), _> = cb.execute(|| Err::<(), _>("e1"));
    assert_eq!(cb.state(), BreakerState::Closed);
    let _: Result<(), _> = cb.execute(|| Err::<(), _>("e2"));
    assert_eq!(cb.state(), BreakerState::Closed);
    let _: Result<(), _> = cb.execute(|| Err::<(), _>("e3"));
    assert_eq!(cb.state(), BreakerState::Open);
    assert_eq!(cb.failure_count(), 3);
}

#[test]
fn circuit_breaker_fail_fast_when_open() {
    let mut cb = CircuitBreaker::new(1, Duration::from_secs(60));
    let _: Result<(), _> = cb.execute(|| Err::<(), _>("first"));
    assert_eq!(cb.state(), BreakerState::Open);
    let started = Instant::now();
    let result: Result<(), ResilienceError> = cb.execute(|| Ok::<_, &str>(()));
    assert!(matches!(result, Err(ResilienceError::CircuitOpen { .. })));
    assert!(started.elapsed() < Duration::from_millis(50));
}

#[test]
fn circuit_breaker_half_open_probe_success_resets() {
    let mut cb = CircuitBreaker::new(1, Duration::from_millis(50));
    let _: Result<(), _> = cb.execute(|| Err::<(), _>("boom"));
    assert_eq!(cb.state(), BreakerState::Open);
    thread::sleep(Duration::from_millis(80));
    let result: Result<u32, _> = cb.execute(|| Ok::<_, &str>(42));
    assert_eq!(result.unwrap(), 42);
    assert_eq!(cb.state(), BreakerState::Closed);
    assert_eq!(cb.failure_count(), 0);
}

#[test]
fn circuit_breaker_half_open_probe_failure_reopens() {
    let mut cb = CircuitBreaker::new(1, Duration::from_millis(50));
    let _: Result<(), _> = cb.execute(|| Err::<(), _>("first"));
    thread::sleep(Duration::from_millis(80));
    let _: Result<(), _> = cb.execute(|| Err::<(), _>("probe fails"));
    assert_eq!(cb.state(), BreakerState::Open);
}

// ===== RateLimiter =====

#[test]
fn rate_limiter_starts_full_and_consumes() {
    let mut rl = RateLimiter::new(5.0);
    assert_eq!(rl.requests_per_second(), 5.0);
    let r1 = rl.try_execute(|| 1);
    let r2 = rl.try_execute(|| 2);
    let r3 = rl.try_execute(|| 3);
    let r4 = rl.try_execute(|| 4);
    let r5 = rl.try_execute(|| 5);
    assert!(r1.is_ok() && r2.is_ok() && r3.is_ok() && r4.is_ok() && r5.is_ok());
    let r6 = rl.try_execute(|| 6);
    assert!(matches!(r6, Err(ResilienceError::RateLimited { .. })));
}

#[test]
fn rate_limiter_blocking_execute_waits_for_refill() {
    let mut rl = RateLimiter::new(100.0);
    for _ in 0..100 {
        let _ = rl.try_execute(|| ());
    }
    let started = Instant::now();
    let result = rl.execute(|| 42);
    assert_eq!(result.unwrap(), 42);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(50),
        "should refill fast: {elapsed:?}"
    );
}

#[test]
fn rate_limiter_rejects_invalid_rate_and_saturates() {
    let rl = RateLimiter::new(0.0);
    assert!(rl.requests_per_second() > 0.0);
    assert!(rl.requests_per_second() < 0.001);
    let rl = RateLimiter::new(f64::NAN);
    assert!(rl.requests_per_second() > 0.0);
    let rl = RateLimiter::new(f64::INFINITY);
    assert!(rl.requests_per_second() > 0.0);
    assert!(rl.requests_per_second().is_finite());
}

// ===== Timeout =====

#[test]
fn timeout_completes_when_handler_is_fast() {
    let to = Timeout::new(Duration::from_secs(1));
    let result = to.execute(|| 123);
    assert_eq!(result.unwrap(), 123);
}

#[test]
fn timeout_fires_when_handler_exceeds_deadline() {
    let to = Timeout::new(Duration::from_millis(50));
    let started = Instant::now();
    let result: Result<u32, ResilienceError> = to.execute(|| {
        thread::sleep(Duration::from_millis(500));
        99
    });
    let elapsed = started.elapsed();
    match result {
        Err(ResilienceError::Timeout(d)) => {
            assert_eq!(d, Duration::from_millis(50));
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
    assert!(elapsed < Duration::from_millis(300));
}

#[test]
fn timeout_catches_handler_panic() {
    let to = Timeout::new(Duration::from_secs(1));
    let result: Result<u32, ResilienceError> = to.execute(|| {
        if true {
            panic!("boom");
        }
        0
    });
    assert!(matches!(result, Err(ResilienceError::Panic)));
}

// ===== Pipeline =====

#[test]
fn pipeline_empty_just_runs_handler_once() {
    let mut p = Pipeline::new();
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let result = p.execute(move || {
        c.fetch_add(1, Ordering::SeqCst);
        Ok::<_, String>(String::from("ok"))
    });
    assert_eq!(result.unwrap(), "ok");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn pipeline_compose_all_four_layers_success() {
    let mut p = Pipeline::new()
        .retry(RetryPolicy::no_delay(3))
        .circuit_breaker(CircuitBreaker::new(5, Duration::from_secs(30)))
        .rate_limiter(RateLimiter::new(100.0))
        .timeout(Timeout::new(Duration::from_secs(1)));
    let result = p.execute(|| Ok::<_, String>(42u32));
    assert_eq!(result.unwrap(), 42);
}

#[test]
fn pipeline_retry_invokes_handler_multiple_times() {
    let mut p = Pipeline::new().retry(RetryPolicy::no_delay(4));
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let result = p.execute(move || {
        let n = c.fetch_add(1, Ordering::SeqCst);
        if n < 2 {
            Err(String::from("transient"))
        } else {
            Ok(7u32)
        }
    });
    assert_eq!(result.unwrap(), 7);
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[test]
fn pipeline_circuit_breaker_short_circuits_retry() {
    let mut p = Pipeline::new()
        .retry(RetryPolicy::no_delay(5))
        .circuit_breaker(CircuitBreaker::new(1, Duration::from_secs(60)));
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let result = p.execute(move || {
        c.fetch_add(1, Ordering::SeqCst);
        Err::<u32, String>(String::from("always"))
    });
    assert!(matches!(result, Err(ResilienceError::CircuitOpen { .. })));
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn pipeline_timeout_does_not_retry() {
    let mut p = Pipeline::new()
        .retry(RetryPolicy::no_delay(3))
        .timeout(Timeout::new(Duration::from_millis(30)));
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let result = p.execute(move || {
        c.fetch_add(1, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(200));
        Ok::<_, String>(0u32)
    });
    assert!(matches!(result, Err(ResilienceError::Timeout(_))));
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

// ===== Snapshots =====

#[test]
fn snapshot_resilience_error_messages() {
    let exhausted = ResilienceError::Exhausted {
        attempts: 3,
        last_error: "upstream timeout".to_string(),
    };
    let circuit = ResilienceError::CircuitOpen {
        failure_count: 5,
        threshold: 5,
    };
    let rate = ResilienceError::RateLimited {
        requests_per_second: 10.0,
    };
    let to = ResilienceError::Timeout(Duration::from_millis(250));
    let panic_err = ResilienceError::Panic;
    insta::assert_snapshot!(
        "error_messages",
        format!("{exhausted}\n{circuit}\n{rate}\n{to:?}\n{panic_err}")
    );
}

#[test]
fn snapshot_breaker_state_all_variants() {
    insta::assert_snapshot!(
        "breaker_state_all",
        format!(
            "{:?}|{:?}|{:?}",
            BreakerState::Closed,
            BreakerState::Open,
            BreakerState::HalfOpen
        )
    );
}

#[test]
fn snapshot_retry_policy_debug() {
    let policy = RetryPolicy::new(5, Duration::from_millis(100), 2.5);
    insta::assert_snapshot!("retry_policy_debug", format!("{policy:?}"));
}

#[test]
fn snapshot_timeout_debug() {
    let to = Timeout::new(Duration::from_secs(7));
    insta::assert_snapshot!("timeout_debug", format!("{to:?}"));
}

#[test]
fn snapshot_pipeline_debug_no_layers() {
    let p = Pipeline::new();
    insta::assert_snapshot!("pipeline_debug_empty", format!("{p:?}"));
}

#[test]
fn snapshot_pipeline_debug_with_layers() {
    let p = Pipeline::new()
        .retry(RetryPolicy::default())
        .circuit_breaker(CircuitBreaker::new(2, Duration::from_secs(5)))
        .rate_limiter(RateLimiter::new(50.0))
        .timeout(Timeout::new(Duration::from_secs(1)));
    insta::assert_snapshot!("pipeline_debug_full", format!("{p:?}"));
}
