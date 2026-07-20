//! `buff add <SPEC> [--branch <X> | --tag <X> | --rev <X>]` — add a
//! dependency to the project's `buff.toml` (T122 git, T127 registry).
//!
//! `<SPEC>` selects the dependency kind:
//!
//! - **`git+<URL>`** (T122): clones the repo to
//!   `~/.buff/git/<sha256(url)[..16]>/` and records it under
//!   `[git-dependencies]`. `--branch`/`--tag`/`--rev` qualifiers
//!   translate to the corresponding git-clone flags. The clone is
//!   reused on subsequent adds (idempotent).
//! - **`<name>` or `<name>@<req>`** (T127): resolves `<name>` against
//!   the buff registry (HTTP `/api/v1/resolve`), fetches its metadata
//!   (`/api/v1/package`), and records it under
//!   `[registry-dependencies]` with the resolved requirement. The
//!   registry URL comes from `$BUFF_REGISTRY_URL` (default
//!   `http://127.0.0.1:7878`). The `--branch`/`--tag`/`--rev` flags
//!   are ignored on this path (a warning is logged to stderr).
//!
//! ## What `buff add` does (git path)
//!
//! 1. **Validate** the spec (`git+` prefix present, URL non-empty).
//! 2. **Derive the dependency name** from the URL's last path segment,
//!    stripping `.buff` or `.git` if present (e.g. `lib.buff` → `lib`).
//! 3. **Clone** the repo to `~/.buff/git/<sha256(url)[..16]>/`. If the
//!    directory already exists, the clone is skipped (idempotent reuse).
//!    `--branch`/`--tag` translate to `git clone --branch <X>`;
//!    `--rev` runs a plain clone then `git -C <dir> checkout <rev>`.
//! 4. **Parse the cloned repo's `buff.toml`** (if present) for its
//!    transitive Buff dependencies and log them to stderr. Transitive
//!    resolution (recursively cloning them) is deferred to a post-v1.0
//!    registry task.
//! 5. **Upsert the entry** in the project's `buff.toml` under the
//!    `[git-dependencies]` section via a `toml::Value` round-trip that
//!    preserves all other sections.
//!
//! ## What `buff add` does (registry path)
//!
//! 1. **Parse the spec** into `(name, optional req)`. If `req` is
//!    absent it defaults to `*` (any version).
//! 2. **Resolve** `<name>` against `/api/v1/resolve/<name>?req=<req>`
//!    to get the highest published version.
//! 3. **Fetch metadata** via `/api/v1/package/<name>` and log the
//!    declared dependencies (informational).
//! 4. **Upsert the entry** in the project's `buff.toml` under the
//!    `[registry-dependencies]` section.
//!
//! ## Path strategy (git)
//!
//! The cloned checkout is shared across projects (one canonical copy per
//! URL). `generate_cargo_toml` emits a local-path dependency entry
//! pointing at the checkout — this is preferred over Cargo's native
//! `{ git = "..." }` form because it's offline-friendly (cargo never
//! re-fetches) and lets the user inspect/patch the checkout directly.
//!
//! ## Errors
//!
//! - Fails if `git` cannot be invoked (not installed / not in `PATH`)
//!   on the git path.
//! - Fails if `git clone` or `git checkout` exits non-zero.
//! - Fails if the project's `buff.toml` is missing or un-parseable.
//! - Fails if the registry is unreachable on the registry path.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::config::{git_checkout_path_for, BuffConfig, GitDependency, RegistryDependency};

/// The mandatory prefix on every `buff add` git spec.
const GIT_PREFIX: &str = "git+";

/// Entry point for `buff add <SPEC> [--branch <X> | --tag <X> | --rev <X>]`.
///
/// Detects whether `<SPEC>` is a git spec (`git+<URL>`) or a registry
/// spec (`<name>` / `<name>@<req>`) and dispatches accordingly. Falls
/// back to the git path with the existing validation error when the
/// spec doesn't match either shape.
pub fn run(spec: &str, branch: Option<&str>, tag: Option<&str>, rev: Option<&str>) -> Result<()> {
    if is_registry_spec(spec) {
        if branch.is_some() || tag.is_some() || rev.is_some() {
            eprintln!(
                "warning: --branch/--tag/--rev are git-dep qualifiers and \
                 are ignored on the registry path (`{spec}`)"
            );
        }
        let base_url = crate::commands::registry::registry_url();
        run_registry(spec, &base_url)
    } else {
        let home = crate::config::buff_home_dir().map_err(anyhow::Error::msg)?;
        run_with_home(spec, branch, tag, rev, &home)
    }
}

