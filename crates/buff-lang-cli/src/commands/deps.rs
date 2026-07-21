//! `buff deps [--why <PKG>]` — print the project's dependency tree
//! (T128).
//!
//! Reads `buff.toml` from the current directory and renders every
//! declared dependency across all three dependency kinds
//! (`[rust-deps]`, `[git-dependencies]`,
//! `[registry-dependencies]`) in cargo-tree style: package name,
//! version requirement, and source. The output shape is familiar to
//! Rust developers (mirrors `cargo tree`).
//!
//! ## Output
//!
//! ```text
//! demo v0.1.0
//! ├── [rust-deps]
//! │   └── tokio v1
//! ├── [git-dependencies]
//! │   └── lib v* (https://example/lib.buff)
//! └── [registry-dependencies]
//!     └── pkg-cycle v* (registry)
//! ```
//!
//! ## `--why <PKG>`
//!
//! Prints the chain explaining why `<PKG>` is present. Buff does not
//! yet resolve transitive dependencies (the cargo-project wiring that
//! would let `buff build` link a downloaded tarball is deferred per
//! the v1.6 milestone), so the chain is currently limited to DIRECT
//! declarations: which section lists `<PKG>`, the recorded version
//! requirement / source, and the root package that requires it.
//!
//! ## Errors
//!
//! - Missing or unparseable `buff.toml` in the current directory.
//! - `--why <PKG>` where `<PKG>` is not declared in any section.

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::config::BuffConfig;

/// Entry point for `buff deps [--why <PKG>]`.
///
/// Reads `buff.toml` from the current directory, renders the
/// dependency tree to stdout, and (if `--why` was supplied) appends
/// the chain explaining why `<PKG>` is present.
pub fn run(why: Option<&str>) -> Result<()> {
    let buff_toml = Path::new("buff.toml");
    let cfg = BuffConfig::load_from_file(buff_toml).with_context(|| {
        format!(
            "failed to load {} — run `buff init` first or change to a Buff project root",
            buff_toml.display(),
        )
    })?;

    let tree = render_tree(&cfg);
    println!("{tree}");

    if let Some(pkg) = why {
        let chain = render_why_chain(&cfg, pkg)?;
        println!();
        println!("{chain}");
    }

    Ok(())
}

/// Render the full dependency tree for a config (cargo-tree style).
///
/// Pure function — does no I/O. Exposed so integration tests can
/// assert against the rendered string without spawning the CLI.
pub fn render_tree(cfg: &BuffConfig) -> String {
    let mut out = String::new();

    let root_name = cfg
        .package
        .as_ref()
        .map(|p| p.name.as_str())
        .unwrap_or("<workspace>");
    let root_version = cfg
        .package
        .as_ref()
        .map(|p| p.version.as_str())
        .unwrap_or("?");
    out.push_str(&format!("{root_name} v{root_version}\n"));

    let sections = collect_sections(cfg);
    if sections.is_empty() {
        out.push_str("(no dependencies declared)\n");
        return out;
    }

    let last_section_idx = sections.len() - 1;
    for (i, section) in sections.iter().enumerate() {
        let is_last_section = i == last_section_idx;
        let branch = if is_last_section {
            "└──"
        } else {
            "├──"
        };
        out.push_str(&format!("{branch} [{}]\n", section.label));

        if section.entries.is_empty() {
            continue;
        }
        let last_entry_idx = section.entries.len() - 1;
        // The vertical-bar prefix keeps nested entries aligned with
        // their parent section (matches `cargo tree` indentation).
        let prefix = if is_last_section { "    " } else { "│   " };
        for (j, entry) in section.entries.iter().enumerate() {
            let is_last_entry = j == last_entry_idx;
            let entry_branch = if is_last_entry {
                "└──"
            } else {
                "├──"
            };
            out.push_str(&format!("{prefix}{entry_branch} {entry}\n"));
        }
    }

    out
}

