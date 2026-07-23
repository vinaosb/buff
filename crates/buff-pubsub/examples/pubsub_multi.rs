// T41 example: fan-out — multiple subscribers receive the same event.
//
// Demonstrates the core acceptance criterion: "Multiple subscribers
// receive same event". Three subscribers register on the "tick"
// topic; a single publish delivers the event to all three. Each
// subscriber captures into its own Arc<Mutex<Vec<_>>> so the test
// can verify independent delivery.

use buff_pubsub::EventBus;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    let bus = EventBus::new().expect("bus");

    let s1 = Arc::new(Mutex::new(Vec::<String>::new()));
    let s2 = Arc::new(Mutex::new(Vec::<String>::new()));
    let s3 = Arc::new(Mutex::new(Vec::<String>::new()));

    let sc1 = s1.clone();
    let _id1 = bus
        .subscribe("tick", move |event| {
            if let Ok(mut g) = sc1.lock() {
                g.push(event.payload().to_string());
            }
        })
        .expect("subscribe 1");

    let sc2 = s2.clone();
    let _id2 = bus
        .subscribe("tick", move |event| {
            if let Ok(mut g) = sc2.lock() {
                g.push(event.payload().to_string());
            }
        })
        .expect("subscribe 2");

    let sc3 = s3.clone();
    let _id3 = bus
        .subscribe("tick", move |event| {
            if let Ok(mut g) = sc3.lock() {
                g.push(event.payload().to_string());
            }
        })
        .expect("subscribe 3");

    println!("subscribers registered: {}", bus.subscriber_count("tick"));

    let delivered = bus.publish("tick", "beat-1".to_string()).expect("publish");
    println!("delivered to {delivered} subscribers (expected 3)");

    wait_for_count(&s1, 1);
    wait_for_count(&s2, 1);
    wait_for_count(&s3, 1);

    assert_eq!(s1.lock().expect("s1").clone(), vec!["beat-1".to_string()]);
    assert_eq!(s2.lock().expect("s2").clone(), vec!["beat-1".to_string()]);
    assert_eq!(s3.lock().expect("s3").clone(), vec!["beat-1".to_string()]);

    bus.publish("tick", "beat-2".to_string())
        .expect("publish 2");
    wait_for_count(&s1, 2);

    let total_events: usize = [s1, s2, s3]
        .iter()
        .map(|s| s.lock().expect("lock").len())
        .sum();
    println!("total events captured across 3 subscribers: {total_events}");
    assert_eq!(total_events, 6);

    bus.clear();
    println!("after clear: topic_count = {}", bus.topic_count());
}

fn wait_for_count(buf: &Arc<Mutex<Vec<String>>>, target: usize) {
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if buf.lock().map(|g| g.len()).unwrap_or(0) >= target {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
