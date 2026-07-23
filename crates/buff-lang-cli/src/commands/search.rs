//! `buff search <QUERY>` — search the buff registry (T70).
//!
//! Calls `GET /api/v1/search?q=<QUERY>` on the buff registry
//! (`$BUFF_REGISTRY_URL`, default `http://127.0.0.1:7878`) and
//! renders each result with quality badges inline:
//!
//! ```text
//! [verified] [maintained] [tested 85%] [documented 72%] buff-dataframe 1.0.0
//! ```
//!
//! A badge is omitted from the line when it's `false` / `None`:
//!
//! ```text
//! [maintained] buff-cli-tool 0.5.0
//! ```
//!
//! When `<QUERY>` is omitted, all published packages are listed
//! (mirrors `cargo search` with an empty query).
//!
//! # Registry URL resolution
//!
//! Inherits [`crate::commands::registry::registry_url`] — `$BUFF_REGISTRY_URL`
//! env var, then the loopback default. See that function's docs for
//! the resolution order.

use anyhow::Result;

use crate::commands::registry::{registry_url, search_packages, QualityBadges, SearchResult};

/// Entry point for `buff search [QUERY]`.
pub fn run(query: Option<&str>) -> Result<()> {
    let base_url = registry_url();
    let q = query.unwrap_or("");
    let results = search_packages(&base_url, q)?;
    let rendered = render_search_results(&results, q);
    print!("{rendered}");
    Ok(())
}

/// Render search results as badge-prefixed lines.
///
/// Pure of stdout / stderr I/O — exposed so unit tests can assert
/// against the formatted output without capturing stdout.
pub fn render_search_results(results: &[SearchResult], query: &str) -> String {
    if results.is_empty() {
        let mut out = String::new();
        if query.is_empty() {
            out.push_str("No packages published on this registry yet.\n");
        } else {
            out.push_str(&format!(
                "No packages matching `{query}` found on the registry.\n"
            ));
        }
        out.push_str("Publish one with `buff publish` (see `buff publish --help`).\n");
        return out;
    }

    let mut out = String::new();
    for row in results {
        let badges = format_badges_inline(&row.badges);
        out.push_str(&format!("{badges}{} {}\n", row.name, row.latest_version));
    }
    out
}

/// Format the four badges as an inline bracket prefix.
///
/// Produces (in fixed order):
/// - `[verified]` when `verified_publisher` is true
/// - `[maintained]` when `maintained` is true
/// - `[tested NN%]` when `tested` is `Some(pct)`
/// - `[documented NN%]` when `documented` is `Some(pct)`
///
/// Each badge is separated by a single space. When ALL badges are
/// false / None, the prefix is empty (the line starts with the name).
/// Trailing space is trimmed so the name sits flush.
pub fn format_badges_inline(badges: &QualityBadges) -> String {
    let mut parts: Vec<String> = Vec::new();
    if badges.verified_publisher {
        parts.push("[verified]".to_string());
    }
    if badges.maintained {
        parts.push("[maintained]".to_string());
    }
    if let Some(pct) = badges.tested {
        parts.push(format!("[tested {}%]", format_percent(pct)));
    }
    if let Some(pct) = badges.documented {
        parts.push(format!("[documented {}%]", format_percent(pct)));
    }
    if parts.is_empty() {
        return String::new();
    }
    // Join with spaces + ensure a trailing space separates the badge
    // prefix from the package name.
    let mut out = parts.join(" ");
    out.push(' ');
    out
}

/// Format a coverage `f32` as a display string: whole-number percents
/// omit the decimal (85.0 → "85"), fractional ones keep one decimal
/// (72.5 → "72.5").
fn format_percent(pct: f32) -> String {
    if pct.fract() < f32::EPSILON {
        format!("{}", pct as u32)
    } else {
        format!("{:.1}", pct)
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the pure formatting helpers. Live HTTP round-trip
    //! coverage lives in `tests/search_cli_t70.rs`.

    use super::*;

    fn badges(
        verified: bool,
        maintained: bool,
        tested: Option<f32>,
        documented: Option<f32>,
    ) -> QualityBadges {
        QualityBadges {
            verified_publisher: verified,
            maintained,
            tested,
            documented,
        }
    }

    #[test]
    fn format_badges_all_present() {
        let b = badges(true, true, Some(85.0), Some(72.0));
        let s = format_badges_inline(&b);
        assert_eq!(s, "[verified] [maintained] [tested 85%] [documented 72%] ");
    }

    #[test]
    fn format_badges_fractional_coverage_keeps_one_decimal() {
        let b = badges(false, true, Some(72.5), None);
        let s = format_badges_inline(&b);
        assert_eq!(s, "[maintained] [tested 72.5%] ");
    }

    #[test]
    fn format_badges_empty_when_all_false_none() {
        let b = badges(false, false, None, None);
        assert_eq!(format_badges_inline(&b), "");
    }

    #[test]
    fn format_badges_verified_only() {
        let b = badges(true, false, None, None);
        assert_eq!(format_badges_inline(&b), "[verified] ");
    }

    #[test]
    fn render_empty_results_with_query_message() {
        let out = render_search_results(&[], "foo");
        assert!(out.contains("No packages matching `foo`"));
        assert!(out.contains("buff publish"));
    }

    #[test]
    fn render_empty_results_without_query_message() {
        let out = render_search_results(&[], "");
        assert!(out.contains("No packages published"));
    }

    #[test]
    fn render_results_line_starts_with_badges_then_name() {
        let results = vec![SearchResult {
            name: "buff-dataframe".to_string(),
            latest_version: "1.0.0".to_string(),
            badges: badges(true, true, Some(85.0), Some(72.0)),
        }];
        let out = render_search_results(&results, "");
        let line = out.lines().next().expect("at least one line");
        assert!(
            line.starts_with("[verified] [maintained] [tested 85%] [documented 72%] "),
            "badges prefix the name: {line:?}"
        );
        assert!(
            line.contains("buff-dataframe 1.0.0"),
            "name + version present: {line:?}"
        );
    }

    #[test]
    fn render_results_no_badges_shows_name_flush() {
        let results = vec![SearchResult {
            name: "bare-pkg".to_string(),
            latest_version: "0.1.0".to_string(),
            badges: badges(false, false, None, None),
        }];
        let out = render_search_results(&results, "");
        let line = out.lines().next().expect("line");
        assert!(
            line.starts_with("bare-pkg 0.1.0"),
            "no badges → name at start: {line:?}"
        );
    }
}
