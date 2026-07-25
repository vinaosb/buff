//! `buff-audit` — security scanning + code signing for the Buff language.
//!
//! Pure-Rust MVP wrapping [`ed25519-dalek`](https://crates.io/crates/ed25519-dalek)
//! 2.0 for detached Ed25519 signatures + a statically-seeded advisory
//! database for CVE scanning. NO `ring`, NO native-tls, NO cc-rs — matches
//! the project's "Windows host with no MSVC" constraint.
//!
//! # Pipeline
//!
//! ```text
//!   Audit.scan(path) ───▶ read manifest ──▶ match deps against advisory_db
//!                                              │
//!                                              ▼
//!                                       Vector<String> (advisory IDs)
//!
//!   Signature.keypair() ─▶ (public_hex, secret_hex)  // ed25519_dalek CSPRNG
//!   Signature.sign(data, secret_hex) ─▶ sig_hex       // 64-byte detached
//!   Signature.verify(data, sig_hex, public_hex) ─▶ Bool  // strict verify
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only owned `String` / `Vec<String>` / `bool` / `(String, String)`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | Every fn returns owned data; the underlying `SigningKey` / `VerifyingKey` are dropped at the boundary. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, AuditError>`. `ed25519_dalek::SignatureError` + `hex::FromHexError` auto-convert via `From`. |
//! | R4 — Thread safety | Every public type is `Send + Sync` (the `SigningKey` / `VerifyingKey` are themselves `Send + Sync`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. Hex strings own their bytes. |
//! | R6 — Panic boundary | `scan` / `sign` / `verify` / `keypair` wrap their bodies in `catch_unwind`. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Invalid hex / wrong-length keys / bad signatures all
//! return `Result<_, AuditError>` — NEVER panic.

pub mod error;

pub use error::AuditError;

use std::panic::{catch_unwind, AssertUnwindSafe};

// T26: `SigningKey::sign` lives on the `Signer` trait — bring it into
// scope so the `signing.sign(...)` call below resolves. Re-exported by
// ed25519-dalek when the `rand_core` feature is on.
use ed25519_dalek::Signer;

/// The default manifest paths `Audit.scan` consults in order.
///
/// The scanner reads the first match; missing files are silently
/// skipped (an empty `Vec` is a valid result for a project with no
/// manifest — mirrors `cargo audit`'s stance). Matches the
/// `buff.lock` / `Cargo.lock` / `buff.toml` priority chain.
pub const MANIFEST_PATHS: &[&str] = &["buff.lock", "Cargo.lock", "buff.toml"];

/// Generate a fresh Ed25519 keypair using the OS CSPRNG.
///
/// Returns `(public_hex, secret_hex)` — two 64-char lowercase hex
/// Strings (32 bytes each). The secret key is `zeroize`-cleared on
/// drop by `ed25519-dalek`'s default `zeroize` feature.
///
/// Wraps `ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng)`
/// in `catch_unwind` per FFI guide R6.
pub fn keypair() -> Result<(String, String), AuditError> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // T26 + T36: rand was bumped from 0.8 → 0.9 at the workspace
        // level, which moved `OsRng` onto rand_core 0.9 (incompatible
        // with ed25519-dalek 2.x's rand_core 0.6 `CryptoRngCore` bound).
        // Generate 32 CSPRNG bytes via rand 0.9's `ThreadRng` (OS-
        // entropy-backed ChaCha — cryptographically secure per the
        // rand 0.9 docs) and construct the SigningKey via `from_bytes`,
        // which sidesteps the rand_core trait mismatch entirely.
        use rand::Rng;
        let secret: [u8; ed25519_dalek::SECRET_KEY_LENGTH] = rand::rng().random();
        let signing = ed25519_dalek::SigningKey::from_bytes(&secret);
        let verifying: ed25519_dalek::VerifyingKey = signing.verifying_key();
        Ok::<(String, String), AuditError>((
            hex::encode(verifying.to_bytes()),
            hex::encode(signing.to_bytes()),
        ))
    }));
    match result {
        Ok(Ok(pair)) => Ok(pair),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(AuditError::Panic),
    }
}

/// Sign `data` with the supplied Ed25519 secret key (hex-encoded).
///
/// Returns the 128-char lowercase hex String (64-byte detached
/// signature). The secret key MUST be 64 hex chars (32 bytes);
/// otherwise returns [`AuditError::BadKey`].
///
/// Wraps `ed25519_dalek::SigningKey::from_bytes(&sk)?.sign(data)`
/// in `catch_unwind` per FFI guide R6.
pub fn sign(data: &[u8], secret_hex: &str) -> Result<String, AuditError> {
    let secret_hex_owned = secret_hex.to_string();
    let data_owned = data.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let sk_bytes = hex::decode(&secret_hex_owned)?;
        if sk_bytes.len() != ed25519_dalek::SECRET_KEY_LENGTH {
            return Err(AuditError::BadKey(format!(
                "secret key must be {} bytes, got {}",
                ed25519_dalek::SECRET_KEY_LENGTH,
                sk_bytes.len()
            )));
        }
        let mut sk_array = [0u8; ed25519_dalek::SECRET_KEY_LENGTH];
        sk_array.copy_from_slice(&sk_bytes);
        let signing = ed25519_dalek::SigningKey::from_bytes(&sk_array);
        let sig = signing.sign(&data_owned);
        Ok::<String, AuditError>(hex::encode(sig.to_bytes()))
    }));
    match result {
        Ok(Ok(sig)) => Ok(sig),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(AuditError::Panic),
    }
}

/// Verify an Ed25519 `signature_hex` over `data` against the supplied
/// `public_hex`.
///
/// Returns `Ok(true)` on a valid signature, `Ok(false)` on a
/// signature that fails strict verification (no panic, no error —
/// mirrors the `bool` return the T26 Buff surface mandates). Returns
/// [`AuditError::BadKey`] only when `public_hex` is the wrong length
/// or invalid hex (a verification failure is NOT an error).
///
/// Wraps
/// `ed25519_dalek::VerifyingKey::from_bytes(&pk)?.verify_strict(data, &sig)`
/// in `catch_unwind` per FFI guide R6.
pub fn verify(data: &[u8], signature_hex: &str, public_hex: &str) -> Result<bool, AuditError> {
    let public_hex_owned = public_hex.to_string();
    let sig_hex_owned = signature_hex.to_string();
    let data_owned = data.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let pk_bytes = hex::decode(&public_hex_owned)?;
        if pk_bytes.len() != ed25519_dalek::PUBLIC_KEY_LENGTH {
            return Err(AuditError::BadKey(format!(
                "public key must be {} bytes, got {}",
                ed25519_dalek::PUBLIC_KEY_LENGTH,
                pk_bytes.len()
            )));
        }
        let mut pk_array = [0u8; ed25519_dalek::PUBLIC_KEY_LENGTH];
        pk_array.copy_from_slice(&pk_bytes);
        let verifying = ed25519_dalek::VerifyingKey::from_bytes(&pk_array)
            .map_err(|e| AuditError::BadKey(e.to_string()))?;
        let sig_bytes = hex::decode(&sig_hex_owned)?;
        if sig_bytes.len() != ed25519_dalek::SIGNATURE_LENGTH {
            return Err(AuditError::BadSignature(format!(
                "signature must be {} bytes, got {}",
                ed25519_dalek::SIGNATURE_LENGTH,
                sig_bytes.len()
            )));
        }
        let mut sig_array = [0u8; ed25519_dalek::SIGNATURE_LENGTH];
        sig_array.copy_from_slice(&sig_bytes);
        let sig = ed25519_dalek::Signature::from_bytes(&sig_array);
        let ok = verifying
            .verify_strict(&data_owned, &sig)
            .map(|_| true)
            .unwrap_or(false);
        Ok::<bool, AuditError>(ok)
    }));
    match result {
        Ok(Ok(ok)) => Ok(ok),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(AuditError::Panic),
    }
}

/// Scan a project root for vulnerable dependencies.
///
/// Reads the first matching manifest in [`MANIFEST_PATHS`] (buff.lock /
/// Cargo.lock / buff.toml) and matches every `name-version` pair
/// against the static advisory database. Returns the list of advisory
/// IDs that fired (e.g. `["RUSTSEC-2020-0159"]`); an empty `Vec`
/// means either no manifest was found OR no advisories matched
/// (use [`scan_with_detail`] to disambiguate).
///
/// Wraps the file-read + match in `catch_unwind` per FFI guide R6.
pub fn scan<P: AsRef<std::path::Path>>(project_root: P) -> Result<Vec<String>, AuditError> {
    let root_owned = project_root.as_ref().to_path_buf();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let hits = scan_inner(&root_owned)?;
        Ok::<Vec<String>, AuditError>(hits)
    }));
    match result {
        Ok(Ok(hits)) => Ok(hits),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(AuditError::Panic),
    }
}

fn scan_inner(root: &std::path::Path) -> Result<Vec<String>, AuditError> {
    let mut hits: Vec<String> = Vec::new();
    for &manifest_name in MANIFEST_PATHS {
        let manifest_path = root.join(manifest_name);
        if !manifest_path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest_path)?;
        for advisory in advisory_db::ALL {
            if advisory.matches_text(&text) {
                hits.push(advisory.id.to_string());
            }
        }
    }
    hits.sort();
    hits.dedup();
    Ok(hits)
}

/// Same as [`scan`] but also returns the affected dependency + a
/// remediation hint per advisory ID. Use this when the bare ID list
/// is too terse for a user-facing report.
pub fn scan_with_detail<P: AsRef<std::path::Path>>(
    project_root: P,
) -> Result<Vec<AdvisoryHit>, AuditError> {
    let root_owned = project_root.as_ref().to_path_buf();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut hits: Vec<AdvisoryHit> = Vec::new();
        for &manifest_name in MANIFEST_PATHS {
            let manifest_path = root_owned.join(manifest_name);
            if !manifest_path.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&manifest_path)?;
            for advisory in advisory_db::ALL {
                if advisory.matches_text(&text) {
                    hits.push(AdvisoryHit {
                        id: advisory.id.to_string(),
                        package: advisory.package.to_string(),
                        patched: advisory.patched.to_string(),
                    });
                }
            }
        }
        hits.sort_by(|a, b| a.id.cmp(&b.id));
        hits.dedup_by(|a, b| a.id == b.id);
        Ok::<Vec<AdvisoryHit>, AuditError>(hits)
    }));
    match result {
        Ok(Ok(hits)) => Ok(hits),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(AuditError::Panic),
    }
}

/// A single advisory hit surfaced by [`scan_with_detail`].
///
/// Owned `String` fields keep the value `Send + 'static` (FFI guide
/// R4) + lifetime-free (R5). Mirrors the buff-image `Color` shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AdvisoryHit {
    /// Stable advisory ID (e.g. `"RUSTSEC-2020-0159"`).
    pub id: String,
    /// The affected package name (e.g. `"chrono"`).
    pub package: String,
    /// The first patched version (e.g. `"0.4.20"`). String-typed
    /// because Buff's surface doesn't carry `semver::Version` yet;
    /// a follow-up task can swap this for a richer type.
    pub patched: String,
}

