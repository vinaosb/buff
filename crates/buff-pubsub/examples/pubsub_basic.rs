// T41 example: basic subscribe + publish roundtrip.
//
// Demonstrates the minimal in-process pub/sub surface: create a
// bus, register one subscriber that captures events into a shared
// Vec, publish a single event, wait for delivery, observe the
// captured payload. No tokio runtime — pure std::thread worker
// spawn path (the fallback branch of subscribe()).

use buff_pubsub::EventBus;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    let bus = EventBus::new().expect("bus");
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let received_clone = received.clone();

    let _id = bus
        .subscribe("greeting", move |event| {
            let mut guard = match received_clone.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            guard.push(event.payload().to_string());
        })
        .expect("subscribe");

    let delivered = bus
        .publish("greeting", "hello, world".to_string())
        .expect("publish");
    println!("delivered to {delivered} subscriber(s)");

    // Worker thread drains the channel asynchronously; poll the
    // shared vec with a short deadline. MVP examples use this
    // pattern instead of a sync-flush API (deferred to v1.18+).
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if received.lock().map(|g| g.len()).unwrap_or(0) >= 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    let captured = received.lock().expect("final lock").clone();
    println!("captured: {captured:?}");
    assert_eq!(captured, vec!["hello, world".to_string()]);

    println!("topic_count: {}", bus.topic_count());
    println!("subscriber_count: {}", bus.subscriber_count("greeting"));
}
