// T36 example: RetryPolicy with exponential backoff.
//
// Demonstrates the retry primitive. A failing handler succeeds on
// the 3rd attempt; the policy transparently retries between attempts.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use buff_resilience::RetryPolicy;

fn main() {
    let policy = RetryPolicy::new(5, Duration::from_millis(10), 2.0);
    println!("backoff schedule:");
    for n in 1..=5 {
        println!(
            "  attempt {n}: pre-delay = {:?}",
            policy.delay_for_attempt(n)
        );
    }

    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let result = policy.execute(move || {
        let n = c.fetch_add(1, Ordering::SeqCst);
        if n < 2 {
            println!("attempt {}: failing", n + 1);
            Err("transient failure")
        } else {
            println!("attempt {}: ok", n + 1);
            Ok(42)
        }
    });

    match result {
        Ok(value) => println!(
            "final: {value} after {} attempts",
            counter.load(Ordering::SeqCst)
        ),
        Err(e) => println!("exhausted: {e}"),
    }
}
