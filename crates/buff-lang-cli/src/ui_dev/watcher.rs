//! File watcher with 200 ms debounce (T131).
//!
//! Wraps `notify::RecommendedWatcher` and debounces events via a
//! `tokio::time::sleep` buffer. On each debounced burst, the watcher
//! task emits a [`WatchBatch`] onto the returned `mpsc::Receiver` so
//! the dev server's rebuild task can act on the change.
//!
//! # Debounce algorithm
//!
//! The naive "wait 200 ms after the first event, then deliver" path
//! drops events that arrive after the timer fires. We use the
//! trailing-edge variant instead: every incoming event resets the
//! 200 ms timer; only when 200 ms passes with NO new events do we
//! deliver. This guarantees we coalesce a burst of editor saves
//! (e.g. format-on-save writes the file twice in quick succession)
//! into a single rebuild trigger.
//!
//! Production code uses [`NotifyWatcher`]; unit tests use the
//! [`mock_watcher`] helper which returns a controllable `mpsc::Sender`
//! so tests can inject synthetic [`WatchEvent`]s directly.

use std::path::PathBuf;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::ui_dev::error::UiDevError;

/// The debounce window (200 ms per T131 spec). The constant is exposed
/// at the module level so tests can use the same value when reasoning
/// about timing.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// A single notify event the watcher surfaces. Carries enough
/// information for the rebuild task to decide whether to act:
///
/// - `path` — the changed file (when known — notify sometimes emits
///   events with a missing path; the rebuild task ignores those).
/// - `kind` — `Create` / `Modify` / `Remove`. The dev server treats
///   all three the same (a sibling .buff may have been renamed, or a
///   static asset deleted → the browser should reload to refresh).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEvent {
    /// The path notify reported (may be `None` for some kernel
    /// backends that omit the path on bulk events).
    pub path: Option<PathBuf>,
    /// Whether the file extension is `.buff` — pre-computed at event
    /// time so the rebuild task can filter without re-stat'ing.
    pub is_buff: bool,
}

impl WatchEvent {
    /// Construct from a raw path. `is_buff` is computed from the
    /// extension at construction time.
    #[must_use]
    pub fn from_path(path: PathBuf) -> Self {
        let is_buff = path.extension().map(|e| e == "buff").unwrap_or(false);
        Self {
            path: Some(path),
            is_buff,
        }
    }

    /// Construct a synthetic event with no path (used in tests to
    /// simulate kernel backends that emit path-less events).
    #[must_use]
    pub fn pathless() -> Self {
        Self {
            path: None,
            is_buff: false,
        }
    }
}

/// A debounced batch of [`WatchEvent`]s delivered to the rebuild task.
///
/// The dev server treats the whole batch as one trigger: run the
/// Buff front-end on every `.buff` file in the batch, run the cargo
/// builder once, broadcast one reload / error message.
#[derive(Debug, Clone, Default)]
pub struct WatchBatch {
    /// Every event coalesced into this batch. May contain duplicates
    /// (same file written twice during the debounce window) — the
    /// rebuild task de-duplicates by path.
    pub events: Vec<WatchEvent>,
}

impl WatchBatch {
    /// Returns `true` iff any event in the batch touches a `.buff`
    /// file. The rebuild task uses this as a fast-path filter:
    /// non-`.buff` events still trigger a `reload` (so saving a CSS
    /// file in `<project>/static/` refreshes the browser) but skip
    /// the expensive Buff front-end + cargo build path.
    #[must_use]
    pub fn touches_buff(&self) -> bool {
        self.events.iter().any(|e| e.is_buff)
    }

    /// Returns `true` iff the batch is non-empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Collect the unique `.buff` paths in this batch. Order is
    /// insertion-stable (first occurrence wins) so test assertions
    /// on the rebuild task's invocation order are deterministic.
    #[must_use]
    pub fn buff_paths(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for e in &self.events {
            if !e.is_buff {
                continue;
            }
            if let Some(p) = &e.path {
                if !out.contains(p) {
                    out.push(p.clone());
                }
            }
        }
        out
    }
}

