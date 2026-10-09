//! Supervisor trees for `buff-actors` (Erlang/OTP-inspired).
//!
//! A [`Supervisor`] manages a set of child actors, each described
//! by a [`ChildSpec`] (a factory closure that produces a fresh
//! `Box<dyn Actor>` on each restart). When a child exits, the
//! supervisor consults its [`RestartStrategy`]:
//!
//! | Strategy | Normal stop | Crash (panic) |
//! |---|---|---|
//! | [`RestartStrategy::Permanent`] | restart | restart |
//! | [`RestartStrategy::Temporary`] | leave dead | leave dead |
//! | [`RestartStrategy::Transient`] | leave dead | restart |
//!
//! "Let it crash" philosophy: a child's panic is caught by the
//! actor loop (`catch_unwind`), surfaced to the supervisor as
//! [`crate::ChildExit::Crashed`], and — for `Permanent` / `Transient`
//! strategies — restarted from the [`ChildSpec`]'s factory.

use crate::{Actor, ActorRef, ActorSystem, ChildExit};
use crossbeam_channel as cb;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;

/// Restart strategy for a [`Supervisor`]'s children.
///
/// Mirrors Erlang/OTP's `Restart` type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RestartStrategy {
    /// Always restart the child on exit (normal or crash).
    /// Mirrors Erlang `permanent`. Use for critical services.
    Permanent,
    /// Never restart the child. Mirrors Erlang `temporary`.
    /// Use for one-shot workers.
    Temporary,
    /// Restart only on crash; leave dead on normal stop.
    /// Mirrors Erlang `transient`. Use for workers where a
    /// normal `ActorAction::Stop` means "done" but a panic
    /// means "retry".
    Transient,
}

impl RestartStrategy {
    /// Lowercase stable name for diagnostics + codegen emission.
    /// Matches the Buff source surface (`RestartStrategy.permanent` /
    /// `.temporary` / `.transient`).
    pub fn as_str(self) -> &'static str {
        match self {
            RestartStrategy::Permanent => "permanent",
            RestartStrategy::Temporary => "temporary",
            RestartStrategy::Transient => "transient",
        }
    }

    /// Decide whether to restart given the child's exit outcome.
    pub(crate) fn should_restart(self, exit: ChildExit) -> bool {
        match (self, exit) {
            (RestartStrategy::Permanent, _) => true,
            (RestartStrategy::Temporary, _) => false,
            (RestartStrategy::Transient, ChildExit::Crashed) => true,
            (RestartStrategy::Transient, ChildExit::Normal) => false,
        }
    }
}

impl std::fmt::Display for RestartStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Default for RestartStrategy {
    /// Default: `Permanent` (mirrors Erlang/OTP's default for
    /// `supervisor:start_child`). The most useful behaviour for
    /// the common case — crashed children self-heal.
    fn default() -> Self {
        RestartStrategy::Permanent
    }
}

/// Factory record describing how to (re-)spawn a supervised child.
///
/// Each supervisor restart calls `(spec.factory)()` to make a fresh
/// actor instance — the factory MUST therefore be `Fn` (stateless
/// or closing over `Arc<Mutex<...>>` shared state).
#[derive(Clone)]
pub struct ChildSpec {
    pub(crate) name: Option<String>,
    pub(crate) factory: Arc<dyn Fn() -> Box<dyn Actor> + Send + Sync>,
}

impl ChildSpec {
    /// Construct a `ChildSpec` from a factory closure that produces
    /// a fresh `Box<dyn Actor>` on each call.
    pub fn new<F>(factory: F) -> Self
    where
        F: Fn() -> Box<dyn Actor> + Send + Sync + 'static,
    {
        ChildSpec {
            name: None,
            factory: Arc::new(factory),
        }
    }

