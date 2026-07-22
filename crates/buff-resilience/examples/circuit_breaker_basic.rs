// T36 example: CircuitBreaker state machine.
//
// Demonstrates the breaker transitioning Closed -> Open (after
// threshold failures), then Open -> HalfOpen -> Closed (after the
// reset timeout elapses and a probe succeeds).

use std::thread;
use std::time::Duration;

use buff_resilience::CircuitBreaker;

fn main() {
    let mut cb = CircuitBreaker::new(3, Duration::from_millis(80));
    println!("initial state: {:?}", cb.state());

    for i in 1..=3 {
        let _: Result<(), _> = cb.execute(|| Err::<(), &str>("fail"));
        println!(
            "after failure {i}: state={:?} count={}",
            cb.state(),
            cb.failure_count()
        );
    }

    println!("breaker should be open now; failing fast:");
    let result: Result<u32, _> = cb.execute(|| Ok::<u32, &str>(999));
    println!("  -> {result:?}");

    println!("sleeping 100ms to let reset_timeout elapse...");
    thread::sleep(Duration::from_millis(100));

    let result: Result<u32, _> = cb.execute(|| Ok::<u32, &str>(42));
    println!("probe result: {result:?}");
    println!("final state: {:?}", cb.state());
    println!("final failure_count: {}", cb.failure_count());
}
