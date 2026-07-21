//! HTML report emitter.
//!
//! Renders [`BuffCoverage`](super::model::BuffCoverage) into a single
//! self-contained HTML page — no external CSS, no JavaScript, no
//! images. Suitable for opening directly from `buff coverage --html
//! --out report.html` or embedding in CI artifacts.
//!
//! # Layout
//!
//! The report mirrors `llvm-cov show --format=html` + `genhtml`'s
//! summary pages:
//!
//! 1. **Header** — title + overall coverage percentage.
//! 2. **Per-file table** — one row per `.buff` file: path, covered /
//!    total lines, percentage (red < 75%, yellow 75–90%, green ≥ 90%).
//! 3. **Per-file detail** — for each file, a `<pre>` block listing every
//!    instrumented line with its hit count, color-coded:
//!    - green background — hit at least once (`count > 0`).
//!    - red background — uncovered (`count == 0`).
//!
//! The HTML is intentionally minimal so that the unit tests can
//! assert on structure (specific tag presence) rather than whitespace.
//!
//! # Determinism
//!
//! [`BuffCoverage`]'s BTreeMap iteration is path-sorted, so the same
//! input always produces byte-identical HTML.

use super::model::BuffCoverage;

/// Render `coverage` as a self-contained HTML page.
///
/// The output is a `String` containing one `<!DOCTYPE html>` document.
/// Empty coverage produces a valid (but uninformative) page that
/// reports 100% — matching [`BuffCoverage::overall_percent`]'s empty
/// convention.
pub fn render_html(coverage: &BuffCoverage) -> String {
    let overall = coverage.overall_percent();
    let overall_color = percent_color(overall);

    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n");
    out.push_str("<html lang=\"en\">\n");
    out.push_str("<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<title>Buff Coverage Report</title>\n");
    out.push_str("<style>\n");
    out.push_str(
        "body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; \
                margin: 2rem; color: #222; }\n",
    );
    out.push_str("h1, h2 { font-weight: 600; margin-bottom: 0.5rem; }\n");
    out.push_str("table { border-collapse: collapse; margin: 1rem 0; width: 100%; }\n");
    out.push_str("th, td { border: 1px solid #ddd; padding: 0.4rem 0.7rem; text-align: left; }\n");
    out.push_str("th { background: #f4f4f4; }\n");
    out.push_str(
        ".pct { font-weight: 600; padding: 0.2rem 0.5rem; border-radius: 3px; color: #fff; }\n",
    );
    out.push_str(
        "pre { background: #f8f8f8; padding: 0.8rem; border-radius: 4px; overflow-x: auto; }\n",
    );
    out.push_str(".hit { background: #e6ffe6; }\n");
    out.push_str(".miss { background: #ffe6e6; }\n");
    out.push_str(
        ".line-no { color: #888; display: inline-block; width: 4em; user-select: none; }\n",
    );
    out.push_str("</style>\n");
    out.push_str("</head>\n");
    out.push_str("<body>\n");
    out.push_str("<h1>Buff Coverage Report</h1>\n");
    out.push_str(&format!(
        "<p>Overall: <span class=\"pct\" style=\"background:{overall_color}\">{overall:.1}%</span></p>\n"
    ));

    if coverage.files.is_empty() {
        out.push_str("<p><em>No coverage data.</em></p>\n");
        out.push_str("</body>\n</html>\n");
        return out;
    }

    // Summary table — one row per file.
    out.push_str("<h2>Files</h2>\n");
    out.push_str("<table>\n");
    out.push_str("<thead><tr><th>File</th><th>Covered</th><th>Total</th><th>%</th></tr></thead>\n");
    out.push_str("<tbody>\n");
    for (path, file_cov) in &coverage.files {
        let covered = file_cov.covered_lines();
        let total = file_cov.total_lines();
        let pct = file_cov.percent();
        let color = percent_color(pct);
        out.push_str(&format!(
            "<tr><td>{path}</td><td>{covered}</td><td>{total}</td>\
             <td><span class=\"pct\" style=\"background:{color}\">{pct:.1}%</span></td></tr>\n",
            path = path.display(),
        ));
    }
    out.push_str("</tbody>\n");
    out.push_str("</table>\n");

    // Per-file line detail.
    out.push_str("<h2>Line Detail</h2>\n");
    for (path, file_cov) in &coverage.files {
        out.push_str(&format!("<h3>{path}</h3>\n", path = path.display()));
        out.push_str("<pre>\n");
        for (line, count) in &file_cov.lines {
            let cls = if *count == 0 { "miss" } else { "hit" };
            out.push_str(&format!(
                "<span class=\"{cls}\"><span class=\"line-no\">{line:>5}</span> count={count}</span>\n"
            ));
        }
        out.push_str("</pre>\n");
    }

    out.push_str("</body>\n</html>\n");
    out
}

