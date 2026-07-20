//! `buff-registry` — Buff package registry server binary.
//!
//! Wires a fresh [`buff_registry::InMemoryStorage`] into the axum
//! [`Router`] and serves it via [`axum::serve`]. The bind address is
//! taken from the [`buff_registry::BIND_ADDR_ENV`] env var
//! (`BUFF_REGISTRY_ADDR`), defaulting to [`buff_registry::DEFAULT_BIND_ADDR`]
//! (`127.0.0.1:7878`).
//!
//! # Running
//!
//! ```text
//! buff-registry                                  # 127.0.0.1:7878
//! BUFF_REGISTRY_ADDR=0.0.0.0:8080 buff-registry # explicit bind
//! ```
//!
//! The v1.6 milestone ships NO token-provisioning flow (GitHub OAuth is
//! deferred — see `lib.rs` docs). To accept any publish requests you
//! must first seed a token by adding a `seed_tokens` env var, etc. —
//! the MVP binary does NOT do this; use the in-process test harness
//! or a custom binary that constructs its own [`buff_registry::AppState`]
//! with tokens seeded.
//!
//! This binary exists primarily to confirm the crate builds as a
//! runnable server; the real validation surface is `cargo test -p
//! buff-registry` (in-process via `tower::ServiceExt::oneshot`).

use std::sync::Arc;

use buff_registry::{app, AppState, InMemoryStorage, BIND_ADDR_ENV, DEFAULT_BIND_ADDR};

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    // Honor `tracing::subscriber::fmt::init`'s default env filter when
    // BUFF_LOG is set (mirrors the rest of the workspace). The
    // `tracing-subscriber` crate is NOT pulled here (to avoid a new dep
    // in the registry crate); a future deployment wrapper can add it.
    let addr = std::env::var(BIND_ADDR_ENV).unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());

    let storage: Arc<dyn buff_registry::Storage> = Arc::new(InMemoryStorage::new());
    let state = AppState::new(storage);

    eprintln!("buff-registry: listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app(state)).await?;
    Ok(())
}
