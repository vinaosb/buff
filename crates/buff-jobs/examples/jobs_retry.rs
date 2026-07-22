// T35 example: retry policy + dead-letter queue.
//
// Demonstrates the retry + backoff + dead-letter pipeline. Enqueues
// a job that always fails with a 2-retry budget, runs the worker,
// then prints the dead-letter queue contents.

use buff_jobs::{Backoff, Job, Queue, Worker};
use std::time::Duration;

fn main() {
    let queue = Queue::memory();
    queue
        .enqueue(
            Job::new("flaky-API-call")
                .unwrap()
                .with_max_retries(2)
                .with_backoff(Backoff::fixed(Duration::from_millis(1))),
        )
        .unwrap();

    println!("initial: {}", queue.stats());

    let worker = Worker::new(queue.clone());
    let mut attempt = 0u32;
    let stats = worker
        .run(|job| {
            attempt = attempt.saturating_add(1);
            Err(format!("always fails (attempt {attempt} for {})", job.payload()))
        })
        .expect("worker.run");

    println!("worker: {stats}");
    println!("final: {}", queue.stats());

    let dead = queue.dead_letter();
    println!("dead-letter queue ({} entries):", dead.len());
    for job in &dead {
        println!("  - {job}");
    }

    assert_eq!(stats.dead_lettered, 1);
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].payload(), "flaky-API-call");
}
