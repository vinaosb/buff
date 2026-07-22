//! Integration tests for the `buff-fsm` crate.
//!
//! Covers all 13 public functions per the T40 spec:
//! - Constructors: `Machine::new`, `Machine::default`, `Guard::new` / `always` / `never`, `Action::new` / `noop`
//! - Modifiers: `add_transition`, `mark_terminal`, `reset`, `fire`
//! - Accessors: `current_state`, `initial_state`, `is_in`, `is_terminal`, `can_fire`
//! - Diagnostic: `states`, `events`, `transitions`
//!
//! 12+ unit tests + 5 insta snapshots per the T40 acceptance criteria.

use buff_fsm::{Action, FsmError, Guard, Machine};
use std::sync::{Arc, Mutex};

fn traffic_light() -> Machine {
    let mut m = Machine::new("green".to_string()).expect("traffic light");
    m.add_transition("green".into(), "tick".into(), "yellow".into(), None, None)
        .expect("g->y");
    m.add_transition("yellow".into(), "tick".into(), "red".into(), None, None)
        .expect("y->r");
    m.add_transition("red".into(), "tick".into(), "green".into(), None, None)
        .expect("r->g");
    m
}

#[test]
fn machine_new_rejects_empty_initial_state() {
    assert!(matches!(
        Machine::new(String::new()).unwrap_err(),
        FsmError::EmptyInitialState
    ));
}

#[test]
fn machine_new_records_initial_state() {
    let m = Machine::new("idle".to_string()).expect("idle");
    assert_eq!(m.current_state(), "idle");
    assert_eq!(m.initial_state(), "idle");
    assert!(m.is_in("idle"));
    assert_eq!(m.states(), vec!["idle"]);
    assert!(m.events().is_empty());
}

#[test]
fn add_transition_rejects_empty_identifiers() {
    let mut m = Machine::new("a".to_string()).expect("a");
    let err = m
        .add_transition("".into(), "evt".into(), "b".into(), None, None)
        .unwrap_err();
    assert!(matches!(err, FsmError::EmptyIdentifier { .. }));
    let err = m
        .add_transition("a".into(), "".into(), "b".into(), None, None)
        .unwrap_err();
    assert!(matches!(err, FsmError::EmptyIdentifier { .. }));
    let err = m
        .add_transition("a".into(), "evt".into(), "".into(), None, None)
        .unwrap_err();
    assert!(matches!(err, FsmError::EmptyIdentifier { .. }));
}

#[test]
fn fire_transitions_correctly() {
    let mut m = traffic_light();
    assert_eq!(m.current_state(), "green");
    m.fire("tick").expect("g->y");
    assert_eq!(m.current_state(), "yellow");
    m.fire("tick").expect("y->r");
    assert_eq!(m.current_state(), "red");
    m.fire("tick").expect("r->g");
    assert_eq!(m.current_state(), "green");
}

#[test]
fn fire_unknown_event_errors() {
    let mut m = traffic_light();
    let err = m.fire("jump").unwrap_err();
    assert!(matches!(
        err,
        FsmError::UnknownEvent {
            current,
            event
        } if current == "green" && event == "jump"
    ));
}

#[test]
fn fire_empty_event_errors() {
    let mut m = traffic_light();
    assert!(matches!(m.fire("").unwrap_err(), FsmError::EmptyEvent));
}

#[test]
fn guard_blocks_transition() {
    let mut m = Machine::new("closed".to_string()).expect("closed");
    m.add_transition(
        "closed".into(),
        "open".into(),
        "opened".into(),
        Some(Guard::never()),
        None,
    )
    .expect("register guarded");
    assert!(!m.can_fire("open"));
    let err = m.fire("open").unwrap_err();
    assert!(matches!(
        err,
        FsmError::GuardBlocked {
            current,
            event,
            to
        } if current == "closed" && event == "open" && to == "opened"
    ));
    assert_eq!(m.current_state(), "closed");
}

#[test]
fn guard_allows_transition() {
    let mut m = Machine::new("closed".to_string()).expect("closed");
    let counter = Arc::new(Mutex::new(0u32));
    let c = counter.clone();
    m.add_transition(
        "closed".into(),
        "open".into(),
        "opened".into(),
        Some(Guard::new(move || {
            let mut g = c.lock().expect("lock");
            *g += 1;
            true
        })),
        None,
    )
    .expect("register guarded");
    assert!(m.can_fire("open"));
    m.fire("open").expect("open");
    assert_eq!(m.current_state(), "opened");
    assert_eq!(*counter.lock().expect("lock"), 1);
}

#[test]
fn action_fires_on_successful_transition() {
    let mut m = Machine::new("idle".to_string()).expect("idle");
    let fired = Arc::new(Mutex::new(false));
    let f = fired.clone();
    m.add_transition(
        "idle".into(),
        "go".into(),
        "running".into(),
        None,
        Some(Action::new(move || {
            *f.lock().expect("lock") = true;
        })),
    )
    .expect("register action");
    m.fire("go").expect("go");
    assert_eq!(m.current_state(), "running");
    assert!(*fired.lock().expect("lock"));
}

#[test]
fn action_consumed_after_first_fire() {
    let mut m = Machine::new("a".to_string()).expect("a");
    let count = Arc::new(Mutex::new(0u32));
    m.add_transition("a".into(), "next".into(), "b".into(), None, {
        let c = count.clone();
        Some(Action::new(move || {
            *c.lock().expect("lock") += 1;
        }))
    })
    .expect("register");
    m.fire("next").expect("a->b");
    assert_eq!(*count.lock().expect("lock"), 1);
    assert!(m.transitions().iter().all(|t| !t.has_action));
}

