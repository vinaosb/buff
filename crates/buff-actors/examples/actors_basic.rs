//! T59 example: actor basics — spawn + send + shutdown.
//!
//! Run via:
//!
//! ```text
//! cargo run -p buff-actors --example actors_basic
//! ```

use buff_actors::{Actor, ActorAction, ActorSystem, Message};
use std::sync::{Arc, Mutex};

struct Echo {
    received: Arc<Mutex<Vec<String>>>,
}

impl Actor for Echo {
    fn handle(&mut self, msg: Message) -> ActorAction {
        if let Ok(s) = msg.downcast::<String>() {
            if let Ok(mut g) = self.received.lock() {
                g.push(s);
            }
        }
        ActorAction::Continue
    }
}

fn main() {
    let sys = ActorSystem::new().expect("system");
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let actor_ref = sys
        .spawn(Box::new(Echo {
            received: received.clone(),
        }))
        .expect("spawn");

    for s in ["alpha", "beta", "gamma"] {
        actor_ref.send(s.to_string()).expect("send");
    }

    while received.lock().map(|g| g.len()).unwrap_or(0) < 3 {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    println!("received: {:?}", received.lock().expect("lock").clone());
    sys.shutdown();
}
