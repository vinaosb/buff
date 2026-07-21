# buff-audit

> Security scanner + code signing for the **Buff** language. Pure-Rust MVP (no `ring`, no native-tls).

`buff-audit` wraps [`ed25519-dalek`](https://crates.io/crates/ed25519-dalek) 2.0 for detached Ed25519 signatures + a statically-seeded advisory database for CVE scanning. It follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md) at every public boundary.

**Status: experimental** (T26 v1.13 frameworks wave 3).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Audit` or `Signature` prelude type.

For direct Rust use:

```bash
cargo add buff-audit --path crates/buff-audit
```

## Quick start

### Sign + verify a payload

```rust
use buff_audit::{keypair, sign, verify};

fn main() {
    let (public_hex, secret_hex) = keypair().expect("keypair");
    let sig = sign(b"hello, audit", &secret_hex).expect("sign");
    let ok = verify(b"hello, audit", &sig, &public_hex).expect("verify");
    assert!(ok);
}
```

### Scan a project for vulnerable deps

```rust
use buff_audit::scan;

fn main() {
    let hits = scan(".").expect("scan");
    for id in &hits {
        println!("advisory: {}", id);
    }
}
```

## Public API

### Sign / verify

| Function | Signature | Notes |
|---|---|---|
| `keypair` | `() -> Result<(String, String), AuditError>` | Fresh Ed25519 CSPRNG keypair. `(public_hex, secret_hex)` — 64 hex chars each. |
| `sign` | `(&[u8], &str) -> Result<String, AuditError>` | Detached 64-byte signature (128-char hex). Deterministic per RFC 8032. |
| `verify` | `(&[u8], &str, &str) -> Result<bool, AuditError>` | Strict Ed25519 verify. Returns `Ok(false)` on bad signature (NOT an error). |

### Scan

| Function | Signature | Notes |
|---|---|---|
| `scan` | `(impl AsRef<Path>) -> Result<Vec<String>, AuditError>` | Substring scan of `buff.lock` / `Cargo.lock` / `buff.toml`. |
| `scan_with_detail` | `(impl AsRef<Path>) -> Result<Vec<AdvisoryHit>, AuditError>` | Same + remediation hint per hit. |
| `known_advisories` | `() -> Vec<String>` | Enumerate the static DB. |

### Types

| Type | Notes |
|---|---|
| `AdvisoryHit { id, package, patched }` | Owned, `Send + Sync + Eq + Hash`. |
| `AuditError` | `BadSignature` / `BadKey` / `Hex` / `Io` / `Panic`. |
| `MANIFEST_PATHS` | `&["buff.lock", "Cargo.lock", "buff.toml"]` — scan priority chain. |

## Advisory DB

The MVP ships with a statically-seeded DB of 1 entry:

| ID | Package | Vuln Version | Patched |
|---|---|---|---|
| `RUSTSEC-2020-0159` | `chrono` | `0.4.19` | `0.4.20` |

Production usage should grow this via PRs (mirrors the [RustSec advisory-db](https://github.com/rustsec/advisory-db) workflow). Live `rustsec` crate integration is deferred to v1.18+.

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md):

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `String`, `Vec<String>`, `bool`, `(String, String)`, `AdvisoryHit`. No `*const`/`*mut`. |
| R2 — Ownership boundary | Every fn returns owned data; `SigningKey` / `VerifyingKey` are dropped at the boundary. |
| R3 — Error mapping | Every fallible op returns `Result<T, AuditError>`. `ed25519_dalek::SignatureError` + `hex::FromHexError` auto-convert. |
| R4 — Thread safety | Every type is `Send + Sync` (ed25519-dalek keys are themselves `Send + Sync`). |
| R5 — Lifetime hiding | No public lifetime parameters. Hex strings own their bytes. |
| R6 — Panic boundary | `keypair` / `sign` / `verify` / `scan` / `scan_with_detail` wrap bodies in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-audit
cargo clippy -p buff-audit --all-targets -- -D warnings
cargo fmt -p buff-audit --check
```

Tests are hermetic: temporary project roots are created inline via `std::env::temp_dir()` (no fixtures needed). 22 tests total (13 API + 9 sign/verify roundtrip).

## Limitations (v1.x MVP)

- **Ed25519 only**: RSA / ECDSA / P-256 deferred. Ed25519 covers the canonical supply-chain use-case (small keys, fast, deterministic).
- **Static advisory DB**: live `rustsec` crate integration deferred to v1.18+ (it pulls `crates-index` which is too heavy for MVP).
- **Scan-only**: `buff audit --fix` auto-upgrade mode is NOT implemented. Requires a Cargo.toml rewriter that doesn't break workspace dep resolution.
- **No sigstore**: T26 plan mentions sigstore/cosign integration; deferred (requires network + a `sigstore-rs` crate that pulls `tokio` + `reqwest` — too heavy for MVP).

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
