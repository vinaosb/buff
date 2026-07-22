//! `buff-fsm` — a state machine library for the Buff language.
//!
//! Pure-Rust hand-rolled MVP. The T40 task spec offered `statig` OR hand-rolled;
//! hand-rolled wins for the MVP scope (no external state-machine dep, <500 LOC,
//! full control over the FFI safety boundary).
//!
//! # Pipeline
//!
//! ```text
//!   Machine.new("green")  ──┐
//!                           ▼
//!       ┌─────────────── Machine { current, transitions, ... }
//!       │                │
//!       │  .add_transition("green", "tick", "yellow", guard?, action?)
//!       │  .add_transition("yellow", "tick", "red",    guard?, action?)
//!       │  .add_transition("red",    "tick", "green",  guard?, action?)
//!       │                │
//!       │                ▼
//!       │  machine.fire("tick")  ──▶ Err(FsmError) | Ok(())
//!       │                │           (guard runs; action fires on success)
//!       │                ▼
//!       └─────────────── machine.current_state() -> &str
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Machine`, `Guard`, `Action`, `TransitionSummary`, `FsmError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `Machine::new` returns an owned `Machine`. `add_transition` takes owned `String`s + owned `Guard` / `Action` boxes. `current_state` borrows. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, FsmError>`. No underlying crate errors (zero external deps) — every failure is mapped to a specific `FsmError` variant. |
//! | R4 — Thread safety | `Machine` is `Send + Sync` (guards/actions require `Send + Sync` closures; no `Rc` / `Cell` anywhere). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `current_state` returns `&str` tied to `&self` (Rust-only callers; the codegen-lowered Buff surface clones to `String`). |
//! | R6 — Panic boundary | `fire` wraps guard + action invocation in `catch_unwind` (per FFI guide §6) so a panicking guard/action becomes `Err(FsmError::Panic)` instead of process abort. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. User-supplied guard/action closures are the ONLY panic
//! source — `catch_unwind` in [`Machine::fire`] is the safety boundary.

pub mod error;

pub use error::FsmError;

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// A guard predicate evaluated before a transition fires.
///
/// Wraps a `Box<dyn Fn() -> bool + Send + Sync>` closure. The closure takes
/// no arguments (the guard is a predicate over external state, not the
/// machine's state — the machine state is queried via [`Machine::current_state`]
/// if the guard needs it). Returns `true` to allow the transition, `false` to
/// block it (surfacing as [`FsmError::GuardBlocked`] from [`Machine::fire`]).
///
/// Constructed via [`Guard::new`] or the [`Guard::always`] / [`Guard::never`]
/// convenience constructors. The `Send + Sync` bound matches the T4 FFI guide
/// rule R4 (Thread Safety) — a `Machine` may be captured by a `spawn` closure.
pub struct Guard(Box<dyn Fn() -> bool + Send + Sync>);

impl Guard {
    /// Construct a guard from any `Fn() -> bool + Send + Sync` closure.
    pub fn new<F>(f: F) -> Self
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        Guard(Box::new(f))
    }

    /// A guard that always allows the transition. Equivalent to passing
    /// `None` as the guard argument to [`Machine::add_transition`]; provided
    /// as a constructor for callers that prefer an explicit always-true
    /// sentinel.
    pub fn always() -> Self {
        Guard(Box::new(|| true))
    }

    /// A guard that always blocks the transition. Useful for stubbing out
    /// a transition during development without removing the registration.
    pub fn never() -> Self {
        Guard(Box::new(|| false))
    }

    /// Evaluate the guard predicate. Inlined because every [`Machine::fire`]
    /// call invokes this exactly once per matching transition.
    #[inline]
    fn check(&self) -> bool {
        (self.0)()
    }
}

impl std::fmt::Debug for Guard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Guard").finish_non_exhaustive()
    }
}

/// An action callback fired AFTER a transition completes successfully.
///
/// Wraps a `Box<dyn FnOnce() + Send + Sync>` closure. The closure takes no
/// arguments; it has access to external state via capture. The closure runs
/// inside [`Machine::fire`] AFTER the new state has been committed — if the
/// closure panics, the new state remains (the transition already happened)
/// and [`Machine::fire`] returns [`FsmError::Panic`].
///
/// Constructed via [`Action::new`] or the [`Action::noop`] convenience
/// constructor.
pub struct Action(Box<dyn FnOnce() + Send + Sync>);

impl Action {
    /// Construct an action from any `FnOnce() + Send + Sync` closure.
    pub fn new<F>(f: F) -> Self
    where
        F: FnOnce() + Send + Sync + 'static,
    {
        Action(Box::new(f))
    }

    /// An action that does nothing. Equivalent to passing `None` as the
    /// action argument to [`Machine::add_transition`]; provided as a
    /// constructor for callers that prefer an explicit no-op sentinel.
    pub fn noop() -> Self {
        Action(Box::new(|| {}))
    }

    /// Run the action exactly once, consuming self.
    #[inline]
    fn run(self) {
        (self.0)();
    }
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Action").finish_non_exhaustive()
    }
}

/// An immutable summary of a registered transition (diagnostic only).
///
/// Returned by [`Machine::transitions`] for introspection / debugging /
/// snapshot tests. Does NOT expose the guard or action (which are not
/// `Clone`); only flags whether they are present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionSummary {
    pub from: String,
    pub event: String,
    pub to: String,
    pub has_guard: bool,
    pub has_action: bool,
}

/// One registered transition inside a [`Machine`]. Internal — the Buff-visible
/// surface for transition introspection is [`TransitionSummary`].
#[derive(Debug)]
struct Transition {
    from: String,
    event: String,
    to: String,
    guard: Option<Guard>,
    action: Option<Action>,
}

/// A state machine.
///
/// Constructed via [`Machine::new`] with the initial state name. Transitions
/// are registered via [`Machine::add_transition`]. Events are dispatched via
/// [`Machine::fire`]. The current state is read via [`Machine::current_state`].
///
/// State names and event names are arbitrary non-empty [`String`]s. The
/// caller is responsible for choosing a naming scheme (string enums like
/// `"green"` / `"yellow"` / `"red"` are idiomatic; hierarchical names like
/// `"idle.loading"` work too).
#[derive(Debug)]
pub struct Machine {
    initial: String,
    current: String,
    transitions: Vec<Transition>,
    states: BTreeSet<String>,
    terminal_states: BTreeSet<String>,
    /// Index from (from_state, event) to indices in `transitions`. A single
    /// (from, event) key may map to multiple transitions if the user registers
    /// more than one — the first whose guard passes wins (deterministic order
    /// matches the registration order, mirroring `xstate` / `stateless`).
    by_key: BTreeMap<(String, String), Vec<usize>>,
}

impl Machine {
    /// Construct a new state machine rooted at `initial_state`.
    ///
    /// Returns [`FsmError::EmptyInitialState`] if the name is empty. The
    /// initial state is automatically added to the machine's known-states
    /// set; subsequent calls to [`Machine::add_transition`] register the
    /// other states. [`Machine::reset`] returns the machine to `initial`.
    pub fn new(initial_state: String) -> Result<Self, FsmError> {
        if initial_state.is_empty() {
            return Err(FsmError::EmptyInitialState);
        }
        let mut states = BTreeSet::new();
        states.insert(initial_state.clone());
        Ok(Machine {
            initial: initial_state.clone(),
            current: initial_state,
            transitions: Vec::new(),
            states,
            terminal_states: BTreeSet::new(),
            by_key: BTreeMap::new(),
        })
    }

    /// Register a transition from `from` to `to` on `event`.
    ///
    /// `guard` and `action` are optional; pass `None` for an unconditional
    /// transition or a transition with no side effect. When `guard` is
    /// `Some` and the predicate returns `false`, the transition is blocked
    /// and [`Machine::fire`] returns [`FsmError::GuardBlocked`]. When
    /// `action` is `Some`, the closure runs after the new state is committed
    /// (a panic inside the action surfaces as [`FsmError::Panic`], but the
    /// new state stays committed — the transition already happened).
    ///
    /// Multiple transitions for the same `(from, event)` pair are allowed —
    /// the FIRST one whose guard passes wins (registration order). This
    /// enables conditional routing: register the guarded transition first,
    /// then a fallback `None`-guard transition.
    ///
    /// Returns [`FsmError::EmptyIdentifier`] if any of `from` / `event` /
    /// `to` is empty.
    pub fn add_transition(
        &mut self,
        from: String,
        event: String,
        to: String,
        guard: Option<Guard>,
        action: Option<Action>,
    ) -> Result<(), FsmError> {
        if from.is_empty() || event.is_empty() || to.is_empty() {
            return Err(FsmError::EmptyIdentifier { from, event, to });
        }
        let key = (from.clone(), event.clone());
        let idx = self.transitions.len();
        self.states.insert(from.clone());
        self.states.insert(to.clone());
        self.transitions.push(Transition {
            from,
            event,
            to,
            guard,
            action,
        });
        self.by_key.entry(key).or_default().push(idx);
        Ok(())
    }

    /// Fire `event` from the current state.
    ///
    /// Walks the registered transitions for `(current_state, event)` in
    /// registration order. The FIRST transition whose guard passes is
    /// applied: the current state is updated, the action (if any) runs,
    /// and `Ok(())` is returned. If no matching transition exists,
    /// [`FsmError::UnknownEvent`] is returned. If a transition matches
    /// but its guard blocks it (and no later transition for the same
    /// `(from, event)` passes), [`FsmError::GuardBlocked`] is returned.
    /// If the current state is marked terminal (see [`Machine::mark_terminal`]),
    /// [`FsmError::TerminalState`] is returned.
    ///
    /// Guards and actions are invoked inside `catch_unwind` per the T4 FFI
    /// guide R6 (Panic Boundary) — a panic inside either surfaces as
    /// [`FsmError::Panic`] instead of process abort. The state update
    /// happens BEFORE the action runs; if the action panics, the new state
    /// is retained (the transition already happened).
    pub fn fire(&mut self, event: &str) -> Result<(), FsmError> {
        if event.is_empty() {
            return Err(FsmError::EmptyEvent);
        }
        if self.terminal_states.contains(&self.current) {
            return Err(FsmError::TerminalState {
                current: self.current.clone(),
                event: event.to_string(),
            });
        }
        let key = (self.current.clone(), event.to_string());
        let indices = match self.by_key.get(&key) {
            Some(v) if !v.is_empty() => v,
            _ => {
                return Err(FsmError::UnknownEvent {
                    current: self.current.clone(),
                    event: event.to_string(),
                });
            }
        };
        let mut chosen: Option<usize> = None;
        let mut any_guarded_blocked = false;
        for &i in indices {
            let guard_passes = match self.transitions[i].guard.as_ref() {
                None => true,
                Some(g) => {
                    let g_check = AssertUnwindSafe(|| g.check());
                    match catch_unwind(g_check) {
                        Ok(true) => true,
                        Ok(false) => {
                            any_guarded_blocked = true;
                            false
                        }
                        Err(_) => return Err(FsmError::Panic),
                    }
                }
            };
            if guard_passes {
                chosen = Some(i);
                break;
            }
        }
        let idx = match chosen {
            Some(i) => i,
            None => {
                let to = self.transitions[indices[0]].to.clone();
                return Err(if any_guarded_blocked {
                    FsmError::GuardBlocked {
                        current: self.current.clone(),
                        event: event.to_string(),
                        to,
                    }
                } else {
                    FsmError::UnknownEvent {
                        current: self.current.clone(),
                        event: event.to_string(),
                    }
                });
            }
        };
        let new_state = self.transitions[idx].to.clone();
        let action = self.transitions[idx].action.take();
        self.current = new_state.clone();
        self.states.insert(new_state);
        if let Some(act) = action {
            let act_runner = AssertUnwindSafe(move || act.run());
            if catch_unwind(act_runner).is_err() {
                return Err(FsmError::Panic);
            }
        }
        Ok(())
    }

    /// Borrow the current state name.
    ///
    /// Returns `&str` tied to `&self`. Rust-only callers can borrow; the
    /// codegen-lowered Buff surface clones to `String` (matches the FFI
    /// guide R5 — no Buff-visible lifetimes).
    #[inline]
    pub fn current_state(&self) -> &str {
        &self.current
    }

    /// Borrow the initial state name (the state the machine resets to via
    /// [`Machine::reset`]).
    #[inline]
    pub fn initial_state(&self) -> &str {
        &self.initial
    }

    /// Returns `true` if firing `event` from the current state would succeed
    /// (i.e. at least one transition matches AND its guard would pass).
    ///
    /// Does NOT mutate the machine. Cheap (linear in transitions for the
    /// current state + event pair; typically O(1)-ish).
    pub fn can_fire(&self, event: &str) -> bool {
        if event.is_empty() || self.terminal_states.contains(&self.current) {
            return false;
        }
        let key = (self.current.clone(), event.to_string());
        match self.by_key.get(&key) {
            Some(indices) => indices.iter().any(|&i| {
                // `can_fire` is a non-mutating peek; a panicking guard is
                // treated as `false` (the safer of the two answers). The
                // full fire() path uses catch_unwind per FFI guide R6.
                let t = &self.transitions[i];
                match t.guard.as_ref() {
                    None => true,
                    Some(g) => {
                        std::panic::catch_unwind(AssertUnwindSafe(|| g.check())).unwrap_or(false)
                    }
                }
            }),
            None => false,
        }
    }

    /// Returns `true` if the current state equals `state`. Convenience
    /// equivalent to `machine.current_state() == state`.
    #[inline]
    pub fn is_in(&self, state: &str) -> bool {
        self.current == state
    }

    /// Returns `true` if the current state has been marked terminal via
    /// [`Machine::mark_terminal`]. A terminal state rejects every event
    /// with [`FsmError::TerminalState`].
    #[inline]
    pub fn is_terminal(&self) -> bool {
        self.terminal_states.contains(&self.current)
    }

    /// Mark `state` as terminal. A terminal state has no outgoing transitions
    /// (fire returns [`FsmError::TerminalState`]). Returns
    /// [`FsmError::UnknownState`] if `state` is not known to the machine.
    ///
    /// Marking a non-terminal state terminal does NOT remove its outgoing
    /// transitions from the table (they remain for diagnostic purposes via
    /// [`Machine::transitions`]); it only causes [`Machine::fire`] to refuse
    /// to dispatch from them.
    pub fn mark_terminal(&mut self, state: &str) -> Result<(), FsmError> {
        if !self.states.contains(state) {
            return Err(FsmError::UnknownState {
                state: state.to_string(),
            });
        }
        self.terminal_states.insert(state.to_string());
        Ok(())
    }

    /// Reset the machine to its initial state. Does NOT remove transitions,
    /// terminal markings, or registered states. Idempotent.
    pub fn reset(&mut self) {
        self.current = self.initial.clone();
    }

    /// Return all known state names (lexicographically sorted). Includes
    /// the initial state + every state referenced by any registered
    /// transition (either as `from` or `to`).
    pub fn states(&self) -> Vec<&str> {
        self.states.iter().map(|s| s.as_str()).collect()
    }

    /// Return all known event names (lexicographically sorted). Includes
    /// every event referenced by any registered transition.
    pub fn events(&self) -> Vec<&str> {
        let mut events: BTreeSet<&str> = BTreeSet::new();
        for t in &self.transitions {
            events.insert(t.event.as_str());
        }
        events.into_iter().collect()
    }

    /// Return summaries of all registered transitions (in registration order).
    /// Diagnostic / introspection surface — used by snapshot tests and the
    /// `examples/fsm_dump.rs` example. Does NOT expose guards or actions
    /// (which are not `Clone`); only flags their presence.
    ///
    /// `has_action` reports `false` for transitions that have already fired
    /// at least once: the action is `FnOnce` and consumed on the first
    /// successful fire (a closure that captures unique resources like a
    /// oneshot channel sender runs exactly once). Guards survive (they are
    /// `Fn`, not `FnOnce`). A future v1.18+ enhancement may add a
    /// repeatable `Action::clonable` variant; the MVP keeps `FnOnce`.
    pub fn transitions(&self) -> Vec<TransitionSummary> {
        self.transitions
            .iter()
            .map(|t| TransitionSummary {
                from: t.from.clone(),
                event: t.event.clone(),
                to: t.to.clone(),
                has_guard: t.guard.is_some(),
                has_action: t.action.is_some(),
            })
            .collect()
    }
}

impl Default for Machine {
    /// Construct a trivial machine rooted at the sentinel state `"<init>"`.
    /// Used by codegen-lowered Buff call sites that need `unwrap_or_default()`
    /// on `Result<Machine, FsmError>` return paths (matches the buff-image
    /// `Default for Image` precedent). The default machine is functional —
    /// `current_state()` returns `"<init>"`, `fire(any)` returns
    /// [`FsmError::UnknownEvent`].
    fn default() -> Self {
        let initial = "<init>".to_string();
        let mut states = BTreeSet::new();
        states.insert(initial.clone());
        Machine {
            initial: initial.clone(),
            current: initial,
            transitions: Vec::new(),
            states,
            terminal_states: BTreeSet::new(),
            by_key: BTreeMap::new(),
        }
    }
}

impl std::fmt::Display for Machine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Machine(current={:?}, initial={:?}, states={}, transitions={})",
            self.current,
            self.initial,
            self.states.len(),
            self.transitions.len(),
        )
    }
}
