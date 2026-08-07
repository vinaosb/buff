//! T2 (v1.13 wave 1): MPSC channel primitive wrapping `tokio::sync::mpsc`.
//!
//! # Scope (v1.13-v1.17)
//!
//! This module implements Buff's in-process producer/consumer primitive:
//!
//! - [`Channel::new`] constructs a bounded `(Sender<T>, Receiver<T>)` pair
//!   backed by `tokio::sync::mpsc::channel`.
//! - [`Sender<T>`] wraps `tokio::sync::mpsc::Sender<T>` and exposes a
//!   single async [`Sender::send`] method.
//! - [`Receiver<T>`] wraps `tokio::sync::mpsc::Receiver<T>` and exposes
//!   async [`Receiver::recv`] + sync [`Receiver::close`].
//!
//! # Why a runtime abstraction
//!
//! Buff's "no function-coloring" design (T31) hides `async`/`await` from
//! the user — the compiler inserts `.await` at async call sites and
//! propagates async-ness up the call graph. The user therefore never
//! sees `tokio::sync::mpsc::Sender` directly; they see the Buff surface
//! `Channel.new(buf_size)` / `sender.send(value)` / `receiver.recv()` /
//! `receiver.close()`. The codegen lowers each call to the matching
//! `buff_lang_runtime::*` path (Metis G6 — framework users see the
//! runtime abstraction, NOT the underlying tokio API).
//!
//! # Memory model
//!
//! Both [`Sender<T>`] and [`Receiver<T>`] are `Send + 'static` when `T`
//! is (matching tokio mpsc's requirement). This lets a sender be moved
//! into a `spawn` body (the producer/consumer pattern) without lifetime
//! gymnastics — Buff's move-by-default semantics hide the transfer.
//!
//! # Deferred to v1.18+
//!
//! Per the T2 spec (REDUCED SCOPE):
//! - `Stream<T>` general async iterable type — deferred.
//! - `select` expression — deferred.
//! - Async-aware locks (`tokio::sync::Mutex`) — `std::sync::Mutex` only
//!   in the MVP (the codegen does not introduce any locks here today;
//!   this caveat applies to future framework code that may share state
//!   across producers).
//! - Broadcast channels — single-consumer MPSC only.
//!
//! # Determinism
//!
//! Channel order is FIFO per sender (tokio mpsc guarantee). Multi-
//! producer ordering is per-producer FIFO; interleaving across
//! producers is runtime-defined (depending on tokio scheduler
//! decisions). See `mpsc_ordering` test for the per-producer guarantee.

use crate::error::RuntimeError;

/// Factory namespace for Buff's bounded MPSC channel primitive.
///
/// The namespace itself is **never a runtime value** in Buff — it's a
/// pure factory for [`Sender<T>`] / [`Receiver<T>`] pairs, mirroring the
/// namespace-only prelude modules (Log / Toml / Math / TCP / UDP / WebSocket).
/// Buff surfaces it as the prelude type `Channel`; users write
/// `Channel.new(buf_size)` and destructure the returned tuple.
///
/// # Example (generated Rust shape)
///
/// ```rust,ignore
/// let (sender, receiver) = buff_lang_runtime::Channel::new::<i64>(10);
/// // sender.send(42) / receiver.recv() / receiver.close() work directly.
/// ```
///
/// # Why a unit struct + inherent `new`
///
/// Tokio's `tokio::sync::mpsc::channel` is a free function returning a
/// tuple. Buff's surface is `Channel.new(buf_size)` (method-style), so
/// we expose an inherent associated function `Channel::new` on a unit
/// struct that mirrors the Buff namespace. There's no state on the
/// `Channel` struct itself — it exists purely as a path anchor for
/// `Channel::new`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel;

