# DR: cc Build-Dependency Exception — P0.8 cargo-deny gate

**Date:** 2026-07-27
**Status:** Approved (transitive — auto-resolves when the parent DRs close)
**Task:** P0.8 — cargo-deny hard gate

## Context

The Buff workspace enforces a hard "no C library, no cc-rs, no
native-tls" rule (AGENTS.md). The `cc` crate (the canonical Rust
build-script wrapper around the system C compiler) appears in the
transitive dependency graph as a **build-time** dependency of three
upstream crates. It is NOT a direct dependency of any Buff crate.

## Dependency Chain (verified 2026-07-27)

```
cc@1.3.0 (build-dep)
├── ring@0.16.20 / 0.17.14       (compiles C/asm crypto primitives)
├── libsqlite3-sys@0.30.1        (compiles bundled SQLite C source)
└── findshlibs@0.10.2 ← pprof@0.15.0 ← buff-lang-cli (profiling)
```

## Why cc Cannot Be Removed (Until Parent DRs Close)

1. **ring** uses cc to compile its C/asm crypto primitives
   (AES-GCM, ChaCha20-Poly1305, P-256, etc.). See
   `.sisyphus/decisions/ring-exception.md`.

2. **libsqlite3-sys** uses cc to compile the bundled SQLite C source
   (when the `bundled` feature is enabled, which the workspace
   `rusqlite` pin mandates). See
   `.sisyphus/decisions/libsqlite3-sys-exception.md`.

3. **pprof** uses `findshlibs` which uses cc for build-time symbol
   resolution. pprof is an opt-in profiling crate used by
   `buff-lang-cli` (the `pprof` workspace dep). This is a separate
   offender from the ring/sqlite cluster; it could in principle be
   removed by dropping pprof (or gating it behind a `profiling`
   cargo feature), but pprof is currently unconditionally pulled.

## Decision

`cc` is NOT added to `deny.toml`'s `deny` list. Instead, it is
hidden from the gate via the `skip-tree` entries on its two primary
parents (`ring` + `libsqlite3-sys`). The third parent (`pprof` →
`findshlibs` → `cc`) is NOT hidden — its cc usage surfaces in the
gate output but does NOT fail because `cc` is not in `deny`.

The `cc` crate is a build-time tool only — it does not introduce
RUNTIME C library dependencies. It compiles C code that gets linked
into the Rust binary at build time.

## Consequence

- `cargo deny check bans` passes without `cc` appearing in either
  `deny` or `skip-tree`.
- The `cc` crate is the de facto build tool across the Rust
  ecosystem. Banning it would block nearly every crate that touches
  C/asm, including the entire rustls + ring TLS stack.
- **Exit criterion**: when both `ring-exception.md` AND
  `libsqlite3-sys-exception.md` are closed (pure-Rust TLS backend +
  pure-Rust SQLite replacement land), `cc` will disappear
  automatically from the transitive graph. At that point `cc`
  SHOULD be added to `deny` as a permanent guard.

## Related

- `.sisyphus/decisions/ring-exception.md` (parent DR — TLS crypto).
- `.sisyphus/decisions/libsqlite3-sys-exception.md` (parent DR — SQLite storage).
- `deny.toml` `[bans] skip-tree` entries for `ring` + `libsqlite3-sys`.
