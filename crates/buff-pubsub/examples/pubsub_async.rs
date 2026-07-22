// T41 example: tokio runtime integration.
//
// Demonstrates that `subscribe` works inside a tokio runtime
// (the worker spawn uses `tokio::task::spawn_blocking` instead
// of `std::thread::spawn` when `Handle::try_current()` succeeds).
// A single publish from inside an async task is observed by a
// subscriber registered from the same runtime.

use buff_pubsub::EventBus;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let bus = EventBus::new().expect("bus");
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let received_clone = received.clone();

    let _id = bus
        .subscribe("async-topic", move |event| {
            if let Ok(mut g) = received_clone.lock() {
                g.push(event.payload().to_string());
            }
        })
        .expect("subscribe");

    // Publish from an async task to exercise the tokio integration.
    let bus_for_task = bus.clone();
    tokio::task::spawn(async move {
        bus_for_task
            .publish("async-topic", "from-tokio".to_string())
            .expect("publish");
    })
    .await
    .expect("task joined");

    // Yield once so the spawn_blocking worker from subscribe() has
    // a chance to drain the queued event. For deterministic test
    // behavior under tokio's test-util feature, examples use a
    // small sleep rather than `tokio::time::pause()` (which would
    // require the `test-util` feature and complicate the example).
    tokio::time::sleep(Duration::from_millis(50)).await;

    let captured = received.lock().expect("final lock").clone();
    println!("async captured: {captured:?}");
    assert_eq!(captured, vec!["from-tokio".to_string()]);

    println!("delivered across tokio runtime + spawn_blocking worker");
}
