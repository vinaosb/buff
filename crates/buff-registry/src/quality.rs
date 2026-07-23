//! Package quality signals — T70.
//!
//! Computes four badge types per published package, going beyond the
//! T0 stability badge that lives in `buff.toml`:
//!
//! | Badge | Field | Source |
//! |-------|-------|--------|
//! | **verified publisher** | [`QualityBadges::verified_publisher`] | `true` when the publishing author is in the registry's verified-author set (MVP: mock — [`crate::InMemoryStorage::add_verified_author`]). Future: GitHub OAuth verified-email check. |
//! | **maintained** | [`QualityBadges::maintained`] | `true` when the latest version was published within [`MAINTAINED_WINDOW`] (180 days) of `now`. |
//! | **tested** | [`QualityBadges::tested`] | Coverage percentage as `f32` (`0.0..=100.0`), or `None` when the publisher attached no coverage report. |
//! | **documented** | [`QualityBadges::documented`] | Doc-comment coverage percentage as `f32`, or `None` when unmeasured. |
//!
//! A fifth field — [`QualityBadges::security_audit`] — carries an optional
//! [`AuditResult`] populated by T26 `buff-audit`. It is `None` in the MVP
//! (no audit wired into the registry); a future integration calls
//! `buff_audit::scan` on the tarball + stores the result here.
//!
//! # Computation model
//!
//! [`compute_badges`] is a PURE function: it takes a [`Package`] view
//! (pre-resolved from storage by the handler) + a `now` timestamp, and
//! returns the badge struct. Storage I/O + author-verification lookup
//! happen in the HANDLER before calling `compute_badges`, so the
//! function is trivially unit-testable without mocking the trait.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!`. The
//! `now.duration_since(last_published_at)` path returns `false` (not
//! maintained) when the duration computation fails (clock skew —
//! `last_published_at` in the future relative to `now`).

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// The maintained-badge window: a package is "maintained" when its
/// latest version was published within this duration of `now`.
///
/// 180 days ≈ 6 months (matches the T70 spec's "commits in 6 months"
/// heuristic — the registry has no commit history, so publish
/// timestamp is the proxy).
pub const MAINTAINED_WINDOW: Duration = Duration::from_secs(180 * 24 * 60 * 60);

/// The four + one quality badges surfaced per package (T70).
///
/// Serialized as JSON in two HTTP responses:
/// - `GET /api/v1/packages/{name}/badges` — the badge struct alone.
/// - `GET /api/v1/search?q=...` — one badge struct per result row,
///   nested under each result's `badges` field.
///
/// `tested` / `documented` / `security_audit` are `Option` so a package
/// with no attached coverage report / doc measurement / audit serializes
/// `null` for those fields (NOT `0.0` — `0.0` would be a real "0%
/// coverage" measurement, which is a valid but distinct signal).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityBadges {
    /// `true` when the publishing author is in the registry's
    /// verified-author set. MVP: mock (see
    /// [`crate::InMemoryStorage::add_verified_author`]).
    pub verified_publisher: bool,
    /// `true` when the latest version was published within
    /// [`MAINTAINED_WINDOW`] of the computation's `now`.
    pub maintained: bool,
    /// Test coverage percentage (`0.0..=100.0`), or `None` when no
    /// coverage report was attached at publish time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tested: Option<f32>,
    /// Doc-comment coverage percentage (`0.0..=100.0`), or `None` when
    /// unmeasured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documented: Option<f32>,
    /// Result of a T26 `buff-audit` scan, or `None` when no audit has
    /// been run. The registry does NOT invoke `buff-audit` itself in
    /// the MVP — a future integration stores the result here after a
    /// publish-triggered scan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_audit: Option<AuditResult>,
}

/// A minimal security-audit result (T26 `buff-audit` integration shape).
///
/// The registry does NOT depend on `buff-audit` at runtime (it's a
/// security tool, not a server dep). This struct is the wire shape a
/// future publish-triggered `buff_audit::scan` call populates + stores
/// alongside the package version. The fields capture the essentials a
/// badge UI needs: did the scan pass, how many advisories were found,
/// and where is the full report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditResult {
    /// `true` when the scan found zero advisories.
    pub passed: bool,
    /// Number of advisories (CVE matches) the scan surfaced.
    pub vulnerabilities: u32,
    /// Optional URL to the full audit report (deferred — the MVP
    /// stores the summary only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_url: Option<String>,
}

/// A read-model view of one package, carrying exactly the data
/// [`compute_badges`] needs. The handler constructs this from storage
/// (latest version's metadata + an author-verification lookup) before
/// calling `compute_badges`.
///
/// Keeping this as a separate struct (rather than passing `&dyn Storage`)
/// means `compute_badges` is pure + unit-testable without the trait.
#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// The package name (for debugging / error messages).
    pub name: String,
    /// Pre-resolved: `true` when the latest version's author is in the
    /// verified-author set.
    pub verified_publisher: bool,
    /// Wall-clock publish time of the latest version, or `None` when
    /// the storage backend did not record one (legacy entries).
    pub last_published_at: Option<SystemTime>,
    /// Test coverage % attached at publish time, or `None`.
    pub tested_coverage: Option<f32>,
    /// Doc coverage % attached at publish time, or `None`.
    pub documented_coverage: Option<f32>,
    /// Security-audit result attached post-publish, or `None`.
    pub security_audit: Option<AuditResult>,
}

impl Package {
    /// Construct a `Package` view with all quality signals absent —
    /// the shape a brand-new package (just published, no attachments)
    /// has. `verified_publisher` defaults to `false` (un-verified until
    /// explicitly added to the set).
    #[must_use]
    pub fn new_empty(name: &str) -> Self {
        Self {
            name: name.to_string(),
            verified_publisher: false,
            last_published_at: None,
            tested_coverage: None,
            documented_coverage: None,
            security_audit: None,
        }
    }
}

