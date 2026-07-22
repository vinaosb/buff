// T40 example: turnstile state machine (2 states, action side-effects).
//
// Demonstrates an FSM whose transitions trigger real side effects via
// the Action callback. The classic turnstile problem: a turnstile
// starts locked; inserting a coin unlocks it; pushing from unlocked
// re-locks it AND dispenses a thank-you message. The action is a
// closure that captures shared mutable state (an Arc<Mutex<u32>>
// counting how many customers have passed through).

use buff_fsm::{Action, Machine};
use std::sync::{Arc, Mutex};

fn main() {
    let customer_count = Arc::new(Mutex::new(0u32));
    let thanked = Arc::new(Mutex::new(false));

    let mut m = Machine::new("locked".to_string()).expect("initial locked");
    m.add_transition(
        "locked".into(),
        "coin".into(),
        "unlocked".into(),
        None,
        None,
    )
    .expect("locked->unlocked");
    {
        let thanked = thanked.clone();
        m.add_transition(
            "unlocked".into(),
            "push".into(),
            "locked".into(),
            None,
            Some(Action::new(move || {
                *thanked.lock().expect("thanked lock") = true;
            })),
        )
        .expect("unlocked->locked");
    }

    println!("initial: {}", m.current_state());

    println!("push while locked (should fail):");
    match m.fire("push") {
        Ok(()) => println!("  unexpected success"),
        Err(e) => println!("  rejected: {e}"),
    }

    m.fire("coin").expect("coin");
    println!("after coin: {}", m.current_state());

    println!("can push? {}", m.can_fire("push"));

    m.fire("push").expect("push");
    println!(
        "after push: {} (thanked={}, customers={})",
        m.current_state(),
        thanked.lock().expect("thanked read"),
        {
            let mut c = customer_count.lock().expect("count lock");
            *c += 1;
            *c
        }
    );

    println!("final states: {:?}", m.states());
    println!("final transitions: {:?}", m.transitions());
}
