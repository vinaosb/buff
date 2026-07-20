//! `buff outdated` — report outdated registry dependencies (T128).
//!
//! For every entry under `[registry-dependencies]`, queries the buff
//! registry (`$BUFF_REGISTRY_URL`, default
//! `http://127.0.0.1:7878`) twice:
//!
//! 1. With the **pinned requirement** from `buff.toml` — what
//!    version you would resolve if you built today.
//! 2. With the **wildcard requirement** (`*`) — the absolute latest
//!    published version.
//!
//! If `latest > current`, the entry is flagged as outdated (mirrors
//! `cargo outdated`). Entries whose resolution fails (registry
//! unreachable, package unknown, semver parse failure) are surfaced
//! as warnings rather than aborting the whole report — partial
//! output is more useful than no output when the registry is flaky.
//!
//! ## Errors
//!
//! - Missing or unparseable `buff.toml` in the current directory.
//!
//! Per-entry resolve errors are NOT propagated — they become
//! warnings in the report (see [`OutdatedEntry::error`]).

use std::path::Path;

use anyhow::{Context, Result};
use semver::Version;

use crate::commands::registry::{registry_url, resolve_version};
use crate::config::BuffConfig;

/// Entry point for `buff outdated`.
///
/// Reads `buff.toml` from the current directory, queries the
/// registry for each `[registry-dependencies]` entry, and prints
/// the report to stdout.
pub fn run() -> Result<()> {
    let buff_toml = Path::new("buff.toml");
    let cfg = BuffConfig::load_from_file(buff_toml).with_context(|| {
        format!(
            "failed to load {} — run `buff init` first or change to a Buff project root",
            buff_toml.display(),
        )
    })?;

    let base_url = registry_url();
    let report = check_outdated(&cfg, &base_url)?;
    let rendered = render_report(&report);
    print!("{rendered}");
    Ok(())
}

/// One row of the outdated report.
///
/// Carries the package name + pinned requirement plus the resolved
/// current / latest versions. If a resolve failed, the error message
/// is captured in `error` rather than aborting the whole report —
/// the user gets partial output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutdatedEntry {
    /// The package name (key in `[registry-dependencies]`).
    pub name: String,
    /// The pinned requirement verbatim from `buff.toml`
    /// (e.g. `^1.0.0`, `*`).
    pub pinned_req: String,
    /// The version the pinned requirement resolves to today, or
    /// `None` if the resolve failed.
    pub current: Option<String>,
    /// The absolute latest version (`req = "*"`), or `None` if the
    /// resolve failed.
    pub latest: Option<String>,
    /// First resolve error encountered (if any). A second failure
    /// appends to the same string with a `; ` separator.
    pub error: Option<String>,
}

impl OutdatedEntry {
    /// Returns `true` when the entry has a newer version available
    /// than the one the pinned requirement resolves to.
    ///
    /// Returns `false` when either side failed to resolve or when
    /// the versions are equal / the latest is older.
    pub fn is_outdated(&self) -> bool {
        match (&self.current, &self.latest) {
            (Some(c), Some(l)) => match (Version::parse(c), Version::parse(l)) {
                (Ok(cv), Ok(lv)) => cv < lv,
                _ => false,
            },
            _ => false,
        }
    }
}

/// Query the registry for each `[registry-dependencies]` entry and
/// build the outdated report.
///
/// Pure of stdout / stderr I/O — exposed so integration tests can
/// assert against the returned rows without scraping output text.
/// Per-entry resolve failures are captured into
/// [`OutdatedEntry::error`] (no early bail).
pub fn check_outdated(cfg: &BuffConfig, base_url: &str) -> Result<Vec<OutdatedEntry>> {
    let mut rows: Vec<OutdatedEntry> = Vec::with_capacity(cfg.registry_dependencies.len());
    for (name, dep) in &cfg.registry_dependencies {
        let mut row = OutdatedEntry {
            name: name.clone(),
            pinned_req: dep.version.clone(),
            current: None,
            latest: None,
            error: None,
        };

        match resolve_version(base_url, name, &dep.version) {
            Ok(r) => row.current = Some(r.version),
            Err(e) => row.error = Some(format!("resolve `{}`: {e:#}", dep.version)),
        }

        match resolve_version(base_url, name, "*") {
            Ok(r) => row.latest = Some(r.version),
            Err(e) => {
                let msg = format!("resolve `*`: {e:#}");
                row.error = match row.error.take() {
                    Some(prev) => Some(format!("{prev}; {msg}")),
                    None => Some(msg),
                };
            }
        }

        rows.push(row);
    }
    Ok(rows)
}