    /// Builder: attach a `name`. Named children are registered with
    /// the [`ActorSystem`] on `start_child` (and on every restart)
    /// so `system.lookup(name)` returns the live ref.
    pub fn with_name<S: Into<String>>(mut self, name: S) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The name attached to this spec, if any.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl std::fmt::Debug for ChildSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildSpec")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Internal record per supervised child: the spec (for re-spawn)
/// and the latest [`crate::ActorId`] (so the monitor can find the
/// record to update on restart).
pub(crate) struct SupervisedChild {
    pub(crate) spec: ChildSpec,
    pub(crate) last_id: u64,
}

/// Internal lifecycle slots for the monitor thread. `ctrl` holds the
/// one-shot control sender (held ONLY here — never given to the
/// monitor — so `shutdown` can always wake a monitor blocked in
/// `recv` by dropping it) and `join` holds the monitor's
/// `JoinHandle` so `shutdown` can join it for deterministic
/// termination. `Option` inside a `Mutex` makes `shutdown(&self)`
/// idempotent: the first caller takes both slots, later callers
/// find `None`.
struct MonitorSlots {
    ctrl: Mutex<Option<cb::Sender<()>>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

/// A supervisor manages a set of child actors, restarting them per
/// the [`RestartStrategy`] when they exit.
///
/// The supervisor runs ONE dedicated monitor thread that drains
/// exit notifications from the actor system's `on_exit` channel.
/// On a child exit, the monitor consults the strategy + spec and
/// either re-spawns the actor (calling `(spec.factory)()` for a
/// fresh instance + re-registering the new [`ActorRef`] under the
/// spec's name) or marks the child dead.
///
/// The monitor waits on the exit channel AND a one-shot control
/// channel via `select!` (the same shape as the actor loop). The
/// control sender is held only by the supervisor's internal
/// [`MonitorSlots`]; [`Supervisor::shutdown`] drops it to wake an
/// idle monitor, then joins the thread — so `shutdown` returning
/// guarantees the monitor has terminated.
///
/// # Example
///
/// ```
/// use buff_actors::{
///     supervisor::{ChildSpec, Supervisor},
///     Actor, ActorAction, ActorSystem, Message,
/// };
/// use std::sync::{Arc, Mutex};
///
/// struct Counter(Arc<Mutex<u32>>);
/// impl Actor for Counter {
///     fn handle(&mut self, _msg: Message) -> ActorAction { ActorAction::Continue }
/// }
///
/// let sys = ActorSystem::new().expect("system");
/// let sup = Supervisor::new(sys.clone()).expect("supervisor");
/// let counter = Arc::new(Mutex::new(0u32));
/// let counter_for_spec = counter.clone();
/// let _ref = sup
///     .start_child(ChildSpec::new(move || {
///         Box::new(Counter(counter_for_spec.clone()))
///     }))
///     .expect("start_child");
/// sup.shutdown();
/// ```
pub struct Supervisor {
    system: ActorSystem,
    strategy: RestartStrategy,
    children: Arc<RwLock<Vec<SupervisedChild>>>,
    exit_tx: cb::Sender<(u64, ChildExit)>,
    monitor_running: Arc<AtomicBool>,
    monitor_slots: Arc<MonitorSlots>,
}

impl Supervisor {
    /// Construct a supervisor with the default strategy
    /// ([`RestartStrategy::Permanent`]) bound to `system`.
    pub fn new(system: ActorSystem) -> Result<Self, crate::ActorError> {
        Self::with_strategy(system, RestartStrategy::default())
    }

    /// Construct a supervisor with an explicit `strategy` bound to
    /// `system`. Starts the internal monitor thread immediately.
    pub fn with_strategy(
        system: ActorSystem,
        strategy: RestartStrategy,
    ) -> Result<Self, crate::ActorError> {
        let (exit_tx, exit_rx) = cb::unbounded::<(u64, ChildExit)>();
        let (ctrl_tx, ctrl_rx) = cb::bounded::<()>(1);
        let children = Arc::new(RwLock::new(Vec::<SupervisedChild>::new()));
        let monitor_running = Arc::new(AtomicBool::new(true));
        let spawn_result = catch_unwind(AssertUnwindSafe(|| {
            spawn_monitor_thread(
                system.clone(),
                strategy,
                children.clone(),
                exit_tx.clone(),
                exit_rx,
                monitor_running.clone(),
                ctrl_rx,
            )
        }));
        match spawn_result {
            Ok(Some(join)) => {
                let slots = MonitorSlots {
                    ctrl: Mutex::new(Some(ctrl_tx)),
                    join: Mutex::new(Some(join)),
                };
                Ok(Supervisor {
                    system,
                    strategy,
                    children,
                    exit_tx,
                    monitor_running,
                    monitor_slots: Arc::new(slots),
                })
            }
            _ => Err(crate::ActorError::Panic),
        }
    }

    /// The configured [`RestartStrategy`].
    pub fn strategy(&self) -> RestartStrategy {
        self.strategy
    }

    /// Spawn a fresh actor from `spec.factory` on this supervisor's
    /// [`ActorSystem`], register it for restart supervision, and
    /// return its [`ActorRef`]. Named specs are also registered
    /// with the system's name registry (and the entry is updated on
    /// every restart).
    pub fn start_child(&self, spec: ChildSpec) -> Result<ActorRef, crate::ActorError> {
        let factory = spec.factory.clone();
        let actor = factory();
        let actor_ref = self.system.spawn_inner(actor, Some(self.exit_tx.clone()))?;
        if let Some(name) = spec.name.as_deref() {
            upsert_named(&self.system, name, actor_ref.clone());
        }
        let record = SupervisedChild {
            spec,
            last_id: actor_ref.id(),
        };
        let _ = self.children.write().map(|mut kids| kids.push(record));
        Ok(actor_ref)
    }

    /// Count children currently tracked by this supervisor
    /// (including restarted instances). Point-in-time snapshot.
    pub fn child_count(&self) -> usize {
        let Ok(kids) = self.children.read() else {
            return 0;
        };
        kids.len()
    }

