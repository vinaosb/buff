//! T68 — Cookbook / Patterns Guide validation tests.
//!
//! These tests do NOT exercise the compiler. They verify the *existence,
//! structure, and contents* of the cookbook shipped as T68 in the v1.21.0
//! "Community & Quality" milestone.
//!
//! The cookbook is a collection of copy-pasteable Buff recipes at
//! `docs-site/content/cookbook/`, organised by category (HTTP, Files,
//! JSON, Database, Parallel, Async, Errors, Testing, Strings, DataFrame).
//! Each recipe follows the Problem → Solution → Explanation shape and
//! must carry at least one ```buff fenced code block.
//!
//! Acceptance (per T68 spec): 50+ recipes published, each recipe tested
//! (code blocks present and non-empty), 5 tests. Failing any of these
//! means the cookbook is incomplete or malformed — release blocker.

use std::fs;
use std::path::{Path, PathBuf};

/// Locate the cookbook directory relative to this crate's manifest.
///
/// `CARGO_MANIFEST_DIR` points at `crates/buff-lang-cli/`, so the repo
/// root is two levels up; the cookbook lives at
/// `<repo-root>/docs-site/content/cookbook/`.
fn cookbook_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs-site")
        .join("content")
        .join("cookbook")
}

/// The required cookbook pages (T68 spec MUST-DO list). Each must exist.
const REQUIRED_PAGES: &[&str] = &[
    "_index.md",
    "http.md",
    "files.md",
    "json.md",
    "database.md",
    "parallel.md",
    "async.md",
    "errors.md",
    "testing.md",
    "strings.md",
    "dataframe.md",
];

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

/// Count the number of `## ` (level-2) ATX headers in `content`.
///
/// Each recipe uses `## Recipe Name` as its header; section headers in
/// `_index.md` are also counted (the 50+ threshold is comfortably met
/// either way).
fn count_h2_headers(content: &str) -> usize {
    content
        .lines()
        .filter(|line| line.starts_with("## "))
        .count()
}

/// Collect the bodies of every `\`\`\`buff ... \`\`\`` fenced code block
/// in `content`. Returns a vec of (start_byte, body_text) pairs.
fn collect_buff_code_blocks(content: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("```buff") {
            let mut body = String::new();
            for inner in lines.by_ref() {
                if inner.trim_start() == "```" {
                    break;
                }
                body.push_str(inner);
                body.push('\n');
            }
            blocks.push(body);
        }
    }
    blocks
}

#[test]
fn all_required_cookbook_pages_exist() {
    let dir = cookbook_dir();
    assert!(
        dir.is_dir(),
        "cookbook directory missing: {} — did the docs-site move?",
        dir.display()
    );

    for rel in REQUIRED_PAGES {
        let path = dir.join(rel);
        assert!(
            path.exists(),
            "required cookbook page missing: {} (full path: {})",
            rel,
            path.display()
        );
    }
}

#[test]
fn at_least_fifty_recipe_headers_exist() {
    let dir = cookbook_dir();
    let mut files = Vec::new();
    collect_markdown(&dir, &mut files);

    assert!(
        !files.is_empty(),
        "no markdown files found under {} — T68 not delivered",
        dir.display()
    );

    let total_headers: usize = files
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok())
        .map(|c| count_h2_headers(&c))
        .sum();

    // The T68 acceptance criterion is "50+ recipes published". Each
    // recipe uses a level-2 header; we sum across all cookbook files
    // (including the index, whose section headers contribute to the
    // total — the threshold is met comfortably either way).
    assert!(
        total_headers >= 50,
        "expected >= 50 recipe (`## `) headers across the cookbook, found \
         {}. T68 acceptance: 50+ recipes published.",
        total_headers
    );
}

#[test]
fn every_cookbook_markdown_file_has_zola_frontmatter() {
    let dir = cookbook_dir();
    let mut files = Vec::new();
    collect_markdown(&dir, &mut files);

    assert!(!files.is_empty(), "no markdown files to check");

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
            "{} cookbook file(s) failed frontmatter check:\n  - {}",
            failures.len(),
            failures.join("\n  - ")
        );
    }
}

#[test]
fn every_recipe_has_a_buff_code_block() {
    let dir = cookbook_dir();
    let mut files = Vec::new();
    collect_markdown(&dir, &mut files);

    // The `_index.md` landing page documents the recipe format itself
    // (and includes a sample `\`\`\`buff` block inside a markdown code
    // fence) — we exclude it from the per-recipe pairing check because
    // its `## ` headers are section headings, not recipes.
    let category_files: Vec<PathBuf> = files
        .iter()
        .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some("_index.md"))
        .cloned()
        .collect();

    assert!(
        !category_files.is_empty(),
        "no category files found — every recipe file is missing?"
    );

    let mut failures: Vec<String> = Vec::new();

    for path in &category_files {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{}: read error: {e}", path.display()));
                continue;
            }
        };

        // Split the file into sections starting at each `## ` header.
        // The body of each section is everything until the next `## `
        // header (or end of file). Each section must contain at least
        // one `\`\`\`buff` fenced code block.
        let mut current_section: Option<(String, String)> = None;
        let mut sections: Vec<(String, String)> = Vec::new();

        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("## ") {
                if let Some((header, body)) = current_section.take() {
                    sections.push((header, body));
                }
                current_section = Some((rest.to_string(), String::new()));
            } else if let Some((_, body)) = current_section.as_mut() {
                body.push_str(line);
                body.push('\n');
            }
        }
        if let Some((header, body)) = current_section {
            sections.push((header, body));
        }

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        for (header, body) in sections {
            if !body.contains("```buff") {
                failures.push(format!(
                    "{}: recipe `## {}` has no ```buff code block",
                    file_name, header
                ));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} recipe(s) missing a ```buff code block:\n  - {}",
            failures.len(),
            failures.join("\n  - ")
        );
    }
}

#[test]
fn no_empty_recipe_code_blocks() {
    let dir = cookbook_dir();
    let mut files = Vec::new();
    collect_markdown(&dir, &mut files);

    let mut failures: Vec<String> = Vec::new();
    let mut total_blocks: usize = 0;

    for path in &files {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{}: read error: {e}", path.display()));
                continue;
            }
        };

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        for (idx, body) in collect_buff_code_blocks(&content).into_iter().enumerate() {
            total_blocks += 1;
            // "Non-empty" = at least one non-blank line after trimming.
            // A code block that's all whitespace is treated as empty.
            let non_blank_lines = body.lines().filter(|l| !l.trim().is_empty()).count();
            if non_blank_lines == 0 {
                failures.push(format!(
                    "{}: code block #{} is empty (no non-blank lines)",
                    file_name,
                    idx + 1
                ));
            }
        }
    }

    assert!(
        total_blocks >= 50,
        "expected >= 50 ```buff code blocks across the cookbook, found {}. \
         T68 acceptance: each recipe tested.",
        total_blocks
    );

    if !failures.is_empty() {
        panic!(
            "{} code block(s) are empty:\n  - {}",
            failures.len(),
            failures.join("\n  - ")
        );
    }
}
