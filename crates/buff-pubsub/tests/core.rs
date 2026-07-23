//! Integration tests for the `buff-pubsub` crate.
//!
//! Covers the 10 public functions per the T41 spec:
//! - Event: `new`, `topic`, `payload`
//! - EventBus: `new`, `subscribe`, `publish`, `unsubscribe`,
//!   `subscriber_count`, `topic_count`, `clear`
//!
//! Plus the acceptance criterion: "Subscribe/publish delivers events
//! to all subscribers. Multiple subscribers receive same event."
//!
//! Tests are hermetic — no external broker needed (in-process only).
//! Each test polls shared `Arc<Mutex<Vec<_>>>` captures with a short
//! deadline because the worker thread drains the crossbeam channel
//! asynchronously. 12+ unit tests + 3 insta snapshots (per T41
//! acceptance criterion of 10+ tests).

use buff_pubsub::{Event, EventBus, PubSubError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const POLL_DEADLINE: Duration = Duration::from_millis(500);
const POLL_INTERVAL: Duration = Duration::from_millis(2);

fn wait_for<F: Fn() -> bool>(predicate: F) -> bool {
    let deadline = Instant::now() + POLL_DEADLINE;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    false
}

fn make_capture() -> Arc<Mutex<Vec<String>>> {
    Arc::new(Mutex::new(Vec::new()))
}

fn register_capture(buf: Arc<Mutex<Vec<String>>>) -> impl Fn(Event) + Send + Sync + 'static {
    move |event| {
        if let Ok(mut g) = buf.lock() {
            g.push(event.payload().to_string());
        }
    }
}

// ---- Event API (3 fns) ----------------------------------------------------

#[test]
fn event_new_constructs_owned_event() {
    let event = Event::new("topic".to_string(), "payload".to_string());
    assert_eq!(event.topic(), "topic");
    assert_eq!(event.payload(), "payload");
}

#[test]
fn event_topic_returns_borrowed_str() {
    let event = Event::new("greeting".to_string(), String::new());
    let topic: &str = event.topic();
    assert_eq!(topic, "greeting");
}

#[test]
fn event_payload_returns_borrowed_str() {
    let event = Event::new(String::new(), "hello".to_string());
    let payload: &str = event.payload();
    assert_eq!(payload, "hello");
}

// ---- EventBus basic (new / subscribe / publish) --------------------------

#[test]
fn eventbus_new_returns_empty_bus() {
    let bus = EventBus::new().expect("new");
    assert_eq!(bus.topic_count(), 0);
    assert_eq!(bus.subscriber_count("anything"), 0);
}

#[test]
fn eventbus_subscribe_returns_unique_ids() {
    let bus = EventBus::new().expect("new");
    let buf = make_capture();
    let id1 = bus
        .subscribe("topic", register_capture(buf.clone()))
        .expect("sub1");
    let id2 = bus.subscribe("topic", register_capture(buf)).expect("sub2");
    assert_ne!(id1, id2, "subscription ids must be unique");
    assert_eq!(bus.subscriber_count("topic"), 2);
}

#[test]
fn eventbus_subscribe_rejects_empty_topic() {
    let bus = EventBus::new().expect("new");
    let buf = make_capture();
    let err = bus.subscribe("", register_capture(buf)).unwrap_err();
    assert!(matches!(err, PubSubError::EmptySubscribeTopic));
}

#[test]
fn eventbus_publish_rejects_empty_topic() {
    let bus = EventBus::new().expect("new");
    let err = bus.publish("", "payload".to_string()).unwrap_err();
    assert!(matches!(err, PubSubError::EmptyTopic));
}

#[test]
fn eventbus_publish_to_unknown_topic_returns_zero() {
    let bus = EventBus::new().expect("new");
    let count = bus
        .publish("never-subscribed", "payload".to_string())
        .expect("publish");
    assert_eq!(count, 0);
}