    /// Shut down this supervisor deterministically: signal the
    /// monitor to stop, wake it (dropping the one-shot control
    /// sender — sufficient even while the monitor is blocked in
    /// `recv` on the exit channel), JOIN the monitor thread, then
    /// delegate actor termination to [`ActorSystem::shutdown`]
    /// (drops every mailbox sentinel + joins every actor thread).
    /// When this returns, the monitor thread has terminated.
    /// Idempotent.
    pub fn shutdown(&self) {
        self.monitor_running.store(false, Ordering::Release);
        if let Ok(mut slot) = self.monitor_slots.ctrl.lock() {
            *slot = None;
        }
        if let Ok(mut slot) = self.monitor_slots.join.lock() {
            if let Some(join) = slot.take() {
                let _ = join.join();
            }
        }
        self.system.shutdown();
    }
}

impl std::fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor")
            .field("strategy", &self.strategy)
            .field("children", &self.child_count())
            .finish_non_exhaustive()
    }
}

impl Clone for Supervisor {
    /// Clone the supervisor handle. The internal monitor thread is
    /// shared (not duplicated). Cheap (state behind `Arc`).
    fn clone(&self) -> Self {
        Supervisor {
            system: self.system.clone(),
            strategy: self.strategy,
            children: self.children.clone(),
            exit_tx: self.exit_tx.clone(),
            monitor_running: self.monitor_running.clone(),
            monitor_slots: self.monitor_slots.clone(),
        }
    }
}

/// Insert or replace the registry entry for `name` so
/// `system.lookup(name)` always returns the live ref. Used on
/// `start_child` and on each restart.
fn upsert_named(system: &ActorSystem, name: &str, actor_ref: ActorRef) {
    if name.is_empty() {
        return;
    }
    let owned = name.to_string();
    let _ = system.registry.write().map(|mut map| {
        map.insert(owned, actor_ref);
    });
}

/// The supervisor's monitor thread loop. Drains exit
/// notifications, applies the restart policy, and re-spawns
/// crashed/Permanent children with the SAME `exit_tx` so the
/// restart chain stays supervised across multiple crashes.
///
/// Waits on the exit channel and the one-shot control channel via
/// `select!` (same shape as `run_actor_loop`). Exits when the
/// control channel fires or its sender is dropped (the
/// [`Supervisor::shutdown`] wake path — necessary because the
/// monitor itself holds an `exit_tx` clone for restart
/// re-injection, so the exit channel can never disconnect while
/// the monitor lives), or when the exit channel disconnects.
/// Returns the `JoinHandle` so `shutdown` can join the thread.
fn spawn_monitor_thread(
    system: ActorSystem,
    strategy: RestartStrategy,
    children: Arc<RwLock<Vec<SupervisedChild>>>,
    exit_tx: cb::Sender<(u64, ChildExit)>,
    exit_rx: cb::Receiver<(u64, ChildExit)>,
    monitor_running: Arc<AtomicBool>,
    ctrl_rx: cb::Receiver<()>,
) -> Option<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("buff-actor-monitor".to_string())
        .spawn(move || loop {
            cb::select! {
                recv(exit_rx) -> notified => {
                    let Ok((id, exit)) = notified else {
                        break;
                    };
                    if !monitor_running.load(Ordering::Acquire) {
                        return;
                    }
                    if !strategy.should_restart(exit) {
                        if let Ok(mut kids) = children.write() {
                            kids.retain(|c| c.last_id != id);
                        }
                        continue;
                    }
                    let spec_opt = children.read().ok().and_then(|kids| {
                        kids.iter()
                            .find(|c| c.last_id == id)
                            .map(|c| c.spec.clone())
                    });
                    let Some(spec) = spec_opt else {
                        continue;
                    };
                    let actor = (spec.factory)();
                    let new_ref = system.spawn_inner(actor, Some(exit_tx.clone()));
                    let new_id = match new_ref {
                        Ok(r) => {
                            if let Some(name) = spec.name.as_deref() {
                                upsert_named(&system, name, r.clone());
                            }
                            r.id()
                        }
                        Err(_) => continue,
                    };
                    if let Ok(mut kids) = children.write() {
                        if let Some(rec) = kids.iter_mut().find(|c| c.last_id == id) {
                            rec.last_id = new_id;
                        }
                    }
                }
                recv(ctrl_rx) -> _signal => return,
            }
        })
        .ok()
}

#[cfg(test)]
mod monitor_tests {
    use super::*;

    /// FIX-D regression (ITER-39): the supervisor's monitor thread
    /// must terminate once `shutdown` returns — not stay blocked in
    /// `recv` forever with its `JoinHandle` discarded. Probed via
    /// the strong count on the shared `children` Arc: the monitor
    /// closure captures a clone, so a leaked monitor keeps the count
    /// above the test's own reference even after the handle is
    /// dropped and shutdown has returned.
    #[test]
    fn monitor_thread_exits_on_shutdown() {
        let sys = ActorSystem::new().expect("system");
        let sup = Supervisor::new(sys.clone()).expect("supervisor");
        let children_probe = sup.children.clone();
        sup.shutdown();
        // Dropping the handle leaves the probe as the only reference
        // besides the monitor's captured clone. If the monitor exited,
        // the count settles at 1.
        drop(sup);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut settled = false;
        while std::time::Instant::now() < deadline {
            if Arc::strong_count(&children_probe) == 1 {
                settled = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(
            settled,
            "monitor thread leaked: children Arc still shared (strong_count={})",
            Arc::strong_count(&children_probe)
        );
    }
}
