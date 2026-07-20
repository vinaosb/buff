//! `buff.toml` manifest parsing + project-layout enforcement (T111).
//!
//! A Buff project is described by a `buff.toml` manifest at its root, mirroring
//! the role Cargo's `Cargo.toml` plays for a Rust crate. This module owns the
//! in-memory representation ([`BuffConfig`]) plus the parsing and validation
//! entry points the CLI needs.
//!
//! ## Layout
//!
//! ```toml
//! [package]
//! name = "my_app"
//! version = "0.2.1"
//! edition = "0.1"
//!
//! [dependencies]
//! serde = "1.0"
//!
//! [profile.release]
//! opt-level = 3
//! lto = true
//! ```
//!
//! ## Design notes
//!
//! - **`toml` + `serde` derive**: the manifest is parsed via [`toml::from_str`]
//!   on top of `serde::Deserialize` impls. This is robust and idiomatic; the
//!   compiler itself is allowed to depend on whatever it needs (the
//!   "no external crates" rule applies to *generated Buff projects*, not to the
//!   Buff compiler).
//! - **Deterministic ordering**: [`BuffConfig::dependencies`] uses
//!   [`BTreeMap`](std::collections::BTreeMap) so that iterating deps yields a
//!   stable, sorted order (important for `buff.lock` generation and snapshot
//!   tests). A `HashMap` would produce non-deterministic output.
//! - **Permissive optionals**: only `name` and `version` under `[package]` are
//!   required. `edition`, `[dependencies]`, and `[profile.release]` are all
//!   optional (defaulting to `None` / empty), so a minimal manifest still
//!   parses. Unknown fields are silently ignored by `serde` (forward-compat).
//! - **Profile option values** (`opt-level`, `lto`, `codegen-units`, …) are
//!   captured as `Option<String>` rather than typed enums. TOML allows both
//!   integers (`opt-level = 3`) and booleans (`lto = true`) for these keys;
//!   coercing to `String` lets us accept both without a per-field enum, and a
//!   future pass can tighten the typing once the options set stabilises.
//!
//! ## v0.5 scope
//!
//! This module provides **parsing + structural validation** only. The wider
//! integration story (loading `buff.toml` from every CLI command, resolving
//! external deps, generating `buff.lock`, workspace support) is intentionally
//! deferred to later v0.5/v1.0 tasks — see the T111 notepad entry for the
//! deferral list. The acceptance gate is the `config_parsing` test suite.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Deserialise a TOML scalar (string / int / float / bool) into a `String`.
///
/// Buff profile options are heterogeneous in TOML — `opt-level = 3` (int),
/// `lto = true` (bool), `panic = "unwind"` (string). Coercing them all to
/// `String` lets us accept any of these without a per-field enum, and preserves
/// the original value's textual form (e.g. `lto` becomes `"true"`).
///
/// Used via `#[serde(deserialize_with = "deserialize_scalar_string")]` on
/// `Option<String>` fields in [`ProfileOpts`].
fn deserialize_scalar_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Deserialise the value as a generic `toml::Value` (which `toml` knows how
    // to produce for any scalar, table, or array) and stringify it.
    //
    // Why not a hand-rolled Visitor? TOML's `Deserializer` is happiest when
    // driven through `toml::Value::deserialize` — it owns the type dispatch
    // (integer → i64, bool → bool, string → str, …) and avoids the
    // `deserialize_option` / `visit_some` round-trip that confused earlier
    // iterations of this code.
    let opt: Option<toml::Value> = Option::deserialize(deserializer)?;
    Ok(opt.map(|v| scalar_value_to_string(&v)))
}

