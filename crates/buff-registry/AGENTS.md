# buff-registry

Minimal Buff package registry HTTP server for the v1.6 "Package registry"
milestone (task T126 of `.sisyphus/plans/buff-post-v10-tooling.md`).

Built on `axum` 0.8 + `semver` + a pure-Rust in-memory `Storage` backend.
NO external services (no Postgres, no S3, no Docker, no libpq) — the
crate must build & test on a bare Windows host with only the standard
Rust toolchain.

## STRUCTURE

```
src/
├── lib.rs        # Module wiring + public API (app(), AppState, run(), validate_name)
├── main.rs       # Thin binary entry — binds TcpListener, calls axum::serve
├── error.rs      # RegistryError (handler) + StorageError (storage) + IntoResponse
├── storage.rs    # Storage trait + InMemoryStorage + DepSpec/PackageMetadata/etc.
└── handlers.rs   # Pure async axum handlers (publish/get_package/download/resolve)
tests/
└── registry_tests.rs  # In-process integration via tower::ServiceExt::oneshot
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new HTTP endpoint | `handlers.rs` (handler fn) + `lib.rs::app()` (route arm) |
| Add a new error variant | `error.rs::RegistryError` + its `IntoResponse` arm |
| Switch to a real DB backend | Implement `storage::Storage` (Postgres/S3) — drop into `AppState::new(your_arc)` |
| Tune rate limit defaults | `lib.rs::DEFAULT_RATE_LIMIT_WINDOW` / `DEFAULT_RATE_LIMIT_MAX` |
| Add a new validation rule for package names | `lib.rs::validate_name` |
| Add publish metadata field | `storage.rs::PublishRequest` (wire) + `PackageVersion` (stored) |
| Change cycle detection algorithm | `handlers.rs::has_cycle` |

## HTTP API

| Method | Path                                  | Auth | Body / Query                  | Success |
|--------|---------------------------------------|------|------------------------------|---------|
| POST   | `/api/v1/publish`                     | yes  | JSON `PublishRequest`         | 201 `PublishResponse` |
| GET    | `/api/v1/package/{name}`              | no   | —                            | 200 `PackageMetadata` |
| GET    | `/api/v1/download/{name}/{version}`   | no   | —                            | 200 octet-stream |
| GET    | `/api/v1/resolve/{name}?req=<semver>` | no   | `req` query                  | 200 `ResolveResponse` |

Error body shape: `{"error": "<message>"}`.

Status codes:
- `201` — publish succeeded
- `200` — get/download/resolve succeeded
- `400` — invalid name / version / body / tarball, or version already exists
- `401` — missing/bad/unknown bearer token (publish only)
- `404` — package/version not found, or no version matches `req`
- `409` — dependency cycle detected
- `429` — per-token publish rate limit exceeded
- `500` — storage layer failure (mutex poisoning in-memory; DB failure later)

## CONVENTIONS (this crate only)

- **Sync `Storage` trait.** The trait's methods return `Result<T, StorageError>` directly (not `Pin<Box<dyn Future>>`). The in-memory backend uses `std::sync::Mutex` held for nanoseconds; the brief block on the tokio runtime is invisible. A real DB-backed impl can wrap its async ops in `tokio::task::spawn_blocking`. NO `async_trait` dep.
- **Handler returns `Result<impl IntoResponse, RegistryError>`.** Every error maps to a fixed status code via `RegistryError::IntoResponse` — NO `unwrap`/`expect`/`panic!` in handler bodies.
- **Validation lives in handlers, not storage.** `validate_name`, `Version::parse`, cycle detection, rate limiting are all handler concerns. The storage trait stays persistence-only so a DB impl doesn't have to re-implement them.
- **BTreeMap for deterministic iteration.** Stored packages + versions use BTreeMap so test assertions on response order are stable.
- **`{name}` axum 0.8 path syntax.** NOT `:name` (axum 0.7). Migration guide: axum 0.8 (released 2024-12) introduced the curly-brace syntax via matchit 0.8.

## DEFERRED (post-v1.6)

The following are explicitly out of scope for T126 — see `src/lib.rs`
top-level docs for the full list:

- Postgres via `diesel` (sync trait → `spawn_blocking` wrapper)
- S3/MinIO blob storage for tarballs (current: in-memory `Vec<u8>`)
- GitHub OAuth token provisioning (current: static tokens via `add_token`)
- Deployment manifests (Docker / Fly.io / Railway)
- Search UI, docs hosting, download stats, webhooks, teams
- RustSec / CVE audit integration (Buff-advisories is a separate crate)

The `Storage` trait is the swappable boundary — a real impl drops in
without touching the HTTP layer.

## TESTING

`tests/registry_tests.rs` drives the `Router<()>` returned by
`buff_registry::app(state)` via `tower::ServiceExt::oneshot` — NO TCP
port allocation, NO subprocess, NO external services. Each test builds
a fresh `InMemoryStorage`, seeds a test token, and exercises the full
HTTP path (auth → validate → cycle → store → render).

The binary entry (`src/main.rs`) is NOT exercised by tests — it exists
to prove the crate builds as a runnable server and to give a future
deployment wrapper a starting point.

## DEPENDENCIES

- `axum` 0.8 (HTTP framework — `Router`, extractors, `IntoResponse`)
- `tower` 0.5 (`ServiceExt::oneshot` in tests only)
- `semver` 1 (Version + VersionReq matching for the resolve endpoint)
- `serde` / `serde_json` (JSON DTOs)
- `tokio` (runtime + TcpListener for the binary)
- `thiserror` (error derives)
- `base64` (tarball bytes in the JSON publish envelope)

NO diesel, NO libpq, NO S3 SDK, NO native C deps.
