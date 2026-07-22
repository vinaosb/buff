//! `buff-actors` — actor model + supervisor trees for the Buff language.
//!
//! Pure-Rust MVP built standalone on T2 Channel<T> semantics
//! (per T59 spec). Wraps [`crossbeam-channel`](https://crates.io/crates/crossbeam-channel)
//! for the per-actor mailbox + [`std::thread`](https://doc.rust-lang.org/std/thread/)
//! for the actor loop (deterministic `JoinHandle::join` for graceful
//! shutdown + supervisor crash detection).
//!
//! Inspiration: Gleam + Erlang/OTP ("let it crash"), Akka, and actix.
//! Each actor runs in its own thread, receives messages via a
//! `crossbeam_channel::unbounded` mailbox, and processes them via
//! the [`Actor::handle`] trait method.
//!
//! Distributed actors (cluster gossip, distributed name registry,
//! location-transparent `send`), hot code swapping, and actor
//! persistence/snapshotting are **deferred to v1.18+** per the T59
//! task spec — single-process only for the MVP.
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `ActorSystem`, `ActorRef`, `Message`, `Actor` (trait), `ActorAction`, `Supervisor`, `ChildSpec`, `RestartStrategy`, `ActorError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | `spawn` takes an owned `Box<dyn Actor>`. `send` takes an owned payload (boxed into a `Message`). `register` takes an owned `ActorRef`. |
//! | R3 — Error mapping | Fallible ops return `Result<T, ActorError>`. `crossbeam_channel::SendError` mapped via `From`. |
//! | R4 — Thread safety | `ActorSystem`, `ActorRef`, `Supervisor`, `ChildSpec` are `Send + Sync` (state behind `Arc<RwLock<...>>` + `Arc<AtomicU64>`). `Actor` requires `Send + 'static`. |
//! | R5 — Lifetime hiding | No public lifetime parameters. All `&str` args (name, etc.) are copied into owned `String` at the boundary. |
//! | R6 — Panic boundary | `new` / `spawn` / `register` / `start_child` wrap their bodies in `catch_unwind`. The actor loop catches `handle()` panics so the supervisor sees a crash (not a process abort). |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Channel send/recv return `Result` explicitly so the
//! only failure modes are user-visible (`ActorStopped`,
//! `DuplicateName`, `UnknownName`, `EmptyName`) or runtime-internal
//! (`Panic`). Actor `handle()` panics are caught by the loop wrapper
//! and surfaced to the supervisor as a crash — never propagated.

pub mod error;
pub mod supervisor;

pub use error::ActorError;
pub use supervisor::{ChildSpec, RestartStrategy, Supervisor};

use crossbeam_channel as cb;
use std::any::Any;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;

/// Stable identifier for a live actor (monotonically increasing
/// per-system). Assigned by [`ActorSystem::spawn`], read via
/// [`ActorRef::id`]. Useful for logging + correlating
/// [`ActorError::ActorStopped`] diagnostics.
pub type ActorId = u64;

/// Internal exit-outcome reported by an actor's thread when it
/// finishes. Consumed by the supervisor to decide restart. Pub(crate)
/// — internal; the public surface returns nothing from the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildExit {
    /// `handle` returned [`ActorAction::Stop`] OR the mailbox was
    /// disconnected (all senders dropped). Normal termination.
    Normal,
    /// `handle` panicked (caught by `catch_unwind`). Crashed
    /// termination.
    Crashed,
}

/// Opaque, type-erased message envelope delivered to an actor's
/// [`Actor::handle`] method.
///
/// Wraps a `Box<dyn Any + Send + 'static>` so a single
/// `crossbeam_channel::unbounded::<Message>` mailbox can carry
/// heterogeneous message types (mirrors Erlang's any-term mailbox
/// and actix's `Message` trait-object pattern). Callers obtain a
/// typed view via [`Self::downcast`] or [`Self::is`].
///
/// The MVP is type-erased (not generic `Message<T>`) per the T59
/// spec preference: a single `ActorSystem::spawn` shape handles
/// every actor regardless of its message type. A future typed
/// `Message<T>` generic can extend this without breaking the
/// erased MVP (same migration shape as `buff-pubsub::Event<T>`).
#[derive(Debug)]
pub struct Message(pub(crate) Box<dyn Any + Send + 'static>);

impl Message {
    /// Construct a message wrapping any `Send + 'static` payload.
    pub fn new<M>(msg: M) -> Self
    where
        M: Any + Send + 'static,
    {
        Message(Box::new(msg))
    }

    /// Recover the typed payload if `self` wraps an `M`. Returns
    /// `Ok(M)` on a runtime-type match; otherwise `Err(Self)` so
    /// the caller can try a different type or forward the message.
    /// Consumes `self` (the payload is moved out of the box on a
    /// successful downcast).
    pub fn downcast<M>(self) -> Result<M, Self>
    where
        M: Any + Send + 'static,
    {
        match self.0.downcast::<M>() {
            Ok(boxed) => Ok(*boxed),
            Err(b) => Err(Message(b)),
        }
    }

    /// Returns `true` if `self` wraps a payload of type `M`,
    /// without consuming the message. Useful for cheap dispatch
    /// before deciding whether to clone or forward.
    pub fn is<M>(&self) -> bool
    where
        M: Any + Send + 'static,
    {
        self.0.is::<M>()
    }
}

/// The directive returned by [`Actor::handle`] after processing a
/// message. Drives the actor loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActorAction {
    /// Keep the mailbox open and process the next message.
    Continue,
    /// Stop the actor gracefully: break the loop, drop `self`,
    /// exit the thread. Outstanding messages in the mailbox are
    /// dropped (NOT processed). The supervisor sees a *normal*
    /// stop (not a crash) and consults its [`RestartStrategy`]:
    /// `Permanent` restarts; `Temporary` and `Transient` mark
    /// dead.
    Stop,
}

