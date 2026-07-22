//! T59 example: named actors — register + lookup.
//!
//! Run via:
//!
//! ```text
//! cargo run -p buff-actors --example actors_named
//! ```

use buff_actors::{Actor, ActorAction, ActorSystem, Message};
use std::sync::{Arc, Mutex};

struct Logger;

impl Actor for Logger {
    fn handle(&mut self, msg: Message) -> ActorAction {
        if let Ok(s) = msg.downcast::<String>() {
            println!("[logger] {s}");
        }
        ActorAction::Continue
    }
}

struct Counter {
    state: Arc<Mutex<u32>>,
}

impl Actor for Counter {
    fn handle(&mut self, _msg: Message) -> ActorAction {
        if let Ok(mut g) = self.state.lock() {
            *g += 1;
        }
        ActorAction::Continue
    }
}

fn main() {
    let sys = ActorSystem::new().expect("system");
    let logger_ref = sys.spawn(Box::new(Logger)).expect("spawn logger");
    sys.register("logger", logger_ref.clone())
        .expect("register logger");

    let counter_state = Arc::new(Mutex::new(0u32));
    let counter_ref = sys
        .spawn(Box::new(Counter {
            state: counter_state.clone(),
        }))
        .expect("spawn counter");
    sys.register("counter", counter_ref.clone())
        .expect("register counter");

    if let Some(found) = sys.lookup("logger") {
        let _ = found.send("hello from lookup".to_string());
    }
    if let Some(found) = sys.lookup("counter") {
        let _ = found.send(());
    }
    if sys.lookup("ghost").is_none() {
        println!("lookup of unknown name returned None as expected");
    }

    std::thread::sleep(std::time::Duration::from_millis(20));
    println!("counter state: {}", counter_state.lock().expect("lock"));
    sys.shutdown();
}
