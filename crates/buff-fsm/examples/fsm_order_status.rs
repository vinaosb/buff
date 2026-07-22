// T40 example: order status state machine (4 states, terminal state).
//
// Demonstrates a real-world business FSM: cart -> paid -> shipped ->
// delivered, with the option to cancel from cart or paid. `delivered`
// is marked terminal so further events are rejected. Uses a guard
// (always-true in this demo, but pluggable) for the pay event to
// model "only allow payment when funds are confirmed" semantics.

use buff_fsm::{Guard, Machine};

fn main() {
    let mut m = Machine::new("cart".to_string()).expect("initial cart");
    m.add_transition(
        "cart".into(),
        "pay".into(),
        "paid".into(),
        Some(Guard::always()),
        None,
    )
    .expect("cart->paid");
    m.add_transition(
        "cart".into(),
        "cancel".into(),
        "cancelled".into(),
        None,
        None,
    )
    .expect("cart->cancelled");
    m.add_transition("paid".into(), "ship".into(), "shipped".into(), None, None)
        .expect("paid->shipped");
    m.add_transition(
        "paid".into(),
        "cancel".into(),
        "cancelled".into(),
        None,
        None,
    )
    .expect("paid->cancelled");
    m.add_transition(
        "shipped".into(),
        "deliver".into(),
        "delivered".into(),
        None,
        None,
    )
    .expect("shipped->delivered");
    m.mark_terminal("delivered").expect("delivered terminal");
    m.mark_terminal("cancelled").expect("cancelled terminal");

    println!("known states: {:?}", m.states());
    println!("known events: {:?}", m.events());
    println!("initial: {}", m.current_state());

    m.fire("pay").expect("pay");
    println!("after pay: {}", m.current_state());

    println!("can cancel from paid? {}", m.can_fire("cancel"));

    m.fire("ship").expect("ship");
    println!("after ship: {}", m.current_state());

    m.fire("deliver").expect("deliver");
    println!(
        "after deliver: {} (terminal={})",
        m.current_state(),
        m.is_terminal()
    );

    let err = m.fire("ship").unwrap_err();
    println!("terminal reject: {err}");
}