/// Render the `--why <PKG>` explanation chain.
///
/// Returns `Err` when `<PKG>` is not declared in any of the three
/// dependency sections (the caller surfaces that as a CLI error).
/// Returns the rendered explanation otherwise.
pub fn render_why_chain(cfg: &BuffConfig, pkg: &str) -> Result<String> {
    let root_name = cfg
        .package
        .as_ref()
        .map(|p| p.name.as_str())
        .unwrap_or("<root>");

    let mut hits: Vec<String> = Vec::new();

    if let Some(v) = cfg.rust_deps.get(pkg) {
        hits.push(format!("Direct dependency in [rust-deps]: {pkg} v{v}"));
    }
    if let Some(d) = cfg.git_dependencies.get(pkg) {
        let mut line = format!("Direct dependency in [git-dependencies]: {pkg} ({}", d.git);
        if let Some(b) = &d.branch {
            line.push_str(&format!(", branch={b}"));
        }
        if let Some(t) = &d.tag {
            line.push_str(&format!(", tag={t}"));
        }
        if let Some(r) = &d.rev {
            line.push_str(&format!(", rev={r}"));
        }
        line.push(')');
        hits.push(line);
    }
    if let Some(d) = cfg.registry_dependencies.get(pkg) {
        hits.push(format!(
            "Direct dependency in [registry-dependencies]: {pkg} v{}",
            d.version
        ));
    }

    if hits.is_empty() {
        bail!(
            "package `{pkg}` is not declared in any dependency section of buff.toml \
             ([rust-deps], [git-dependencies], or [registry-dependencies])"
        );
    }

    let mut out = String::new();
    out.push_str(&format!("Why is `{pkg}` in the dependency tree?\n"));
    for hit in &hits {
        out.push_str(hit);
        out.push('\n');
        out.push_str(&format!("└── required by: {root_name}\n"));
    }
    Ok(out)
}

// -----------------------------------------------------------------------
// Internal helpers
// -----------------------------------------------------------------------

/// A single section in the dependency tree: a header label and the
/// formatted entries under it.
struct DepSection {
    label: &'static str,
    entries: Vec<String>,
}

/// Collect all non-empty dependency sections from the config, in
/// canonical order: rust-deps, git-dependencies, registry-dependencies.
///
/// Deterministic because each `BTreeMap` iterates in sorted-key order
/// and the section order is fixed. The same config always yields the
/// same sections vector — important for snapshot-style assertions.
fn collect_sections(cfg: &BuffConfig) -> Vec<DepSection> {
    let mut sections = Vec::new();

    if !cfg.rust_deps.is_empty() {
        let entries: Vec<String> = cfg
            .rust_deps
            .iter()
            .map(|(n, v)| format!("{n} v{v}"))
            .collect();
        sections.push(DepSection {
            label: "rust-deps",
            entries,
        });
    }

    if !cfg.git_dependencies.is_empty() {
        let entries: Vec<String> = cfg
            .git_dependencies
            .iter()
            .map(|(n, d)| format_git_entry(n, d))
            .collect();
        sections.push(DepSection {
            label: "git-dependencies",
            entries,
        });
    }

    if !cfg.registry_dependencies.is_empty() {
        let entries: Vec<String> = cfg
            .registry_dependencies
            .iter()
            .map(|(n, d)| format!("{n} v{} (registry)", d.version))
            .collect();
        sections.push(DepSection {
            label: "registry-dependencies",
            entries,
        });
    }

    sections
}

/// Render a `[git-dependencies]` entry as a tree leaf line.
///
/// Format: `name v* (qualifiers) (url)`. The version is always `*`
/// because git-dependencies pin by commit / branch / tag, not semver
/// (mirrors how Cargo renders git deps as `(git+url)`).
fn format_git_entry(name: &str, dep: &crate::config::GitDependency) -> String {
    let mut s = format!("{name} v*");
    if let Some(branch) = &dep.branch {
        s.push_str(&format!(" (branch={branch})"));
    }
    if let Some(tag) = &dep.tag {
        s.push_str(&format!(" (tag={tag})"));
    }
    if let Some(rev) = &dep.rev {
        s.push_str(&format!(" (rev={rev})"));
    }
    s.push_str(&format!(" ({})", dep.git));
    s
}

