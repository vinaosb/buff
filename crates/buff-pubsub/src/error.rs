//! Error type for the `buff-pubsub` crate.
//!
//! All fallible operations surface as [`PubSubError`]. The public
//! entry points ([`crate::EventBus::new`], `subscribe`, `publish`,
//! `unsubscribe`) wrap their bodies in `catch_unwind` per the T4 FFI
//! guide R6 so panics never propagate across the FFI boundary into
//! Buff code.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. The channel primitives
//! (`crossbeam_channel::Sender::send`) return `Result` explicitly so
//! the only failure modes are user-visible ([`Self::UnknownSubscription`],
//! [`Self::EmptyTopic`], [`Self::EmptySubscribeTopic`]) or
//! runtime-internal ([`Self::Panic`]).

use thiserror::Error;

/// The single error type returned by every fallible `buff-pubsub`
/// operation.
#[derive(Debug, Error)]
pub enum PubSubError {
    /// `unsubscribe` was called with an id that no longer matches
    /// any active subscription. Either the id was already
    /// unsubscribed, or it was never returned by `subscribe`.
    /// Includes the offending id so the caller can correlate.
    #[error("unknown subscription id: {0}")]
    UnknownSubscription(u64),

    /// `publish` was called with an empty topic string. Empty
    /// topics are rejected because the bus uses `""` as a sentinel
    /// for "no topic" in its internal map; allowing empty would
    /// collide with legitimate lookups.
    #[error("publish called with empty topic")]
    EmptyTopic,

    /// `subscribe` was called with an empty topic string. Same
    /// rationale as [`Self::EmptyTopic`].
    #[error("subscribe called with empty topic")]
    EmptySubscribeTopic,

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: pub/sub operation panicked")]
    Panic,
}