impl Channel {
    /// Construct a bounded MPSC channel pair.
    ///
    /// Wraps `tokio::sync::mpsc::channel(buffer)`; the buffer size is
    /// the maximum number of in-flight messages the channel holds
    /// before `send` blocks (the canonical "bounded backpressure"
    /// semantic). A buffer of `n` lets up to `n` sends complete without
    /// a recv.
    ///
    /// A `buffer` of `0` is coerced to `1`. Historically tokio treated
    /// `channel(0)` as a "rendezvous" channel (send blocks until recv is
    /// ready), but current tokio (1.40+) **panics** on `channel(0)` with
    /// `"mpsc bounded channel requires buffer > 0"`. To preserve this
    /// factory's "never panics" contract (see `# Panics` below), a `0` is
    /// silently clamped to the minimum legal bound of `1`. Callers that
    /// pass `0` therefore get a 1-slot bounded channel — the closest
    /// practical approximation of rendezvous semantics that current tokio
    /// supports without panicking.
    ///
    /// # Type parameter
    ///
    /// `T: Send + 'static` matches tokio mpsc's bound. The Buff
    /// codegen does NOT emit a turbofish at the call site (Rust's
    /// type inference derives `T` from subsequent `sender.send(value)`
    /// / `receiver.recv()` usage). Programs that never use the
    /// sender/receiver after construction would need an explicit
    /// type annotation — Buff surfaces this as a regular Rust
    /// inference error (no special codegen-time diagnostic).
    ///
    /// # Panics
    ///
    /// NEVER. `tokio::sync::mpsc::channel` is infallible (returns a
    /// tuple directly, no `Result`). A `buffer` of `0` is coerced to `1`
    /// before the call so the upstream "requires buffer > 0" panic
    /// (tokio 1.40+) cannot fire — see the construction notes above.
    #[allow(clippy::new_ret_no_self)] // Factory by design: returns (Sender, Receiver), not Channel.
    #[must_use]
    pub fn new<T: Send + 'static>(buffer: usize) -> (Sender<T>, Receiver<T>) {
        // Coerce 0 → 1: tokio 1.40+ panics on `mpsc::channel(0)` with
        // "mpsc bounded channel requires buffer > 0". Clamping to 1
        // preserves the documented "never panics" contract; a 1-slot
        // bounded channel is the closest non-panicking approximation of
        // the historical rendezvous semantic. max(1, buffer) is the
        // cheapest correct guard.
        let buffer = buffer.max(1);
        let (tx, rx) = tokio::sync::mpsc::channel(buffer);
        (Sender(tx), Receiver(rx))
    }
}

/// Sending half of a Buff MPSC channel.
///
/// Constructed exclusively via [`Channel::new`]. Wraps
/// `tokio::sync::mpsc::Sender<T>` and exposes [`Sender::send`] as the
/// single async API. `Clone` IS exposed (matching tokio mpsc) so the
/// multi-producer pattern works directly: `let s2 = sender.clone();`
/// produces a second sender that shares the channel's queue; dropping
/// ALL clones is what signals "channel closed" to the receiver.
#[derive(Debug)]
pub struct Sender<T>(pub tokio::sync::mpsc::Sender<T>);

// Manual `Clone` impl (instead of `#[derive(Clone)]`) because Rust's
// derive macro adds a `T: Clone` bound that is INCORRECT here —
// `tokio::sync::mpsc::Sender<T>` is `Clone` for ANY `T` (the sender
// holds an `Arc` internally; the value `T` is never cloned at the
// sender site). The derive-generated bound broke `buff-pipeline`'s
// multi-producer `parallel` stage (T14) where the sender's `T` is a
// stage-output type that is not `Clone`. Mirrors the upstream tokio
// impl: `impl<T> Clone for tokio::sync::mpsc::Sender<T>`.
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender(self.0.clone())
    }
}

impl<T> Sender<T> {
    /// Send a value on the channel.
    ///
    /// Wraps `tokio::sync::mpsc::Sender::send`. Returns:
    /// - `Ok(())` on successful send (the receiver holds or will
    ///   eventually hold the value).
    /// - `Err(RuntimeError::Unsupported { .. })` if the receiver was
    ///   dropped before the value could be delivered (the canonical
    ///   "channel closed" case). The error's `detail` string carries
    ///   "channel send failed (receiver dropped)" so callers can
    ///   pattern-match on the message if they need to distinguish
    ///   send-failure from other runtime errors.
    ///
    /// # Async
    ///
    /// `async` (returns a future). The Buff codegen inserts `.await`
    /// automatically at the call site per T31's auto-await rule —
    /// the user writes `sender.send(value)` and the codegen emits
    /// `runtime_sender.send(value).await`.
    ///
    /// # Backpressure
    ///
    /// On a bounded channel at capacity, the future returned by this
    /// method pending until a slot is available (the canonical
    /// "bounded backpressure" semantic). See the `backpressure_*`
    /// tests in `tests/channel_tests.rs` for the runtime-pinned
    /// behavior.
    ///
    /// # Panics
    ///
    /// NEVER. The underlying tokio send is awaitable; failures surface
    /// as `Err` per the variant table above.
    pub async fn send(&self, value: T) -> Result<(), RuntimeError> {
        self.0
            .send(value)
            .await
            .map_err(|_| RuntimeError::Unsupported {
                detail: "channel send failed (receiver dropped)".to_string(),
                span: None,
            })
    }
}