#[cfg(test)]
mod tests {
    //! Unit tests for the pure rendering helpers. End-to-end coverage
    //! (loading buff.toml + render) lives in `tests/deps_outdated_t128.rs`.

    use super::*;
    use crate::config::{
        BuffConfig, GitDependency, PackageSection, Profiles, RegistryDependency, WorkspaceSection,
    };
    use std::collections::BTreeMap;

    fn pkg(name: &str, version: &str) -> Option<PackageSection> {
        Some(PackageSection {
            name: name.to_string(),
            version: version.to_string(),
            edition: None,
            stability: None,
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
            features: Default::default(),
            lints: Default::default(),
            prelude: Default::default(),
        }
    }

    #[test]
    fn render_tree_empty_shows_placeholder() {
        let cfg = empty_cfg();
        let tree = render_tree(&cfg);
        assert!(tree.contains("demo v0.1.0"), "root line present: {tree}");
        assert!(
            tree.contains("(no dependencies declared)"),
            "empty placeholder: {tree}"
        );
    }

    #[test]
    fn render_tree_workspace_root_label() {
        let mut cfg = empty_cfg();
        cfg.package = None;
        cfg.workspace = Some(WorkspaceSection::default());
        let tree = render_tree(&cfg);
        assert!(
            tree.starts_with("<workspace> v?"),
            "workspace root label: {tree}"
        );
    }

    #[test]
    fn render_tree_includes_rust_deps_sorted() {
        let mut cfg = empty_cfg();
        cfg.rust_deps.insert("zlib".to_string(), "1".to_string());
        cfg.rust_deps.insert("serde".to_string(), "1.0".to_string());

        let tree = render_tree(&cfg);
        assert!(tree.contains("[rust-deps]"), "section header: {tree}");
        assert!(tree.contains("serde v1.0"), "serde entry: {tree}");
        assert!(tree.contains("zlib v1"), "zlib entry: {tree}");
        // Sorted: serde before zlib.
        let serde_pos = tree.find("serde v1.0").expect("serde emitted");
        let zlib_pos = tree.find("zlib v1").expect("zlib emitted");
        assert!(serde_pos < zlib_pos, "sorted order: {tree}");
    }

    #[test]
    fn render_tree_includes_git_dependencies_with_url() {
        let mut cfg = empty_cfg();
        cfg.git_dependencies.insert(
            "lib".to_string(),
            GitDependency {
                git: "https://example/lib.buff".to_string(),
                branch: Some("dev".to_string()),
                tag: None,
                rev: None,
            },
        );

        let tree = render_tree(&cfg);
        assert!(tree.contains("lib v*"), "lib entry: {tree}");
        assert!(tree.contains("branch=dev"), "branch qualifier: {tree}");
        assert!(tree.contains("https://example/lib.buff"), "git url: {tree}");
    }

    #[test]
    fn render_tree_includes_registry_dependencies() {
        let mut cfg = empty_cfg();
        cfg.registry_dependencies.insert(
            "pkg-cycle".to_string(),
            RegistryDependency {
                version: "^1.0.0".to_string(),
                checksum: None,
            },
        );

        let tree = render_tree(&cfg);
        assert!(tree.contains("pkg-cycle"), "name: {tree}");
        assert!(tree.contains("v^1.0.0"), "version: {tree}");
        assert!(tree.contains("(registry)"), "source: {tree}");
    }

    #[test]
    fn render_tree_renders_all_three_sections_together() {
        let mut cfg = empty_cfg();
        cfg.rust_deps.insert("tokio".to_string(), "1".to_string());
        cfg.git_dependencies.insert(
            "alpha".to_string(),
            GitDependency {
                git: "https://example/a.buff".to_string(),
                branch: None,
                tag: None,
                rev: None,
            },
        );
        cfg.registry_dependencies.insert(
            "beta".to_string(),
            RegistryDependency {
                version: "*".to_string(),
                checksum: None,
            },
        );

        let tree = render_tree(&cfg);
        let rust_pos = tree.find("[rust-deps]").expect("rust-deps section");
        let git_pos = tree
            .find("[git-dependencies]")
            .expect("git-dependencies section");
        let reg_pos = tree
            .find("[registry-dependencies]")
            .expect("registry-dependencies section");
        assert!(rust_pos < git_pos, "rust before git: {tree}");
        assert!(git_pos < reg_pos, "git before registry: {tree}");
    }

    #[test]
    fn render_tree_uses_last_branch_for_last_section() {
        let mut cfg = empty_cfg();
        cfg.rust_deps.insert("serde".to_string(), "1.0".to_string());
        cfg.registry_dependencies.insert(
            "pkg".to_string(),
            RegistryDependency {
                version: "*".to_string(),
                checksum: None,
            },
        );

        let tree = render_tree(&cfg);
        // Last section header should use └── not ├──.
        let reg_line = tree
            .lines()
            .find(|l| l.contains("[registry-dependencies]"))
            .expect("registry section line");
        assert!(
            reg_line.starts_with("└──"),
            "last section uses └──: {reg_line}"
        );
    }

    #[test]
    fn why_chain_errors_when_pkg_not_declared() {
        let cfg = empty_cfg();
        let result = render_why_chain(&cfg, "missing-pkg");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("missing-pkg"), "error names the pkg: {msg}");
    }