/// Stringify a [`toml::Value`] scalar. Booleans render as `"true"`/`"false"`,
/// integers/floats via `Display`, strings verbatim. Non-scalar variants
/// (arrays/tables) fall back to `Debug` so we never silently drop information.
fn scalar_value_to_string(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

/// Top-level `buff.toml` representation.
///
/// Fields are kept `pub` because the CLI commands that consume a config (e.g.
/// `buff build`, `buff run`) read them directly — there is no invariant to
/// guard that would justify getter methods for v0.5.
///
/// # Virtual workspace manifests (T123)
///
/// Exactly one of [`BuffConfig::package`] / [`BuffConfig::workspace`] must
/// be `Some` after parsing. A *regular* Buff project has `[package]` and
/// no `[workspace]`. A *virtual workspace* manifest has `[workspace]` with
/// `members = [...]` and no `[package]` — mirroring Cargo's virtual
/// manifest rule. [`BuffConfig::parse`] enforces this invariant after
/// deserialisation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BuffConfig {
    /// `[package]` — required for single-package projects, MUST be `None`
    /// on a virtual workspace manifest (T123). Made `Option` so a single
    /// `BuffConfig` type can represent both shapes.
    pub package: Option<PackageSection>,
    /// `[dependencies]` — name → version-req string. Defaults to empty.
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    /// `[profile]` — optional profile tables. Defaults to empty.
    #[serde(default)]
    pub profile: Profiles,
    /// `[rust-deps]` — Rust crate dependencies auto-populated from
    /// `extern` declarations (T119). Each entry is `name = "version"`
    /// mirroring `[dependencies]`. Defaults to empty (programs without
    /// extern declarations have no Rust deps beyond the Buff runtime).
    /// Unknown keys under `[rust-deps]` are silently ignored (forward-compat
    /// — same behaviour as the rest of the manifest).
    #[serde(default, rename = "rust-deps")]
    pub rust_deps: BTreeMap<String, String>,
    /// `[git-dependencies]` — git-source Buff package dependencies (T122).
    /// Each entry is `name = { git = "URL", branch/tag/rev = "..." }`,
    /// mirroring Cargo's git-dep shape. The CLI `buff add git+URL` command
    /// clones the repo to `~/.buff/git/<sha256(url)[..16]>/` and inserts
    /// an entry here. Defaults to empty (forward-compat — manifests
    /// written before T122 still parse).
    #[serde(default, rename = "git-dependencies")]
    pub git_dependencies: BTreeMap<String, GitDependency>,
    /// `[registry-dependencies]` — registry-source Buff package
    /// dependencies (T127). Each entry is `name = { version = "req" }`,
    /// where `version` is a Cargo-style semver requirement (`^1.0.0`,
    /// `*`, etc.). Populated by `buff add <name>[@version]` after
    /// resolving against a `buff-registry` HTTP endpoint. Defaults to
    /// empty (forward-compat — manifests written before T127 still
    /// parse).
    ///
    /// The actual tarball download / unpack / link step into the
    /// generated Cargo project is deferred (mirrors the v0.5+v1.0
    /// "cargo-project wiring is deferred" precedent the rest of the
    /// CLI already follows); for v1.6 the entry is recorded so a
    /// future `buff build` can resolve it.
    #[serde(default, rename = "registry-dependencies")]
    pub registry_dependencies: BTreeMap<String, RegistryDependency>,
    /// `[workspace]` — virtual workspace manifest (T123). When `Some`,
    /// this `buff.toml` is a workspace ROOT that lists member project
    /// subdirectories; the generated `Cargo.toml` emits a matching
    /// `[workspace]` virtual manifest (no `[package]`). When `None`,
    /// the manifest is a regular single-package project.
    ///
    /// Mutually-exclusive with [`BuffConfig::package`] — see the type-level
    /// doc. [`BuffConfig::parse`] enforces the invariant.
    #[serde(default)]
    pub workspace: Option<WorkspaceSection>,
}

/// `[workspace]` section of a virtual `buff.toml` (T123).
///
/// Mirrors Cargo's `[workspace]` table: a list of member subdirectories
/// plus an optional `resolver` version. The Buff CLI is a strict
/// passthrough — it does not invent its own workspace semantics, it
/// simply emits a virtual `Cargo.toml` and lets `cargo build` / `cargo
/// test` fan out to all members.
///
/// # Determinism
///
/// [`WorkspaceSection::members`] is a `Vec<String>` (NOT a `BTreeSet`)
/// because member ORDER is user-meaningful — cargo resolves duplicate
/// dependency versions across members in workspace order, and the user
/// may want a specific member to win ties. We preserve declared order.
/// [`generate_cargo_toml`] output is still idempotent for a given input.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct WorkspaceSection {
    /// Member project subdirectories (e.g. `["crates/core", "crates/utils"]`).
    /// Paths are relative to the workspace root (where `buff.toml` lives).
    /// Order is preserved as-declared — see the type-level doc.
    #[serde(default)]
    pub members: Vec<String>,
    /// Workspace resolver version (`"1"` or `"2"`). When `None` at emit
    /// time, [`generate_cargo_toml`] defaults to `"2"` (the modern
    /// resolver; this matches Cargo's recommendation for edition 2021+).
    #[serde(default)]
    pub resolver: Option<String>,
}

/// A single `[git-dependencies]` entry (T122).
///
/// Mirrors Cargo's git-dep shape: a mandatory `git = "URL"` plus optional
/// `branch` / `tag` / `rev` qualifiers. All fields are serialised so the
/// entry round-trips through `toml::Value` for the buff.toml upsert path
/// (see [`commands::add`](crate::commands::add)).
///
/// # Determinism
///
/// `Serialize` skips `Option::None` fields so the emitted TOML contains
/// only the qualifiers the user actually set (no `branch = ""` clutter).
/// Iterating a `BTreeMap<String, GitDependency>` yields entries in
/// alphabetical-name order, so [`generate_cargo_toml`] output is
/// byte-deterministic for a given config.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GitDependency {
    /// Clone URL (e.g. `https://github.com/user/lib.buff`). The `git+`
    /// prefix from the CLI spec is stripped before this field is stored.
    pub git: String,
    /// Optional branch qualifier (passed to `git clone --branch`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Optional tag qualifier (also passed to `git clone --branch`; git
    /// accepts tags there).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Optional commit-ish (SHA short or long) — applied via
    /// `git -C <checkout> checkout <rev>` AFTER the initial clone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
}