/// The trait every actor implements.
///
/// `handle` is invoked once per message by the actor's own thread.
/// The actor may mutate its internal state (`&mut self`); access
/// to outer shared state (counters, log sinks, etc.) is achieved
/// by closing over `Arc<Mutex<...>>` at construction time
/// (mirrors the `buff-pubsub` handler-Arc precedent and the
/// classic Erlang/OTP stateful-server pattern).
///
/// # Panics
///
/// A panic inside `handle` is caught by the actor loop wrapper
/// (`catch_unwind`). The actor is then considered *crashed* and
/// exits; its supervisor (if any) is notified. Panics never
/// propagate to the actor's siblings or the [`ActorSystem`]
/// (mirrors Erlang "let it crash" semantics — a crashed actor
/// doesn't take down the VM).
pub trait Actor: Send + 'static {
    /// Process `message`. Return [`ActorAction::Continue`] to keep
    /// the mailbox open or [`ActorAction::Stop`] to exit the
    /// actor's loop cleanly.
    fn handle(&mut self, message: Message) -> ActorAction;
}

/// Type-safe handle to a running actor.
///
/// Constructed by [`ActorSystem::spawn`] (or
/// [`Supervisor::start_child`]). Cheap to clone (state behind an
/// `Arc`); the recommended pattern for cross-thread sharing is
/// `let ref2 = actor_ref.clone()` (mirrors the `EventBus::clone`
/// precedent in `buff-pubsub`).
///
/// Dropping every clone of an `ActorRef` causes the mailbox's
/// sender count to drop to zero, which causes the actor's
/// `rx.recv()` to return `Err(Disconnected)` and the actor to exit
/// cleanly. This is the primary shutdown path (alongside
/// [`ActorSystem::shutdown`] which drops every `ActorRef` in the
/// system).
#[derive(Clone)]
pub struct ActorRef {
    pub(crate) sender: cb::Sender<Message>,
    pub(crate) id: ActorId,
    pub(crate) stop_signal: Arc<AtomicBool>,
}

impl ActorRef {
    /// Send `message` to the actor's mailbox. Returns
    /// [`ActorError::ActorStopped`] if the actor has exited (its
    /// mailbox sender was disconnected — by supervisor restart,
    /// explicit stop, or system shutdown).
    ///
    /// Delivery is asynchronous: `send` returns once the message
    /// is queued. The actor's thread processes it on its own
    /// schedule.
    pub fn send<M>(&self, message: M) -> Result<(), ActorError>
    where
        M: Any + Send + 'static,
    {
        let env = Message::new(message);
        self.sender
            .send(env)
            .map_err(|_| ActorError::ActorStopped(self.id))
    }

    /// Signal the actor to stop after it finishes processing the
    /// current message (best-effort). Idempotent. The actor exits
    /// on its next loop iteration (the loop checks the signal
    /// between messages) or when its last `ActorRef` is dropped.
    pub fn stop(&self) {
        self.stop_signal.store(true, Ordering::Release);
    }

    /// The stable identifier assigned by [`ActorSystem::spawn`].
    pub fn id(&self) -> ActorId {
        self.id
    }
}