/// Detect whether the spec is a registry dep (`name` / `name@req`) or
/// a git dep (`git+<URL>`).
///
/// A registry spec must satisfy [`is_valid_registry_name`] on the
/// `<name>` portion (the part before any `@`). Anything else falls
/// through to the git path (which will reject it with the existing
/// "spec must start with `git+`" error).
pub fn is_registry_spec(spec: &str) -> bool {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with(GIT_PREFIX) {
        return false;
    }
    let (name, _req) = split_registry_spec(trimmed);
    is_valid_registry_name(name)
}

/// Split `<name>@<req>` into `(name, Some(req))`. Returns `(spec, None)`
/// when no `@` separator is present.
pub fn split_registry_spec(spec: &str) -> (&str, Option<&str>) {
    match spec.find('@') {
        Some(idx) => {
            let (name, rest) = spec.split_at(idx);
            // Skip the '@' itself.
            let req = &rest[1..];
            (name, Some(req))
        }
        None => (spec, None),
    }
}

/// Validate a registry package name locally. Mirrors the charset the
/// registry itself enforces (see `buff_registry::validate_name`):
/// ASCII lowercase, digits, hyphen, underscore; no path separators;
/// no `..` segment.
pub fn is_valid_registry_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Same as the git-path-only portion of [`run`], taking the Buff cache
/// home directory explicitly.
///
/// `buff_home` is the directory under which `git/<hash>/` checkouts are
/// placed (typically `~/.buff`). Used by integration tests to isolate
/// the checkout cache to a per-test tempdir. The registry path doesn't
/// need a cache home — see [`run_registry`] for that flow.
pub fn run_with_home(
    spec: &str,
    branch: Option<&str>,
    tag: Option<&str>,
    rev: Option<&str>,
    buff_home: &Path,
) -> Result<()> {
    // --- validate spec ---------------------------------------------------
    let url = spec.strip_prefix(GIT_PREFIX).with_context(|| {
        format!("spec must start with `git+` (e.g. `git+https://...`); got: {spec:?}")
    })?;
    if url.is_empty() {
        bail!("git dependency spec is missing its URL: {spec:?}");
    }

    let name = derive_dep_name(url)?;
    eprintln!("Resolving git dependency `{name}` from {url}");

    // --- clone (or reuse) checkout ---------------------------------------
    let checkout_dir = git_checkout_path_for(url, buff_home);
    if checkout_dir.exists() {
        eprintln!("Reusing existing checkout at {}", checkout_dir.display());
    } else {
        clone_checkout(url, branch, tag, rev, &checkout_dir)?;
    }

    // --- parse transitive deps from the cloned buff.toml -----------------
    if let Some(transitive) = read_transitive_deps(&checkout_dir)? {
        let n = transitive.dependencies.len();
        if n == 0 {
            eprintln!("Checked-out dep has no transitive Buff dependencies");
        } else {
            eprintln!("Checked-out dep declares {n} transitive Buff dependencies:");
            for (n, v) in &transitive.dependencies {
                eprintln!("  {n} = \"{v}\"");
            }
        }
    } else {
        eprintln!("Note: checked-out dep has no `buff.toml`; skipping transitive-dep parse");
    }

    // --- upsert the entry in the project's buff.toml ---------------------
    let project_toml = PathBuf::from("buff.toml");
    if !project_toml.is_file() {
        bail!(
            "no `buff.toml` in current directory — run `buff init` first or \
             change to a Buff project root"
        );
    }
    let dep = GitDependency {
        git: url.to_string(),
        branch: branch.map(String::from),
        tag: tag.map(String::from),
        rev: rev.map(String::from),
    };
    upsert_git_dependency(&project_toml, &name, &dep)?;
    eprintln!(
        "Added git dependency `{name}` to `[git-dependencies]` in {}",
        project_toml.display()
    );
    Ok(())
}

