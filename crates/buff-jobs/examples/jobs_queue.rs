// T35 example: in-memory queue + worker (success path).
//
// Demonstrates the queue + worker pipeline. Enqueues 3 jobs, runs
// the worker with a handler that prints each payload, then asserts
// the worker completed every job and the queue is empty.

use buff_jobs::{Job, Priority, Queue, Worker};

fn main() {
    let queue = Queue::memory();
    queue
        .enqueue(Job::new("send-welcome-email").unwrap())
        .unwrap();
    queue
        .enqueue(
            Job::new("generate-thumbnail")
                .unwrap()
                .with_priority(Priority::High),
        )
        .unwrap();
    queue
        .enqueue(Job::new("cleanup-temp-files").unwrap())
        .unwrap();

    println!("initial: {}", queue.stats());

    let worker = Worker::new(queue.clone());
    let stats = worker
        .run(|job| {
            println!("processing: {}", job.payload());
            Ok(())
        })
        .expect("worker.run");

    println!("final: {}", queue.stats());
    println!("worker: {stats}");
    assert_eq!(stats.succeeded, 3);
    assert!(queue.is_empty());
}
