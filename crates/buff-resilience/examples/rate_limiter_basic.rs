// T36 example: RateLimiter token bucket.
//
// Demonstrates both the non-blocking (try_execute) and blocking
// (execute) variants. Burns through the initial token budget, then
// shows the blocking variant waiting for the refill.

use std::thread;
use std::time::{Duration, Instant};

use buff_resilience::{RateLimiter, ResilienceError};

fn main() {
    let mut rl = RateLimiter::new(5.0);
    println!("rate: {} req/s", rl.requests_per_second());

    println!("consuming initial budget:");
    for i in 1..=5 {
        let r = rl.try_execute(|| i);
        println!("  try_execute({i}) -> {r:?}");
    }

    let result = rl.try_execute(|| 6);
    println!("6th try_execute (should be RateLimited):");
    match result {
        Err(ResilienceError::RateLimited {
            requests_per_second,
        }) => {
            println!("  RateLimited({requests_per_second})");
        }
        other => println!("  unexpected: {other:?}"),
    }

    println!("blocking execute (waits for refill):");
    let started = Instant::now();
    let result = rl.execute(|| 42);
    let elapsed = started.elapsed();
    println!("  -> {result:?} after {elapsed:?}");

    thread::sleep(Duration::from_millis(50));
    let result = rl.execute(|| 99);
    println!("  after another 50ms sleep: {result:?}");
}
