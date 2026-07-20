//! `buff ui dev` — UI dev server (T131).
//!
//! Vite / trunk / cargo-leptos-style dev loop:
//!
//! 1. Start an HTTP server on `127.0.0.1:<port>` serving
//!    `<project>/static/` + the generated Wasm bundle.
//! 2. Watch `.buff` files in `<project>` (recursive) via the `notify`
//!    crate with a 200 ms trailing-edge debounce.
//! 3. On each debounced change: re-run the Buff front-end
//!    (`pipeline::compile_to_rust`) on every changed `.buff` file,
//!    then call the configured [`Builder`] (`CargoBuilder` in
//!    production, `MockBuilder` in tests) for the wasm32 cargo +
//!    wasm-bindgen half.
//! 4. Broadcast the outcome (reload / error) over the
//!    [`ReloadBroadcaster`] to every connected browser via the
//!    `/__buff_reload__` WebSocket endpoint.
//! 5. The injected client snippet (`<1 KB`) opens the WS, listens
//!    for `{"type":"reload"}` or `{"type":"error", message:"..."}`
//!    and reacts (full `location.reload()` for reload; red banner
//!    for error). LIVE RELOAD — not state-preserving HMR (v1.9+).
//!
//! # Module layout
//!
//! - [`error`] — `UiDevError` (thiserror enum).
//! - [`client_js`] — the injected client snippet + HTML injection.
//! - [`broadcaster`] — `ReloadMessage` + `ReloadBroadcaster`.
//! - [`watcher`] — notify + 200 ms debounce → `mpsc::Receiver<WatchBatch>`.
//! - [`builder`] — `Builder` trait + `CargoBuilder` + `MockBuilder`.
//! - [`http`] — axum `Router<()>` + WS upgrade + static-file serving.
//!
//! # Public API
//!
//! [`serve`] is the production entry point — takes a project path +
//! port, wires up the production `CargoBuilder` + `NotifyWatcher`,
//! and blocks until Ctrl-C / SIGINT. Unit tests bypass `serve` and
//! exercise the individual modules.

pub mod broadcaster;
pub mod builder;
pub mod client_js;
pub mod error;
pub mod http;
pub mod watcher;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ui_dev::broadcaster::ReloadBroadcaster;
use crate::ui_dev::builder::{BuildOutcome, Builder, CargoBuilder};
use crate::ui_dev::error::UiDevError;
use crate::ui_dev::http::{app as http_app, SharedState};
use crate::ui_dev::watcher::spawn_watcher;

/// Bind address family — always loopback. The dev server is NEVER
/// exposed to the network; users who want that can run a reverse
/// proxy. Matches Vite / cargo-leptos defaults.
const BIND_HOST: &str = "127.0.0.1";

/// Entry point for `buff ui dev <PATH> --port <PORT>`.
///
/// Canonicalises the project root, installs the notify watcher,
/// constructs the [`CargoBuilder`], wires up the broadcaster, and
/// `axum::serve`s the HTTP server. Blocks until Ctrl-C / SIGINT,
/// at which point axum's graceful-shutdown path drains in-flight
/// requests and the function returns `Ok(())`.
///
/// # Errors
///
/// - [`UiDevError::ProjectRootNotFound`] when `<PATH>` is missing.
/// - [`UiDevError::Canonicalize`] on `std::fs::canonicalize` failure.
/// - [`UiDevError::Bind`] when `127.0.0.1:<port>` is in use.
/// - [`UiDevError::WatcherInstall`] when notify fails to install.
/// - Anything else returned by axum's serving loop is wrapped in
///   [`UiDevError::Other`].
pub async fn serve(project_root: &Path, port: u16) -> Result<(), UiDevError> {
    let root = validate_root(project_root)?;
    let broadcaster = ReloadBroadcaster::new();
    let builder: Arc<dyn Builder> = Arc::new(CargoBuilder::new(root.clone()));

    eprintln!(
        "[buff ui dev] serving {} on http://{}:{}",
        root.display(),
        BIND_HOST,
        port
    );
    eprintln!("[buff ui dev] watching .buff files (200 ms debounce)");
    eprintln!("[buff ui dev] press Ctrl-C to stop");

    // Start the HTTP server first so the browser can connect
    // immediately. The watcher is installed next; until then, the
    // server just serves whatever static assets already exist.
    let state = SharedState::new(root.clone(), broadcaster.clone());
    let router = http_app(state);

    let listener = tokio::net::TcpListener::bind((BIND_HOST, port))
        .await
        .map_err(|source| UiDevError::Bind { port, source })?;

    // Install the watcher + spawn the rebuild task.
    let (_watcher_guard, mut batch_rx) = spawn_watcher(root.clone())?;
    let rebuild_broadcaster = broadcaster.clone();
    let rebuild_root = root.clone();
    tokio::spawn(async move {
        rebuild_loop(&mut batch_rx, &rebuild_root, &*builder, rebuild_broadcaster).await;
    });

    // axum::serve handles Ctrl-C / SIGINT gracefully when
    // `with_graceful_shutdown` is wired. We use tokio::signal so the
    // behavior is consistent across Windows + Unix.
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        eprintln!("[buff ui dev] shutting down (Ctrl-C received)");
    };

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| UiDevError::Other(format!("axum serve error: {e}")))?;

    Ok(())
}

