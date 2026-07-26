//! Behavioral equivalence test: Rust original vs Buff port (transition.buff).
//!
//! Mirrors the TransitionSummary struct from `selfhost/transition.buff`.
//!
//! Run: `cargo run -p buff-fsm --example equivalence_transition`
//! Expected output: `idle\ngo\nrunning\ntrue\nfalse`

use buff_fsm::TransitionSummary;

fn main() {
    let t = TransitionSummary {
        from: "idle".to_string(),
        event: "go".to_string(),
        to: "running".to_string(),
        has_guard: true,
        has_action: false,
    };
    println!("{}", t.from);
    println!("{}", t.event);
    println!("{}", t.to);
    println!("{}", t.has_guard);
    println!("{}", t.has_action);
}