/// Install a notify watcher on `root` (recursive) and return an
/// `mpsc::Receiver<WatchBatch>` that yields one batch per debounced
/// burst of events.
///
/// Spawns a tokio task that owns the [`RecommendedWatcher`] + the
/// debounce timer + the buffering logic. The task lives until the
/// returned receiver is dropped OR the watcher errors fatally (which
/// surfaces as a final batch with no events followed by channel
/// close — the rebuild task handles this gracefully).
///
/// # Errors
///
/// Returns [`UiDevError::WatcherInstall`] when notify fails to
/// install the OS-level watch (e.g. the path is a file rather than a
/// directory, or kernel watcher limits are exhausted).
pub fn spawn_watcher(
    root: PathBuf,
) -> Result<(RecommendedWatcher, mpsc::Receiver<WatchBatch>), UiDevError> {
    if !root.is_dir() {
        return Err(UiDevError::WatcherInstall {
            path: root,
            message: "not a directory".to_string(),
        });
    }

    // Channel from notify's callback to the debounce task.
    let (raw_tx, mut raw_rx) = mpsc::channel::<notify::Result<notify::Event>>(256);
    // Channel from the debounce task to the rebuild task.
    let (batch_tx, batch_rx) = mpsc::channel::<WatchBatch>(16);

    // notify callback: just forward events onto `raw_tx`. The callback
    // runs on notify's internal thread — keep it cheap.
    let raw_tx_for_cb = raw_tx.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        // send errors when the receiver is gone — that means the
        // debounce task exited; we just drop the event.
        let _ = raw_tx_for_cb.blocking_send(res);
    })
    .map_err(|e| UiDevError::WatcherInstall {
        path: root.clone(),
        message: e.to_string(),
    })?;

    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| UiDevError::WatcherInstall {
            path: root.clone(),
            message: e.to_string(),
        })?;

    // Spawn the debounce task. Owns the watcher indirectly (the
    // caller retains the handle so it can drop it to stop watching).
    tokio::spawn(async move {
        let _ = raw_tx; // drop our copy so only the callback holds a sender
        let mut pending: WatchBatch = WatchBatch::default();
        let mut next_deadline: Option<tokio::time::Instant> = None;
        loop {
            match next_deadline {
                None => {
                    // No pending batch — wait for the first event.
                    match raw_rx.recv().await {
                        Some(Ok(ev)) => {
                            for path in &ev.paths {
                                pending.events.push(WatchEvent::from_path(path.clone()));
                            }
                            next_deadline = Some(tokio::time::Instant::now() + DEBOUNCE);
                        }
                        Some(Err(_)) | None => break,
                    }
                }
                Some(deadline) => {
                    // Pending batch — wait for either the next event
                    // (resetting the trailing-edge timer) OR the
                    // debounce deadline (flush the batch).
                    let sleep = tokio::time::sleep_until(deadline);
                    tokio::pin!(sleep);
                    tokio::select! {
                        biased;
                        ev = raw_rx.recv() => {
                            match ev {
                                Some(Ok(raw_ev)) => {
                                    for path in &raw_ev.paths {
                                        pending.events.push(WatchEvent::from_path(path.clone()));
                                    }
                                    // Trailing-edge debounce: every
                                    // new event resets the deadline.
                                    next_deadline =
                                        Some(tokio::time::Instant::now() + DEBOUNCE);
                                }
                                Some(Err(_)) | None => break,
                            }
                        }
                        _ = &mut sleep => {
                            if !pending.is_empty() {
                                let to_send = std::mem::take(&mut pending);
                                if batch_tx.send(to_send).await.is_err() {
                                    break;
                                }
                            }
                            next_deadline = None;
                        }
                    }
                }
            }
        }
    });

    Ok((watcher, batch_rx))
}