    #[test]
    fn why_chain_for_rust_dep() {
        let mut cfg = empty_cfg();
        cfg.rust_deps.insert("serde".to_string(), "1.0".to_string());

        let chain = render_why_chain(&cfg, "serde").expect("rust-dep chain");
        assert!(chain.contains("Why is `serde`"), "question: {chain}");
        assert!(chain.contains("[rust-deps]"), "section: {chain}");
        assert!(chain.contains("v1.0"), "version: {chain}");
        assert!(chain.contains("required by: demo"), "root: {chain}");
    }

    #[test]
    fn why_chain_for_git_dep_includes_qualifiers() {
        let mut cfg = empty_cfg();
        cfg.git_dependencies.insert(
            "lib".to_string(),
            GitDependency {
                git: "https://example/lib.buff".to_string(),
                branch: None,
                tag: Some("v1.0.0".to_string()),
                rev: None,
            },
        );

        let chain = render_why_chain(&cfg, "lib").expect("git-dep chain");
        assert!(chain.contains("[git-dependencies]"), "section: {chain}");
        assert!(chain.contains("tag=v1.0.0"), "tag qualifier: {chain}");
        assert!(chain.contains("https://example/lib.buff"), "url: {chain}");
    }

    #[test]
    fn why_chain_for_registry_dep() {
        let mut cfg = empty_cfg();
        cfg.registry_dependencies.insert(
            "pkg-cycle".to_string(),
            RegistryDependency {
                version: "^1.0.0".to_string(),
                checksum: None,
            },
        );

        let chain = render_why_chain(&cfg, "pkg-cycle").expect("reg-dep chain");
        assert!(
            chain.contains("[registry-dependencies]"),
            "section: {chain}"
        );
        assert!(chain.contains("v^1.0.0"), "version: {chain}");
    }

    #[test]
    fn format_git_entry_with_no_qualifiers() {
        let dep = GitDependency {
            git: "https://example/x.buff".to_string(),
            branch: None,
            tag: None,
            rev: None,
        };
        let s = format_git_entry("x", &dep);
        assert_eq!(s, "x v* (https://example/x.buff)");
    }

    #[test]
    fn format_git_entry_with_all_qualifiers() {
        let dep = GitDependency {
            git: "https://example/x.buff".to_string(),
            branch: Some("dev".to_string()),
            tag: Some("v1".to_string()),
            rev: Some("abc".to_string()),
        };
        let s = format_git_entry("x", &dep);
        // All three qualifiers appear in declaration order.
        assert!(s.contains("branch=dev"), "branch: {s}");
        assert!(s.contains("tag=v1"), "tag: {s}");
        assert!(s.contains("rev=abc"), "rev: {s}");
    }
}
