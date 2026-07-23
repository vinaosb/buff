//! T71 — Stability promise document validation tests.
//!
//! These tests do NOT exercise the compiler. They verify the *existence and
//! structure* of the formal stability contract delivered by T71:
//!
//! 1. The canonical decision record at
//!    `.sisyphus/decisions/stability-promise.md` exists and is non-empty.
//! 2. The document covers all seven stability dimensions (the section
//!    headers §1 through §7 must all be present, in order).
//! 3. The root `README.md` references the stability promise (so users can
//!    discover it).
//!
//! Failing any of these means the stability contract is missing, incomplete,
//! or undiscoverable — all three are release blockers per the T71 acceptance
//! criteria ("Document published. Covers all stability dimensions. Referenced
//! from README. 3 tests (doc validation).").
//!
//! The docs-site mirror at `docs-site/content/stability/_index.md` is
//! separately covered by the T67 `docs_site.rs` frontmatter checks, so it is
//! not re-validated here.

use std::fs;

/// Locate the repository root relative to this crate's manifest.
///
/// `CARGO_MANIFEST_DIR` points at `crates/buff-lang-cli/`, so the repo root
/// is two levels up.
fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// The canonical decision record path (relative to repo root).
const DECISION_REL: &str = ".sisyphus/decisions/stability-promise.md";

#[test]
fn stability_promise_decision_doc_exists_and_is_nonempty() {
    let path = repo_root().join(DECISION_REL);
    let meta = fs::metadata(&path).unwrap_or_else(|e| {
        panic!(
            "stability promise decision record missing at {}: {e}",
            path.display()
        )
    });
    assert!(
        meta.is_file(),
        "stability promise path is not a regular file: {}",
        path.display()
    );

    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    // The T71 LOC budget is <=1000 lines for the document. We assert a
    // generous lower bound (a real stability contract is not a one-liner)
    // and the budget as an upper bound.
    let line_count = content.lines().count();
    assert!(
        line_count >= 100,
        "stability promise is suspiciously short ({} lines) at {} — expected a \
         substantive document covering all 7 dimensions",
        line_count,
        path.display()
    );
    assert!(
        line_count <= 1000,
        "stability promise exceeds the <=1000 LOC budget ({} lines) at {}",
        line_count,
        path.display()
    );

    // Non-empty in the bytewise sense too.
    assert!(
        !content.trim().is_empty(),
        "stability promise is empty after trimming whitespace: {}",
        path.display()
    );
}

#[test]
fn stability_promise_covers_all_seven_sections_in_order() {
    let path = repo_root().join(DECISION_REL);
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    // The seven canonical stability dimensions from the T71 spec. These must
    // appear as level-2 section headers, in order, so that the document has a
    // coherent narrative (what's stable -> what's not -> editions -> deprecation
    // -> security -> versioning -> yanking).
    //
    // We match the exact header text the canonical document uses, so renaming
    // a section here requires updating this list (and vice versa).
    let required_sections = [
        "## 1. What's Guaranteed Stable",
        "## 2. What May Change",
        "## 3. Edition Contract",
        "## 4. Deprecation Policy",
        "## 5. Security Exception",
        "## 6. Versioning Scheme",
        "## 7. Yanked Versions",
    ];

    // Record the byte offset of each required header so we can assert order.
    let mut last_offset: usize = 0;
    let mut missing: Vec<&str> = Vec::new();
    let mut out_of_order: Vec<String> = Vec::new();

    for header in required_sections {
        match content.find(header) {
            Some(offset) => {
                if offset < last_offset {
                    out_of_order.push(format!(
                        "'{}' found at byte {} but previous section was at {}",
                        header, offset, last_offset
                    ));
                }
                last_offset = offset;
            }
            None => missing.push(header),
        }
    }

    if !missing.is_empty() {
        panic!(
            "stability promise is missing {} of 7 required section header(s):\n  - {}\n\
             Found headers were:\n{}",
            missing.len(),
            missing.join("\n  - "),
            content
                .lines()
                .filter(|l| l.starts_with("## "))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    if !out_of_order.is_empty() {
        panic!(
            "stability promise sections are out of order:\n  - {}",
            out_of_order.join("\n  - ")
        );
    }

    // Bonus structural check: the strongest guarantee (ErrorCode stability)
    // must be called out somewhere in §1, since it is the headline rule from
    // AGENTS.md §19 and the T71 spec explicitly lists it.
    assert!(
        content.contains("E10xx")
            && content.contains("E11xx")
            && content.contains("E12xx")
            && content.contains("E13xx"),
        "stability promise must reference all four ErrorCode bands \
         (E10xx/E11xx/E12xx/E13xx) — these are the FOREVER-stable codes per \
         AGENTS.md §19"
    );
}

#[test]
fn readme_references_the_stability_promise() {
    let readme_path = repo_root().join("README.md");
    let readme = fs::read_to_string(&readme_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", readme_path.display()));

    // The README must link to the decision record so users can discover it
    // (T71 acceptance: "Referenced from README"). We accept either the bare
    // filename or a Markdown link wrapping it.
    let decision_filename = "stability-promise.md";
    assert!(
        readme.contains(decision_filename),
        "README.md does not reference '{}' — the stability promise is \
         undiscoverable from the project README (T71 requires a link).",
        decision_filename
    );

    // And it should be a real Markdown link (not just a passing mention in
    // prose), so the URL is clickable on GitHub.
    assert!(
        readme.contains("](./.sisyphus/decisions/stability-promise.md)")
            || readme.contains("](.sisyphus/decisions/stability-promise.md)"),
        "README.md mentions the stability promise filename but does not link \
         to it as Markdown — expected a link of the form \
         `[text](./.sisyphus/decisions/stability-promise.md)`."
    );
}
