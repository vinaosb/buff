# DR: libsqlite3-sys Exception — P0.8 cargo-deny gate

**Date:** 2026-07-27
**Status:** Approved
**Task:** P0.8 — cargo-deny hard gate

## Context

The Buff workspace enforces a hard "no C library, no cc-rs, no
native-tls" rule (AGENTS.md). However, `libsqlite3-sys` — the
`*-sys` style binding crate for the upstream SQLite C library —
remains in the transitive dependency graph and cannot be removed
without breaking two framework crates.

## Dependency Chain (verified 2026-07-27 via `cargo tree -i libsqlite3-sys`)

```
libsqlite3-sys@0.30.1
├── rusqlite@0.32.1
│   └── buff-registry v1.0.0 (/workspace/crates/buff-registry)
│       [dev-dependencies]
│       └── buff-lang-cli v1.0.0 (/workspace/crates/buff-lang-cli)
└── sqlx-sqlite@0.8.6
    └── sqlx@0.8.6
        └── buff-db v1.0.0 (/workspace/crates/buff-db)
```

Note: the `[dev-dependencies]` marker in the tree output indicates
that `buff-lang-cli` has `buff-registry` as a dev-dependency (for
integration testing the registry in-process). `buff-registry` itself
declares `rusqlite.workspace = true` in its regular `[dependencies]`
section (verified by reading `crates/buff-registry/Cargo.toml`).
rusqlite is a real runtime dependency of `buff-registry`.

## Why libsqlite3-sys Cannot Be Removed

1. **buff-registry** uses `rusqlite` (workspace pin: `rusqlite = {
   version = "0.32", features = ["bundled"] }`) for T57 OAuth session
   persistence. Source code: `crates/buff-registry/src/storage_sqlite.rs`.
   The `bundled` feature compiles SQLite from C source via cc-rs.

2. **buff-db** uses `sqlx` with the `sqlite` cargo feature (workspace
   pin: `sqlx = { version = "0.8", default-features = false, features
   = ["runtime-tokio-rustls", "sqlite", "postgres", "any"] }`) for
   the T18 database MVP. Source code: `crates/buff-db/src/{lib,pool,
   query,row}.rs`. The `sqlite` feature pulls `sqlx-sqlite` which
   pulls `libsqlite3-sys`.

3. There is no production-quality pure-Rust SQLite implementation.
   The closest candidate (`limbo`) is pre-1.0 and lacks the feature
   surface sqlx/rusqlite depend on. A storage redesign (e.g. moving
   buff-registry to sled / redb and buff-db to a query-builder-only
   surface with no driver) is a v2.x-scale undertaking.

## Decision

Allow `libsqlite3-sys` as a grandfathered exception in `deny.toml`
via the `skip-tree` mechanism. This:

- Does NOT add `libsqlite3-sys` to the `deny` list.
- Hides libsqlite3-sys's transitive cc-rs / pkg-config / vcpkg usage
  from the gate's recursive scan.
- Catches any NEW direct addition of libsqlite3-sys (e.g. a third
  framework crate pulling it via a different driver).

New dependencies MUST NOT add libsqlite3-sys directly. It may enter
the tree only through the established `rusqlite` / `sqlx-sqlite`
paths documented above.

## Consequence

- `cargo deny check bans` passes with libsqlite3-sys in the
  `skip-tree` list.
- The bundled SQLite C source is the canonical upstream (public
  domain — the SQLite team's guarantee of no license entanglement).
  It is the most-audited C code in widespread Rust use.
- **Exit criterion**: when a production pure-Rust SQLite impl lands
  (or when buff-registry + buff-db are refactored to a non-SQLite
  storage backend), this exception can be closed and
  `libsqlite3-sys` added to `deny` as a permanent guard.

## Related

- `deny.toml` `[bans] skip-tree` entry for `libsqlite3-sys`.
- `.sisyphus/decisions/ring-exception.md` (fellow cc-rs exception).
- `.sisyphus/decisions/cc-build-dep-exception.md` (transitive consequence).