/// Render the report as a fixed-width columnar table.
///
/// Exposed so tests can assert against the formatted output without
/// capturing stdout. The format is intentionally similar to
/// `cargo outdated`:
///
/// ```text
/// Package                 Req              Current          Latest
/// ------------------------------------------------------------------------
/// pkg-req                 ^1.0.0           1.0.0            1.2.0 *
/// ```
///
/// The trailing ` *` marks outdated entries.
pub fn render_report(rows: &[OutdatedEntry]) -> String {
    let mut out = String::new();
    if rows.is_empty() {
        out.push_str("No registry dependencies declared.\n");
        out.push_str(
            "Add one with `buff add <name>` (see `buff add --help` for the spec format).\n",
        );
        return out;
    }

    out.push_str(&format!(
        "{:<24} {:<16} {:<16} {:<16}\n",
        "Package", "Req", "Current", "Latest"
    ));
    let separator = "-".repeat(72);
    out.push_str(&separator);
    out.push('\n');

    let mut any_outdated = false;
    for row in rows {
        let current = row.current.as_deref().unwrap_or("-");
        let latest = row.latest.as_deref().unwrap_or("-");
        let marker = if row.is_outdated() {
            any_outdated = true;
            " *"
        } else {
            ""
        };
        out.push_str(&format!(
            "{:<24} {:<16} {:<16} {:<16}{}\n",
            row.name, row.pinned_req, current, latest, marker
        ));
        if let Some(err) = &row.error {
            out.push_str(&format!("    warning: {err}\n"));
        }
    }

    if any_outdated {
        out.push('\n');
        out.push_str("(*) Outdated — newer version available.\n");
        out.push_str("    Update with `buff add <name>@<new-req>`.\n");
    } else {
        out.push('\n');
        out.push_str("All registry dependencies are up to date.\n");
    }

    out
}

#[cfg(test)]
mod tests {
    //! Unit tests for the pure helpers. Live HTTP round-trip
    //! coverage lives in `tests/deps_outdated_t128.rs`.

    use super::*;
    use crate::config::{BuffConfig, GitDependency, PackageSection, Profiles, WorkspaceSection};
    use std::collections::BTreeMap;

    fn pkg(name: &str, version: &str) -> Option<PackageSection> {
        Some(PackageSection {
            name: name.to_string(),
            version: version.to_string(),
            edition: None,
        })
    }

    fn empty_cfg() -> BuffConfig {
        BuffConfig {
            package: pkg("demo", "0.1.0"),
            dependencies: BTreeMap::new(),
            profile: Profiles::default(),
            rust_deps: BTreeMap::new(),
            git_dependencies: BTreeMap::new(),
            registry_dependencies: BTreeMap::new(),
            workspace: None,
        }
    }

    #[test]
    fn is_outdated_true_when_latest_higher() {
        let row = OutdatedEntry {
            name: "demo".to_string(),
            pinned_req: "^1.0.0".to_string(),
            current: Some("1.0.0".to_string()),
            latest: Some("1.2.0".to_string()),
            error: None,
        };
        assert!(row.is_outdated());
    }

    #[test]
    fn is_outdated_false_when_equal() {
        let row = OutdatedEntry {
            name: "demo".to_string(),
            pinned_req: "*".to_string(),
            current: Some("1.0.0".to_string()),
            latest: Some("1.0.0".to_string()),
            error: None,
        };
        assert!(!row.is_outdated());
    }

    #[test]
    fn is_outdated_false_when_resolve_missing() {
        let row = OutdatedEntry {
            name: "demo".to_string(),
            pinned_req: "*".to_string(),
            current: None,
            latest: None,
            error: Some("resolve failed".to_string()),
        };
        assert!(!row.is_outdated());
    }

