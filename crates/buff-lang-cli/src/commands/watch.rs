//! `buff watch <PATH> [--exec <CMD>]` — file watcher + auto-rebuild (T64).
//!
//! SIMPLIFIED SCOPE: this commit ships the standalone file-watcher +
//! rebuild loop only. Server route hot-swap (the original T64 spec
//! requiring buff-web integration) is explicitly deferred — `buff
//! watch` does NOT touch buff-web.
//!
//! # Pipeline
//!
//! 1. Resolve the watch `path` (file or directory). When given a
//!    single `.buff` file, observe its **containing directory** so
//!    edits to sibling modules also trigger rebuilds (matches
//!    cargo-watch behaviour).
//! 2. Install a `notify::RecommendedWatcher` (recursive).
//! 3. Pump events through a trailing-edge 500ms debounce buffer (same
//!    algorithm as [`crate::ui_dev::watcher`], but 500ms here vs 200ms
//!    there — `buff watch` rebuilds are heavier than the dev-server
//!    reload, so the longer window coalesces multi-file saves better).
//! 4. On each debounced batch containing at least one `.buff` change,
//!    pick the first changed `.buff` file and call
//!    [`crate::commands::build::run`] with `Some(file)`.
//! 5. When `--exec <CMD>` is set, run `<CMD>` via `sh -c` (Unix) /
//!    `cmd /c` (Windows). Failures surface as stderr notes but do NOT
//!    exit the loop (you typically keep iterating even when the
//!    post-build hook fails).
//! 6. Loop until the watcher errors fatally OR the process receives
//!    Ctrl-C / SIGINT (handled by the OS — `buff watch` exits when
//!    `notify::RecommendedWatcher` is dropped via the receiver going
//!    away).
//!
//! # Why not tokio?
//!
//! `buff watch` is a deliberately small standalone watcher — it does
//! not need tokio's async machinery. We use a sync
//! `std::sync::mpsc::Receiver` + `std::thread::sleep` for the
//! debounce, keeping the dependency surface minimal. Compare to
//! `buff ui dev` which IS tokio-based (it serves HTTP + WebSocket).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// The trailing-edge debounce window. 500ms per T64 spec — wider
/// than the dev-server's 200ms because `buff watch` rebuilds are
/// heavier (full rustc compile, not just a browser reload).
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// Entry point for `buff watch <PATH> [--exec <CMD>]`.
///
/// Blocks until Ctrl-C / SIGINT. Returns `Ok(())` only on a clean
/// shutdown (watcher install failure OR a fatal watcher error
/// bubbles up as `Err`).
///
/// See the module docs for the full pipeline description.
pub fn run(path: &Path, exec: Option<&str>) -> Result<()> {
    let (watch_root, initial_target) = resolve_watch_root(path)?;

    eprintln!("buff watch: watching `{}`", watch_root.display());
    if let Some(t) = &initial_target {
        eprintln!("buff watch: initial build of `{}`", t.display());
        // Run an initial build so the user gets immediate feedback
        // (mirrors `cargo watch -x build`). Failures are logged but
        // do not abort the loop — the user can fix + save.
        rebuild(t, exec);
    } else {
        eprintln!("buff watch: no initial `.buff` file to build — waiting for changes");
    }

    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        // ignore send errors: receiver gone == watcher should stop.
        let _ = tx.send(res);
    })
    .context("failed to create file watcher")?;

    watcher
        .watch(&watch_root, RecursiveMode::Recursive)
        .with_context(|| format!("failed to install watch on `{}`", watch_root.display()))?;

    eprintln!(
        "buff watch: debounce={}ms — Ctrl-C to exit",
        DEBOUNCE.as_millis()
    );

    // Trailing-edge debounce loop (sync — see module docs).
    let mut pending: Vec<PathBuf> = Vec::new();
    let mut next_deadline: Option<Instant> = None;
    loop {
        match next_deadline {
            None => {
                // No pending batch — block on the channel.
                match rx.recv() {
                    Ok(Ok(ev)) => {
                        extend_pending(&mut pending, &ev);
                        if !pending.is_empty() {
                            next_deadline = Some(Instant::now() + DEBOUNCE);
                        }
                    }
                    Ok(Err(e)) => {
                        // notify surfaced a non-fatal error (e.g. the
                        // OS dropped an event). Log + keep watching.
                        eprintln!("buff watch: watcher note: {e}");
                    }
                    Err(_) => {
                        // Sender gone — watcher thread exited. Clean exit.
                        eprintln!("buff watch: watcher closed — exiting");
                        break;
                    }
                }
            }
            Some(deadline) => {
                // Pending batch — drain the channel non-blocking,
                // reset the deadline on every new event.
                let now = Instant::now();
                if now >= deadline {
                    flush(&mut pending, exec);
                    next_deadline = None;
                    continue;
                }
                // Sleep in small slices so rx remains responsive.
                let remaining = deadline - now;
                match rx.recv_timeout(remaining) {
                    Ok(Ok(ev)) => {
                        extend_pending(&mut pending, &ev);
                        // Trailing-edge: reset the deadline.
                        next_deadline = Some(Instant::now() + DEBOUNCE);
                    }
                    Ok(Err(e)) => {
                        eprintln!("buff watch: watcher note: {e}");
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        flush(&mut pending, exec);
                        next_deadline = None;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        flush(&mut pending, exec);
                        eprintln!("buff watch: watcher closed — exiting");
                        break;
                    }
                }
            }
        }
    }

    // Drop the watcher before returning so the OS-level handle is
    // released deterministically (otherwise tests that spawn this fn
    // could leak watchers).
    drop(watcher);
    Ok(())
}