#[test]
fn eventbus_publish_delivers_to_single_subscriber() {
    let bus = EventBus::new().expect("new");
    let buf = make_capture();
    let _id = bus
        .subscribe("topic", register_capture(buf.clone()))
        .expect("subscribe");

    let delivered = bus.publish("topic", "hello".to_string()).expect("publish");
    assert_eq!(delivered, 1);

    assert!(
        wait_for(|| buf.lock().map(|g| g.len() >= 1).unwrap_or(false)),
        "event was not delivered within deadline"
    );
    assert_eq!(buf.lock().expect("lock").clone(), vec!["hello".to_string()]);
}

#[test]
fn eventbus_publish_delivers_to_multiple_subscribers() {
    let bus = EventBus::new().expect("new");
    let buf_a = make_capture();
    let buf_b = make_capture();
    let buf_c = make_capture();

    let _id_a = bus
        .subscribe("broadcast", register_capture(buf_a.clone()))
        .expect("sub a");
    let _id_b = bus
        .subscribe("broadcast", register_capture(buf_b.clone()))
        .expect("sub b");
    let _id_c = bus
        .subscribe("broadcast", register_capture(buf_c.clone()))
        .expect("sub c");

    let delivered = bus
        .publish("broadcast", "ping".to_string())
        .expect("publish");
    assert_eq!(delivered, 3);

    assert!(wait_for(|| buf_a
        .lock()
        .map(|g| g.len() >= 1)
        .unwrap_or(false)));
    assert!(wait_for(|| buf_b
        .lock()
        .map(|g| g.len() >= 1)
        .unwrap_or(false)));
    assert!(wait_for(|| buf_c
        .lock()
        .map(|g| g.len() >= 1)
        .unwrap_or(false)));

    assert_eq!(buf_a.lock().expect("a").clone(), vec!["ping".to_string()]);
    assert_eq!(buf_b.lock().expect("b").clone(), vec!["ping".to_string()]);
    assert_eq!(buf_c.lock().expect("c").clone(), vec!["ping".to_string()]);
}

#[test]
fn eventbus_multiple_publishes_deliver_in_order() {
    let bus = EventBus::new().expect("new");
    let buf = make_capture();
    let _id = bus
        .subscribe("topic", register_capture(buf.clone()))
        .expect("subscribe");

    for i in 0..5 {
        bus.publish("topic", format!("event-{i}")).expect("publish");
    }

    assert!(wait_for(|| buf
        .lock()
        .map(|g| g.len() >= 5)
        .unwrap_or(false)));
    let captured = buf.lock().expect("lock").clone();
    assert_eq!(
        captured,
        vec![
            "event-0".to_string(),
            "event-1".to_string(),
            "event-2".to_string(),
            "event-3".to_string(),
            "event-4".to_string(),
        ]
    );
}

// ---- unsubscribe + clear --------------------------------------------------

#[test]
fn eventbus_unsubscribe_stops_delivery() {
    let bus = EventBus::new().expect("new");
    let buf = make_capture();
    let id = bus
        .subscribe("topic", register_capture(buf.clone()))
        .expect("subscribe");

    bus.unsubscribe(id).expect("unsubscribe");
    // topic_count drops to 0 because the only subscriber's list
    // became empty and is pruned by unsubscribe.
    assert_eq!(bus.topic_count(), 0);
    assert_eq!(bus.subscriber_count("topic"), 0);

    let delivered = bus
        .publish("topic", "post-unsubscribe".to_string())
        .expect("publish");
    assert_eq!(delivered, 0);

    std::thread::sleep(Duration::from_millis(20));
    assert!(buf.lock().expect("lock").is_empty());
}

#[test]
fn eventbus_unsubscribe_unknown_id_returns_error() {
    let bus = EventBus::new().expect("new");
    let err = bus.unsubscribe(99999).unwrap_err();
    assert!(matches!(err, PubSubError::UnknownSubscription(99999)));
}

