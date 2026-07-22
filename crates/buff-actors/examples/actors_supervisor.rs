//! T59 example: supervisor trees + "let it crash".
//!
//! Run via:
//!
//! ```text
//! cargo run -p buff-actors --example actors_supervisor
//! ```

use buff_actors::{
    supervisor::{ChildSpec, Supervisor},
    Actor, ActorAction, ActorSystem, Message,
};
use std::sync::{Arc, Mutex};

struct Counter {
    count: Arc<Mutex<u32>>,
    crash_until: Arc<Mutex<u32>>,
}

impl Actor for Counter {
    fn handle(&mut self, _msg: Message) -> ActorAction {
        let budget = self
            .crash_until
            .lock()
            .map(|mut g| {
                if *g > 0 {
                    *g -= 1;
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if budget {
            return ActorAction::Continue;
        }
        if let Ok(mut g) = self.count.lock() {
            *g += 1;
        }
        ActorAction::Continue
    }
}

fn main() {
    let sys = ActorSystem::new().expect("system");
    let sup = Supervisor::new(sys.clone()).expect("supervisor");
    let count = Arc::new(Mutex::new(0u32));
    let crash_budget = Arc::new(Mutex::new(1u32));
    let count_for_spec = count.clone();
    let crash_for_spec = crash_budget.clone();
    let r = sup
        .start_child(ChildSpec::new(move || {
            Box::new(Counter {
                count: count_for_spec.clone(),
                crash_until: crash_for_spec.clone(),
            })
        }))
        .expect("start_child");

    for _ in 0..5 {
        let _ = r.send(());
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    for _ in 0..5 {
        let _ = r.send(());
    }

    while count.lock().map(|g| *g).unwrap_or(0) < 1 {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    println!("processed (post-restart): {}", count.lock().expect("lock"));
    sup.shutdown();
}
