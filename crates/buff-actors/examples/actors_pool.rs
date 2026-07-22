//! T59 example: actor pool — N workers, fan-out a job batch.
//!
//! Run via:
//!
//! ```text
//! cargo run -p buff-actors --example actors_pool
//! ```

use buff_actors::{Actor, ActorAction, ActorSystem, Message};
use std::sync::{Arc, Mutex};

struct PoolWorker {
    worker_id: u32,
    completed: Arc<Mutex<u32>>,
}

impl Actor for PoolWorker {
    fn handle(&mut self, msg: Message) -> ActorAction {
        if let Ok(n) = msg.downcast::<u32>() {
            let result = n * n;
            if let Ok(mut g) = self.completed.lock() {
                *g += 1;
            }
            println!("[worker {}] {}^2 = {}", self.worker_id, n, result);
        }
        ActorAction::Continue
    }
}

fn main() {
    let sys = ActorSystem::new().expect("system");
    let completed = Arc::new(Mutex::new(0u32));
    const POOL_SIZE: u32 = 4;
    const JOBS_PER_WORKER: u32 = 5;
    let mut refs = Vec::new();
    for worker_id in 0..POOL_SIZE {
        let r = sys
            .spawn(Box::new(PoolWorker {
                worker_id,
                completed: completed.clone(),
            }))
            .expect("spawn");
        refs.push(r);
    }
    for (i, r) in refs.iter().enumerate() {
        for j in 0..JOBS_PER_WORKER {
            let _ = r.send((i as u32 + 1) * (j + 1));
        }
    }
    let target = POOL_SIZE * JOBS_PER_WORKER;
    while completed.lock().map(|g| *g).unwrap_or(0) < target {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    println!("all {} jobs completed across {} workers", target, POOL_SIZE);
    sys.shutdown();
}