/// Registry-path entry point for `buff add <name>[@<req>]`.
///
/// 1. Parse the spec into `(name, req)`, defaulting `req` to `*`.
/// 2. Resolve via `/api/v1/resolve/<name>?req=<req>`.
/// 3. Fetch metadata via `/api/v1/package/<name>` (informational).
/// 4. Upsert the entry in the project's `buff.toml` under
///    `[registry-dependencies]`.
pub fn run_registry(spec: &str, base_url: &str) -> Result<()> {
    let (name, req_opt) = split_registry_spec(spec.trim());
    let req = req_opt.unwrap_or("*").trim();
    if !is_valid_registry_name(name) {
        bail!(
            "invalid registry package name `{name}` — allowed charset is \
             [a-z0-9_-], max 64 chars, no path separators or `..`"
        );
    }
    if req.is_empty() {
        bail!("invalid version requirement in spec `{spec}` — empty after `@`");
    }

    eprintln!("Resolving registry dependency `{name}` (req = {req}) on {base_url}");
    let resolved = crate::commands::registry::resolve_version(base_url, name, req)
        .with_context(|| format!("could not resolve `{name}` (req `{req}`) on {base_url}"))?;
    eprintln!("Resolved {name} -> {}", resolved.version);

    // Metadata fetch is informational — if it fails, the resolve already
    // proved the package exists, so we don't bail.
    match crate::commands::registry::fetch_package_metadata(base_url, name) {
        Ok(meta) => {
            let n_versions = meta.versions.len();
            let total_deps: usize = meta.versions.iter().map(|v| v.deps.len()).sum();
            eprintln!(
                "Package `{name}` has {n_versions} published version(s); \
                 {total_deps} total declared dep edge(s)"
            );
        }
        Err(e) => {
            eprintln!("warning: could not fetch metadata for `{name}` ({e:#})");
        }
    }

    let project_toml = PathBuf::from("buff.toml");
    if !project_toml.is_file() {
        bail!(
            "no `buff.toml` in current directory — run `buff init` first or \
             change to a Buff project root"
        );
    }
    let dep = RegistryDependency {
        version: req.to_string(),
        checksum: None,
    };
    upsert_registry_dependency(&project_toml, name, &dep)?;
    eprintln!(
        "Added registry dependency `{name}` (req `{req}`, resolved {}) to \
         `[registry-dependencies]` in {}",
        resolved.version,
        project_toml.display()
    );
    Ok(())
}

/// Derive the dependency name from a git URL: last path segment, with
/// `.buff` or `.git` stripped if present. URL `?query` / `#fragment`
/// parts are ignored.
///
/// # Examples
///
/// | Input | Output |
/// |-------|--------|
/// | `https://github.com/u/lib.buff` | `lib` |
/// | `https://github.com/u/lib.git` | `lib` |
/// | `https://github.com/u/mylib` | `mylib` |
/// | `file:///path/to/repo` | `repo` |
pub fn derive_dep_name(url: &str) -> Result<String> {
    // Drop `?query` and `#fragment` (Cargo permits them; the name comes
    // from the path alone).
    let url_no_qs = url.split(['?', '#']).next().unwrap_or(url);
    let last = url_no_qs
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("could not derive dependency name from URL: {url:?}"))?;
    let stripped = last
        .strip_suffix(".buff")
        .or_else(|| last.strip_suffix(".git"))
        .unwrap_or(last);
    if stripped.is_empty() {
        bail!("could not derive dependency name from URL: {url:?}");
    }
    Ok(stripped.to_string())
}

