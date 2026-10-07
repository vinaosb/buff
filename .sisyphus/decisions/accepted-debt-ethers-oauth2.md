# ADR: Accepted Debt — ethers-rs 3.x / oauth2 5.x Upgrades (h2 0.3 removal)

- **Status:** Accepted (debt-zero W4, todo 26 — formally deferred)
- **Date:** 2026-10-07
- **Supersedes:** the informal "tracked tech debt" note in `audit.toml`
  (RUSTSEC-2026-0258 comment block)

## Context

`cargo audit` reports **RUSTSEC-2026-0258** (h2 memory exhaustion /
stream handling) against the workspace's **h2 0.3.27** instance. There is
NO 0.3.x patch — the fixed line is h2 **≥ 0.4.16** (the workspace's other,
directly-controlled h2 0.4 instance is already patched).

The 0.3 line is reachable ONLY through the legacy hyper-0.14 chain:

```
buff-web3 ──▶ ethers 2 ──▶ reqwest 0.11 ──▶ hyper 0.14 ──▶ h2 0.3
oauth2 4  ──────────────────▶ reqwest 0.11 ──▶ hyper 0.14 ──▶ h2 0.3
httpmock 0.7 ───────────────▶ hyper 0.14 ────▶ h2 0.3
```

Removing h2 0.3 therefore requires **ethers 2 → 3+** and **oauth2 4 → 5**
upgrades. Both are breaking, multi-day ecosystem migrations:

- `ethers-rs` 3.x is a substantial API break (providers, middleware,
  contract bindings), and the crate family has itself been deprecated in
  favor of `alloy` — the "real" fix may be a `buff-web3` rewrite onto
  alloy, not an ethers bump.
- `oauth2` 5.x changes the client/flow types consumed by the registry's
  OAuth code path.
- `httpmock` 0.7 → a hyper-0.14-free test double would additionally be
  needed to fully drop the chain from dev builds.

## Decision

**Defer** the ethers-3 / oauth2-5 upgrades as accepted technical debt.
`RUSTSEC-2026-0258` stays in `audit.toml`'s ignore list until the chain
is removed. This is fork-default #4 from the debt-zero plan review (the
user did not override).

## Alternatives considered

1. **Upgrade ethers + oauth2 now** — rejected: multi-day migration with
   real regression risk in `buff-web3` and `buff-registry` OAuth, for a
   vulnerability whose exploitability in this workspace is low (the h2
   0.3 instance is reached only through legacy HTTP/2 paths of the
   ethers provider; no production service fronts it).
2. **Rewrite `buff-web3` onto `alloy` now** — rejected for the same
   scope reasons; recorded as the preferred shape for the eventual fix.
3. **Fork/patch h2 0.3** — rejected: unmaintainable, and upstream
   explicitly ships no 0.3 patch.
4. **Ignore the advisory indefinitely** — rejected: we keep an explicit
   revisit trigger (below) rather than silent acceptance.

## Consequences

- `cargo audit` remains green on CI (security.yml) with the documented
  ignore — the ignore is the tracked, commented exception, not an
  accident.
- h2 0.3 remains in `Cargo.lock` until the migration lands.
- The `windows-sys`/`hashbrown`-style duplicate-chain acceptance in root
  `Cargo.toml` cross-references this file for the h2-specific chain.

## Revisit trigger

Re-evaluate (and then execute) the migration when EITHER:

1. a **RUSTSEC advisory** lands against h2 0.3 with worse severity or
   practical exploitability, OR
2. **either parent crate ships a major bump** we would take anyway
   (ethers/alloy ecosystem move, oauth2 5 adoption by a new dependency)
   — i.e. when the migration becomes incremental rather than
   standalone.