/// A single `[registry-dependencies]` entry (T127).
///
/// Mirrors the registry-source slice of Cargo's `[dependencies]` table:
/// a mandatory `version` requirement string. The CLI `buff add
/// <name>[@req]` command resolves the requirement against a
/// `buff-registry` HTTP endpoint and inserts an entry here.
///
/// # Determinism
///
/// `Serialize` skips `Option::None` fields so the emitted TOML contains
/// only the qualifiers the user actually set (no `checksum = ""`
/// clutter). Iterating a `BTreeMap<String, RegistryDependency>` yields
/// entries in alphabetical-name order, so any future `generate_cargo_toml`
/// extension stays byte-deterministic for a given config.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RegistryDependency {
    /// Cargo-style semver requirement resolved by `buff add` (e.g.
    /// `^1.0.0`, `*`, `=1.2.3`). Stored verbatim from the user-facing
    /// `name@req` spec, defaulting to `*` when no version is given.
    pub version: String,
    /// Optional content hash (sha256 hex) of the resolved tarball, for
    /// future integrity verification on `buff build`. Currently never
    /// set by `buff add`; reserved here so adding it later is a pure
    /// addition (no schema break).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

/// `[package]` section of `buff.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PackageSection {
    /// Crate name. Must match Buff identifier rules (validated downstream by
    /// [`scaffold::validate_project_name`](crate::scaffold::validate_project_name)).
    pub name: String,
    /// Semver-style version string (e.g. `"0.2.1"`). Stored verbatim — the
    /// parser does not enforce semver well-formedness in v0.5.
    pub version: String,
    /// Optional Buff language edition (e.g. `"0.1"`). Mirrors Cargo's
    /// `edition` field; absent in legacy manifests.
    #[serde(default)]
    pub edition: Option<String>,
}

/// `[profile]` table — collection of named build profiles.
///
/// Only `release` is modelled today (mirroring the T111 spec); `dev` and
/// custom profiles can be added later without breaking the parse (serde will
/// ignore unknown sub-tables).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Profiles {
    /// `[profile.release]` — optimisation flags for release builds.
    #[serde(default)]
    pub release: Option<ProfileOpts>,
}

/// A single `[profile.*]` table.
///
/// All fields are optional and stored as `String` to accept the heterogeneous
/// types TOML permits (ints, bools, strings). Unknown profile keys are ignored.
///
/// **Field renaming**: TOML kebab-case keys (`opt-level`, `codegen-units`) are
/// mapped to snake_case Rust fields via `#[serde(rename = ...)]`. `lto`,
/// `panic`, `strip` need no rename because they're already single-word keys.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct ProfileOpts {
    /// Optimisation level (e.g. `3`). TOML int → string.
    #[serde(
        default,
        rename = "opt-level",
        deserialize_with = "deserialize_scalar_string"
    )]
    pub opt_level: Option<String>,
    /// Link-time optimisation (e.g. `true`). TOML bool → string.
    #[serde(default, deserialize_with = "deserialize_scalar_string")]
    pub lto: Option<String>,
    /// Codegen units (e.g. `16`). TOML int → string.
    #[serde(
        default,
        rename = "codegen-units",
        deserialize_with = "deserialize_scalar_string"
    )]
    pub codegen_units: Option<String>,
    /// Panic strategy (`"unwind"` or `"abort"`). TOML string.
    #[serde(default, deserialize_with = "deserialize_scalar_string")]
    pub panic: Option<String>,
    /// Strip debuginfo (`true` / `false`). TOML bool → string.
    #[serde(default, deserialize_with = "deserialize_scalar_string")]
    pub strip: Option<String>,
}

/// Errors surfaced by the config module.
///
/// Each variant preserves the underlying cause via [`std::fmt::Display`] so the
/// CLI's error mapper can render a useful diagnostic without losing the
/// original `toml`/`io` message.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// TOML syntax error OR serde deserialisation failure (missing required
    /// field, wrong type, …). Wraps the `toml` crate's `de::Error`.
    #[error("failed to parse buff.toml: {0}")]
    Parse(#[from] toml::de::Error),
    /// File-system error reading `buff.toml` from disk.
    #[error("failed to read buff.toml: {0}")]
    Io(#[from] std::io::Error),
    /// Project layout violation — required directory missing (e.g. no `src/`).
    #[error("invalid project layout: {0}")]
    Layout(String),
}

impl BuffConfig {
    /// Parse a `buff.toml` document from its text representation.
    ///
    /// Returns [`ConfigError::Parse`] on any syntax or schema error — never
    /// panics. The caller is expected to surface the error through the CLI's
    /// normal error mapper.
    ///
    /// # Post-parse invariant (T123)
    ///
    /// Exactly one of `[package]` / `[workspace]` must be present:
    ///
    /// - Regular project → `[package]` present, `[workspace]` absent.
    /// - Virtual workspace → `[workspace]` present, `[package]` absent.
    ///
    /// Both absent or both present returns [`ConfigError::Layout`] — the
    /// caller surfaces it as a user-facing diagnostic.
    pub fn parse(toml_text: &str) -> Result<Self, ConfigError> {
        let cfg: Self = toml::from_str(toml_text)?;
        match (&cfg.package, &cfg.workspace) {
            (Some(_), None) | (None, Some(_)) => Ok(cfg),
            (None, None) => Err(ConfigError::Layout(
                "buff.toml has neither [package] nor [workspace] \
                 — a regular project needs [package], a workspace root needs [workspace]"
                    .to_string(),
            )),
            (Some(_), Some(_)) => Err(ConfigError::Layout(
                "buff.toml is ambiguous: contains both [package] and [workspace] \
                 — a virtual workspace manifest must omit [package]"
                    .to_string(),
            )),
        }
    }

    /// Load and parse a `buff.toml` file from disk.
    ///
    /// Reads the file synchronously then delegates to [`BuffConfig::parse`].
    /// A missing file surfaces as [`ConfigError::Io`].
    pub fn load_from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        Self::parse(&text)
    }

    /// `true` when this config is a virtual workspace manifest (T123).
    ///
    /// Convenience accessor — equivalent to `self.workspace.is_some()`.
    /// Used by `buff build` / `buff test` to switch into workspace mode
    /// (emit virtual `Cargo.toml`, transpile each member, shell out to
    /// cargo at the workspace root).
    pub fn is_workspace(&self) -> bool {
        self.workspace.is_some()
    }
}