impl QualityBadges {
    /// All-off / all-`None` default — the badge profile of a package
    /// that has no quality data attached and an un-verified author.
    /// Used as the fallback when storage has no metadata for a name.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            verified_publisher: false,
            maintained: false,
            tested: None,
            documented: None,
            security_audit: None,
        }
    }
}

impl Default for QualityBadges {
    fn default() -> Self {
        Self::empty()
    }
}

/// Compute the four + one badges for `package` relative to `now`.
///
/// Pure function — no I/O, no trait lookup. The caller resolves
/// `package.verified_publisher` and gathers `last_published_at` /
/// coverage fields from storage BEFORE calling this.
///
/// # Maintained logic
///
/// `maintained` is `true` iff:
/// - `last_published_at` is `Some(t)`, AND
/// - `now.duration_since(t)` succeeds (no clock skew) AND yields a
///   duration `< [`MAINTAINED_WINDOW`]`.
///
/// When `last_published_at` is `None` (legacy entry with no recorded
/// timestamp), `maintained` is `false` — the absence of a timestamp is
/// treated as "we don't know, so we don't claim maintained".
///
/// # Errors
///
/// Never returns `Err` / never panics. The `duration_since` failure
/// path (clock skew) maps to `maintained = false`.
#[must_use]
pub fn compute_badges(package: &Package, now: SystemTime) -> QualityBadges {
    let maintained = package
        .last_published_at
        .and_then(|published| now.duration_since(published).ok())
        .map(|elapsed| elapsed < MAINTAINED_WINDOW)
        .unwrap_or(false);
    QualityBadges {
        verified_publisher: package.verified_publisher,
        maintained,
        tested: package.tested_coverage,
        documented: package.documented_coverage,
        security_audit: package.security_audit.clone(),
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the pure badge-computation logic. HTTP-layer
    //! coverage lives in `tests/quality_badges.rs`.

    use super::*;
    use std::time::UNIX_EPOCH;

    fn pkg(name: &str) -> Package {
        Package::new_empty(name)
    }

    #[test]
    fn empty_package_yields_all_false_all_none() {
        let badges = compute_badges(&pkg("demo"), SystemTime::now());
        assert!(!badges.verified_publisher);
        assert!(!badges.maintained);
        assert_eq!(badges.tested, None);
        assert_eq!(badges.documented, None);
        assert_eq!(badges.security_audit, None);
    }

    #[test]
    fn verified_publisher_passes_through() {
        let mut p = pkg("demo");
        p.verified_publisher = true;
        let badges = compute_badges(&p, SystemTime::now());
        assert!(badges.verified_publisher);
    }

    #[test]
    fn maintained_true_when_recent_publish() {
        let now = SystemTime::now();
        let mut p = pkg("demo");
        // Published 10 days ago — well within the 180-day window.
        p.last_published_at = Some(now - Duration::from_secs(10 * 24 * 60 * 60));
        let badges = compute_badges(&p, now);
        assert!(badges.maintained);
    }

    #[test]
    fn maintained_false_when_stale() {
        let now = SystemTime::now();
        let mut p = pkg("demo");
        // Published 200 days ago — outside the 180-day window.
        p.last_published_at = Some(now - Duration::from_secs(200 * 24 * 60 * 60));
        let badges = compute_badges(&p, now);
        assert!(!badges.maintained);
    }

    #[test]
    fn maintained_false_when_no_timestamp() {
        let p = pkg("demo");
        let badges = compute_badges(&p, SystemTime::now());
        assert!(!badges.maintained);
    }

    #[test]
    fn maintained_false_on_clock_skew() {
        // last_published_at is IN THE FUTURE relative to now —
        // duration_since fails, maintained falls back to false.
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut p = pkg("demo");
        p.last_published_at = Some(now + Duration::from_secs(999_999_999));
        let badges = compute_badges(&p, now);
        assert!(!badges.maintained);
    }

    #[test]
    fn tested_and_documented_pass_through() {
        let mut p = pkg("demo");
        p.tested_coverage = Some(85.0);
        p.documented_coverage = Some(72.5);
        let badges = compute_badges(&p, SystemTime::now());
        assert_eq!(badges.tested, Some(85.0));
        assert_eq!(badges.documented, Some(72.5));
    }

    #[test]
    fn security_audit_passes_through() {
        let audit = AuditResult {
            passed: true,
            vulnerabilities: 0,
            report_url: Some("https://example.test/report".to_string()),
        };
        let mut p = pkg("demo");
        p.security_audit = Some(audit.clone());
        let badges = compute_badges(&p, SystemTime::now());
        assert_eq!(badges.security_audit, Some(audit));
    }

    #[test]
    fn maintained_boundary_exactly_180_days_is_stale() {
        // The boundary is `<` (strict), so exactly 180 days is stale.
        let now = SystemTime::now();
        let mut p = pkg("demo");
        p.last_published_at = Some(now - MAINTAINED_WINDOW);
        let badges = compute_badges(&p, now);
        assert!(!badges.maintained, "exactly 180 days → stale (< is strict)");
    }

    #[test]
    fn empty_default_round_trips_through_json() {
        let badges = QualityBadges::empty();
        let json = serde_json::to_string(&badges).expect("serialize");
        // `skip_serializing_if = "Option::is_none"` drops the None fields.
        assert!(json.contains("\"verified_publisher\":false"));
        assert!(json.contains("\"maintained\":false"));
        assert!(!json.contains("tested"));
        assert!(!json.contains("documented"));
        assert!(!json.contains("security_audit"));
        let back: QualityBadges = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, badges);
    }
}
