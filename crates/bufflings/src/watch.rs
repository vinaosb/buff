//! File watcher for exercises with 200ms debounce.
//!
//! Reuses the same trailing-edge debounce pattern from T131
//! (`buff-lang-cli/src/ui_dev/watcher.rs`). On each debounced burst,
//! re-verifies the changed file and prints the result.

use std::path::PathBuf;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::exercise::ExerciseManifest;
use crate::progress::ProgressStore;
use crate::verify::{self, VerifyConfig};

/// The debounce window (200 ms, matching T131).
pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// Run the watch loop. Watches `exercises_dir` recursively, and on each
/// debounced file change, verifies the changed exercise and prints the
/// result.
///
/// This is the async entry point called from `cmd_watch` which sets up
/// the tokio runtime.
pub async fn run_watch(
    manifest: &ExerciseManifest,
    progress: &mut ProgressStore,
    exercises_dir: &PathBuf,
) -> anyhow::Result<()> {
    println!("Watching {exercises_dir:?} for changes...");
    println!("Press Ctrl+C to stop.\n");

    let (watcher, mut rx) = spawn_watcher(exercises_dir.clone())?;
    let config = VerifyConfig::default();

    // Keep the watcher alive for the duration of the loop.
    let _watcher = watcher;

    loop {
        let batch = match rx.recv().await {
            Some(b) => b,
            None => break,
        };

        for path in &batch.paths {
            // Only process .buff files
            if path.extension().map(|e| e != "buff").unwrap_or(true) {
                continue;
            }

            // Find the matching exercise entry
            let entry = match find_entry_by_path(manifest, path) {
                Some(e) => e,
                None => continue,
            };

            // Clear screen
            print!("\x1B[2J\x1B[H");
            let _ = std::io::Write::flush(&mut std::io::stdout());

            // Read and verify
            let source = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    println!("Error reading {}: {e}", path.display());
                    continue;
                }
            };

            let outcome = verify::verify_exercise(&source, path, &config);
            match outcome {
                verify::VerifyOutcome::Solved => {
                    println!("\x1B[32m[SOLVED]\x1B[0m {}", entry.name);
                    progress.mark_done(&entry.name);
                    let _ = progress.save();
                }
                verify::VerifyOutcome::NotDoneYet => {
                    println!("\x1B[33m[NOT DONE YET]\x1B[0m {}", entry.name);
                    println!("  Remove // TODO: markers and complete the code.");
                }
                verify::VerifyOutcome::CompileError(msg) => {
                    println!("\x1B[31m[FAILED]\x1B[0m {}", entry.name);
                    for line in msg.lines().take(10) {
                        println!("  {line}");
                    }
                }
                verify::VerifyOutcome::BuffNotFound => {
                    println!("[SKIP] {}", entry.name);
                    println!("  `buff` CLI not found. Install it to verify exercises.");
                }
            }
        }
    }

    Ok(())
}

/// Find an exercise entry by its file path.
fn find_entry_by_path<'a>(
    manifest: &'a ExerciseManifest,
    path: &PathBuf,
) -> Option<&'a crate::exercise::ExerciseEntry> {
    for group in &manifest.topics {
        for entry in &group.exercises {
            if entry.path == *path {
                return Some(entry);
            }
        }
    }
    None
}

/// A debounced batch of file paths.
#[derive(Debug, Clone, Default)]
pub(crate) struct WatchBatch {
    /// Unique file paths in this batch.
    paths: Vec<PathBuf>,
}