    #[test]
    fn is_outdated_false_on_semver_parse_failure() {
        // Defensive: non-semver strings should NOT be flagged as
        // outdated (would produce noisy false positives otherwise).
        let row = OutdatedEntry {
            name: "demo".to_string(),
            pinned_req: "weird".to_string(),
            current: Some("not-a-version".to_string()),
            latest: Some("also-not-a-version".to_string()),
            error: None,
        };
        assert!(!row.is_outdated());
    }

    #[test]
    fn render_report_empty_suggests_add() {
        let out = render_report(&[]);
        assert!(
            out.contains("No registry dependencies declared"),
            "empty message: {out}"
        );
        assert!(out.contains("buff add"), "hint links to `buff add`: {out}");
    }

    #[test]
    fn render_report_marks_outdated_with_star() {
        let rows = vec![OutdatedEntry {
            name: "pkg-req".to_string(),
            pinned_req: "^1.0.0".to_string(),
            current: Some("1.0.0".to_string()),
            latest: Some("1.2.0".to_string()),
            error: None,
        }];
        let out = render_report(&rows);
        assert!(out.contains("pkg-req"), "name appears: {out}");
        assert!(out.contains("1.0.0"), "current: {out}");
        assert!(out.contains("1.2.0"), "latest: {out}");
        // The ` *` marker is column-padded to the right of the latest
        // version (`{:<16}{}`), so we assert it terminates the
        // pkg-req row line rather than looking for `1.2.0 *` as a
        // substring (which would fail under the padding).
        let pkg_line = out
            .lines()
            .find(|l| l.contains("pkg-req"))
            .expect("pkg-req line present");
        assert!(
            pkg_line.trim_end().ends_with('*'),
            "row ends with the outdated `*` marker: {pkg_line:?}"
        );
        assert!(
            out.contains("Outdated — newer version available"),
            "outdated legend: {out}"
        );
    }

    #[test]
    fn render_report_up_to_date_message() {
        let rows = vec![OutdatedEntry {
            name: "pkg".to_string(),
            pinned_req: "*".to_string(),
            current: Some("1.0.0".to_string()),
            latest: Some("1.0.0".to_string()),
            error: None,
        }];
        let out = render_report(&rows);
        assert!(
            out.contains("All registry dependencies are up to date"),
            "up-to-date message: {out}"
        );
        // No outdated marker.
        assert!(
            !out.contains("1.0.0 *"),
            "no star marker on up-to-date row: {out}"
        );
    }

    #[test]
    fn render_report_surfaces_resolve_errors_as_warnings() {
        let rows = vec![OutdatedEntry {
            name: "broken".to_string(),
            pinned_req: "^1.0.0".to_string(),
            current: None,
            latest: None,
            error: Some("resolve `^1.0.0`: connection refused".to_string()),
        }];
        let out = render_report(&rows);
        assert!(out.contains("warning:"), "warning prefix present: {out}");
        assert!(
            out.contains("connection refused"),
            "error message text preserved: {out}"
        );
        // The dashed column row still prints, with `-` placeholders.
        assert!(out.contains("broken"), "package name still in table: {out}");
    }

    // -----------------------------------------------------------------
    // Defensive: `check_outdated` with an empty registry-deps section
    // returns an empty report (does NOT touch the network).
    // -----------------------------------------------------------------

    #[test]
    fn check_outdated_empty_deps_yields_empty_rows_without_network() {
        let cfg = empty_cfg();
        // Point at an unreachable URL to PROVE no network call happens
        // when the dep section is empty (the iterator body is never
        // entered — Vec::with_capacity(0) leaves the row set empty).
        let rows = check_outdated(&cfg, "http://127.0.0.1:1").expect("no network");
        assert!(rows.is_empty(), "no rows for empty deps: {rows:?}");
    }

    // -----------------------------------------------------------------
    // Static-check the `WorkspaceSection` import stays valid against
    // future schema drift in `BuffConfig` (the empty_cfg helper must
    // compile against the real shape).
    // -----------------------------------------------------------------

    #[test]
    fn empty_cfg_helper_compiles_with_full_struct() {
        let cfg = empty_cfg();
        // Just touch the workspace field so the unused `WorkspaceSection`
        // import is not flagged.
        let _: &Option<WorkspaceSection> = &cfg.workspace;
        let _: &BTreeMap<String, GitDependency> = &cfg.git_dependencies;
    }
}