/// Pick a coverage-color hex string for a percentage.
///
/// - `< 75.0` → red (`#e44`).
/// - `< 90.0` → yellow (`#ec3`).
/// - `>= 90.0` → green (`#3c3`).
///
/// The thresholds match the `genhtml` defaults so the report looks
/// familiar to lcov users.
fn percent_color(pct: f64) -> &'static str {
    if pct < 75.0 {
        "#e44"
    } else if pct < 90.0 {
        "#ec3"
    } else {
        "#3c3"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn file_cov(entries: &[(usize, u64)]) -> super::super::model::BuffFileCoverage {
        let mut lines = BTreeMap::new();
        for (l, c) in entries {
            lines.insert(*l, *c);
        }
        super::super::model::BuffFileCoverage { lines }
    }

    #[test]
    fn render_empty_coverage_yields_valid_html() {
        let cov = BuffCoverage::default();
        let html = render_html(&cov);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("No coverage data."));
        assert!(html.contains("Overall:"));
        // Overall of empty coverage is 100% by convention.
        assert!(html.contains("100.0%"));
    }

    #[test]
    fn render_includes_doctype_and_title() {
        let mut files = BTreeMap::new();
        files.insert(p("main.buff"), file_cov(&[(1, 1)]));
        let cov = BuffCoverage { files };
        let html = render_html(&cov);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<title>Buff Coverage Report</title>"));
        assert!(html.contains("</html>"));
    }

    #[test]
    fn render_summary_table_has_file_row() {
        let mut files = BTreeMap::new();
        files.insert(p("main.buff"), file_cov(&[(1, 1), (2, 0)]));
        let cov = BuffCoverage { files };
        let html = render_html(&cov);
        assert!(html.contains("<h2>Files</h2>"));
        assert!(html.contains("main.buff"));
        // 1 covered of 2 = 50%.
        assert!(html.contains("50.0%"));
    }

    #[test]
    fn render_line_detail_includes_count() {
        let mut files = BTreeMap::new();
        files.insert(p("main.buff"), file_cov(&[(1, 3), (5, 0)]));
        let cov = BuffCoverage { files };
        let html = render_html(&cov);
        assert!(html.contains("<h2>Line Detail</h2>"));
        // The hit line has class "hit" + shows count=3.
        assert!(html.contains("class=\"hit\""));
        assert!(html.contains("count=3"));
        // The uncovered line has class "miss" + count=0.
        assert!(html.contains("class=\"miss\""));
        assert!(html.contains("count=0"));
    }

    #[test]
    fn render_color_thresholds() {
        // Red < 75%, yellow 75-90%, green >= 90%.
        assert_eq!(percent_color(0.0), "#e44");
        assert_eq!(percent_color(74.9), "#e44");
        assert_eq!(percent_color(75.0), "#ec3");
        assert_eq!(percent_color(89.9), "#ec3");
        assert_eq!(percent_color(90.0), "#3c3");
        assert_eq!(percent_color(100.0), "#3c3");
    }

    #[test]
    fn render_multi_file_emits_detail_for_each() {
        let mut files = BTreeMap::new();
        files.insert(p("alpha.buff"), file_cov(&[(1, 1)]));
        files.insert(p("beta.buff"), file_cov(&[(1, 0)]));
        let cov = BuffCoverage { files };
        let html = render_html(&cov);
        assert!(html.contains("alpha.buff"));
        assert!(html.contains("beta.buff"));
        // Both should appear in the summary table AND the line detail.
        let alpha_count = html.matches("alpha.buff").count();
        assert!(
            alpha_count >= 2,
            "alpha.buff should appear at least twice (summary + detail)"
        );
    }
}