/// Receiving half of a Buff MPSC channel.
///
/// Constructed exclusively via [`Channel::new`]. Wraps
/// `tokio::sync::mpsc::Receiver<T>` and exposes [`Receiver::recv`]
/// (async) + [`Receiver::close`] (sync). Single-consumer ONLY (the
/// T2 MVP) — Buff's prelude type `Receiver` is non-Clone; multi-
/// consumer broadcast is deferred to v1.18+.
///
/// # Memory model
///
/// `Send + 'static` when `T` is. The canonical consumer pattern is
/// to keep the receiver in the spawning task (NOT move it into a
/// child task) and drain it via a loop.
#[derive(Debug)]
pub struct Receiver<T>(pub tokio::sync::mpsc::Receiver<T>);

impl<T> Receiver<T> {
    /// Receive the next value from the channel.
    ///
    /// Wraps `tokio::sync::mpsc::Receiver::recv`. Returns:
    /// - `Some(value)` when a value is available.
    /// - `None` when ALL senders have been dropped (the canonical
    ///   "channel closed" semantic — the consumer treats None as
    ///   the iteration-terminator in the producer/consumer loop).
    ///
    /// # Async
    ///
    /// `async` (returns a future). Buff's codegen inserts `.await`
    /// automatically per T31's auto-await rule.
    ///
    /// # Panics
    ///
    /// NEVER. The underlying tokio recv is awaitable; channel-closed
    /// surfaces as `None` per the variant table above.
    pub async fn recv(&mut self) -> Option<T> {
        self.0.recv().await
    }

    /// Close the receiving half of the channel.
    ///
    /// Wraps `tokio::sync::mpsc::Receiver::close`. Sync (NOT async) —
    /// returns immediately after marking the receiver closed. Any
    /// pending `send` on the sender half returns
    /// `Err(SendError)` after this call (which surfaces as
    /// `Err(RuntimeError::Unsupported { .. })` on the Buff side via
    /// [`Sender::send`]'s mapping).
    ///
    /// Idempotent: calling close on an already-closed receiver is a
    /// no-op (mirrors tokio's behavior).
    ///
    /// # Panics
    ///
    /// NEVER.
    pub fn close(&mut self) {
        self.0.close();
    }
}

// ---------------------------------------------------------------------------
// Tests — inline unit smoke tests. The behavioural test suite lives in
// `tests/channel_tests.rs` (integration tests, exercises end-to-end send
// / recv / close / multi-producer ordering).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke: `Channel::new` returns a non-None sender + receiver pair
    /// for the simplest type (i64). The full behavioural matrix lives
    /// in `tests/channel_tests.rs`.
    #[test]
    fn channel_new_returns_sender_receiver_pair() {
        // Channel::new itself is sync (it just constructs the tokio
        // pair — no runtime needed). We can call it directly without
        // a tokio context.
        let (_sender, _receiver): (Sender<i64>, Receiver<i64>) = Channel::new(8);
        // The existence of the bindings is the smoke assertion —
        // Channel::new returned a typed pair instead of panicking.
    }

    /// Channel struct is unit-Copy (matches the namespace-only stance).
    #[test]
    fn channel_struct_is_copy() {
        let c1 = Channel;
        let c2 = c1; // Copy
                     // Equality on the unit struct is trivially true; the
                     // assertion confirms `PartialEq` + `Copy` derive.
        assert_eq!(c1, c2);
    }
}