/// Validate that a Buff project directory conforms to the required layout.
///
/// The contract mirrors what `buff new` / `buff init` scaffold:
///
/// - **Required:** `src/` directory (with at least one `.buff` file is
///   *recommended* but not enforced here — see [`has_entry_point`]).
/// - **Recommended:** `tests/` directory. When absent a [`ConfigError::Layout`]
///   error is returned whose message mentions `tests`; callers may downgrade
///   this to a warning if they want to permit the project anyway (e.g. a
///   brand-new project created before any test was written).
///
/// Returns `Ok(())` only when both required and recommended checks pass.
pub fn validate_project_layout(dir: &Path) -> Result<(), ConfigError> {
    if !dir.is_dir() {
        return Err(ConfigError::Layout(format!(
            "project root does not exist or is not a directory: {}",
            dir.display()
        )));
    }
    let src = dir.join("src");
    if !src.is_dir() {
        return Err(ConfigError::Layout(format!(
            "missing required `src/` directory under {}",
            dir.display()
        )));
    }
    let tests = dir.join("tests");
    if !tests.is_dir() {
        return Err(ConfigError::Layout(format!(
            "missing recommended `tests/` directory under {} \
             (create it with: mkdir tests)",
            dir.display()
        )));
    }
    Ok(())
}

/// Generate a complete `Cargo.toml` string from a `BuffConfig` manifest.
///
/// The output is deterministic (sorted keys, stable formatting) so that
/// running this function twice on the same config yields byte-identical
/// output (idempotency guarantee).
///
/// # Two emission modes
///
/// - **Single-package mode** (default, v0.1 behaviour): `cfg.workspace`
///   is `None`, `cfg.package` is `Some`. Emits a regular `Cargo.toml`
///   with `[package]` + `[[bin]]` + `[dependencies]`.
/// - **Virtual workspace mode** (T123): `cfg.workspace` is `Some`,
///   `cfg.package` is `None`. Emits a Cargo virtual manifest with
///   `[workspace]` + `members` + `resolver = "2"` and NO `[package]`.
///   `cargo build` / `cargo test` invoked at this root fans out to all
///   members automatically (the whole point of passthrough — Buff does
///   NOT reinvent workspace dep-dedup or shared-`target/`).
///
/// # Idempotency
///
/// `generate_cargo_toml(cfg) == generate_cargo_toml(cfg)` for any `cfg`.
/// The function uses sorted iteration and a fixed format string — no
/// HashMap iteration, no environment-dependent output. Member order in
/// workspace mode is preserved as-declared (see [`WorkspaceSection`]
/// doc for why order matters).
pub fn generate_cargo_toml(cfg: &BuffConfig) -> String {
    // Workspace virtual manifest mode (T123).
    if let Some(ws) = &cfg.workspace {
        return generate_workspace_cargo_toml(ws);
    }

    // Single-package mode (v0.1 behaviour).
    // Defensive: parse() guarantees package is Some when workspace is None,
    // but we never panic — fall through to an empty-comment Cargo.toml so
    // any future edge case surfaces as a build error rather than a crash.
    let package = match &cfg.package {
        Some(p) => p,
        None => {
            return "# ERROR: buff.toml has neither [package] nor [workspace]\n".to_string();
        }
    };

    let mut out = String::new();

    // [package]
    out.push_str("[package]\n");
    out.push_str(&format!("name = \"{}\"\n", package.name));
    out.push_str(&format!("version = \"{}\"\n", package.version));
    out.push_str("edition = \"2021\"\n");

    // Optional edition override from buff.toml
    if let Some(ed) = &package.edition {
        // Map Buff edition to Rust edition. For now all Buff editions map to
        // Rust 2021 (the pinned workspace edition).
        out.push_str(&format!("# buff edition: {ed}\n"));
    }

    // [[bin]] — the transpiled entry point
    out.push_str("\n[[bin]]\n");
    out.push_str(&format!("name = \"{}\"\n", package.name));
    out.push_str("path = \"src/main.rs\"\n");

    // [dependencies] — sorted by key (BTreeMap guarantees this)
    if !cfg.dependencies.is_empty() {
        out.push_str("\n[dependencies]\n");
        for (name, version) in &cfg.dependencies {
            out.push_str(&format!("{name} = \"{version}\"\n"));
        }
    }

    // [rust-deps] entries go into [dependencies] in the generated Cargo.toml
    // (they are Rust crate dependencies, not Buff package deps).
    if !cfg.rust_deps.is_empty() {
        // If [dependencies] section already exists, entries are appended.
        // If not, we need to start the section.
        if cfg.dependencies.is_empty() {
            out.push_str("\n[dependencies]\n");
        }
        for (name, version) in &cfg.rust_deps {
            out.push_str(&format!("{name} = \"{version}\"\n"));
        }
    }

    // [git-dependencies] entries emit as local-path deps pointing at the
    // cloned checkout (T122). The local-path form is preferred over
    // Cargo's native `{ git = "..." }` form because:
    //   - it's offline-friendly (cargo never re-fetches),
    //   - it matches the "clone to ~/.buff/git" design (one canonical
    //     checkout per URL, shared across projects),
    //   - it lets the user inspect / patch the checkout directly.
    // Path resolution is delegated to [`git_checkout_path`], which is
    // deterministic (sha256 of the URL). On the rare case the home dir
    // can't be resolved (env unset), we emit a comment instead of a
    // malformed dep entry so the build breaks loudly rather than silently.
    if !cfg.git_dependencies.is_empty() {
        if cfg.dependencies.is_empty() && cfg.rust_deps.is_empty() {
            out.push_str("\n[dependencies]\n");
        }
        for (name, dep) in &cfg.git_dependencies {
            match git_checkout_path(&dep.git) {
                Ok(path) => {
                    let path_str = path.display().to_string().replace('\\', "/");
                    out.push_str(&format!("{name} = {{ path = \"{path_str}\" }}\n"));
                }
                Err(_) => {
                    out.push_str(&format!(
                        "# WARNING: could not resolve git checkout path for `{name}` \
                         (set USERPROFILE or HOME)\n"
                    ));
                }
            }
        }
    }

    // [registry-dependencies] entries (T127) are recorded in buff.toml
    // but have NO cargo-project wiring yet — the tarball download /
    // unpack / vendor step is deferred (mirrors the v0.5+v1.0
    // "cargo-project wiring is deferred" precedent). We emit a
    // comment-only block so:
    //   - the user sees the dep is recorded,
    //   - the build breaks loudly (no silent skipped dep),
    //   - a future `buff build` extension can replace the comment with
    //     a real `name = { path = "<vendor>/<name>-<version>" }` entry
    //     once vendoring lands.
    if !cfg.registry_dependencies.is_empty() {
        out.push_str("\n# [registry-dependencies] (T127 — cargo wiring TODO)\n");
        for (name, dep) in &cfg.registry_dependencies {
            out.push_str(&format!(
                "# {name} = \"{}\" (resolve + vendor on next `buff build`)\n",
                dep.version
            ));
        }
    }

    out
}