/// Test helper: return a controllable pair `(Sender, Receiver)` so
/// unit tests can inject synthetic [`WatchEvent`]s and assert on the
/// debounced batches.
///
/// The sender accepts individual events; the receiver yields
/// debounced batches.
#[cfg(test)]
pub fn mock_watcher() -> (mpsc::Sender<WatchEvent>, mpsc::Receiver<WatchBatch>) {
    let (raw_tx, mut raw_rx) = mpsc::channel::<WatchEvent>(64);
    let (batch_tx, batch_rx) = mpsc::channel::<WatchBatch>(16);
    tokio::spawn(async move {
        let mut pending = WatchBatch::default();
        let mut next_deadline: Option<tokio::time::Instant> = None;
        loop {
            match next_deadline {
                None => match raw_rx.recv().await {
                    Some(ev) => {
                        pending.events.push(ev);
                        next_deadline = Some(tokio::time::Instant::now() + DEBOUNCE);
                    }
                    None => break,
                },
                Some(deadline) => {
                    let sleep = tokio::time::sleep_until(deadline);
                    tokio::pin!(sleep);
                    tokio::select! {
                        biased;
                        ev = raw_rx.recv() => {
                            match ev {
                                Some(ev) => {
                                    pending.events.push(ev);
                                    next_deadline =
                                        Some(tokio::time::Instant::now() + DEBOUNCE);
                                }
                                None => break,
                            }
                        }
                        _ = &mut sleep => {
                            if !pending.is_empty() {
                                let to_send = std::mem::take(&mut pending);
                                if batch_tx.send(to_send).await.is_err() {
                                    break;
                                }
                            }
                            next_deadline = None;
                        }
                    }
                }
            }
        }
    });
    (raw_tx, batch_rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_event_from_buff_path() {
        let ev = WatchEvent::from_path(PathBuf::from("src/main.buff"));
        assert!(ev.is_buff);
        assert_eq!(ev.path.as_ref().unwrap(), &PathBuf::from("src/main.buff"));
    }

    #[test]
    fn watch_event_from_non_buff_path() {
        let ev = WatchEvent::from_path(PathBuf::from("static/style.css"));
        assert!(!ev.is_buff);
    }

    #[test]
    fn watch_event_path_with_no_extension() {
        let ev = WatchEvent::from_path(PathBuf::from("README"));
        assert!(!ev.is_buff);
    }

    #[test]
    fn watch_batch_empty_state() {
        let b = WatchBatch::default();
        assert!(b.is_empty());
        assert!(!b.touches_buff());
        assert!(b.buff_paths().is_empty());
    }

    #[test]
    fn watch_batch_touches_buff_predicate() {
        let mut b = WatchBatch::default();
        b.events
            .push(WatchEvent::from_path(PathBuf::from("static/style.css")));
        assert!(!b.touches_buff());
        b.events
            .push(WatchEvent::from_path(PathBuf::from("src/main.buff")));
        assert!(b.touches_buff());
    }

    #[test]
    fn watch_batch_buff_paths_dedupes() {
        let mut b = WatchBatch::default();
        b.events
            .push(WatchEvent::from_path(PathBuf::from("src/main.buff")));
        b.events
            .push(WatchEvent::from_path(PathBuf::from("src/main.buff")));
        b.events
            .push(WatchEvent::from_path(PathBuf::from("src/other.buff")));
        let paths = b.buff_paths();
        assert_eq!(paths.len(), 2, "deduped to 2 paths");
        assert_eq!(paths[0], PathBuf::from("src/main.buff"));
        assert_eq!(paths[1], PathBuf::from("src/other.buff"));
    }

    #[test]
    fn watch_batch_buff_paths_skips_non_buff() {
        let mut b = WatchBatch::default();
        b.events
            .push(WatchEvent::from_path(PathBuf::from("static/x.css")));
        b.events
            .push(WatchEvent::from_path(PathBuf::from("src/main.buff")));
        let paths = b.buff_paths();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("src/main.buff"));
    }

    #[tokio::test(start_paused = true)]
    async fn mock_watcher_debounces_bursts() {
        let (tx, mut rx) = mock_watcher();
        tx.send(WatchEvent::from_path(PathBuf::from("a.buff")))
            .await
            .unwrap();
        tx.send(WatchEvent::from_path(PathBuf::from("b.buff")))
            .await
            .unwrap();

        // Advance past the debounce window.
        tokio::time::advance(DEBOUNCE + Duration::from_millis(10)).await;

        let batch = rx.recv().await.expect("batch delivered");
        assert_eq!(batch.events.len(), 2);
        assert!(batch.touches_buff());
    }

    #[tokio::test(start_paused = true)]
    async fn mock_watcher_resets_timer_on_each_event() {
        let (tx, mut rx) = mock_watcher();
        tx.send(WatchEvent::from_path(PathBuf::from("a.buff")))
            .await
            .unwrap();
        // Advance less than DEBOUNCE; the timer should not fire yet.
        tokio::time::advance(Duration::from_millis(150)).await;

        tx.send(WatchEvent::from_path(PathBuf::from("b.buff")))
            .await
            .unwrap();
        // Advance another 150ms — total elapsed 300ms but timer was
        // reset after the second send, so no batch should be ready
        // (still 50ms short of the post-reset debounce).
        tokio::time::advance(Duration::from_millis(150)).await;

        // No batch delivered yet (timer was reset at 150ms and we
        // only advanced 150ms since — 50ms short).
        assert!(
            rx.try_recv().is_err(),
            "no batch expected before second debounce window elapses"
        );

        // Cross the post-reset debounce threshold.
        tokio::time::advance(Duration::from_millis(60)).await;
        let batch = rx.recv().await.expect("batch after extended debounce");
        assert_eq!(batch.events.len(), 2);
    }

    #[test]
    fn spawn_watcher_rejects_non_existent_path() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let res = spawn_watcher(PathBuf::from("C:/definitely/does/not/exist/xyz123"));
            assert!(
                res.is_err(),
                "expected WatcherInstall error for non-existent path"
            );
        });
    }

    #[test]
    fn spawn_watcher_accepts_real_directory() {
        let tmp = std::env::temp_dir().join("buff-ui-dev-watcher-test-dir");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let res = spawn_watcher(tmp.clone());
            assert!(res.is_ok(), "expected watcher install to succeed");
        });
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
