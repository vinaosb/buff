//! `buff-registry` — Buff package registry server binary.
//!
//! Wires a storage backend into the axum [`Router`] and serves it via
//! [`axum::serve`]. The bind address is taken from the
//! [`buff_registry::BIND_ADDR_ENV`] env var (`BUFF_REGISTRY_ADDR`),
//! defaulting to [`buff_registry::DEFAULT_BIND_ADDR`]
//! (`127.0.0.1:7878`).
//!
//! # Storage backend selection (T57)
//!
//! The backend is selected by the `BUFF_REGISTRY_DB_PATH` env var:
//!
//! - **Unset**: uses [`buff_registry::InMemoryStorage`] (ephemeral —
//!   state is lost on restart; matches the v1.6 MVP behaviour for
//!   backwards compatibility).
//! - **Set to a file path** (e.g. `registry.db`): uses
//!   [`buff_registry::SqliteStorage`] backed by a durable SQLite file.
//!   The database file is created on first run; all published packages,
//!   tokens, and rate-limit counters persist across restarts.
//! - **Set to `:memory:`**: uses [`buff_registry::SqliteStorage`] with
//!   an in-memory database (ephemeral, but exercises the SQLite code
//!   path — useful for smoke-testing).
//!
//! # Running
//!
//! ```text
//! buff-registry                                  # in-memory, 127.0.0.1:7878
//! BUFF_REGISTRY_DB_PATH=registry.db buff-registry  # durable SQLite
//! BUFF_REGISTRY_ADDR=0.0.0.0:8080 buff-registry    # explicit bind
//! ```
//!
//! This binary exists primarily to confirm the crate builds as a
//! runnable server; the real validation surface is `cargo test -p
//! buff-registry` (in-process via `tower::ServiceExt::oneshot`).

use std::sync::Arc;

use buff_registry::{
    app, AppState, InMemoryStorage, SqliteStorage, Storage, BIND_ADDR_ENV, DEFAULT_BIND_ADDR,
};

/// The env-var name used to select the SQLite database path.
/// When unset, the in-memory backend is used (backwards compat).
const DB_PATH_ENV: &str = "BUFF_REGISTRY_DB_PATH";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::var(BIND_ADDR_ENV).unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());

    let storage: Arc<dyn Storage> = match std::env::var(DB_PATH_ENV) {
        Ok(path) if !path.is_empty() => {
            eprintln!("buff-registry: using SQLite backend at {path}");
            let path_owned = path.clone();
            Arc::new(
                SqliteStorage::open(&path_owned)
                    .map_err(|e| format!("failed to open SQLite at {path}: {e}"))?,
            )
        }
        _ => {
            eprintln!("buff-registry: using in-memory backend (set {DB_PATH_ENV} for durability)");
            Arc::new(InMemoryStorage::new())
        }
    };
    let state = AppState::new(storage);

    eprintln!("buff-registry: listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app(state)).await?;
    Ok(())
}