impl std::fmt::Debug for ActorRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorRef")
            .field("id", &self.id)
            .field("stopping", &self.stop_signal.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

/// Internal record stored per live actor in the system's child
/// list. Carries the canonical `JoinHandle` (so [`ActorSystem::shutdown`]
/// can join the thread for graceful termination) and a clone of the
/// mailbox sender (so `shutdown` can force-disconnect the mailbox
/// even if the caller is still holding a cloned [`ActorRef`]).
pub(crate) struct SystemChild {
    pub(crate) join: Option<JoinHandle<()>>,
    pub(crate) sentinel: cb::Sender<Message>,
}

/// The top-level container for actors.
///
/// Construct via [`ActorSystem::new`]. Each [`Self::spawn`] call
/// starts an actor thread and returns an [`ActorRef`];
/// [`Self::register`] associates a string name with an `ActorRef`
/// for later [`Self::lookup`]. [`Self::shutdown`] joins every
/// live actor thread (graceful termination).
///
/// `ActorSystem` is `Send + Sync` and cheap to clone (inner state
/// behind an `Arc`); the recommended pattern is
/// `let sys = ActorSystem::new()?; let sys2 = sys.clone();` for
/// cross-thread sharing (mirrors the `EventBus` precedent).
///
/// # Example
///
/// ```
/// use buff_actors::{Actor, ActorAction, ActorSystem, Message};
///
/// struct Echo;
/// impl Actor for Echo {
///     fn handle(&mut self, _msg: Message) -> ActorAction { ActorAction::Continue }
/// }
///
/// let sys = ActorSystem::new().expect("system");
/// let actor_ref = sys.spawn(Box::new(Echo)).expect("spawn");
/// actor_ref.send("hello").expect("send");
/// sys.shutdown();
/// ```
#[derive(Clone)]
pub struct ActorSystem {
    next_id: Arc<AtomicU64>,
    registry: Arc<RwLock<HashMap<String, ActorRef>>>,
    children: Arc<RwLock<Vec<SystemChild>>>,
}

impl ActorSystem {
    /// Construct an empty actor system.
    ///
    /// Wraps the (trivial) allocation in `catch_unwind` per T4 FFI
    /// guide R6. The MVP constructor cannot fail in normal use;
    /// the `Result` return mirrors the precedent set by
    /// `EventBus::new` / `Cache::new` so a future config knob
    /// (capacity limits, default supervisor, etc.) slots in
    /// without breaking the surface.
    pub fn new() -> Result<Self, ActorError> {
        let result = catch_unwind(AssertUnwindSafe(|| ActorSystem {
            next_id: Arc::new(AtomicU64::new(1)),
            registry: Arc::new(RwLock::new(HashMap::new())),
            children: Arc::new(RwLock::new(Vec::new())),
        }));
        match result {
            Ok(sys) => Ok(sys),
            Err(_) => Err(ActorError::Panic),
        }
    }

    /// Spawn `actor` in its own thread and return an [`ActorRef`]
    /// for sending messages to it.
    ///
    /// Internally creates a `crossbeam_channel::unbounded::<Message>()`
    /// mailbox, assigns the next [`ActorId`], and spawns a thread
    /// that loops `rx.recv() ─▶ actor.handle(msg)`. The thread
    /// exits cleanly when the mailbox disconnects (every sender
    /// dropped) or when `handle` returns [`ActorAction::Stop`].
    ///
    /// The thread's `JoinHandle` is stored internally so
    /// [`Self::shutdown`] can join every thread for graceful
    /// termination. `on_exit` (when `Some`) is notified of the
    /// actor's exit outcome — used by [`Supervisor`] to detect
    /// crashes and decide restart.
    pub fn spawn(&self, actor: Box<dyn Actor>) -> Result<ActorRef, ActorError> {
        self.spawn_inner(actor, None)
    }

    /// Pub(crate) spawn variant that also delivers the actor's
    /// exit outcome (`(ActorId, ChildExit)`) to `on_exit`. Used by
    /// [`Supervisor::start_child`] so the supervisor's monitor can
    /// react to crashes + consult the [`RestartStrategy`].
    pub(crate) fn spawn_inner(
        &self,
        mut actor: Box<dyn Actor>,
        on_exit: Option<cb::Sender<(ActorId, ChildExit)>>,
    ) -> Result<ActorRef, ActorError> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let (tx, rx) = cb::unbounded::<Message>();
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let stop_signal = Arc::new(AtomicBool::new(false));
            let stop_for_thread = stop_signal.clone();
            let on_exit_for_thread = on_exit.clone();
            let thread = std::thread::Builder::new()
                .name(format!("buff-actor-{id}"))
                .spawn(move || {
                    let exit = run_actor_loop(&mut actor, &rx, &stop_for_thread);
                    if let Some(tx_exit) = on_exit_for_thread {
                        let _ = tx_exit.send((id, exit));
                    }
                });
            match thread {
                Ok(join) => {
                    let sentinel = tx.clone();
                    let child = SystemChild {
                        join: Some(join),
                        sentinel,
                    };
                    let _ = self.children.write().map(|mut kids| kids.push(child));
                    Ok(ActorRef {
                        sender: tx,
                        id,
                        stop_signal,
                    })
                }
                Err(_) => Err(ActorError::Panic),
            }
        }));
        match result {
            Ok(Ok(actor_ref)) => Ok(actor_ref),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(ActorError::Panic),
        }
    }

    /// Associate `name` with `actor_ref` for later lookup via
    /// [`Self::lookup`]. Names are unique per system; re-registering
    /// a live name returns [`ActorError::DuplicateName`]. Empty
    /// names return [`ActorError::EmptyName`].
    ///
    /// Mirrors Erlang's `register/2` BIF. Lookup is O(1).
    pub fn register(&self, name: &str, actor_ref: ActorRef) -> Result<(), ActorError> {
        if name.is_empty() {
            return Err(ActorError::EmptyName);
        }
        let owned = name.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut map = self.registry.write().map_err(|_| ActorError::Panic)?;
            if map.contains_key(&owned) {
                return Err(ActorError::DuplicateName(owned));
            }
            map.insert(owned, actor_ref);
            Ok(())
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(ActorError::Panic),
        }
    }

    /// Look up a previously-registered actor by name. Returns
    /// `None` if `name` was never registered or its registrant
    /// has been shut down + cleared.
    ///
    /// The returned `ActorRef` is a fresh clone (cheap — bumps
    /// the inner `Arc` count). Point-in-time: the actor may exit
    /// between lookup and `send` (the send then fails with
    /// [`ActorError::ActorStopped`]).
    pub fn lookup(&self, name: &str) -> Option<ActorRef> {
        let Ok(map) = self.registry.read() else {
            return None;
        };
        map.get(name).cloned()
    }

    /// Count live actors tracked by this system (including
    /// supervised children). Point-in-time snapshot.
    pub fn actor_count(&self) -> usize {
        let Ok(kids) = self.children.read() else {
            return 0;
        };
        kids.len()
    }

    /// Gracefully shut down every actor in this system.
    ///
    /// Drops every mailbox sender sentinel (causing each actor's
    /// `rx.recv()` to return `Err(Disconnected)` and the thread to
    /// exit cleanly) and then joins every thread (blocking until
    /// all actors have observed the disconnect and returned from
    /// their current `handle` call). After `shutdown` returns, no
    /// more messages can be delivered; further `send` calls on any
    /// cloned `ActorRef` return [`ActorError::ActorStopped`].
    ///
    /// Idempotent: calling shutdown on an already-shut-down system
    /// is a no-op (children vec is drained).
    pub fn shutdown(&self) {
        let Ok(mut kids) = self.children.write() else {
            return;
        };
        let drained: Vec<SystemChild> = std::mem::take(&mut *kids);
        drop(kids);
        for mut child in drained {
            drop(child.sentinel);
            if let Some(join) = child.join.take() {
                let _ = join.join();
            }
        }
    }
}

