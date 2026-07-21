# buff-audit

Security scanner + code signing for the Buff language. Pure-Rust MVP (no `ring`, no native-tls, no cc-rs). Wraps [`ed25519-dalek`](https://crates.io/crates/ed25519-dalek) 2.0 for detached Ed25519 signatures + a statically-seeded advisory database for CVE scanning.

**Status: experimental** (T26 v1.13 frameworks wave 3).

## STRUCTURE

```
buff-audit/
├── Cargo.toml            # ed25519-dalek + sha2 + hex + rand + thiserror + insta deps
├── src/
│   ├── lib.rs            # keypair / sign / verify / scan / scan_with_detail + AdvisoryHit + advisory_db (~330 LOC)
│   └── error.rs          # AuditError enum (~70 LOC)
├── examples/
│   ├── sign.rs           # full Ed25519 sign/verify roundtrip
│   ├── scan.rs           # scan a fake project for vulnerable chrono
│   └── audit/
│       ├── sign.buff     # Buff-side forward-decl (matches sign.rs)
│       └── scan.buff     # Buff-side forward-decl (matches scan.rs)
└── tests/
    ├── api.rs            # 13 integration tests (public surface)
    └── sign_verify.rs    # 9 roundtrip / negative tests (sign + verify)
```

Total: ~550 LOC (well under the 2000 LOC T26 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new advisory entry | `src/lib.rs::advisory_db::ALL` (add `Advisory { id, package, vuln_version, patched }`) |
| Add a new sign/verify mode | `src/lib.rs` (add `pub fn`) + test in `tests/sign_verify.rs` |
| Add a new error variant | `src/error.rs` + `From` impl if it wraps an underlying error |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (`PreludeAssocFn` + `assoc_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` |

## PUBLIC API (8 functions, ≤15 cap)

### Top-level functions
- `keypair() -> Result<(String, String), AuditError>` — fresh Ed25519 CSPRNG keypair. Returns `(public_hex, secret_hex)`.
- `sign(data: &[u8], secret_hex: &str) -> Result<String, AuditError>` — detached signature (128-char hex).
- `verify(data: &[u8], sig_hex: &str, public_hex: &str) -> Result<bool, AuditError>` — strict verify. Returns `Ok(false)` on signature failure (NOT an error).
- `scan<P: AsRef<Path>>(root: P) -> Result<Vec<String>, AuditError>` — manifest substring scan.
- `scan_with_detail<P: AsRef<Path>>(root: P) -> Result<Vec<AdvisoryHit>, AuditError>` — same but with `package` + `patched` fields.
- `known_advisories() -> Vec<String>` — static DB enumeration.

### Types
- `AdvisoryHit { id, package, patched }` — owned, `Send + Sync + Eq + Hash`.
- `AuditError` — BadSignature / BadKey / Hex / Io / Panic.
- `MANIFEST_PATHS: &["buff.lock", "Cargo.lock", "buff.toml"]` — scan priority chain.

## CONVENTIONS

- **Pure-Rust only**: ed25519-dalek's `default-features = false` + `features = ["rand_core", "zeroize"]`. NO `fast` (nightly SIMD), NO `asm` (cc-rs). Matches the "no C library, no Docker" hard rule.
- **No `ring`, no native-tls**: the T26 task spec explicitly forbids both (`ring` requires `vcruntime.h` on Windows MSVC; native-tls pulls OpenSSL/SChannel). ed25519-dalek is the canonical pure-Rust Ed25519.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. All fallible ops return `Result<_, AuditError>`.
- **catch_unwind boundary**: `keypair` / `sign` / `verify` / `scan` / `scan_with_detail` wrap their bodies in `catch_unwind` per FFI guide R6.
- **Deterministic signatures**: Ed25519 is deterministic (RFC 8032) — same key + same data ⇒ same signature. The `sign_verify::signatures_are_detached_and_deterministic_per_key` test asserts this.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `ed25519-dalek` | Upstream Ed25519 provider. `buff-audit` is a safe wrapper; never re-exports `ed25519_dalek::*` types directly. |
| `sha2` | Already pinned at workspace level (T124k). T26 reuses that pin for the manifest-hashing path (deferred — MVP uses substring scan). |
| `hex` | Already pinned at workspace level (T124h). T26 reuses that pin for sig/key encode/decode. |
| `rand` | Already pinned at workspace level (T124f). T26 reuses that pin for `rand::rngs::OsRng` consumed by `SigningKey::generate`. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Audit` + `PreludeType::Signature` (both namespace-only — return `Type::Void`). `PreludeAssocFn::{Scan, List, Sign, Verify, Keypair}` dispatched on the matching `(type, method)` pair. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::lower_prelude_type_assoc_fn` has `(Audit, Scan)` / `(Audit, List)` / `(Signature, Sign)` / `(Signature, Verify)` / `(Signature, Keypair)` arms. `program_uses_namespace("Audit")` / `program_uses_namespace("Signature")` records `buff-audit` + `ed25519-dalek` + `sha2` + `hex` + `rand` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **MSVC host blocker**: `cargo test -p buff-audit` fails on this Windows host with the same `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` issue that blocks `cargo check --workspace` here. CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. `cargo check -p buff-audit --lib` and `cargo clippy -p buff-audit --all-targets -- -D warnings` both pass clean on a host with the Windows SDK installed.
- **Advisory DB is static MVP**: the seeded DB has 1 entry (`RUSTSEC-2020-0159` for chrono 0.4.19). Production usage should grow this via PRs (mirrors the RustSec advisory-db workflow). Live `rustsec` crate integration is deferred to v1.18+ (it pulls `crates-index` which needs git History — too heavy for the MVP).
- **Code signing is opt-in for v1.13-v1.17**: the T26 task spec mandates that signature verification is opt-in until v1.18+. `Signature.verify` returns `Ok(false)` (NEVER panics, NEVER errors) on signature failure so a future `buff add --no-verify` bypass can layer cleanly on top.
- **No `--fix` mode yet**: the T26 plan describes `buff audit --fix` to auto-upgrade patched versions; the MVP ships scan-only. The `--fix` mode requires a Cargo.toml rewriter that doesn't break workspace dep resolution — deferred to a follow-up task.
- **Ed25519-only for v1.x**: RSA / ECDSA / P-256 are deferred (ed25519-dalek covers the canonical 90% use-case for supply-chain signing — small keys, fast sign/verify, deterministic sigs).
