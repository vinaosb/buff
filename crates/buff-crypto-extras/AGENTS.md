# buff-crypto-extras

AES-GCM / RSA / ECDH (P-256/P-384) / Argon2 KDF for the **Buff** language. Pure-Rust MVP (CPU-only) wrapping RustCrypto crates ([`aes-gcm`](https://docs.rs/aes-gcm), [`rsa`](https://docs.rs/rsa), [`p256`](https://docs.rs/p256), [`p384`](https://docs.rs/p384), [`argon2`](https://docs.rs/argon2)) behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T49 v1.18 frameworks wave 4).

## STRUCTURE

```
buff-crypto-extras/
├── Cargo.toml           # aes-gcm + rsa + p256 + p384 + argon2 + sha2 + rand + hex +
│                        # signature + thiserror + insta deps (all workspace-pinned)
├── AGENTS.md            # this file
├── src/
│   ├── lib.rs           # 4 namespace re-exports (aes_gcm_api / rsa_api / ecdh_api /
│                        # argon2_api) + CryptoError re-export (~72 LOC)
│   ├── aes.rs           # AES-256-GCM: generate_key / generate_nonce / encrypt / decrypt
│   │                    # (140 LOC — KEY_LEN=32, NONCE_LEN=12, TAG_LEN=16 constants)
│   ├── rsa.rs           # RSA PKCS#1 v1.5 SHA-256: generate_keypair / sign / verify +
│   │                    # RsaKeypair struct (~124 LOC — MIN_BITS=2048 floor)
│   ├── ecc.rs           # ECDH P-256 + P-384: p256_generate_private /
│   │                    # p256_public_from_private / p256_derive_shared / p384_generate_private
│   │                    # (139 LOC — P256_PRIVATE_LEN=32, P256_PUBLIC_LEN=65, P256_SHARED_LEN=32)
│   ├── argon2.rs        # Argon2id raw KDF: generate_salt / derive_key (75 LOC —
│   │                    # OWASP defaults: m=19456 KiB, t=2, p=1, output=32B, salt=16B)
│   └── error.rs         # CryptoError enum + From impls for aes_gcm::aead::Error /
                         # rsa::errors::Error / rsa::pkcs8::Error / argon2::Error (99 LOC)
```

Total: ~650 LOC (well under the 2500 LOC T49 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new top-level helper | `src/lib.rs` (add `pub mod <name>_api { ... }`) |
| Add a new AES mode | `src/aes.rs` (add `pub fn`) — raw AES-ECB / AES-CBC are FORBIDDEN per T49 spec |
| Add a new RSA operation | `src/rsa.rs` (add `pub fn`) — RSAES encryption is FORBIDDEN per T49 spec |
| Add a new curve | `src/ecc.rs` (add `pub fn p<NNN>_*`) |
| Add a new error variant | `src/error.rs` + `From` impl if it wraps an underlying RustCrypto error |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (`PreludeType::{AES, RSA, ECDH, Argon2, RsaKeypair}` + `PreludeAssocFn::*`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` |

## PUBLIC API (12 functions + 1 type, ≤20 cap)

### `aes_gcm_api` (4 functions)
- `generate_key() -> Vec<u8>` — 32-byte AES-256 key via `OsRng`
- `generate_nonce() -> Vec<u8>` — 12-byte GCM nonce via `OsRng`
- `encrypt(key: &[u8], nonce: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError>` — returns `ciphertext || 16-byte GCM tag`
- `decrypt(key: &[u8], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError>` — verifies tag before returning plaintext

### `rsa_api` (3 functions + 1 type)
- `generate_keypair(bits: usize) -> Result<RsaKeypair, CryptoError>` — min 2048 bits; returns Spki + PKCS#8 PEM strings
- `sign(private_pem: &str, data: &[u8]) -> Result<Vec<u8>, CryptoError>` — PKCS#1 v1.5 SHA-256; signature length = modulus bytes
- `verify(public_pem: &str, data: &[u8], signature: &[u8]) -> bool` — false on ANY failure (signature mismatch, malformed PEM, panic)
- `RsaKeypair { public_pem: String, private_pem: String }` — `Debug + Clone + PartialEq + Eq + Default + Send + Sync`

### `ecdh_api` (4 functions)
- `p256_generate_private() -> Vec<u8>` — 32-byte P-256 scalar
- `p256_public_from_private(private: &[u8]) -> Result<Vec<u8>, CryptoError>` — 65-byte SEC1 uncompressed point
- `p256_derive_shared(private: &[u8], public: &[u8]) -> Result<Vec<u8>, CryptoError>` — 32-byte shared secret (x-coord)
- `p384_generate_private() -> Vec<u8>` — 48-byte P-384 scalar (P-384 derive is deferred to v1.20+)

### `argon2_api` (2 functions)
- `generate_salt() -> Vec<u8>` — 16-byte CSPRNG salt
- `derive_key(password: &str, salt: &[u8]) -> Result<Vec<u8>, CryptoError>` — 32-byte AES-256 key

## CONVENTIONS

- **Pure-Rust only**: NO `ring`, NO `native-tls`, NO `cc-rs`. All deps are RustCrypto (or pure-Rust wrappers thereof) — matches the "no C library" hard rule from AGENTS.md. The workspace pins are at the root `[workspace.dependencies]` with T-numbered rationale.
- **CPU-only**: NO GPU dispatch. AEAD / signatures / ECDH / KDF never run on the GPU path (Metis G7 lock — single-threaded CPU is more than sufficient for crypto ops which are not data-parallel).
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` / `todo!` in non-test code. Every public fn wraps its body in `catch_unwind` per FFI guide R6 (panic → `CryptoError::Panic` or `false` for verify).
- **Default trait on `RsaKeypair`**: derived so the codegen-lowered `RSA.generate_keypair(bits)` call can collapse `Result<RsaKeypair, CryptoError>` to a default on failure via `.unwrap_or_default()` — matching Buff's "no panicking generated code" rule (mirrors T48 buff_web3::Wallet + buff_web3::Provider's Default-impl precedent). The default value (empty PEM strings) is a malformed key — NEVER use in production; it exists solely as a panic-free failure fallback.
- **Wire format**: AES-GCM ciphertext = `plaintext || 16-byte GCM tag` (matches OpenSSL / pycryptodome / BouncyCastle). RSA PEM = Spki (public) + PKCS#8 (private) — same shape `openssl genrsa` / `openssl rsa -pubout` produce. ECDH public = SEC1 uncompressed (`0x04 || X || Y`). Argon2 output = raw 32 bytes (NOT a PHC string — distinct from T34 Password.hash).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `aes-gcm` + `rsa` + `p256` + `p384` + `argon2` + `sha2` + `signature` + `rand` | Upstream RustCrypto providers. `buff-crypto-extras` is a safe wrapper; never re-exports `aes_gcm::*` / `rsa::*` / `p256::*` / `argon2::*` types directly. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::{AES, RSA, ECDH, Argon2, RsaKeypair}` + 12 `PreludeAssocFn` variants + 2 `PreludeInstanceFn` variants (`PublicPem` / `PrivatePem`). `ty.rs` has the matching `Type::{AES, RSA, ECDH, Argon2, RsaKeypair}` variants + `is_prelude_*` predicates. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::lower_prelude_type_assoc_fn` has the 12 `(AES/RSA/ECDH/Argon2, *)` arms. `lower_prelude_type_instance_fn` has the 2 RsaKeypair arms. `program_uses_namespace("AES" | "RSA" | "ECDH" | "Argon2" | "RsaKeypair")` records `buff-crypto-extras` + 8 RustCrypto crates in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |
| `buff-auth` (T34 sibling) | T34 uses the `argon2` crate for PHC-string Password hashing (`Password.hash`); T49 uses the same crate for raw Argon2id KDF bytes (`Argon2.derive_key`). Distinct purposes, same upstream — recorded explicitly in both crate's Cargo.toml rationale. |
| `buff-msgpack` (T51 sibling) | Closest structural analog — same namespace-only MVP shape (4 namespace-only types + 1 instance type). |

## NOTES

- **No RSA encryption in MVP**: the T49 spec scopes RSA to signatures (sign / verify / generate_keypair). RSAES-PKCS1-v1_5 / RSAES-OAEP encryption are deliberately NOT exposed — for public-key encryption use hybrid AES-GCM + ECDH (the user-facing pattern is `let shared = ECDH.derive_shared(sk, pk); let key = Argon2.derive_key(password, shared); let ct = AES.encrypt(key, nonce, pt);`).
- **No P-384 derive in MVP**: `p384_generate_private` is exposed (returns 48-byte scalar) but `p384_public_from_private` / `p384_derive_shared` are deferred to v1.20+. The Buff surface today uses only P-256 (32 / 65 / 32 byte lengths). P-384 is the next-most-supported curve cross-language.
- **No RSA-PSS in MVP**: PKCS#1 v1.5 is the deterministic baseline with widest cross-language support (OpenSSL / pycryptodome / BouncyCastle / System.Security.Cryptography all ship it in stdlib). RSA-PSS would also be acceptable but is left for a follow-up.
- **No streaming AEAD in MVP**: `AES.encrypt` / `AES.decrypt` are one-shot (full plaintext / ciphertext in memory). Streaming AEAD (for files / sockets) is deferred to v1.20+ via `aes_gcm::aead::AeadInPlace`.
- **MSVC host blocker**: `cargo test -p buff-crypto-extras` may fail on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` (pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue — same family that blocks `cargo check --workspace` here). CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. The crate's library `cargo check -p buff-crypto-extras --lib` and `cargo clippy -p buff-crypto-extras --all-targets -- -D warnings` both pass clean.
- **Send + Sync**: all public types are `Send + Sync`. `RsaKeypair` owns only `String`s. `CryptoError` derives `Error` (thiserror) and is `Send + Sync` by construction.
- **Cross-language interop**: AES-GCM wire format (ciphertext || tag), RSA PEM (Spki + PKCS#8), ECDH SEC1 uncompressed points, and Argon2id raw bytes are all 6/6 compatible with the task spec's cross-language targets (pycryptodome / BouncyCastle / System.Security.Cryptography / openssl / ring / crypto++).

## DEFERRED (per T49 spec + v1.20+ roadmap)

- **Streaming AEAD**: `AES.encrypt_stream(reader, writer, key, nonce)` / `AES.decrypt_stream(...)`. v1.20+.
- **RSA-PSS signature scheme**: PKCS#1 v1.5 only in MVP. v1.20+.
- **RSAES-OAEP encryption**: forbidden per T49 spec (use hybrid AES-GCM + ECDH). NOT planned.
- **P-384 ECDH derive**: `p384_public_from_private` / `p384_derive_shared`. v1.20+.
- **X25519 / Ed25519**: not in scope for T49 (covered by T26 Signature for Ed25519; X25519 deferred to v1.20+).
- **Key wrapping (RFC 5649)**: AES-KW / AES-KWP. v1.20+.
- **HKDF**: `ECDH.derive_shared` returns raw shared secret; users feed it to `Argon2.derive_key` for KDF. A dedicated `HKDF.expand` namespace is v1.20+.
- **Constant-time comparison helpers**: not exposed in MVP (users compare `Bool` return of `RSA.verify` which is already constant-time inside the `rsa` crate). A `ConstantTime.eq(a, b)` namespace is v1.20+.

## Testing

```bash
cargo test -p buff-crypto-extras
cargo clippy -p buff-crypto-extras --all-targets -- -D warnings
cargo fmt -p buff-crypto-extras --check
```

Tests live in `tests/` (per-crate integration tests, NOT inline `#[cfg(test)]` in `src/*.rs`):
- `tests/aes.rs` — AES-256-GCM KATs (McGrew-Viega / NIST SP 800-38D Test Case 13, `ring` `aead_aes_256_gcm_tests.txt`) + round-trip + tamper/auth-failure + InvalidLength coverage.
- `tests/rsa.rs` — RSA PKCS#1 v1.5 SHA-256 round-trip + wrong-key + tampered-signature + malformed-PEM + MIN_BITS enforcement.
- `tests/ecc.rs` — P-256 ECDH: RFC 6979 A.2.5 public-key-derivation KAT + Diffie-Hellman symmetry (NIST SP 800-56A §5.7.1.2) + P-384 length check + malformed-input robustness.
- `tests/argon2.rs` — Argon2id determinism + password/salt avalanche + length validation + AES-GCM hybrid-pattern composition. NOTE: RFC 9106 §4 raw-output vectors use lighter params (m=32, t=3, p=4) than this crate's pinned OWASP defaults (m=19456, t=2, p=1) and so cannot be reproduced byte-for-byte through the public API; the determinism + differentiation tests are the KAT-equivalent for the fixed-parameter surface.

NO insta snapshots (the cryptographic output is randomized per-call via OsRng — byte-level snapshots would be non-deterministic).

Codegen integration tests live in `crates/buff-lang-codegen-rust/tests/crypto_extras_codegen.rs` — they verify the Rust lowering shape (NOT the cryptographic correctness, which the per-crate unit tests cover).

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