/// Clone `url` into `dest`. If `branch` or `tag` is set, pass
/// `--branch <X>` (precedence: tag > branch, mirroring Cargo's
/// finer-grained-wins convention). If `rev` is set, run a plain clone
/// then `git -C <dest> checkout <rev>`.
fn clone_checkout(
    url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
    rev: Option<&str>,
    dest: &Path,
) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut cmd = Command::new("git");
    cmd.arg("clone");
    // Tag wins over branch if both somehow set (tag is the finer ref).
    if let Some(t) = tag {
        cmd.arg("--branch").arg(t);
    } else if let Some(b) = branch {
        cmd.arg("--branch").arg(b);
    }
    cmd.arg(url).arg(dest);
    let result = cmd
        .output()
        .context("failed to invoke `git` — is it installed and on your PATH?")?;
    if !result.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&result.stderr));
    }
    if !result.status.success() {
        bail!("git clone exited with status {}", result.status);
    }
    if let Some(r) = rev {
        let result = Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("checkout")
            .arg(r)
            .output()
            .with_context(|| format!("failed to invoke git checkout {r} in {}", dest.display()))?;
        if !result.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&result.stderr));
        }
        if !result.status.success() {
            bail!("git checkout {r} exited with status {}", result.status);
        }
    }
    Ok(())
}

/// Read `<checkout>/buff.toml` and parse it into a [`BuffConfig`].
///
/// Returns `Ok(None)` when the checkout has no `buff.toml` (the
/// dependency is a pure-Rust crate or a non-Buff repo). Returns
/// `Err` on I/O errors other than "missing" or on parse failure.
pub fn read_transitive_deps(checkout: &Path) -> Result<Option<BuffConfig>> {
    let toml_path = checkout.join("buff.toml");
    let text = match fs::read_to_string(&toml_path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e).with_context(|| format!("failed to read {}", toml_path.display()));
        }
    };
    let cfg = BuffConfig::parse(&text)
        .map_err(|e| anyhow::anyhow!("failed to parse {}'s buff.toml: {e}", checkout.display()))?;
    Ok(Some(cfg))
}

