// T36 example: Pipeline composing all 4 layers + Timeout firing.
//
// Demonstrates the full composition: Retry -> CircuitBreaker ->
// RateLimiter -> Timeout -> handler. The handler succeeds on the
// 2nd retry attempt. A second pipeline demonstrates Timeout firing
// on a handler that sleeps too long.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use buff_resilience::{
    CircuitBreaker, Pipeline, RateLimiter, ResilienceError, RetryPolicy, Timeout,
};

fn main() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let mut pipeline = Pipeline::new()
        .retry(RetryPolicy::no_delay(4))
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
    println!("pipeline result (succeeds on 2nd attempt): {result:?}");
    println!(
        "total handler invocations: {}",
        counter.load(Ordering::SeqCst)
    );

    let mut slow_pipeline = Pipeline::new()
        .retry(RetryPolicy::no_delay(2))
        .timeout(Timeout::new(Duration::from_millis(50)));
    let result = slow_pipeline.execute(|| {
        thread::sleep(Duration::from_millis(300));
        Ok::<_, String>(0u32)
    });
    println!("slow pipeline result (timeout fires, no retry): ");
    match result {
        Err(ResilienceError::Timeout(d)) => println!("  Timeout({d:?})"),
        other => println!("  unexpected: {other:?}"),
    }

    let mut empty_pipeline = Pipeline::new();
    let result = empty_pipeline.execute(|| Ok::<_, String>(String::from("no layers")));
    println!("empty pipeline result: {result:?}");
}
