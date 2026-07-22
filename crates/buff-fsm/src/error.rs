//! Error type for the `buff-fsm` crate.
//!
//! Every fallible operation on [`crate::Machine`] surfaces as [`FsmError`].
//! The single public entry points map every internal failure mode into this
//! enum so the crate's public surface depends only on `buff-fsm`'s own types
//! (Buff code never sees a raw `std::*` or third-party error type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in this
//! module or any non-test code path. Per the T4 FFI guide R6 (Panic Boundary)
//! the [`crate::Machine::fire`] entry point uses `catch_unwind` so panics
//! raised inside user-supplied guards or actions never propagate across the
//! FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-fsm` operation.
#[derive(Debug, Error)]
pub enum FsmError {
    /// The user constructed a [`crate::Machine`] with an empty initial-state
    /// name. State names MUST be non-empty so the diagnostic can reference
    /// them unambiguously (the empty string is reserved as a "no transition
    /// found" sentinel internally and would collide with a valid name).
    #[error("machine initial state name must not be empty")]
    EmptyInitialState,

    /// The user supplied an empty `from` / `event` / `to` identifier to
    /// [`crate::Machine::add_transition`]. Identifiers MUST be non-empty
    /// for the same reason as [`Self::EmptyInitialState`].
    #[error(
        "transition identifiers must not be empty (from={from:?}, event={event:?}, to={to:?})"
    )]
    EmptyIdentifier {
        from: String,
        event: String,
        to: String,
    },

    /// The user called [`crate::Machine::fire`] with an empty event name.
    /// Distinct from [`Self::UnknownEvent`] (which fires for a non-empty but
    /// unrecognised name) so the diagnostic can be specific.
    #[error("fire called with empty event name")]
    EmptyEvent,

    /// The user fired an event that has no transition registered from the
    /// current state. The (current, event) tuple is included so the
    /// diagnostic can say "event 'pay' not valid from state 'cart'".
    #[error("event {event:?} not valid from state {current:?}")]
    UnknownEvent { current: String, event: String },

    /// A registered guard blocked the transition. The (current, event, to)
    /// tuple is included so the diagnostic can explain which transition was
    /// suppressed and where it would have led.
    #[error("guard blocked transition {current:?} --{event:?}--> {to:?}")]
    GuardBlocked {
        current: String,
        event: String,
        to: String,
    },

    /// The user attempted to fire an event from a terminal state. Terminal
    /// states have no outgoing transitions by definition; firing from one
    /// is a programming error worth a distinct diagnostic (vs. the generic
    /// [`Self::UnknownEvent`] which fires for missing transitions).
    #[error("machine is in terminal state {current:?} and cannot fire {event:?}")]
    TerminalState { current: String, event: String },

    /// The user attempted to register or mark a state that has never been
    /// observed by the machine (i.e. it is not the initial state and does
    /// not appear as `from` or `to` on any transition).
    #[error("unknown state {state:?} (register a transition that references it first)")]
    UnknownState { state: String },

    /// A wrapper-internal panic was caught by `catch_unwind` (per T4 FFI
    /// guide R6). The user sees a stable diagnostic instead of a process
    /// abort. Most commonly raised when a user-supplied guard or action
    /// closure panics.
    #[error("internal error: state machine operation panicked")]
    Panic,
}
