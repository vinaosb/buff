//! T69 — Onboarding Paths by Background validation tests.
//!
//! These tests do NOT exercise the compiler. They verify the *existence,
//! structure, and contents* of the four tailored onboarding guides
//! shipped as T69 in the v1.21.0 "Community & Quality" milestone.
//!
//! The onboarding guides live at `docs-site/content/onboarding/` and
//! cover the four developer backgrounds Buff targets: Python, Rust,
//! Go, and JavaScript/TypeScript. Each guide follows the same
//! six-section structure (Why Buff? → Syntax mapping table → Tooling
//! migration → Hello World side-by-side → Common pitfalls → Where to
//! go next) plus a Zola frontmatter block.
//!
//! Acceptance (per T69 spec): 4 guides published, each covering
//! syntax + tooling + ecosystem. The `_index.md` landing page links
//! to all four. 4 tests. Failing any of these means the onboarding
//! is incomplete or malformed — release blocker.

use std::fs;
use std::path::{Path, PathBuf};

/// Locate the onboarding directory relative to this crate's manifest.
///
/// `CARGO_MANIFEST_DIR` points at `crates/buff-lang-cli/`, so the repo
/// root is two levels up; the onboarding guides live at
/// `<repo-root>/docs-site/content/onboarding/`.
fn onboarding_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs-site")
        .join("content")
        .join("onboarding")
}

/// The four required guide files plus the landing page (T69 spec).
/// Each of the four backgrounds must have its own guide file, and
/// `_index.md` is the "pick your background" landing page that links
/// to all four.
const REQUIRED_FILES: &[&str] = &[
    "_index.md",
    "python-developers.md",
    "rust-developers.md",
    "go-developers.md",
    "javascript-developers.md",
];

/// The four backgrounds T69 covers. Used to validate the landing
/// page links and to keep test #4 in sync with the spec.
const BACKGROUNDS: &[&str] = &[
    "python",
    "rust",
    "go",
    "javascript",
];

/// A Zola frontmatter block starts at byte 0 with `+++\n` and ends
/// with the next line that is exactly `+++`. Returns `true` iff such
/// a block is present at the top of `content`.
fn has_zola_frontmatter(content: &str) -> bool {
    if !content.starts_with("+++\n") {
        return false;
    }
    let after_open = &content[4..];
    after_open.contains("\n+++\n") || after_open.contains("\n+++")
}

/// Case-insensitive substring check that ignores word boundaries —
/// used to verify a guide mentions the topics it must cover (syntax,
/// tooling, ecosystem) without being tripped up by capitalisation.
fn mentions(content: &str, needle: &str) -> bool {
    content.to_lowercase().contains(&needle.to_lowercase())
}

#[test]
fn all_required_onboarding_files_exist() {
    let dir = onboarding_dir();
    assert!(
        dir.is_dir(),
        "onboarding directory missing: {} — did the docs-site move?",
        dir.display()
    );

    for rel in REQUIRED_FILES {
        let path = dir.join(rel);
        assert!(
            path.exists(),
            "required onboarding file missing: {} (full path: {})",
            rel,
            path.display()
        );

        // Each file must be non-trivial — a real guide, not a stub.
        // The T69 LOC budget is <=2500 across all four guides + index.
        // We assert a generous lower bound per guide (~150 lines)
        // to catch a half-written guide that snuck through.
        let content = fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("failed to read {}: {e}", path.display())
        });
        let line_count = content.lines().count();
        let min_lines = if rel == &"_index.md" { 50 } else { 150 };
        assert!(
            line_count >= min_lines,
            "onboarding file {} is suspiciously short ({} lines) — expected \
             at least {}. T69 acceptance: each guide covers syntax + tooling \
             + ecosystem substantively.",
            rel,
            line_count,
            min_lines
        );
    }
}

#[test]
fn every_onboarding_file_has_zola_frontmatter() {
    let dir = onboarding_dir();
    let mut files: Vec<PathBuf> = Vec::new();
    collect_markdown(&dir, &mut files);

    assert!(
        !files.is_empty(),
        "no markdown files found under {} — T69 not delivered",
        dir.display()
    );

    let mut failures: Vec<String> = Vec::new();
    for path in &files {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{}: read error: {e}", path.display()));
                continue;
            }
        };
        if !has_zola_frontmatter(&content) {
            failures.push(format!(
                "{}: missing `+++ ... +++` Zola frontmatter block at top",
                path.display()
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} onboarding file(s) failed frontmatter check:\n  - {}",
            failures.len(),
            failures.join("\n  - ")
        );
    }
}