#[test]
fn eventbus_unsubscribe_does_not_affect_other_subscribers() {
    let bus = EventBus::new().expect("new");
    let buf_a = make_capture();
    let buf_b = make_capture();

    let _id_a = bus
        .subscribe("topic", register_capture(buf_a.clone()))
        .expect("sub a");
    let id_b = bus
        .subscribe("topic", register_capture(buf_b.clone()))
        .expect("sub b");

    bus.unsubscribe(id_b).expect("unsub b");
    assert_eq!(bus.subscriber_count("topic"), 1);

    let delivered = bus
        .publish("topic", "post-unsub".to_string())
        .expect("publish");
    assert_eq!(delivered, 1);

    assert!(wait_for(|| buf_a
        .lock()
        .map(|g| g.len() >= 1)
        .unwrap_or(false)));
    std::thread::sleep(Duration::from_millis(20));
    assert!(buf_b.lock().expect("b empty").is_empty());
}

#[test]
fn eventbus_clear_drops_all_subscriptions() {
    let bus = EventBus::new().expect("new");
    let buf = make_capture();
    let _id1 = bus
        .subscribe("t1", register_capture(buf.clone()))
        .expect("sub t1");
    let _id2 = bus
        .subscribe("t2", register_capture(buf.clone()))
        .expect("sub t2");
    let _id3 = bus.subscribe("t3", register_capture(buf)).expect("sub t3");
    assert_eq!(bus.topic_count(), 3);

    bus.clear();
    assert_eq!(bus.topic_count(), 0);
    assert_eq!(bus.subscriber_count("t1"), 0);
    assert_eq!(bus.subscriber_count("t2"), 0);
    assert_eq!(bus.subscriber_count("t3"), 0);

    // After clear, the bus is reusable.
    let buf2 = make_capture();
    let _id_new = bus
        .subscribe("new-topic", register_capture(buf2.clone()))
        .expect("resubscribe");
    bus.publish("new-topic", "after-clear".to_string())
        .expect("publish");
    assert!(wait_for(|| buf2
        .lock()
        .map(|g| g.len() >= 1)
        .unwrap_or(false)));
}

// ---- topic_count + subscriber_count --------------------------------------

#[test]
fn eventbus_topic_count_reflects_active_topics_only() {
    let bus = EventBus::new().expect("new");
    assert_eq!(bus.topic_count(), 0);

    let buf = make_capture();
    let _id = bus
        .subscribe("a", register_capture(buf.clone()))
        .expect("sub a");
    assert_eq!(bus.topic_count(), 1);

    let _id = bus
        .subscribe("b", register_capture(buf.clone()))
        .expect("sub b");
    assert_eq!(bus.topic_count(), 2);

    bus.unsubscribe(_id).expect("unsub b");
    assert_eq!(bus.topic_count(), 1);
}

#[test]
fn eventbus_subscriber_count_is_per_topic() {
    let bus = EventBus::new().expect("new");
    let buf = make_capture();
    let _ = bus
        .subscribe("t1", register_capture(buf.clone()))
        .expect("sub t1");
    let _ = bus
        .subscribe("t1", register_capture(buf.clone()))
        .expect("sub t1 again");
    let _ = bus.subscribe("t2", register_capture(buf)).expect("sub t2");

    assert_eq!(bus.subscriber_count("t1"), 2);
    assert_eq!(bus.subscriber_count("t2"), 1);
    assert_eq!(bus.subscriber_count("never"), 0);
}

// ---- Concurrency: panicking handler does not kill worker ------------------

#[test]
fn panicking_handler_does_not_kill_worker_or_drop_subsequent_events() {
    let bus = EventBus::new().expect("new");
    let buf = make_capture();
    let buf_clone = buf.clone();
    let _id = bus
        .subscribe("topic", move |event| {
            if event.payload() == "boom" {
                panic!("intentional test panic");
            }
            if let Ok(mut g) = buf_clone.lock() {
                g.push(event.payload().to_string());
            }
        })
        .expect("subscribe");

    // First event: normal delivery.
    bus.publish("topic", "before".to_string())
        .expect("publish 1");
    assert!(wait_for(|| buf
        .lock()
        .map(|g| g.len() >= 1)
        .unwrap_or(false)));

    // Second event: handler panics. Worker catches it and survives.
    bus.publish("topic", "boom".to_string()).expect("publish 2");

    // Third event: must still be delivered (worker did not die).
    bus.publish("topic", "after".to_string())
        .expect("publish 3");
    assert!(wait_for(|| buf
        .lock()
        .map(|g| g.len() >= 2)
        .unwrap_or(false)));

    let captured = buf.lock().expect("lock").clone();
    assert_eq!(captured, vec!["before".to_string(), "after".to_string()]);
}

