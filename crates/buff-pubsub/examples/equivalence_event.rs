//! Behavioral equivalence test: Rust original vs Buff port (event.buff).
//!
//! Mirrors the Event struct from `selfhost/event.buff`.
//!
//! Run: `cargo run -p buff-pubsub --example equivalence_event`
//! Expected output: `user.created\nhello world`

use buff_pubsub::Event;

fn main() {
    let e = Event::new("user.created".to_string(), "hello world".to_string());
    println!("{}", e.topic());
    println!("{}", e.payload());
}