/// Insert or replace `<name>` in the project's `buff.toml` under the
/// `[git-dependencies]` section.
///
/// The whole document is round-tripped through `toml::Value` to
/// preserve all other sections. On success the file is atomically
/// rewritten with the updated entry.
pub fn upsert_git_dependency(path: &Path, name: &str, dep: &GitDependency) -> Result<()> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut root: toml::Value = toml::from_str(&text)
        .with_context(|| format!("failed to parse {} as TOML", path.display()))?;
    let table = root
        .as_table_mut()
        .with_context(|| format!("{} root is not a table", path.display()))?;
    let git_deps = table
        .entry("git-dependencies".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let git_table = git_deps
        .as_table_mut()
        .with_context(|| "[git-dependencies] is not a table")?;
    let entry_value = toml::Value::try_from(dep)
        .with_context(|| format!("failed to serialize git dep {name}"))?;
    git_table.insert(name.to_string(), entry_value);
    let new_text = toml::to_string_pretty(&root)
        .with_context(|| format!("failed to serialize {}", path.display()))?;
    fs::write(path, new_text).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Insert or replace `<name>` in the project's `buff.toml` under the
/// `[registry-dependencies]` section (T127).
///
/// Mirrors [`upsert_git_dependency`]: the whole document is round-tripped
/// through `toml::Value` so every other section is preserved byte-for-byte
/// (modulo TOML pretty-printing).
pub fn upsert_registry_dependency(path: &Path, name: &str, dep: &RegistryDependency) -> Result<()> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut root: toml::Value = toml::from_str(&text)
        .with_context(|| format!("failed to parse {} as TOML", path.display()))?;
    let table = root
        .as_table_mut()
        .with_context(|| format!("{} root is not a table", path.display()))?;
    let reg_deps = table
        .entry("registry-dependencies".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let reg_table = reg_deps
        .as_table_mut()
        .with_context(|| "[registry-dependencies] is not a table")?;
    let entry_value = toml::Value::try_from(dep)
        .with_context(|| format!("failed to serialize registry dep {name}"))?;
    reg_table.insert(name.to_string(), entry_value);
    let new_text = toml::to_string_pretty(&root)
        .with_context(|| format!("failed to serialize {}", path.display()))?;
    fs::write(path, new_text).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Unit tests for the pure helpers in `commands::add`. The full
    //! end-to-end pipeline (clone → upsert) is exercised by
    //! `tests/git_dependencies_t122.rs` against a local git repo.

    use super::*;

    #[test]
    fn derive_name_strips_buff_suffix() {
        assert_eq!(
            derive_dep_name("https://github.com/u/lib.buff").unwrap(),
            "lib"
        );
    }

    #[test]
    fn derive_name_strips_git_suffix() {
        assert_eq!(
            derive_dep_name("https://github.com/u/lib.git").unwrap(),
            "lib"
        );
    }

    #[test]
    fn derive_name_keeps_other_suffixes() {
        assert_eq!(
            derive_dep_name("https://github.com/u/mylib").unwrap(),
            "mylib"
        );
    }

    #[test]
    fn derive_name_strips_query_and_fragment() {
        assert_eq!(
            derive_dep_name("https://github.com/u/lib.buff?rev=abc#frag").unwrap(),
            "lib"
        );
    }

    #[test]
    fn derive_name_handles_file_url() {
        assert_eq!(derive_dep_name("file:///path/to/repo").unwrap(), "repo");
    }

    #[test]
    fn derive_name_errors_on_empty_url() {
        assert!(derive_dep_name("").is_err());
    }

    #[test]
    fn git_prefix_must_be_present() {
        // Sanity check the prefix-stripping logic used by run().
        let s = "https://example/x.buff";
        assert!(s.strip_prefix(GIT_PREFIX).is_none());
        let s2 = "git+https://example/x.buff";
        assert_eq!(s2.strip_prefix(GIT_PREFIX), Some("https://example/x.buff"));
    }

    #[test]
    fn upsert_git_dependency_into_minimal_manifest() {
        let dir = std::env::temp_dir().join(format!(
            "buff-add-upsert-{}-{}",
            std::process::id(),
            "minimal"
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("buff.toml");
        fs::write(&path, "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();

        let dep = GitDependency {
            git: "https://example/lib.buff".to_string(),
            branch: Some("dev".to_string()),
            tag: None,
            rev: None,
        };
        upsert_git_dependency(&path, "lib", &dep).expect("upsert must succeed");

        let written = fs::read_to_string(&path).unwrap();
        // `toml::to_string_pretty` may emit the entry either as
        // `[git-dependencies]\nlib = {...}` (inline) or as
        // `[git-dependencies.lib]` (nested table) — both parse
        // identically. Assert the round-trip via BuffConfig::parse, plus
        // the substring "git-dependencies" appears as a section header
        // somewhere in the output.
        assert!(
            written.contains("git-dependencies"),
            "missing git-dependencies section: {written}"
        );
        assert!(written.contains("lib"), "missing dep name: {written}");
        assert!(
            written.contains("https://example/lib.buff"),
            "missing url: {written}"
        );
        assert!(written.contains("dev"), "missing branch: {written}");
        // Original [package] section must be preserved.
        assert!(
            written.contains("[package]"),
            "missing original section: {written}"
        );
        assert!(
            written.contains("name = \"demo\""),
            "missing original name: {written}"
        );

        // Re-parse with BuffConfig to validate schema round-trip.
        let cfg = BuffConfig::parse(&written).expect("round-trip parse");
        let lib = cfg.git_dependencies.get("lib").expect("lib present");
        assert_eq!(lib.git, "https://example/lib.buff");
        assert_eq!(lib.branch.as_deref(), Some("dev"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_git_dependency_replaces_existing_entry() {
        let dir = std::env::temp_dir().join(format!(
            "buff-add-upsert-{}-{}",
            std::process::id(),
            "replace"
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("buff.toml");
        fs::write(
            &path,
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\
             \n[git-dependencies]\n\
             lib = { git = \"https://example/lib.buff\", branch = \"old\" }\n",
        )
        .unwrap();

        // Upsert with a NEW branch — must replace, not duplicate.
        let dep = GitDependency {
            git: "https://example/lib.buff".to_string(),
            branch: Some("new".to_string()),
            tag: None,
            rev: None,
        };
        upsert_git_dependency(&path, "lib", &dep).expect("upsert must succeed");

        let written = fs::read_to_string(&path).unwrap();
        let cfg = BuffConfig::parse(&written).expect("round-trip parse");
        assert_eq!(cfg.git_dependencies.len(), 1, "must not duplicate");
        let lib = cfg.git_dependencies.get("lib").expect("lib present");
        assert_eq!(
            lib.branch.as_deref(),
            Some("new"),
            "branch must be replaced"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_git_dependency_preserves_unrelated_sections() {
        let dir = std::env::temp_dir().join(format!(
            "buff-add-upsert-{}-{}",
            std::process::id(),
            "preserve"
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("buff.toml");
        fs::write(
            &path,
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\
             \n[dependencies]\nserde = \"1.0\"\n\
             \n[rust-deps]\ntokio = \"1\"\n",
        )
        .unwrap();

        let dep = GitDependency {
            git: "https://example/lib.buff".to_string(),
            branch: None,
            tag: None,
            rev: None,
        };
        upsert_git_dependency(&path, "lib", &dep).expect("upsert must succeed");

        let written = fs::read_to_string(&path).unwrap();
        let cfg = BuffConfig::parse(&written).expect("round-trip parse");
        assert_eq!(
            cfg.dependencies.get("serde").map(|s| s.as_str()),
            Some("1.0")
        );
        assert_eq!(cfg.rust_deps.get("tokio").map(|s| s.as_str()), Some("1"));
        assert!(cfg.git_dependencies.contains_key("lib"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_transitive_deps_returns_none_when_no_toml() {
        let dir =
            std::env::temp_dir().join(format!("buff-add-transitive-{}-none", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // No buff.toml written.
        let cfg = read_transitive_deps(&dir).expect("must not error");
        assert!(cfg.is_none(), "no buff.toml → None");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_transitive_deps_parses_when_toml_present() {
        let dir = std::env::temp_dir().join(format!(
            "buff-add-transitive-{}-present",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("buff.toml"),
            "[package]\nname = \"lib\"\nversion = \"0.1.0\"\n\
             \n[dependencies]\nserde = \"1.0\"\n",
        )
        .unwrap();
        let cfg = read_transitive_deps(&dir)
            .expect("must parse")
            .expect("must be some");
        assert_eq!(cfg.package.as_ref().expect("package present").name, "lib");
        assert_eq!(
            cfg.dependencies.get("serde").map(|s| s.as_str()),
            Some("1.0")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // -------------------------------------------------------------------
    // T127 — registry-spec detection + upsert
    // -------------------------------------------------------------------

    #[test]
    fn is_registry_spec_recognizes_bare_name() {
        assert!(is_registry_spec("demo"));
        assert!(is_registry_spec("my-lib"));
        assert!(is_registry_spec("lib_2"));
    }

    #[test]
    fn is_registry_spec_recognizes_name_at_version() {
        assert!(is_registry_spec("demo@1.0.0"));
        assert!(is_registry_spec("demo@^1.0.0"));
        assert!(is_registry_spec("my-lib@*"));
    }

    #[test]
    fn is_registry_spec_rejects_git_prefix() {
        assert!(!is_registry_spec("git+https://example/x.buff"));
    }

    #[test]
    fn is_registry_spec_rejects_uppercase_and_path_traversal() {
        // Uppercase doesn't satisfy validate_name.
        assert!(!is_registry_spec("Demo"));
        // Path traversal rejected.
        assert!(!is_registry_spec("../evil"));
        assert!(!is_registry_spec("a/b"));
        // Empty.
        assert!(!is_registry_spec(""));
        assert!(!is_registry_spec("   "));
    }

    #[test]
    fn split_registry_spec_separates_name_and_req() {
        assert_eq!(split_registry_spec("demo"), ("demo", None));
        assert_eq!(split_registry_spec("demo@1.0.0"), ("demo", Some("1.0.0")));
        assert_eq!(split_registry_spec("demo@^1.0.0"), ("demo", Some("^1.0.0")));
        // Bare `@` (empty req) still splits — validation happens later.
        assert_eq!(split_registry_spec("demo@"), ("demo", Some("")));
    }

    #[test]
    fn is_valid_registry_name_accepts_canonical_form() {
        assert!(is_valid_registry_name("foo"));
        assert!(is_valid_registry_name("foo-bar"));
        assert!(is_valid_registry_name("foo_bar"));
        assert!(is_valid_registry_name("foo123"));
    }

    #[test]
    fn is_valid_registry_name_rejects_uppercase_traversal_long() {
        assert!(!is_valid_registry_name("Foo"));
        assert!(!is_valid_registry_name(""));
        assert!(!is_valid_registry_name("../evil"));
        assert!(!is_valid_registry_name("a/b"));
        assert!(!is_valid_registry_name(&"a".repeat(65)));
    }

    #[test]
    fn upsert_registry_dependency_into_minimal_manifest() {
        let dir = std::env::temp_dir().join(format!(
            "buff-add-reg-upsert-{}-minimal",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("buff.toml");
        fs::write(&path, "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();

        let dep = RegistryDependency {
            version: "^1.0.0".to_string(),
            checksum: None,
        };
        upsert_registry_dependency(&path, "lib", &dep).expect("upsert must succeed");

        let written = fs::read_to_string(&path).unwrap();
        assert!(
            written.contains("registry-dependencies"),
            "missing registry-dependencies section: {written}"
        );
        assert!(written.contains("lib"), "missing dep name: {written}");
        assert!(written.contains("^1.0.0"), "missing version req: {written}");
        // Original [package] section must be preserved.
        assert!(
            written.contains("[package]"),
            "missing original section: {written}"
        );
        assert!(
            written.contains("name = \"demo\""),
            "missing original name: {written}"
        );

        // Re-parse with BuffConfig to validate schema round-trip.
        let cfg = BuffConfig::parse(&written).expect("round-trip parse");
        let lib = cfg
            .registry_dependencies
            .get("lib")
            .expect("lib present in registry_dependencies");
        assert_eq!(lib.version, "^1.0.0");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_registry_dependency_replaces_existing_entry() {
        let dir = std::env::temp_dir().join(format!(
            "buff-add-reg-upsert-{}-replace",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("buff.toml");
        fs::write(
            &path,
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\
             \n[registry-dependencies]\n\
             lib = { version = \"^1.0.0\" }\n",
        )
        .unwrap();

        // Upsert with a NEW version requirement — must replace, not duplicate.
        let dep = RegistryDependency {
            version: "^2.0.0".to_string(),
            checksum: None,
        };
        upsert_registry_dependency(&path, "lib", &dep).expect("upsert must succeed");

        let written = fs::read_to_string(&path).unwrap();
        let cfg = BuffConfig::parse(&written).expect("round-trip parse");
        assert_eq!(
            cfg.registry_dependencies.len(),
            1,
            "must not duplicate: {written}"
        );
        let lib = cfg.registry_dependencies.get("lib").expect("lib present");
        assert_eq!(lib.version, "^2.0.0", "version must be replaced");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_registry_dependency_preserves_other_dep_sections() {
        let dir = std::env::temp_dir().join(format!(
            "buff-add-reg-upsert-{}-preserve",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("buff.toml");
        fs::write(
            &path,
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\
             \n[dependencies]\nserde = \"1.0\"\n\
             \n[git-dependencies]\n\
             gitlib = { git = \"https://example/g.buff\" }\n",
        )
        .unwrap();

        let dep = RegistryDependency {
            version: "*".to_string(),
            checksum: None,
        };
        upsert_registry_dependency(&path, "reglib", &dep).expect("upsert");

        let written = fs::read_to_string(&path).unwrap();
        let cfg = BuffConfig::parse(&written).expect("round-trip parse");
        // All three dependency sections must coexist.
        assert_eq!(
            cfg.dependencies.get("serde").map(|s| s.as_str()),
            Some("1.0")
        );
        assert!(cfg.git_dependencies.contains_key("gitlib"));
        assert!(cfg.registry_dependencies.contains_key("reglib"));

        let _ = fs::remove_dir_all(&dir);
    }
}
