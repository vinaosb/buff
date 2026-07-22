# buff-auth

> JWT + OAuth2 + password hashing + RBAC for the **Buff** language. Pure-Rust MVP (no `ring`, no native-tls).

`buff-auth` wraps [`jsonwebtoken`](https://crates.io/crates/jsonwebtoken) 10 (HS256 via pure-Rust `rust_crypto` backend) + [`argon2`](https://crates.io/crates/argon2) 0.5 (Argon2id PHC) + [`oauth2`](https://crates.io/crates/oauth2) 4 (auth-code + PKCE) + an in-tree RBAC engine. It follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md) at every public boundary.

**Status: experimental** (T34 v1.16 frameworks wave 4).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `JWT`, `OAuth2Client`, `Password`, or `Rbac` prelude types.

For direct Rust use:

```bash
cargo add buff-auth --path crates/buff-auth
```

## Quick start

### JWT encode + decode roundtrip

```rust
use buff_auth::{jwt_encode, jwt_decode};
use serde_json::{Map, Value};

fn main() {
    let mut claims = Map::new();
    claims.insert("sub".to_string(), Value::String("user-42".to_string()));
    let token = jwt_encode(&claims, "secret").expect("encode");
    let decoded = jwt_decode(&token, "secret").expect("decode");
    assert_eq!(decoded.get("sub").and_then(|v| v.as_str()), Some("user-42"));
}
```

### Password hash + verify

```rust
use buff_auth::{password_hash, password_verify};

fn main() {
    let hash = password_hash("hunter2").expect("hash");
    assert!(password_verify("hunter2", &hash).expect("verify shape"));
    assert!(!password_verify("wrong", &hash).expect("verify shape"));
}
```

### OAuth2 authorization URL

```rust
use buff_auth::OAuth2Client;

fn main() {
    let client = OAuth2Client::new(
        "my-client".to_string(),
        None, // public client -> PKCE flow
        "https://accounts.example.com/auth".to_string(),
        "https://accounts.example.com/token".to_string(),
        "myapp://callback".to_string(),
        vec!["profile".to_string()],
    );
    let url = client.authorization_url().expect("auth url");
    println!("open in browser: {url}");
}
```

### RBAC enforce

```rust
use buff_auth::Rbac;

fn main() {
    let mut policy = Rbac::new();
    policy.add("admin", "*", "read").expect("rule");
    policy.add("admin", "users", "delete").expect("rule");
    assert!(policy.enforce(&["admin".to_string()], "anything", "read"));
    assert!(!policy.enforce(&["user".to_string()], "users", "delete"));
}
```

## Public API

### `JWT`

| Function | Signature | Notes |
|---|---|---|
| `jwt_encode` | `(&Map<String, Value>, &str) -> Result<String, AuthError>` | HS256 compact JWS. |
| `jwt_decode` | `(&str, &str) -> Result<Map<String, Value>, AuthError>` | Verify + decode to claims map. No `exp` enforcement (MVP). |

### `Password`

| Function | Signature | Notes |
|---|---|---|
| `password_hash` | `(&str) -> Result<String, AuthError>` | Argon2id PHC string. |
| `password_verify` | `(&str, &str) -> Result<bool, AuthError>` | `Ok(false)` on mismatch (NEVER errors on plain mismatch). |

### `OAuth2Client`

| Method | Signature | Notes |
|---|---|---|
| `OAuth2Client::new` | `(String, Option<String>, String, String, String, Vec<String>) -> OAuth2Client` | Builder. `secret=None` → PKCE public client. |
| `client.authorization_url` | `() -> Result<String, AuthError>` | Embeds `#pkce_verifier=...` fragment for public clients. |
| `client.exchange_code` | `(&str, Option<&str>) -> Result<Map<String, Value>, AuthError>` | Blocking POST. PKCE verifier matches `authorization_url` output. |

### `Rbac`

| Method | Signature | Notes |
|---|---|---|
| `Rbac::new` | `() -> Rbac` | Empty policy. |
| `rbac.add` | `(&mut self, &str, &str, &str) -> Result<(), AuthError>` | Dedup insert + empty-field reject. |
| `rbac.enforce` | `(&[String], &str, &str) -> bool` | Wildcard `*` match on any field. |
| `rbac.len` / `rbac.is_empty` | `() -> usize` / `() -> bool` | State inspection. |
| `rbac.rules` | `() -> &BTreeSet<RbacRule>` | Deterministic snapshot. |

### Types

| Type | Notes |
|---|---|
| `RbacRule { role, resource, action }` | Owned, `Send + 'static + Eq + Hash + Ord`. |
| `AuthError` | `Jwt` / `PasswordHash` / `PasswordMismatch` / `OAuth2` / `Rbac` / `Json` / `Panic`. |

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md):

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `String`, `bool`, `Map<String, Value>`, `OAuth2Client`, `Rbac`, `RbacRule`. No `*const`/`*mut`. |
| R2 — Ownership boundary | Every fn returns owned data; `EncodingKey` / `DecodingKey` / `Argon2` / `oauth2::Client` are dropped at the boundary. |
| R3 — Error mapping | Every fallible op returns `Result<T, AuthError>`. `jsonwebtoken::errors::Error` + `argon2::password_hash::Error` + `oauth2::RequestTokenError` + `reqwest::Error` + `serde_json::Error` auto-convert. |
| R4 — Thread safety | Every public type is `Send + 'static` (all fields are owned `String` / `Vec<String>` / `BTreeSet`). |
| R5 — Lifetime hiding | No public lifetime parameters. All input slices are cloned into owned `String`s at the FFI boundary. |
| R6 — Panic boundary | `jwt_encode` / `jwt_decode` / `password_hash` / `password_verify` / `OAuth2Client::authorization_url` / `OAuth2Client::exchange_code` wrap bodies in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-auth
cargo clippy -p buff-auth --all-targets -- -D warnings
cargo fmt -p buff-auth --check
```

Tests are hermetic: all JWT / password / RBAC tests run without network. OAuth2 tests cover URL construction only (no token-exchange HTTP calls — deferred to integration test infrastructure per the T34 task spec). 22 tests total (6 JWT + 5 password + 7 RBAC + 4 OAuth2 URL).

## Limitations (v1.x MVP)

- **HS256 only**: RS256 / ES256 / EdDSA deferred (require PEM key surface).
- **Argon2id only**: bcrypt / scrypt / PBKDF2 deferred (OWASP 2024 primary recommendation is Argon2id).
- **OAuth2 auth-code + PKCE only**: password grant, client_credentials, device-code, refresh-token grant deferred.
- **Flat RBAC only**: hierarchical RBAC (role inheritance), ABAC, ACL deferred.
- **No `exp` validation in `jwt_decode`**: the MVP disables exp validation so test fixtures work. Production callers should validate `exp` themselves, or a future task exposes the `Validation` builder surface.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