#[test]
fn multiple_transitions_for_same_event_first_passing_guard_wins() {
    let mut m = Machine::new("idle".to_string()).expect("idle");
    m.add_transition(
        "idle".into(),
        "go".into(),
        "slow".into(),
        Some(Guard::never()),
        None,
    )
    .expect("guarded slow");
    m.add_transition(
        "idle".into(),
        "go".into(),
        "fast".into(),
        Some(Guard::always()),
        None,
    )
    .expect("guarded fast");
    m.fire("go").expect("go");
    assert_eq!(m.current_state(), "fast");
}

#[test]
fn can_fire_distinguishes_known_and_unknown_events() {
    let mut m = traffic_light();
    assert!(m.can_fire("tick"));
    assert!(!m.can_fire("jump"));
    m.fire("tick").expect("g->y");
    assert!(m.can_fire("tick"));
    m.fire("tick").expect("y->r");
    assert!(m.can_fire("tick"));
}

#[test]
fn reset_returns_to_initial_state() {
    let mut m = traffic_light();
    m.fire("tick").expect("g->y");
    m.fire("tick").expect("y->r");
    assert_eq!(m.current_state(), "red");
    m.reset();
    assert_eq!(m.current_state(), "green");
    assert_eq!(m.initial_state(), "green");
}

#[test]
fn mark_terminal_blocks_further_events() {
    let mut m = traffic_light();
    m.fire("tick").expect("g->y");
    m.fire("tick").expect("y->r");
    m.mark_terminal("red").expect("mark");
    assert!(m.is_terminal());
    let err = m.fire("tick").unwrap_err();
    assert!(matches!(
        err,
        FsmError::TerminalState {
            current,
            event
        } if current == "red" && event == "tick"
    ));
}

#[test]
fn mark_terminal_unknown_state_errors() {
    let mut m = traffic_light();
    let err = m.mark_terminal("purple").unwrap_err();
    assert!(matches!(err, FsmError::UnknownState { state } if state == "purple"));
}

#[test]
fn states_and_events_list_lexicographically() {
    let m = traffic_light();
    assert_eq!(m.states(), vec!["green", "red", "yellow"]);
    assert_eq!(m.events(), vec!["tick"]);
}

#[test]
fn transitions_summary_lists_in_registration_order() {
    let m = traffic_light();
    let summaries = m.transitions();
    assert_eq!(summaries.len(), 3);
    assert_eq!(summaries[0].from, "green");
    assert_eq!(summaries[0].event, "tick");
    assert_eq!(summaries[0].to, "yellow");
    assert!(!summaries[0].has_guard);
    assert!(!summaries[0].has_action);
}

#[test]
fn default_machine_is_functional() {
    let mut m = Machine::default();
    assert_eq!(m.current_state(), "<init>");
    assert!(matches!(
        m.fire("anything").unwrap_err(),
        FsmError::UnknownEvent { .. }
    ));
}

#[test]
fn fire_catches_panic_in_guard() {
    let mut m = Machine::new("a".to_string()).expect("a");
    m.add_transition(
        "a".into(),
        "go".into(),
        "b".into(),
        Some(Guard::new(|| panic!("boom"))),
        None,
    )
    .expect("register panicking guard");
    assert!(matches!(m.fire("go").unwrap_err(), FsmError::Panic));
    assert_eq!(m.current_state(), "a");
}

#[test]
fn fire_catches_panic_in_action_and_keeps_new_state() {
    let mut m = Machine::new("a".to_string()).expect("a");
    m.add_transition(
        "a".into(),
        "go".into(),
        "b".into(),
        None,
        Some(Action::new(|| panic!("boom"))),
    )
    .expect("register panicking action");
    assert!(matches!(m.fire("go").unwrap_err(), FsmError::Panic));
    assert_eq!(m.current_state(), "b");
}

#[test]
fn machine_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Machine>();
    assert_send_sync::<Guard>();
    assert_send_sync::<Action>();
}

// ---- Insta snapshots (5+) ---------------------------------------------------

#[test]
fn snapshot_machine_display() {
    let m = traffic_light();
    insta::assert_snapshot!("machine_display", format!("{m}"));
}

#[test]
fn snapshot_transitions_summary() {
    let m = traffic_light();
    let summaries = m.transitions();
    let rendered: Vec<String> = summaries
        .iter()
        .map(|t| {
            format!(
                "{} --{}--> {} [guard={}, action={}]",
                t.from, t.event, t.to, t.has_guard, t.has_action
            )
        })
        .collect();
    insta::assert_snapshot!("transitions_summary", rendered.join("\n"));
}

#[test]
fn snapshot_fsm_error_messages() {
    let e1 = FsmError::EmptyInitialState;
    let e2 = FsmError::UnknownEvent {
        current: "green".into(),
        event: "jump".into(),
    };
    let e3 = FsmError::GuardBlocked {
        current: "closed".into(),
        event: "open".into(),
        to: "opened".into(),
    };
    let e4 = FsmError::TerminalState {
        current: "done".into(),
        event: "tick".into(),
    };
    let e5 = FsmError::Panic;
    insta::assert_snapshot!(
        "fsm_error_messages",
        format!("{e1}\n{e2}\n{e3}\n{e4}\n{e5}")
    );
}

#[test]
fn snapshot_default_machine() {
    let m = Machine::default();
    insta::assert_snapshot!("default_machine", format!("{m}"));
}

#[test]
fn snapshot_states_and_events_after_lifecycle() {
    let mut m = traffic_light();
    m.fire("tick").expect("g->y");
    m.mark_terminal("red").expect("mark");
    let states = m.states().join(",");
    let events = m.events().join(",");
    let terminal = m.is_terminal();
    insta::assert_snapshot!(
        "after_lifecycle",
        format!("states={states}\nevents={events}\nterminal={terminal}")
    );
}
