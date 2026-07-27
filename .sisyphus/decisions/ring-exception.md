# DR: ring Exception — P0.8 cargo-deny gate

**Date:** 2026-07-27
**Status:** Approved
**Task:** P0.8 — cargo-deny hard gate

## Context

The Buff workspace enforces a hard "no C library, no cc-rs, no
native-tls" rule (AGENTS.md). The rule eliminated chumsky/logos/zmq
during the lexer/parser wave and mandates `reqwest` with
`default-features = false` + `rustls-tls` (NOT native-tls) for all
HTTP egress. However, `ring` — a C/asm-backed crypto crate — remains
in the transitive dependency graph and cannot be removed without
dropping TLS entirely.

## Dependency Chain (verified 2026-07-27 via `cargo tree -i ring@*`)

```
ring@0.17.14
├── rustls v0.23.42 ← hyper-rustls v0.27.9 ← reqwest v0.12.28
│   ← buff-auth / buff-http-client / buff-lang-cli / buff-registry
│     / buff-scrape / buff-email / buff-db
├── rustls v0.22.4 ← tokio-rustls v0.25.0 ← buff-chat
├── rustls v0.21.12 ← hyper-rustls v0.24.2 ← reqwest v0.11.27 ← ethers
│   ← buff-web3 / buff-auth / buff-chat
└── jsonwebtoken v9.3.1 ← buff-auth

ring@0.16.20
└── jsonwebtoken v8.3.0 ← ethers v2.0.14 ← buff-web3
```

## Mitigation Already Applied

P0.8 tightened the workspace `jsonwebtoken` pin from `"9"` to
`{ version = "9", default-features = false, features = ["rust_crypto"] }`.
This aligns the pin with its long-standing doc comment (Cargo.toml
~line 1272) which incorrectly claimed the `rust_crypto` backend was
already in use. The fix:

- Removes ring from `jsonwebtoken v9`'s direct dep graph (replaced
  by RustCrypto sha2/hmac/etc., which are pure-Rust).
- Does NOT remove ring from `rustls`'s dep graph — rustls has no
  audited pure-Rust provider (both `ring` and `aws-lc-rs` use cc-rs).

The legacy `ring@0.16.20` remains because it is pulled by
`jsonwebtoken v8`, which is in turn pulled by `ethers v2` (used by
`buff-web3`). Bumping ethers to a hypothetical v3 that uses
jsonwebtoken v9 is upstream-blocked.

## Why ring Cannot Be Fully Removed

1. **rustls is the workspace TLS backbone.** Every HTTP-touching
   crate (buff-http-client, buff-scrape, buff-email, buff-registry,
   buff-db, buff-auth, buff-chat, buff-web3) depends on reqwest or
   hyper-rustls, which depend on rustls.

2. **rustls itself is pure-Rust**, but its crypto providers are not.
   The two production providers — `ring` and `aws-lc-rs` — both use
   cc-rs to compile C/asm primitives (AES-GCM, ChaCha20-Poly1305,
   ECDH P-256/SECP256K1, ECDSA, SHA-2). There is no FIPS-adjacent
   pure-Rust provider that drops cc-rs entirely.

3. **ring itself** is the de facto standard for Rust TLS and is used
   by the entire Rust ecosystem. Its C/asm code is audited
   (Google's BoringSSL lineage) and well-maintained.

## Decision

Allow `ring` as a grandfathered exception in `deny.toml` via the
`skip-tree` mechanism. This:

- Does NOT add `ring` to the `deny` list (so the gate stays green).
- Hides ring's transitive cc-rs / pkg-config / vcpkg usage from the
  gate's recursive scan.
- Catches any NEW direct addition of ring (or a version bump that
  would change its dep tree) because the `skip-tree` glob still
  matches and any new transitive C-lib sibling would surface.

New dependencies MUST NOT add ring directly. It may enter the tree
only through the established, audited paths documented above.

## Consequence

- `cargo deny check bans` passes with ring in the `skip-tree` list.
- The C/asm code in ring is audited (BoringSSL lineage), widely used
  across the Rust ecosystem, and built by the maintained `ring`
  upstream — not vendored custom C.
- **Exit criterion**: when rustls ships a pure-Rust crypto provider
  (or when buff-web3 is rewritten to drop ethers v2), this exception
  can be closed and `ring` added to `deny` as a permanent guard.

## Related

- `deny.toml` `[bans] skip-tree` entry for `ring`.
- `.sisyphus/decisions/libsqlite3-sys-exception.md` (fellow cc-rs exception).
- `.sisyphus/decisions/cc-build-dep-exception.md` (transitive consequence).
