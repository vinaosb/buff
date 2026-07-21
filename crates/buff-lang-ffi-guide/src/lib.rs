//! # Buff FFI Safety Guide
//!
//! This crate contains no code. It exists as a documentation home for the
//! rules and conventions governing `extern` FFI usage across all Buff
//! framework wrapper crates.
//!
//! **If you are writing a wrapper crate** (e.g. buff-web, buff-db, buff-template),
//! read [`../../../crates/buff-lang-ffi-guide/GUIDE.md`](../GUIDE.md) first.
//! Every rule in that guide applies to your wrapper. The Wave 4 wrappers
//! (T17-T21) are the first consumers of this guide.
//!
//! ## Quick summary
//!
//! 1. No raw pointer exposure to Buff users.
//! 2. Rust owns heap memory; Buff sees borrowed views.
//! 3. Map Rust `Result<T,E>` to Buff `Result<T,BuffError>` with span-aware errors.
//! 4. Only `Send + 'static` types cross `spawn` boundaries.
//! 5. No Rust lifetimes in Buff types. Owned types or `'static` only.
//! 6. Catch panics at the boundary; never propagate them into Buff code.
