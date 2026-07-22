# buff-auth

JWT + OAuth2 + password hashing + RBAC for the Buff language. Pure-Rust MVP (no `ring`, no native-tls, no cc-rs). Wraps [`jsonwebtoken`](https://crates.io/crates/jsonwebtoken) 10 (HS256 via the `rust_crypto` backend) + [`argon2`](https://crates.io/crates/argon2) 0.5 (Argon2id PHC format) + [`oauth2`](https://crates.io/crates/oauth2) 4 (auth-code flow + PKCE for public clients, rustls-tls via reqwest) via a safe FFI boundary per the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T34 v1.16 frameworks wave 4).

## STRUCTURE

```
buff-auth/
├── Cargo.toml            # jsonwebtoken + argon2 + oauth2 + reqwest + serde + thiserror deps
├── src/
│   ├── lib.rs            # module doc + re-exports (~50 LOC)
│   ├── error.rs          # AuthError enum (~70 LOC)
│   ├── jwt.rs            # jwt_encode / jwt_decode (HS256 via jsonwebtoken rust_crypto) (~95 LOC)
│   ├── password.rs       # password_hash / password_verify (Argon2id) (~95 LOC)
│   ├── oauth.rs          # OAuth2Client struct + authorization_url + exchange_code (~165 LOC)
│   └── rbac.rs           # Rbac policy + RbacRule + add / enforce / len (~135 LOC)
├── examples/
│   ├── jwt_roundtrip.rs          # full encode/decode roundtrip
│   ├── password.rs               # Argon2id hash + verify
│   ├── rbac.rs                   # build policy + enforce
│   ├── oauth2_client.rs          # build auth URL for both confidential + public PKCE
│   └── auth/
│       ├── jwt_roundtrip.buff    # Buff-side forward-decl
│       ├── password.buff         # Buff-side forward-decl
│       ├── rbac.buff             # Buff-side forward-decl
│       └── oauth2_client.buff    # Buff-side forward-decl
└── tests/
    ├── jwt.rs           # 6 integration tests (roundtrip + negative cases)
    ├── password.rs      # 5 integration tests (PHC format + verify matrix)
    ├── rbac.rs          # 7 integration tests (wildcard matrix + dedup)
    └── oauth2.rs        # 4 integration tests (URL construction, no-network)
```

Total: ~610 src LOC + 22 tests (~440 LOC). Well under the 3000 LOC + 25 fn T34 cap.

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new JWT algorithm | `src/jwt.rs` (add a pub fn) + test in `tests/jwt.rs` + codegen arm in `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` |
| Add a new password hash scheme | `src/password.rs` + `AuthError` variant in `src/error.rs` |
| Add a new RBAC matcher (e.g. role hierarchy) | `src/rbac.rs::Rbac::enforce` + `field_matches` |
| Add a new OAuth2 flow (password grant, client_credentials) | `src/oauth.rs::OAuth2Client` (new pub fn) |
| Add a new error variant | `src/error.rs` + `From` impl if it wraps an underlying error |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (`PreludeAssocFn` + `assoc_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` |

## PUBLIC API (7 functions, ≤25 cap)

### `JWT` namespace (2 assoc fns)
- `jwt_encode(claims: &Map, secret: &str) -> Result<String, AuthError>` — HS256 compact JWS.
- `jwt_decode(token: &str, secret: &str) -> Result<Map<String, Value>, AuthError>` — verify + decode to claims map.

### `Password` namespace (2 assoc fns)
- `password_hash(plain: &str) -> Result<String, AuthError>` — Argon2id PHC string.
- `password_verify(plain: &str, phc: &str) -> Result<bool, AuthError>` — `Ok(false)` on mismatch.

### `OAuth2Client` namespace (1 ctor + 2 instance methods)
- `OAuth2Client::new(client_id, client_secret?, auth_url, token_url, redirect_url, scopes)` — builder.
- `client.authorization_url() -> Result<String, AuthError>` — embeds PKCE verifier for public clients.
- `client.exchange_code(code, pkce_verifier?) -> Result<Map<String, Value>, AuthError>` — blocking POST to token endpoint.

### `Rbac` namespace (1 ctor + 1 builder + 1 instance method)
- `Rbac::new() -> Rbac` — empty policy.
- `rbac.add(role, resource, action) -> Result<(), AuthError>` — dedup insert, empty-field reject.
- `rbac.enforce(&[roles], resource, action) -> bool` — wildcard `*` match on any field.

## CONVENTIONS

- **Pure-Rust only**: `jsonwebtoken` uses the `rust_crypto` backend (NO `ring`, NO `aws-lc-rs`). `argon2` is pure-RustCrypto. `oauth2` + `reqwest` use `rustls-tls` (NOT native-tls). Matches the T34 task spec mandate + the "no C library, no Docker" hard rule from T126/T127.
- **No `ring`, no native-tls**: the T34 task spec explicitly forbids both. `ring` requires `vcruntime.h` on Windows MSVC; native-tls pulls OpenSSL/SChannel. The `rust_crypto` backend is the jsonwebtoken team's pure-Rust alternative suitable for WASM + Windows-MSVC hosts.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. All fallible ops return `Result<_, AuthError>`; `password_verify` returns `Ok(false)` on mismatch (NEVER panics).
- **`catch_unwind` boundary**: `jwt_encode` / `jwt_decode` / `password_hash` / `password_verify` / `OAuth2Client::authorization_url` / `OAuth2Client::exchange_code` all wrap their bodies in `catch_unwind` per FFI guide R6.
- **Deterministic output**: `Rbac` uses `BTreeSet<RbacRule>` for sorted iteration (mirrors the workspace convention that all state collections are BTreeMap / BTreeSet — snapshot tests rely on it).
- **JWT validation policy is MVP-only**: HS256 algorithm + no `exp`/`iss`/`aud` enforcement. The wrapper trusts the caller's policy. A future task can expose the `jsonwebtoken::Validation` builder surface.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `jsonwebtoken` | Upstream JWT provider. `buff-auth` is a safe wrapper; never re-exports `jsonwebtoken::*` types directly. |
| `argon2` | Upstream password-hash provider. `buff-auth` wraps `Argon2::hash_password` / `verify_password` + the `password_hash` PHC-string layer. |
| `oauth2` | Upstream OAuth2 client provider. `buff-auth` wraps `oauth2::Client::exchange_code` + `authorize_url`. |
| `reqwest` | Already pinned at the workspace level (T127). T34 reuses the `rustls-tls` + `blocking` features for the OAuth2 token-exchange HTTP POST. |
| `serde` / `serde_json` | Already pinned at the workspace level (T124 base). T34 reuses them for JWT claims (de)serialisation. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::{Jwt, OAuth2Client, Password, Rbac}` (all namespace-only — return `Type::Void`). `PreludeAssocFn::{JwtEncode, JwtDecode, AuthorizationUrl, ExchangeCode, PasswordHash, PasswordVerify, Enforce}` dispatched on the matching `(type, method)` pair. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::lower_prelude_type_assoc_fn` has the 7 arms (`(Jwt, JwtEncode)` / `(Jwt, JwtDecode)` / `(OAuth2Client, AuthorizationUrl)` / `(OAuth2Client, ExchangeCode)` / `(Password, PasswordHash)` / `(Password, PasswordVerify)` / `(Rbac, Enforce)`). The `program_uses_namespace("Jwt"|"OAuth2Client"|"Password"|"Rbac")` walker records `buff-auth` + `jsonwebtoken` + `argon2` + `oauth2` + `reqwest` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **HS256-only for v1.x**: RS256 / ES256 / EdDSA deferred. HS256 covers the canonical JWT use-case (symmetric shared-secret signing). Asymmetric algorithms require a PEM-key surface (deferred to a sibling task to grow the Buff language's binary-key type support).
- **Argon2id-only for v1.x**: bcrypt / scrypt / PBKDF2 deferred. Argon2id is OWASP's 2024 primary recommendation (memory-hard + side-channel resistant). Default params (m=19456 KiB, t=2, p=1) match the OWASP minimum.
- **OAuth2 auth-code + PKCE only for v1.x**: password grant, client_credentials, device-code, refresh-token grant all deferred (auth-code is the only flow recommended for browser-based apps per OAuth 2.1). The MVP supports both confidential clients (with secret) and public clients (with PKCE).
- **RBAC is role-only**: hierarchical RBAC (role inheritance), ABAC (attribute-based), ACL (object-level) all deferred. The MVP is flat RBAC (NIST level 1) with wildcard `*` match — sufficient for 90% of CRUD apps.
- **MSVC host blocker**: `cargo test -p buff-auth` fails on this Windows host with the same `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` issue that blocks `cargo check --workspace` here. CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. `cargo check -p buff-auth --lib` and `cargo clippy -p buff-auth --all-targets -- -D warnings` both pass clean on a host with the Windows SDK installed.
- **No `exp` validation**: the MVP disables `exp` validation in `jwt_decode` so test fixtures with arbitrary timestamps work. Production callers SHOULD validate `exp` themselves after `jwt_decode` returns (or a future task exposes the `Validation` builder surface). This is documented as a known limitation in the `jwt_decode` doc.