// ---- Send + Sync + Clone (FFI R4) ----------------------------------------

#[test]
fn eventbus_is_send_sync_clone() {
    fn assert_send_sync_clone<T: Send + Sync + Clone + 'static>() {}
    assert_send_sync_clone::<EventBus>();
}

#[test]
fn eventbus_clone_shares_inner_state() {
    let bus = EventBus::new().expect("new");
    let bus_clone = bus.clone();
    let buf = make_capture();
    let _id = bus
        .subscribe("topic", register_capture(buf.clone()))
        .expect("subscribe on original");

    // Publish through the clone — subscriber registered on the
    // original should still receive the event (Arc-shared inner).
    let delivered = bus_clone
        .publish("topic", "via-clone".to_string())
        .expect("publish via clone");
    assert_eq!(delivered, 1);
    assert!(wait_for(|| buf
        .lock()
        .map(|g| g.len() >= 1)
        .unwrap_or(false)));
}

// ---- Async integration (tokio runtime active) ----------------------------

#[tokio::test]
async fn subscribe_publish_works_under_tokio_runtime() {
    let bus = EventBus::new().expect("new");
    let buf = make_capture();
    let _id = bus
        .subscribe("async-topic", register_capture(buf.clone()))
        .expect("subscribe under tokio");

    let bus_for_task = bus.clone();
    tokio::task::spawn(async move {
        bus_for_task
            .publish("async-topic", "from-tokio".to_string())
            .expect("publish");
    })
    .await
    .expect("task joined");

    // Poll briefly for the spawn_blocking worker to drain.
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if buf.lock().map(|g| g.len() >= 1).unwrap_or(false) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    assert_eq!(
        buf.lock().expect("lock").clone(),
        vec!["from-tokio".to_string()]
    );
}

// ---- Debug + Display -----------------------------------------------------

#[test]
fn event_display_format_matches_convention() {
    let event = Event::new("greeting".to_string(), "hi".to_string());
    assert_eq!(format!("{event}"), r#"Event(greeting, "hi")"#);
}

#[test]
fn eventbus_debug_includes_counts() {
    let bus = EventBus::new().expect("new");
    let debug = format!("{bus:?}");
    assert!(debug.contains("EventBus"));
    assert!(debug.contains("topics"));
    assert!(debug.contains("subscriptions"));

    let buf = make_capture();
    let _ = bus
        .subscribe("t1", register_capture(buf))
        .expect("subscribe");
    let debug_after = format!("{bus:?}");
    assert!(debug_after.contains("topics: 1"));
    assert!(debug_after.contains("subscriptions: 1"));
}

// ---- Default impl --------------------------------------------------------

#[test]
fn eventbus_default_matches_new() {
    let new_bus = EventBus::new().expect("new");
    let default_bus = EventBus::default();
    assert_eq!(new_bus.topic_count(), default_bus.topic_count());
}

// ---- Insta snapshots (3+) ------------------------------------------------

#[test]
fn snapshot_event_debug() {
    let event = Event::new("snapshot".to_string(), "payload".to_string());
    insta::assert_snapshot!("event_debug", format!("{event:?}"));
}

#[test]
fn snapshot_event_display() {
    let event = Event::new("greeting".to_string(), "hello".to_string());
    insta::assert_snapshot!("event_display", format!("{event}"));
}

#[test]
fn snapshot_pubsub_error_all_variants() {
    let e1 = PubSubError::UnknownSubscription(42);
    let e2 = PubSubError::EmptyTopic;
    let e3 = PubSubError::EmptySubscribeTopic;
    let e4 = PubSubError::Panic;
    insta::assert_snapshot!("pubsub_error_all", format!("{e1}\n{e2}\n{e3}\n{e4}"));
}