/// Emit a Cargo virtual workspace manifest from a [`WorkspaceSection`] (T123).
///
/// Produces:
///
/// ```toml
/// [workspace]
/// resolver = "2"
///
/// members = [
///     "pkg-a",
///     "pkg-b",
/// ]
/// ```
///
/// - `resolver` defaults to `"2"` (the modern resolver; recommended for
///   edition 2021+) when the user left it unset in `buff.toml`.
/// - `members` preserves declared order (see [`WorkspaceSection`] doc).
/// - Empty `members` emits `members = []` on a single line (degenerate
///   but syntactically valid; guards against a panic-on-empty regression).
fn generate_workspace_cargo_toml(ws: &WorkspaceSection) -> String {
    let mut out = String::new();
    out.push_str("[workspace]\n");
    let resolver = ws.resolver.as_deref().unwrap_or("2");
    out.push_str(&format!("resolver = \"{resolver}\"\n"));

    if ws.members.is_empty() {
        out.push_str("members = []\n");
    } else {
        out.push_str("\nmembers = [\n");
        for member in &ws.members {
            out.push_str(&format!("    \"{member}\",\n"));
        }
        out.push_str("]\n");
    }
    out
}

/// Returns `true` if `dir/src/` contains at least one `.buff` file.
///
/// Convenience helper for commands that want to verify an entry point exists
/// before invoking the pipeline. Not part of the structural layout contract
/// enforced by [`validate_project_layout`].
pub fn has_entry_point(dir: &Path) -> bool {
    let src = match dir.join("src").read_dir() {
        Ok(rd) => rd,
        Err(_) => return false,
    };
    for entry in src.flatten() {
        if entry.path().extension().is_some_and(|ext| ext == "buff") {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// T122 — git-dependency checkout path helpers
// ---------------------------------------------------------------------------

/// Resolve the Buff cache home directory: `<BUFF_HOME>` if set, else
/// `<USERPROFILE>/.buff` (Windows) or `<HOME>/.buff` (Unix).
///
/// The `BUFF_HOME` override lets integration tests isolate the checkout
/// cache without mutating process-wide env vars. The `USERPROFILE` /
/// `HOME` fallback covers normal CLI use.
///
/// Returns [`ConfigError::Layout`] when no env var is set — the error
/// variant's message field explains how to fix it.
pub fn buff_home_dir() -> Result<PathBuf, ConfigError> {
    if let Ok(custom) = std::env::var("BUFF_HOME") {
        if !custom.is_empty() {
            return Ok(PathBuf::from(custom));
        }
    }
    let base = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| {
            ConfigError::Layout(
                "could not determine home directory \
                 (set BUFF_HOME, USERPROFILE, or HOME)"
                    .to_string(),
            )
        })?;
    Ok(PathBuf::from(base).join(".buff"))
}

/// Compute the deterministic checkout path for a git URL:
/// `<buff_home>/git/<sha256(url)[..16]>/`.
///
/// Pure variant of [`git_checkout_path`] that takes the home dir
/// explicitly — used by tests and by `commands::add::run_with_home` so
/// they don't read env vars. The hash is the first 16 hex chars of
/// SHA-256(URL) — 64 bits of entropy, plenty to avoid collisions in a
/// single user's checkout cache.
pub fn git_checkout_path_for(url: &str, buff_home: &Path) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    // 32 bytes -> 64 hex chars; we take the first 16 (64 bits).
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let short = &hex[..16];
    buff_home.join("git").join(short)
}

/// Compute the checkout path for a git URL using the env-resolved
/// [`buff_home_dir`]. Returns [`ConfigError::Layout`] if home can't be
/// resolved.
pub fn git_checkout_path(url: &str) -> Result<PathBuf, ConfigError> {
    let home = buff_home_dir()?;
    Ok(git_checkout_path_for(url, &home))
}

/// Render a `[rust-deps]` TOML block from a set of crate names (T119).
///
/// Each name becomes a `<name> = "*"` entry — the wildcard version says
/// "use the latest compatible version" (the Cargo equivalent of omitting
/// the version requirement). This is the loosest possible spec and is
/// appropriate for the auto-populated section: the user can tighten a
/// specific version later by editing `buff.toml` directly.
///
/// The function is deterministic: a [`BTreeSet`] (sorted) iterator
/// guarantees byte-identical output across runs for the same input set
/// (the T29 flaky-test lesson — never rely on HashSet iteration order
/// for codegen output).
///
/// Returns an empty `String` when the set is empty — callers can detect
/// "no extern declarations" via `is_empty()` and skip emitting the
/// section entirely.
///
/// # Example output
///
/// For the input `{"serde_json", "tokio"}`, the output is:
///
/// ```toml
/// [rust-deps]
/// serde_json = "*"
/// tokio = "*"
/// ```
pub fn render_rust_deps_toml(deps: &std::collections::BTreeSet<String>) -> String {
    if deps.is_empty() {
        return String::new();
    }
    let mut out = String::from("[rust-deps]\n");
    for name in deps {
        // Use Debug-style quoting on the name? No — TOML keys accept
        // bare identifiers (incl. `_` and `-`), so emit them verbatim.
        // The version is always `"*"` for the auto-populated form.
        out.push_str(&format!("{name} = \"*\"\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    //! Unit smoke tests for the config module. The full acceptance-gate suite
    //! lives in `tests/config_parsing.rs` (integration) — these inline tests
    //! cover the small helpers (`has_entry_point`, profile coercion) that
    //! don't warrant a separate integration binary.

    use super::*;
    use std::fs;

    fn temp(unique: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "buff-lang-cli-config-unit-{}-{}",
            std::process::id(),
            unique
        ));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn has_entry_point_false_when_no_buff_file() {
        let dir = temp("no_entry");
        let _ = fs::create_dir_all(dir.join("src"));
        assert!(!has_entry_point(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn has_entry_point_true_with_buff_file() {
        let dir = temp("with_entry");
        let src = dir.join("src");
        let _ = fs::create_dir_all(&src);
        fs::write(src.join("main.buff"), "// stub\n").expect("write stub");
        assert!(has_entry_point(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn has_entry_point_false_when_no_src_dir() {
        let dir = temp("no_src");
        // Deliberately do NOT create src/.
        assert!(!has_entry_point(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn profile_opts_accepts_unknown_fields_silently() {
        // Forward-compat: unknown profile keys must not break the parse.
        let toml = r#"[package]
name = "x"
version = "0.1.0"

[profile.release]
opt-level = 3
future-flag = "wow"
"#;
        let cfg = BuffConfig::parse(toml).expect("unknown profile key should be tolerated");
        let release = cfg.profile.release.expect("release present");
        assert_eq!(release.opt_level.as_deref(), Some("3"));
    }

    // -----------------------------------------------------------------------
    // T119: `[rust-deps]` auto-population helpers
    // -----------------------------------------------------------------------

    #[test]
    fn render_rust_deps_toml_empty_set_yields_empty_string() {
        let deps = std::collections::BTreeSet::new();
        assert_eq!(render_rust_deps_toml(&deps), "");
    }

    #[test]
    fn render_rust_deps_toml_single_entry() {
        let mut deps = std::collections::BTreeSet::new();
        deps.insert("serde_json".to_string());
        let toml = render_rust_deps_toml(&deps);
        assert!(toml.contains("[rust-deps]"));
        assert!(toml.contains("serde_json = \"*\""));
    }

    #[test]
    fn render_rust_deps_toml_deterministic_sorted_order() {
        // BTreeSet iteration is sorted — output must be deterministic
        // regardless of insertion order (the T29 flaky-test lesson).
        let mut deps = std::collections::BTreeSet::new();
        deps.insert("zlib".to_string());
        deps.insert("serde".to_string());
        deps.insert("tokio".to_string());
        let toml = render_rust_deps_toml(&deps);
        // Lines after the header must be in alphabetical order.
        let lines: Vec<&str> = toml.lines().skip(1).collect();
        assert_eq!(
            lines,
            vec!["serde = \"*\"", "tokio = \"*\"", "zlib = \"*\""]
        );
    }

    #[test]
    fn config_accepts_rust_deps_section() {
        let toml = r#"[package]
name = "demo"
version = "0.1.0"

[rust-deps]
serde_json = "*"
tokio = "1"
"#;
        let cfg = BuffConfig::parse(toml).expect("[rust-deps] must parse");
        assert_eq!(
            cfg.rust_deps.get("serde_json").map(|s| s.as_str()),
            Some("*")
        );
        assert_eq!(cfg.rust_deps.get("tokio").map(|s| s.as_str()), Some("1"));
    }

    #[test]
    fn config_rust_deps_defaults_empty_when_absent() {
        let toml = r#"[package]
name = "demo"
version = "0.1.0"
"#;
        let cfg = BuffConfig::parse(toml).expect("minimal manifest must parse");
        assert!(
            cfg.rust_deps.is_empty(),
            "absent [rust-deps] defaults to empty"
        );
    }

    // -----------------------------------------------------------------------
    // T122: `[git-dependencies]` parsing + Cargo.toml emission
    // -----------------------------------------------------------------------

    #[test]
    fn config_accepts_git_dependencies_section_plain_url() {
        let toml = r#"[package]
name = "demo"
version = "0.1.0"

[git-dependencies]
lib = { git = "https://github.com/u/lib.buff" }
"#;
        let cfg = BuffConfig::parse(toml).expect("[git-dependencies] must parse");
        let lib = cfg.git_dependencies.get("lib").expect("lib entry present");
        assert_eq!(lib.git, "https://github.com/u/lib.buff");
        assert!(lib.branch.is_none());
        assert!(lib.tag.is_none());
        assert!(lib.rev.is_none());
    }

    #[test]
    fn config_accepts_git_dependencies_with_branch() {
        let toml = r#"[package]
name = "demo"
version = "0.1.0"

[git-dependencies]
lib = { git = "https://github.com/u/lib.buff", branch = "dev" }
"#;
        let cfg = BuffConfig::parse(toml).expect("[git-dependencies] with branch must parse");
        let lib = cfg.git_dependencies.get("lib").expect("lib entry");
        assert_eq!(lib.branch.as_deref(), Some("dev"));
        assert!(lib.tag.is_none());
        assert!(lib.rev.is_none());
    }

    #[test]
    fn config_accepts_git_dependencies_with_tag() {
        let toml = r#"[package]
name = "demo"
version = "0.1.0"

[git-dependencies]
lib = { git = "https://github.com/u/lib.buff", tag = "v1.0.0" }
"#;
        let cfg = BuffConfig::parse(toml).expect("[git-dependencies] with tag must parse");
        let lib = cfg.git_dependencies.get("lib").expect("lib entry");
        assert_eq!(lib.tag.as_deref(), Some("v1.0.0"));
        assert!(lib.branch.is_none());
        assert!(lib.rev.is_none());
    }

    #[test]
    fn config_accepts_git_dependencies_with_rev() {
        let toml = r#"[package]
name = "demo"
version = "0.1.0"

[git-dependencies]
lib = { git = "https://github.com/u/lib.buff", rev = "abc1234" }
"#;
        let cfg = BuffConfig::parse(toml).expect("[git-dependencies] with rev must parse");
        let lib = cfg.git_dependencies.get("lib").expect("lib entry");
        assert_eq!(lib.rev.as_deref(), Some("abc1234"));
        assert!(lib.branch.is_none());
        assert!(lib.tag.is_none());
    }

    #[test]
    fn config_git_dependencies_defaults_empty_when_absent() {
        let toml = r#"[package]
name = "demo"
version = "0.1.0"
"#;
        let cfg = BuffConfig::parse(toml).expect("minimal manifest must parse");
        assert!(
            cfg.git_dependencies.is_empty(),
            "absent [git-dependencies] defaults to empty"
        );
    }

    #[test]
    fn config_git_dependencies_multiple_entries_sorted() {
        // BTreeMap iteration is sorted — determinism contract.
        let toml = r#"[package]
name = "demo"
version = "0.1.0"

[git-dependencies]
zlib = { git = "https://example/z.buff" }
alpha = { git = "https://example/a.buff" }
mid = { git = "https://example/m.buff" }
"#;
        let cfg = BuffConfig::parse(toml).expect("multiple git deps must parse");
        let keys: Vec<&str> = cfg.git_dependencies.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["alpha", "mid", "zlib"]);
    }

    #[test]
    fn git_dependency_serialize_skips_none_qualifiers() {
        let dep = GitDependency {
            git: "https://example/x.buff".to_string(),
            branch: None,
            tag: None,
            rev: None,
        };
        let v = toml::Value::try_from(&dep).expect("serialize");
        let table = v.as_table().expect("serialized as table");
        assert_eq!(
            table.get("git").and_then(|v| v.as_str()),
            Some("https://example/x.buff")
        );
        // skip_serializing_if = Option::is_none → qualifiers must be absent
        // (not present as empty strings).
        assert!(!table.contains_key("branch"));
        assert!(!table.contains_key("tag"));
        assert!(!table.contains_key("rev"));
    }

    #[test]
    fn git_dependency_serialize_emits_set_qualifiers() {
        let dep = GitDependency {
            git: "https://example/x.buff".to_string(),
            branch: Some("dev".to_string()),
            tag: None,
            rev: Some("deadbeef".to_string()),
        };
        let v = toml::Value::try_from(&dep).expect("serialize");
        let table = v.as_table().expect("serialized as table");
        assert_eq!(table.get("branch").and_then(|v| v.as_str()), Some("dev"));
        assert_eq!(table.get("rev").and_then(|v| v.as_str()), Some("deadbeef"));
        // tag is None → must be absent.
        assert!(!table.contains_key("tag"));
    }

    #[test]
    fn generate_cargo_toml_emits_path_dep_for_git_dependency() {
        let cfg = BuffConfig {
            package: Some(PackageSection {
                name: "demo".to_string(),
                version: "0.1.0".to_string(),
                edition: None,
            }),
            dependencies: BTreeMap::new(),
            profile: Default::default(),
            rust_deps: BTreeMap::new(),
            git_dependencies: {
                let mut m = BTreeMap::new();
                m.insert(
                    "lib".to_string(),
                    GitDependency {
                        git: "https://example/lib.buff".to_string(),
                        branch: None,
                        tag: None,
                        rev: None,
                    },
                );
                m
            },
            registry_dependencies: BTreeMap::new(),
            workspace: None,
        };
        let cargo = generate_cargo_toml(&cfg);
        // Section header present.
        assert!(cargo.contains("[dependencies]"), "cargo.toml: {cargo}");
        // Path entry present (forward-slash form regardless of host OS).
        assert!(
            cargo.contains("lib = { path ="),
            "missing path dep entry: {cargo}"
        );
        // The path points under `.buff/git/<16-hex-chars>/`.
        assert!(
            cargo.contains("/.buff/git/") || cargo.contains("\\.buff\\git\\"),
            "missing buff/git dir in path: {cargo}"
        );
    }

    #[test]
    fn generate_cargo_toml_emits_multiple_git_deps_in_sorted_order() {
        let cfg = BuffConfig {
            package: Some(PackageSection {
                name: "demo".to_string(),
                version: "0.1.0".to_string(),
                edition: None,
            }),
            dependencies: BTreeMap::new(),
            profile: Default::default(),
            rust_deps: BTreeMap::new(),
            git_dependencies: {
                let mut m = BTreeMap::new();
                m.insert(
                    "zlib".to_string(),
                    GitDependency {
                        git: "https://example/z.buff".to_string(),
                        branch: None,
                        tag: None,
                        rev: None,
                    },
                );
                m.insert(
                    "alpha".to_string(),
                    GitDependency {
                        git: "https://example/a.buff".to_string(),
                        branch: None,
                        tag: None,
                        rev: None,
                    },
                );
                m
            },
            registry_dependencies: BTreeMap::new(),
            workspace: None,
        };
        let cargo = generate_cargo_toml(&cfg);
        let alpha_pos = cargo.find("alpha = { path =").expect("alpha emitted");
        let zlib_pos = cargo.find("zlib = { path =").expect("zlib emitted");
        assert!(alpha_pos < zlib_pos, "alpha must precede zlib: {cargo}");
    }

    #[test]
    fn generate_cargo_toml_no_git_section_when_empty() {
        let cfg = BuffConfig {
            package: Some(PackageSection {
                name: "demo".to_string(),
                version: "0.1.0".to_string(),
                edition: None,
            }),
            dependencies: BTreeMap::new(),
            profile: Default::default(),
            rust_deps: BTreeMap::new(),
            git_dependencies: BTreeMap::new(),
            registry_dependencies: BTreeMap::new(),
            workspace: None,
        };
        let cargo = generate_cargo_toml(&cfg);
        // No [dependencies] section at all when ALL four are empty.
        assert!(
            !cargo.contains("[dependencies]"),
            "unexpected deps section: {cargo}"
        );
        // The `[[bin]]` block always emits `path = "src/main.rs"` so we
        // check for the dependency-path form specifically.
        assert!(
            !cargo.contains("= { path ="),
            "no path dep entries when git deps empty: {cargo}"
        );
    }

    #[test]
    fn git_checkout_path_for_is_deterministic_and_differs_per_url() {
        let home = PathBuf::from("/tmp/buff-home-fixture");
        let a1 = git_checkout_path_for("https://example/a.buff", &home);
        let a2 = git_checkout_path_for("https://example/a.buff", &home);
        let b = git_checkout_path_for("https://example/b.buff", &home);
        assert_eq!(a1, a2, "same URL must yield same path");
        assert_ne!(
            a1.file_name(),
            b.file_name(),
            "different URLs must yield different hash dirs"
        );
        // Hash dir is exactly 16 hex chars.
        let name = a1.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name.len(), 16, "hash dir name length: {name}");
        assert!(
            name.chars().all(|c| c.is_ascii_hexdigit()),
            "hash dir name must be hex: {name}"
        );
    }

    #[test]
    fn git_checkout_path_for_targets_buff_git_subdir() {
        let home = PathBuf::from("/tmp/buff-home-fixture");
        let p = git_checkout_path_for("https://example/x.buff", &home);
        // Path components: `<home>/git/<16-hex>/`.
        assert!(
            p.starts_with(&home),
            "must start with home: {}",
            p.display()
        );
        assert!(
            p.starts_with(home.join("git")),
            "must be under git/: {}",
            p.display()
        );
    }
}
