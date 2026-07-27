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

## Mitigations Investigated

### `jsonwebtoken` `rust_crypto` feature (REJECTED)

The long-standing doc comment on the workspace `jsonwebtoken = "9"`
pin (Cargo.toml ~line 1272) and `crates/buff-auth/{Cargo.toml,AGENTS.md,
README.md,src/jwt.rs,src/lib.rs}` claim jsonwebtoken 9 ships a
`rust_crypto` cargo feature that swaps ring for RustCrypto sha2/hmac.

**This is a documentation error.** Verified 2026-07-27 via
`cargo tree -i ring --workspace --locked` + the cargo resolver error
`package buff-auth depends on jsonwebtoken with feature rust_crypto
but jsonwebtoken does not have that feature. available features:
default, pem, simple_asn1, use_pem`:

- jsonwebtoken 9.3.1 has NO `rust_crypto` cargo feature.
- jsonwebtoken 9.x unconditionally depends on `ring 0.17.x`.
- The RustCrypto integration was proposed in jsonwebtoken issue
  trackers but never landed in the 9.x line.

The doc comments claiming otherwise across `buff-auth` are tracked as
a separate follow-up to correct. The workspace pin stays at the
plain `jsonwebtoken = "9"` (no features).

### `rustls` crypto-provider swap (REJECTED)

rustls 0.23+ supports pluggable crypto providers (`ring` or
`aws-lc-rs`). Both use cc-rs to compile C/asm primitives — there is
no FIPS-adjacent pure-Rust provider. Swapping `ring` for `aws-lc-rs`
would not change the cc-rs situation (both fail the "no C library"
rule equally).

## Remaining Sources of ring

- `rustls v0.21.12` / `v0.22.4` / `v0.23.42` — pulled transitively by
  every HTTP-touching crate in the workspace (reqwest, hyper-rustls,
  tokio-rustls, lettre, sqlx-core).
- `jsonwebtoken v9.3.1` — pulled by `buff-auth`.
- `jsonwebtoken v8.3.0` (pulls the legacy `ring@0.16.20` duplicate) —
  pulled by `ethers v2.0.14` ← `buff-web3`. Bumping ethers to a
  hypothetical v3 that uses jsonwebtoken v9 is upstream-blocked.

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