/// Validate + canonicalise the project root.
fn validate_root(path: &Path) -> Result<PathBuf, UiDevError> {
    if !path.exists() {
        return Err(UiDevError::ProjectRootNotFound {
            path: path.to_path_buf(),
        });
    }
    if !path.is_dir() {
        return Err(UiDevError::ProjectRootNotFound {
            path: path.to_path_buf(),
        });
    }
    std::fs::canonicalize(path).map_err(|source| UiDevError::Canonicalize {
        path: path.to_path_buf(),
        source,
    })
}

/// Rebuild loop: one task that consumes debounced batches from
/// `batch_rx`, runs the Buff front-end on every `.buff` file, calls
/// `builder.build()`, and broadcasts the outcome.
///
/// This loop is the heart of the dev server. It runs forever (until
/// the watcher's sender side closes, which happens when notify
/// errors fatally OR the watcher handle is dropped).
async fn rebuild_loop(
    batch_rx: &mut tokio::sync::mpsc::Receiver<watcher::WatchBatch>,
    project_root: &Path,
    builder: &dyn Builder,
    broadcaster: ReloadBroadcaster,
) {
    while let Some(batch) = batch_rx.recv().await {
        if batch.is_empty() {
            continue;
        }

        // Step 1: run the Buff front-end on every changed .buff file.
        // This catches lex / parse / codegen errors and regenerates
        // the .rs files alongside the sources.
        if batch.touches_buff() {
            let buff_paths = batch.buff_paths();
            let mut errors: Vec<String> = Vec::new();
            for path in &buff_paths {
                if let Err(e) = run_buff_front_end(path) {
                    errors.push(format!("{}: {}", path.display(), e));
                }
            }
            if !errors.is_empty() {
                broadcaster.error(errors.join("\n"));
                continue;
            }
        }

        // Step 2: cargo + wasm-bindgen rebuild via the Builder trait.
        let _ = project_root; // builder owns its own project_root copy
        match builder.build() {
            Ok(BuildOutcome::Ok) => broadcaster.reload(),
            Ok(BuildOutcome::Failed { message }) => broadcaster.error(message),
            Err(e) => broadcaster.error(e.to_string()),
        }
    }
}

