//! T59 example: graceful shutdown.
//!
//! Run via:
//!
//! ```text
//! cargo run -p buff-actors --example actors_shutdown
//! ```

use buff_actors::{Actor, ActorAction, ActorSystem, Message};
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Worker {
    processed: Arc<Mutex<u32>>,
}

impl Actor for Worker {
    fn handle(&mut self, _msg: Message) -> ActorAction {
        if let Ok(mut g) = self.processed.lock() {
            *g += 1;
        }
        ActorAction::Continue
    }
}

fn main() {
    let sys = ActorSystem::new().expect("system");
    let processed = Arc::new(Mutex::new(0u32));
    let mut refs = Vec::new();
    for _ in 0..5 {
        let r = sys
            .spawn(Box::new(Worker {
                processed: processed.clone(),
            }))
            .expect("spawn");
        refs.push(r);
    }
    for r in &refs {
        for _ in 0..10 {
            let _ = r.send(());
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
    println!(
        "processed before shutdown: {}",
        processed.lock().expect("lock")
    );
    let start = Instant::now();
    sys.shutdown();
    let elapsed = start.elapsed();
    println!("shutdown joined all threads in {:?}", elapsed);
    match refs[0].send(()) {
        Err(buff_actors::ActorError::ActorStopped(_)) => {
            println!("post-shutdown send correctly rejected with ActorStopped");
        }
        _ => panic!("expected ActorStopped"),
    }
}