/// Actor loop body, factored out so the spawn closure stays small.
/// Loops `rx.recv() ─▶ actor.handle(msg)`, honouring the stop
/// signal and catching `handle()` panics (which surface as
/// [`ChildExit::Crashed`]). Returns the exit outcome so the
/// spawning thread can forward it to the supervisor.
fn run_actor_loop(
    actor: &mut Box<dyn Actor>,
    rx: &cb::Receiver<Message>,
    stop_signal: &AtomicBool,
) -> ChildExit {
    let mut exit = ChildExit::Normal;
    while let Ok(msg) = rx.recv() {
        if stop_signal.load(Ordering::Acquire) {
            break;
        }
        let decided = catch_unwind(AssertUnwindSafe(|| actor.handle(msg)));
        match decided {
            Ok(ActorAction::Continue) => continue,
            Ok(ActorAction::Stop) => break,
            Err(_) => {
                exit = ChildExit::Crashed;
                break;
            }
        }
    }
    exit
}

impl Default for ActorSystem {
    /// Default-constructs an empty `ActorSystem`. Equivalent to
    /// [`ActorSystem::new`] but infallible (the only failure mode
    /// of `new` is the catch_unwind boundary, which never fires
    /// for the trivial HashMap + AtomicU64 allocation). Provided
    /// so the codegen lowering can use `unwrap_or_default()` for
    /// panic-free `Result<ActorSystem, ActorError>` paths —
    /// matches the Image / DataFrame / Cache / EventBus precedent.
    fn default() -> Self {
        ActorSystem {
            next_id: Arc::new(AtomicU64::new(1)),
            registry: Arc::new(RwLock::new(HashMap::new())),
            children: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl std::fmt::Debug for ActorSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let actor_count = self.actor_count();
        let name_count = self.registry.read().map(|map| map.len()).unwrap_or(0);
        f.debug_struct("ActorSystem")
            .field("actors", &actor_count)
            .field("named", &name_count)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for ActorSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names = self.registry.read().map(|m| m.len()).unwrap_or(0);
        write!(
            f,
            "ActorSystem({} actors, {} named)",
            self.actor_count(),
            names
        )
    }
}