/// Resolve a user-supplied `path` (file or directory) into:
///
/// - `watch_root` — the directory to install the recursive watcher on.
/// - `initial_target` — the `.buff` file to build on the initial run
///   (`None` when `path` is a directory with no obvious single entry
///   point — in that case `buff watch` waits for the first change).
///
/// When `path` is a single `.buff` file, the watch root is its
/// containing directory (so sibling edits trigger rebuilds) and the
/// initial target is the file itself. When `path` is a directory, the
/// watch root is the directory itself; the initial target is
/// `path/src/main.buff` if present, else `None` (no implicit walk —
/// we don't want to rebuild every `.buff` in a directory tree on
/// startup).
fn resolve_watch_root(path: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("`{}` does not exist", path.display()))?;

    if canonical.is_file() {
        let parent = canonical
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        return Ok((parent, Some(canonical)));
    }

    if canonical.is_dir() {
        // Look for the canonical entry point `src/main.buff` inside
        // the directory. Mirrors `commands::build::find_project_entry`
        // but without the warning-shelling (keep watch output quiet).
        let main_buff = canonical.join("src").join("main.buff");
        let initial = if main_buff.is_file() {
            Some(main_buff)
        } else {
            None
        };
        return Ok((canonical, initial));
    }

    anyhow::bail!(
        "`{}` is neither a file nor a directory",
        canonical.display()
    );
}

/// Append the `.buff` paths from a notify event to the pending batch.
/// Non-`.buff` events are ignored (matches the dev-server filter —
/// only `.buff` source edits should trigger a Buff rebuild).
fn extend_pending(pending: &mut Vec<PathBuf>, ev: &notify::Event) {
    for p in &ev.paths {
        if p.extension().is_some_and(|e| e == "buff") && !pending.contains(p) {
            pending.push(p.clone());
        }
    }
}

/// Rebuild the first pending `.buff` file + run the optional `--exec`
/// hook. The pending batch is cleared regardless of build success
/// (we don't want to retry the same failing file forever — the user
/// will save again to retry).
fn flush(pending: &mut Vec<PathBuf>, exec: Option<&str>) {
    let target = match pending.first().cloned() {
        Some(t) => t,
        None => return,
    };
    pending.clear();
    rebuild(&target, exec);
}

