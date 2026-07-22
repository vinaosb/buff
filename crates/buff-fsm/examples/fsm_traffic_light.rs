// T40 example: traffic light state machine (3 states, cyclic).
//
// Demonstrates the canonical FSM "hello world": green -> yellow -> red
// -> green on each `tick` event. No guards, no actions. Prints the
// state after each transition to verify the cycle is unbounded.

use buff_fsm::Machine;

fn main() {
    let mut m = Machine::new("green".to_string()).expect("initial green");
    m.add_transition("green".into(), "tick".into(), "yellow".into(), None, None)
        .expect("g->y");
    m.add_transition("yellow".into(), "tick".into(), "red".into(), None, None)
        .expect("y->r");
    m.add_transition("red".into(), "tick".into(), "green".into(), None, None)
        .expect("r->g");

    println!("initial: {}", m.current_state());
    for i in 0..6 {
        m.fire("tick").expect("tick");
        println!("after tick {i}: {}", m.current_state());
    }
    println!("cycle repeats unbounded");
}