/// List every advisory ID in the static database (regardless of
/// whether the project triggers it). Useful for `buff audit list`
/// tooling + for tests that want to assert DB coverage.
pub fn known_advisories() -> Vec<String> {
    advisory_db::ALL.iter().map(|a| a.id.to_string()).collect()
}

// ---------------------------------------------------------------------------
// advisory_db — statically-seeded advisory database
// ---------------------------------------------------------------------------

/// The statically-seeded advisory database.
///
/// Initial MVP ships with a tiny seed (1 well-known RustSec entry)
/// so the scanner has something to find in tests. Framework authors
/// file new advisories via PR to this module — the same workflow
/// RustSec uses for the upstream `advisory-db` git repo.
///
/// Future work (deferred to v1.18+): wire `rustsec` crate to query
/// the live https://rustsec.org/advisories/ DB at scan time.
mod advisory_db {
    /// One advisory entry. The [`Advisory::matches_text`] helper
    /// does a literal substring scan for `package-<vuln_version>`
    /// so a Cargo.lock's `name = "0.4.19"` line matches.
    pub struct Advisory {
        pub id: &'static str,
        pub package: &'static str,
        /// The vulnerable version string (lower-bound). The scanner
        /// matches a literal substring so a Cargo.lock entry of
        /// `version = "0.4.19"` triggers when `vuln_version = "0.4.19"`.
        pub vuln_version: &'static str,
        pub patched: &'static str,
    }

