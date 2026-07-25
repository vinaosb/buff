#![allow(clippy::all, dead_code)]
//! `buff-auth` — JWT + OAuth2 + password hashing + RBAC for Buff.
//!
//! Pure-Rust MVP wrapping four crates via a safe FFI boundary per
//! [`crates/buff-lang-ffi-guide/GUIDE.md`](../buff-lang-ffi-guide/GUIDE.md):
//! - [`jsonwebtoken`](https://crates.io/crates/jsonwebtoken) 10 (HS256
//!   only, via the pure-Rust `rust_crypto` backend — NO `ring`, NO
//!   `aws-lc-rs`, NO cc-rs)
//! - [`argon2`](https://crates.io/crates/argon2) 0.5 (Argon2id PHC
//!   format — pure-Rust, NO `ring`)
//! - [`oauth2`](https://crates.io/crates/oauth2) 4 (auth-code flow +
//!   PKCE for public clients — pure-Rust, rustls-tls via reqwest)
//! - in-tree RBAC (no extern — `BTreeSet<(role, resource, action)>`
//!   with wildcard match)
//!
//! NO `ring`, NO native-tls, NO cc-rs — matches the project's "Windows
//! host with no MSVC" constraint + the T34 task spec mandate.
//!
//! # Pipeline
//!
//! ```text
//!   JWT.encode(claims, secret) ─▶ token (HS256)
//!   JWT.decode(token, secret) ─▶ Map<String, Unknown> (claims)
//!
//!   Password.hash(plain) ─▶ PHC string  (Argon2id)
//!   Password.verify(plain, phc) ─▶ Bool
//!
//!   OAuth2Client.authorization_url() ─▶ String (browser URL)
//!   OAuth2Client.exchange_code(code) ─▶ Map<String, Unknown> (token)
//!
//!   Rbac.new() / .add(role, resource, action) / .enforce(roles, res, act)
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `String`, `bool`, `OAuth2Client`, `Rbac`, `RbacRule`, `Map<String, Value>` (lowered to `HashMap<String, ?>`). No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | Every fn returns owned data; the underlying `EncodingKey` / `DecodingKey` / `argon2::Argon2` / `oauth2::Client` are dropped at the boundary. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, AuthError>`. `jsonwebtoken::errors::Error` + `argon2::password_hash::Error` + `oauth2::RequestTokenError` + `reqwest::Error` + `serde_json::Error` auto-convert via `From`. |
//! | R4 — Thread safety | Every public type is `Send + 'static` (all fields are owned `String` / `Vec<String>` / `BTreeSet`). `Rbac` is `Clone + Send + Sync` (BTreeSet is Send + Sync). |
//! | R5 — Lifetime hiding | No public lifetime parameters. All input slices are cloned into owned `String`s at the FFI boundary. |
//! | R6 — Panic boundary | Every fallible entry point (`jwt_encode` / `jwt_decode` / `password_hash` / `password_verify` / `OAuth2Client::authorization_url` / `OAuth2Client::exchange_code`) wraps its body in `catch_unwind`. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Invalid inputs / bad signatures / wrong passwords /
//! network failures all return `Result<_, AuthError>` or `Ok(false)` —
//! NEVER panic.

pub mod error;
pub mod jwt;
pub mod oauth;
pub mod password;
pub mod rbac;

pub use error::AuthError;
pub use jwt::{jwt_decode, jwt_encode};
pub use oauth::OAuth2Client;
pub use password::{password_hash, password_verify};
pub use rbac::{Rbac, RbacRule};
