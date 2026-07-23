//! T67 — Documentation site smoke tests.
//!
//! These tests do NOT invoke `zola` (the build is documented in
//! `docs-site/README.md` but the binary is not a workspace dependency).
//! Instead they verify the *inputs* to a Zola build are well-formed:
//!
//! 1. `docs-site/config.toml` exists and parses as valid TOML.
//! 2. At least 10 Markdown files live under `docs-site/content/`.
//! 3. Every Markdown file begins with a Zola `+++` frontmatter block
//!    that itself parses as TOML and contains a `title`.
//!
//! Failing any of these means `zola build` would either crash or silently
//! emit a broken site, so they belong in CI.

use std::fs;
use std::path::{Path, PathBuf};

/// Locate the `docs-site/` directory relative to this crate's manifest.
///
/// `CARGO_MANIFEST_DIR` points at `crates/buff-lang-cli/`, so the repo
/// root is two levels up and `docs-site/` sits at the repo root.
fn docs_site_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs-site")
}

fn collect_markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

/// A Zola frontmatter block starts at byte 0 with `+++\n` and ends with
/// the next line that is exactly `+++`. Returns the inner TOML text on
/// success.
fn extract_frontmatter(content: &str) -> Option<&str> {
    let after_open = content.strip_prefix("+++\n")?;
    let close = after_open.find("\n+++\n").or_else(|| {
        // Last block in a file may end with `+++` without a trailing newline.
        after_open.find("\n+++")
    })?;
    Some(&after_open[..close])
}

#[test]
fn config_toml_exists_and_parses() {
    let path = docs_site_dir().join("config.toml");
    let toml_str = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    let parsed: toml::Value =
        toml::from_str(&toml_str).unwrap_or_else(|e| panic!("config.toml is not valid TOML: {e}"));

    let title = parsed
        .get("title")
        .and_then(|v| v.as_str())
        .expect("config.toml must define `title`");
    assert!(
        title.contains("Buff"),
        "config title should mention Buff, got: {title}"
    );

    let base_url = parsed
        .get("base_url")
        .and_then(|v| v.as_str())
        .expect("config.toml must define `base_url`");
    assert!(
        base_url.starts_with("https://"),
        "base_url should be https, got: {base_url}"
    );

    let search_enabled = parsed
        .get("build_search_index")
        .and_then(|v| v.as_bool())
        .expect("config.toml must define `build_search_index`");
    assert!(
        search_enabled,
        "docs site must enable build_search_index for in-site search"
    );
}

#[test]
fn at_least_ten_markdown_pages_exist() {
    let content_dir = docs_site_dir().join("content");
    let mut files = Vec::new();
    collect_markdown_files(&content_dir, &mut files);

    assert!(
        !files.is_empty(),
        "no .md files found under {} — did the docs-site move?",
        content_dir.display()
    );

    assert!(
        files.len() >= 10,
        "expected >= 10 markdown pages, found {}: {:?}",
        files.len(),
        files
    );
}

#[test]
fn every_markdown_file_has_zola_frontmatter_with_title() {
    let content_dir = docs_site_dir().join("content");
    let mut files = Vec::new();
    collect_markdown_files(&content_dir, &mut files);

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

        let fm = match extract_frontmatter(&content) {
            Some(f) => f,
            None => {
                failures.push(format!(
                    "{}: missing `+++ ... +++` frontmatter block at top of file",
                    path.display()
                ));
                continue;
            }
        };

        let parsed: toml::Value = match toml::from_str(fm) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!(
                    "{}: frontmatter is not valid TOML: {e}",
                    path.display()
                ));
                continue;
            }
        };

        let has_title = parsed
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if !has_title {
            failures.push(format!(
                "{}: frontmatter must define a non-empty `title`",
                path.display()
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} markdown file(s) failed frontmatter checks:\n  - {}",
            failures.len(),
            failures.join("\n  - ")
        );
    }
}

#[test]
fn required_pages_exist() {
    let required = [
        "_index.md",
        "getting-started/_index.md",
        "getting-started/installation.md",
        "getting-started/first-program.md",
        "getting-started/project-structure.md",
        "language/_index.md",
        "language/syntax.md",
        "language/types.md",
        "language/async.md",
        "language/error-handling.md",
        "language/attributes.md",
        "frameworks/_index.md",
        "frameworks/overview.md",
        "cookbook/_index.md",
        "migration/_index.md",
        // T63: error catalog pages (errors.buff-lang.org/E1xxx).
        "errors/_index.md",
        "errors/E10xx-lexer.md",
        "errors/E11xx-parser.md",
        "errors/E12xx-type.md",
        "errors/E13xx-codegen.md",
    ];

    let content_dir = docs_site_dir().join("content");
    for rel in required {
        let path = content_dir.join(rel);
        assert!(
            path.exists(),
            "required docs page missing: {} (full path: {})",
            rel,
            path.display()
        );
    }
}

#[test]
fn templates_and_static_assets_exist() {
    let site = docs_site_dir();
    let required = [
        "templates/base.html",
        "templates/index.html",
        "templates/page.html",
        "templates/section.html",
        "static/style.css",
        "static/robots.txt",
        "README.md",
    ];
    for rel in required {
        let path = site.join(rel);
        assert!(
            path.exists(),
            "required docs-site asset missing: {} (full path: {})",
            rel,
            path.display()
        );
    }
}