/// Install a notify watcher and return the watcher handle + receiver.
pub fn spawn_watcher(
    root: PathBuf,
) -> anyhow::Result<(RecommendedWatcher, tokio::sync::mpsc::Receiver<WatchBatch>)> {
    if !root.is_dir() {
        anyhow::bail!("watch root `{}` is not a directory", root.display());
    }

    let (raw_tx, mut raw_rx) = tokio::sync::mpsc::channel::<notify::Result<notify::Event>>(256);
    let (batch_tx, batch_rx) = tokio::sync::mpsc::channel::<WatchBatch>(16);

    let raw_tx_for_cb = raw_tx.clone();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = raw_tx_for_cb.blocking_send(res);
    })
    .map_err(|e| anyhow::anyhow!("failed to install watcher: {e}"))?;

    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| anyhow::anyhow!("failed to watch directory: {e}"))?;

    tokio::spawn(async move {
        let _ = raw_tx;
        let mut pending = WatchBatch::default();
        let mut next_deadline: Option<tokio::time::Instant> = None;

        loop {
            match next_deadline {
                None => match raw_rx.recv().await {
                    Some(Ok(ev)) => {
                        for path in &ev.paths {
                            let abs = if path.is_absolute() {
                                path.clone()
                            } else {
                                std::env::current_dir().unwrap_or_default().join(path)
                            };
                            if !pending.paths.contains(&abs) {
                                pending.paths.push(abs);
                            }
                        }
                        next_deadline = Some(tokio::time::Instant::now() + DEBOUNCE);
                    }
                    Some(Err(_)) | None => break,
                },
                Some(deadline) => {
                    let sleep = tokio::time::sleep_until(deadline);
                    tokio::pin!(sleep);
                    tokio::select! {
                        biased;
                        ev = raw_rx.recv() => {
                            match ev {
                                Some(Ok(raw_ev)) => {
                                    for path in &raw_ev.paths {
                                        let abs = if path.is_absolute() {
                                            path.clone()
                                        } else {
                                            std::env::current_dir()
                                                .unwrap_or_default()
                                                .join(path)
                                        };
                                        if !pending.paths.contains(&abs) {
                                            pending.paths.push(abs);
                                        }
                                    }
                                    next_deadline =
                                        Some(tokio::time::Instant::now() + DEBOUNCE);
                                }
                                Some(Err(_)) | None => break,
                            }
                        }
                        _ = &mut sleep => {
                            if !pending.paths.is_empty() {
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

/// Test helper: return a controllable sender/receiver pair for injecting
/// synthetic watch events in tests.
#[cfg(test)]
pub fn mock_watcher() -> (
    tokio::sync::mpsc::Sender<PathBuf>,
    tokio::sync::mpsc::Receiver<WatchBatch>,
) {
    let (raw_tx, mut raw_rx) = tokio::sync::mpsc::channel::<PathBuf>(64);
    let (batch_tx, batch_rx) = tokio::sync::mpsc::channel::<WatchBatch>(16);

    tokio::spawn(async move {
        let mut pending = WatchBatch::default();
        let mut next_deadline: Option<tokio::time::Instant> = None;

        loop {
            match next_deadline {
                None => match raw_rx.recv().await {
                    Some(path) => {
                        pending.paths.push(path);
                        next_deadline = Some(tokio::time::Instant::now() + DEBOUNCE);
                    }
                    None => break,
                },
                Some(deadline) => {
                    let sleep = tokio::time::sleep_until(deadline);
                    tokio::pin!(sleep);
                    tokio::select! {
                        biased;
                        p = raw_rx.recv() => {
                            if let Some(p) = p {
                                pending.paths.push(p);
                                next_deadline =
                                    Some(tokio::time::Instant::now() + DEBOUNCE);
                            } else {
                                break;
                            }
                        }
                        _ = &mut sleep => {
                            if !pending.paths.is_empty() {
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

    #[tokio::test(start_paused = true)]
    async fn mock_watcher_delivers_batch_after_debounce() {
        let (tx, mut rx) = mock_watcher();
        tx.send(PathBuf::from("a.buff")).await.unwrap();
        tx.send(PathBuf::from("b.buff")).await.unwrap();

        tokio::time::advance(DEBOUNCE + Duration::from_millis(10)).await;

        let batch = rx.recv().await.expect("batch delivered");
        assert_eq!(batch.paths.len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn mock_watcher_resets_timer_on_each_event() {
        let (tx, mut rx) = mock_watcher();
        tx.send(PathBuf::from("a.buff")).await.unwrap();
        tokio::time::advance(Duration::from_millis(150)).await;

        tx.send(PathBuf::from("b.buff")).await.unwrap();
        tokio::time::advance(Duration::from_millis(150)).await;

        // Timer was reset at 150ms, only 150ms since reset — not yet
        assert!(
            rx.try_recv().is_err(),
            "no batch expected before second debounce window"
        );

        tokio::time::advance(Duration::from_millis(60)).await;
        let batch = rx.recv().await.expect("batch after extended debounce");
        assert_eq!(batch.paths.len(), 2);
    }

    #[test]
    fn spawn_watcher_rejects_nonexistent_path() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let res = spawn_watcher(PathBuf::from("C:/definitely/does/not/exist/xyz123"));
            assert!(res.is_err());
        });
    }
}