/// Rebuild a single `.buff` file via [`commands::build::run`] + run
/// the optional `--exec <CMD>` hook. Both steps log to stderr; neither
/// can fail the watch loop (the user keeps editing).
fn rebuild(file: &Path, exec: Option<&str>) {
    eprintln!(
        "buff watch: rebuild `{}` at {}",
        file.display(),
        humantime_now()
    );
    let args = crate::commands::build::run(
        Some(file),
        None,  // output: let build pick default
        false, // release
        false, // minimal
        false, // fast
        false, // no_cache (cache stays on — watch rebuilds benefit)
        false, // incremental (watch.rs uses the legacy path for now; a
        //           follow-up can thread a persistent salsa DB
        //           through the watch loop for true incremental
        //           rebuilds — T7 lays the foundation)
        true,  // no_incremental (force legacy path; see above)
        false, // sccache
        None,  // target
        crate::pipeline::LinkerChoice::default(), // linker
        crate::pipeline::DebugInfoChoice::default(), // debuginfo
        crate::pipeline::BackendChoice::default(), // backend
        false, // detect_races
    );
    match args {
        Ok(()) => {
            eprintln!("buff watch: build ok");
            if let Some(cmd) = exec {
                run_exec_hook(cmd);
            }
        }
        Err(e) => {
            eprintln!("buff watch: build FAILED — {e:#}");
            // Do NOT run the exec hook on build failure (you typically
            // don't want to restart the server when the binary did not
            // change).
        }
    }
}

/// Run `--exec <CMD>` via `sh -c` (Unix) / `cmd /c` (Windows).
/// Failures surface as stderr notes only.
fn run_exec_hook(cmd: &str) {
    eprintln!("buff watch: exec `{cmd}`");
    let mut inv = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.arg("/c").arg(cmd);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    };
    match inv.status() {
        Ok(status) => {
            if !status.success() {
                eprintln!("buff watch: exec exited {status} — keeping watch");
            }
        }
        Err(e) => {
            eprintln!("buff watch: could not spawn exec `{cmd}` — {e}");
        }
    }
}

/// Format the current wall-clock as `HH:MM:SS` for log lines.
/// We hand-roll this (instead of pulling `chrono`) because `buff watch`
/// is the ONE module that needs wall-clock strings and we want to
/// keep the dependency surface minimal.
fn humantime_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let day_secs = 24 * 60 * 60;
    let time_of_day = secs % day_secs;
    let hours = (time_of_day / 3600) % 24;
    let minutes = (time_of_day / 60) % 60;
    let seconds = time_of_day % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_is_500ms() {
        assert_eq!(DEBOUNCE, Duration::from_millis(500));
    }

    #[test]
    fn extend_pending_filters_non_buff() {
        let mut pending = Vec::new();
        let ev = notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Any));
        // simulate paths by constructing a second event with paths set
        let mut ev2 = ev.clone();
        ev2.paths = vec![
            PathBuf::from("foo.rs"),
            PathBuf::from("bar.buff"),
            PathBuf::from("baz.BUFF"), // case-sensitive: NOT .buff
        ];
        extend_pending(&mut pending, &ev2);
        assert_eq!(pending, vec![PathBuf::from("bar.buff")]);
    }

    #[test]
    fn extend_pending_dedups() {
        let mut pending = vec![PathBuf::from("a.buff")];
        let mut ev = notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Any));
        ev.paths = vec![PathBuf::from("a.buff"), PathBuf::from("b.buff")];
        extend_pending(&mut pending, &ev);
        assert_eq!(
            pending,
            vec![PathBuf::from("a.buff"), PathBuf::from("b.buff")]
        );
    }

    #[test]
    fn resolve_watch_root_rejects_nonexistent() {
        let p = Path::new("this-path-does-not-exist-12345");
        let r = resolve_watch_root(p);
        assert!(r.is_err());
    }

    #[test]
    fn humantime_now_is_hh_mm_ss_shape() {
        let s = humantime_now();
        // HH:MM:SS — 8 chars + 2 colons.
        assert_eq!(s.len(), 8, "expected HH:MM:SS, got `{s}`");
        let parts: Vec<&str> = s.split(':').collect();
        assert_eq!(parts.len(), 3);
        for p in parts {
            assert!(p.len() == 2, "each component is 2 digits: `{p}`");
            assert!(p.chars().all(|c| c.is_ascii_digit()));
        }
    }
}