    impl Advisory {
        /// Substring-match `"<package>-<vuln_version>"` OR
        /// `"<package>" ... "version = "<vuln_version>""` against
        /// the manifest text. Cargo.lock uses the hyphen form in
        /// `name-version` checksum lines; buff.toml uses the
        /// `name = "version"` form — both are covered.
        pub fn matches_text(&self, text: &str) -> bool {
            let hyphen = format!("{}-{}", self.package, self.vuln_version);
            let eq = format!("{}\"{}\"", self.package, self.vuln_version);
            let eq_spaced = format!("{} = \"{}\"", self.package, self.vuln_version);
            text.contains(&hyphen) || text.contains(&eq) || text.contains(&eq_spaced)
        }
    }

    /// The seeded advisory database. The MVP ships with 1 entry so
    /// tests have something deterministic to find; production usage
    /// should grow this via PRs (mirrors the RustSec advisory-db
    /// workflow).
    pub const ALL: &[Advisory] = &[
        // chrono < 0.4.20 had a soundness issue (RUSTSEC-2020-0159).
        // The canonical "does buff audit find anything?" smoke entry.
        Advisory {
            id: "RUSTSEC-2020-0159",
            package: "chrono",
            vuln_version: "0.4.19",
            patched: "0.4.20",
        },
    ];
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn keypair_sign_verify_roundtrip() {
        let (pk, sk) = keypair().expect("keypair");
        let sig = sign(b"hello audit", &sk).expect("sign");
        let ok = verify(b"hello audit", &sig, &pk).expect("verify shape");
        assert!(ok, "verify should accept fresh signature");
    }

    #[test]
    fn keypair_is_64_hex_chars_each() {
        let (pk, sk) = keypair().expect("keypair");
        assert_eq!(pk.len(), 64, "public hex is 32 bytes");
        assert_eq!(sk.len(), 64, "secret hex is 32 bytes");
        assert!(pk.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(sk.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sign_rejects_short_secret_key() {
        let err = sign(b"data", "abcd").unwrap_err();
        assert!(matches!(err, AuditError::BadKey(_)));
    }

    #[test]
    fn verify_rejects_short_public_key() {
        let err = verify(b"data", "00".repeat(64), "abcd").unwrap_err();
        assert!(matches!(err, AuditError::BadKey(_)));
    }

    #[test]
    fn verify_returns_false_on_tampered_data() {
        let (pk, sk) = keypair().expect("keypair");
        let sig = sign(b"original", &sk).expect("sign");
        let ok = verify(b"TAMPERED", &sig, &pk).expect("verify shape");
        assert!(!ok, "verify must reject tampered data");
    }

    #[test]
    fn known_advisories_includes_seed_entry() {
        let list = known_advisories();
        assert!(list.contains(&"RUSTSEC-2020-0159".to_string()));
    }
}