/// Run the Buff front-end (`pipeline::compile_to_rust`) on a single
/// `.buff` file. Wraps the pipeline's `anyhow::Error` into a plain
/// `String` so the rebuild loop can collect + format multiple errors.
fn run_buff_front_end(path: &Path) -> Result<(), String> {
    crate::pipeline::compile_to_rust(path).map_err(|e| format!("{e:#}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_dev::broadcaster::ReloadMessage;
    use crate::ui_dev::builder::MockBuilder;
    use crate::ui_dev::watcher::{WatchBatch, WatchEvent};
    use std::path::PathBuf;

    #[test]
    fn validate_root_rejects_missing_path() {
        let res = validate_root(Path::new("C:/does/not/exist/xyz/buff"));
        assert!(res.is_err());
        match res {
            Err(UiDevError::ProjectRootNotFound { .. }) => {}
            other => panic!("expected ProjectRootNotFound, got {other:?}"),
        }
    }

    #[test]
    fn validate_root_rejects_file_path() {
        let tmp = std::env::temp_dir().join("buff-ui-dev-validate-file.txt");
        std::fs::write(&tmp, b"x").unwrap();
        let res = validate_root(&tmp);
        assert!(res.is_err());
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn validate_root_accepts_real_directory() {
        let tmp = std::env::temp_dir().join("buff-ui-dev-validate-dir");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let res = validate_root(&tmp);
        assert!(res.is_ok());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test(start_paused = true)]
    async fn rebuild_loop_broadcasts_reload_on_ok_builder() {
        let (tx, mut batch_rx) = tokio::sync::mpsc::channel::<watcher::WatchBatch>(4);
        let broadcaster = ReloadBroadcaster::new();
        let mut sub = broadcaster.subscribe();
        let builder = MockBuilder::ok();
        let root = PathBuf::from(".");

        // Push a non-`.buff` batch so the Buff front-end step is
        // skipped (no real .buff file to compile) and the builder
        // runs directly.
        let mut batch = WatchBatch::default();
        batch
            .events
            .push(WatchEvent::from_path(PathBuf::from("style.css")));
        tx.send(batch).await.unwrap();
        // Close the channel so the loop exits.
        drop(tx);

        rebuild_loop(&mut batch_rx, &root, &builder, broadcaster.clone()).await;

        let msg = sub.try_recv().expect("reload should have been broadcast");
        assert_eq!(msg, ReloadMessage::Reload);
    }

    #[tokio::test(start_paused = true)]
    async fn rebuild_loop_broadcasts_error_on_failed_builder() {
        let (tx, mut batch_rx) = tokio::sync::mpsc::channel::<watcher::WatchBatch>(4);
        let broadcaster = ReloadBroadcaster::new();
        let mut sub = broadcaster.subscribe();
        let builder = MockBuilder::failed("wasm compile exploded");
        let root = PathBuf::from(".");

        let mut batch = WatchBatch::default();
        batch
            .events
            .push(WatchEvent::from_path(PathBuf::from("style.css")));
        tx.send(batch).await.unwrap();
        drop(tx); // close → rebuild_loop exits after draining

        rebuild_loop(&mut batch_rx, &root, &builder, broadcaster.clone()).await;

        let msg = sub.try_recv().expect("error should have been broadcast");
        match msg {
            ReloadMessage::Error { message } => {
                assert!(message.contains("wasm compile exploded"), "got: {message}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn rebuild_loop_runs_buff_pipeline_on_buff_changes() {
        // Use a real .buff file that pipeline::compile_to_rust can
        // succeed on (the ola example).
        let example_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
            .join("ola.buff");

        if !example_path.exists() {
            eprintln!(
                "skipping: ola.buff not present at {}",
                example_path.display()
            );
            return;
        }

        let (tx, mut batch_rx) = tokio::sync::mpsc::channel::<watcher::WatchBatch>(4);
        let broadcaster = ReloadBroadcaster::new();
        let mut sub = broadcaster.subscribe();
        let builder = MockBuilder::ok();
        let root = example_path.parent().unwrap().to_path_buf();

        let mut batch = WatchBatch::default();
        batch
            .events
            .push(WatchEvent::from_path(example_path.clone()));
        tx.send(batch).await.unwrap();
        drop(tx);

        rebuild_loop(&mut batch_rx, &root, &builder, broadcaster.clone()).await;

        let msg = sub
            .try_recv()
            .expect("reload should fire after Buff success");
        assert_eq!(msg, ReloadMessage::Reload);
    }

    #[tokio::test(start_paused = true)]
    async fn rebuild_loop_broadcasts_error_on_buff_compile_failure() {
        // Hand-craft a syntactically broken .buff file in a temp dir.
        let tmp = std::env::temp_dir().join("buff-ui-dev-broken-buff");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let broken = tmp.join("broken.buff");
        // Invalid: `func` with no body / signature.
        std::fs::write(&broken, "func 123_invalid\n").unwrap();

        let (tx, mut batch_rx) = tokio::sync::mpsc::channel::<watcher::WatchBatch>(4);
        let broadcaster = ReloadBroadcaster::new();
        let mut sub = broadcaster.subscribe();
        let builder = MockBuilder::ok();
        let root = tmp.clone();

        let mut batch = WatchBatch::default();
        batch.events.push(WatchEvent::from_path(broken.clone()));
        tx.send(batch).await.unwrap();
        drop(tx);

        rebuild_loop(&mut batch_rx, &root, &builder, broadcaster.clone()).await;

        let msg = sub.try_recv().expect("error should fire");
        match msg {
            ReloadMessage::Error { message } => {
                assert!(
                    !message.is_empty(),
                    "error message should mention the .buff failure"
                );
            }
            other => panic!("expected Error, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Test that `serve` rejects a missing path with the right
    /// error variant. We don't run the full server here.
    #[test]
    fn serve_rejects_missing_path() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let res = rt.block_on(serve(Path::new("C:/no/such/path/xyz"), 8081));
        assert!(res.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn rebuild_loop_skips_empty_batch() {
        // Empty batches should not trigger builder.build() — saves
        // a wasted cargo invocation.
        let (tx, mut batch_rx) = tokio::sync::mpsc::channel::<watcher::WatchBatch>(4);
        let broadcaster = ReloadBroadcaster::new();
        let mut sub = broadcaster.subscribe();
        let builder = MockBuilder::failed("should-not-fire");
        let root = PathBuf::from(".");

        let empty_batch = WatchBatch::default();
        tx.send(empty_batch).await.unwrap();
        // Push a real batch that triggers build.
        let mut real_batch = WatchBatch::default();
        real_batch
            .events
            .push(WatchEvent::from_path(PathBuf::from("style.css")));
        tx.send(real_batch).await.unwrap();
        drop(tx);

        rebuild_loop(&mut batch_rx, &root, &builder, broadcaster.clone()).await;

        // Should see exactly ONE error from the failed builder (the
        // empty batch must not have triggered it).
        let first = sub.try_recv();
        assert!(
            first.is_ok(),
            "expected one error from builder, got {first:?}"
        );
        let second = sub.try_recv();
        assert!(
            second.is_err(),
            "expected only one broadcast, got second: {second:?}"
        );
    }
}
