//! Error type for the `buff-actors` crate.
//!
//! All fallible operations surface as [`ActorError`]. The public
//! entry points ([`crate::ActorSystem::new`], `spawn`, `register`,
//! `lookup`, [`crate::Supervisor::start_child`]) wrap their bodies in
//! `catch_unwind` per the T4 FFI guide R6 so panics never propagate
//! across the FFI boundary into Buff code.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. The channel primitives
//! (`crossbeam_channel::Sender::send`) return `Result` explicitly so
//! the only failure modes are user-visible ([`Self::ActorStopped`],
//! [`Self::DuplicateName`], [`Self::UnknownName`],
//! [`Self::EmptyName`]) or runtime-internal ([`Self::Panic`]).

use thiserror::Error;

/// The single error type returned by every fallible `buff-actors`
/// operation.
#[derive(Debug, Error)]
pub enum ActorError {
    /// `ActorRef::send` was called after the actor's mailbox was
    /// already disconnected (the actor exited, was shut down, or was
    /// restarted by its supervisor — the new instance has a fresh
    /// `ActorRef`). Includes the actor id so the caller can correlate
    /// against `ActorSystem::actor_count` / named-registry lookups.
    #[error("actor {0} stopped: mailbox disconnected")]
    ActorStopped(u64),

    /// `ActorSystem::register` was called with a name already in use
    /// by another live `ActorRef`. Names are unique per
    /// `ActorSystem`; re-registering a live name is rejected so
    /// `lookup` is deterministic. Includes the offending name.
    #[error("duplicate actor name: {0}")]
    DuplicateName(String),

    /// `ActorSystem::register` (or `ChildSpec::with_name`) was called
    /// with an empty string. Empty names are rejected because the
    /// registry uses `""` as the internal sentinel for "no name"
    /// (matching the `buff-pubsub` topic-sentinel convention).
    #[error("actor name must be non-empty")]
    EmptyName,

    /// `ActorSystem::lookup` was called with a name that does not
    /// match any registered actor (either never registered, or the
    /// registrant was shut down + the registry entry cleared).
    /// Returned as `Option<ActorRef>::None` by `lookup` (so this
    /// variant is for callers that need a typed error rather than
    /// `None`).
    #[error("unknown actor name: {0}")]
    UnknownName(String),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: actor operation panicked")]
    Panic,
}