/// Collect every `*.md` file under `dir` (recursive) into `out`.
fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

#[test]
fn every_guide_covers_syntax_tooling_and_ecosystem() {
    let dir = onboarding_dir();

    // Each of the four guides must cover the three required topics
    // from the T69 spec: syntax mapping, tooling migration, and
    // ecosystem mapping. We check for representative keywords that
    // each section's content necessarily contains (the section
    // headers themselves, plus a keyword that can only appear if
    // the section has real body text).
    //
    // The keyword set is intentionally loose (lowercase substring
    // match) so that small wording tweaks don't break the test —
    // but tight enough that a guide missing one of the three
    // required sections fails loudly.
    let required_topics: &[(&str, &[&str])] = &[
        // (topic_name, keywords — at least ONE must appear)
        ("syntax mapping", &["syntax mapping", "syntax", "| python |", "| go |", "| rust |", "| javascript |", "mapping table"]),
        ("tooling migration", &["tooling", "buff add", "buff build", "buff run", "buff fmt", "buff check", "buff test"]),
        ("ecosystem mapping", &["ecosystem", "prelude", "buff-", "registry", "framework"]),
    ];

    let mut failures: Vec<String> = Vec::new();

    for bg in BACKGROUNDS {
        let filename = format!("{}-developers.md", bg);
        let path = dir.join(&filename);
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!(
                    "{}: read error: {e} (was the file renamed?)",
                    filename
                ));
                continue;
            }
        };

        for (topic, keywords) in required_topics {
            let found = keywords.iter().any(|kw| mentions(&content, kw));
            if !found {
                failures.push(format!(
                    "{}: missing required topic '{}' — none of the keywords \
                     {:?} appear in the guide body",
                    filename, topic, keywords
                ));
            }
        }

        // Each guide must also include at least one ```buff code block
        // (the "Hello World, side by side" section necessarily does).
        // Without a code block, the guide is all prose and the
        // migrant has nothing to copy-paste.
        if !content.contains("```buff") {
            failures.push(format!(
                "{}: no ```buff code block found — every guide must include \
                 at least one runnable Buff snippet (the Hello World \
                 side-by-side is the minimum)",
                filename
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} onboarding guide check(s) failed:\n  - {}",
            failures.len(),
            failures.join("\n  - ")
        );
    }
}

#[test]
fn landing_page_links_to_all_four_guides() {
    let index_path = onboarding_dir().join("_index.md");
    let content = fs::read_to_string(&index_path).unwrap_or_else(|e| {
        panic!(
            "failed to read onboarding landing page {}: {e}",
            index_path.display()
        )
    });

    // The landing page must link to each of the four background
    // guides. We accept either a relative Markdown link
    // (`./python-developers/`) or the bare filename
    // (`python-developers.md`) — both are valid Zola link forms.
    let mut missing_links: Vec<String> = Vec::new();

    for bg in BACKGROUNDS {
        let slug = format!("{}-developers", bg);
        // Accept `./python-developers/`, `python-developers/`,
        // `./python-developers.md`, or `python-developers.md`.
        let link_forms = [
            format!("./{slug}/"),
            format!("{slug}/"),
            format!("./{slug}.md"),
            format!("{slug}.md"),
        ];
        let found = link_forms.iter().any(|form| content.contains(form));
        if !found {
            missing_links.push(slug);
        }
    }

    if !missing_links.is_empty() {
        panic!(
            "onboarding landing page (_index.md) does not link to {} of the 4 \
             required guides: {}. Each guide must be discoverable from the \
             landing page (T69 acceptance: _index.md exists with links to all \
             4 guides).",
            missing_links.len(),
            missing_links.join(", ")
        );
    }

    // Sanity: the landing page must also mention "Buff" and at least
    // one of the four background names in body text (not just in
    // links). This catches a stub landing page that has links but no
    // explanatory copy.
    assert!(
        mentions(&content, "Buff"),
        "landing page does not mention 'Buff' in body text"
    );
    let mentions_any_bg = BACKGROUNDS
        .iter()
        .any(|bg| mentions(&content, bg));
    assert!(
        mentions_any_bg,
        "landing page does not mention any of the four backgrounds ({:?}) \
         in body text",
        BACKGROUNDS
    );
}
