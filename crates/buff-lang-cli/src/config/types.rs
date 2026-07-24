//! Config struct/enum definitions - extracted from `config.rs` (T106 mechanical split).
//!
//! All serialisable config sections (BuffConfig, WorkspaceSection,
//! PackageSection, Profiles, FeaturesSection, LintsSection, PreludeSection),
//! dependency structs (GitDependency, RegistryDependency), the Stability
//! enum, ConfigError, and the TOML deserialisation helpers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;
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
    /// `[features]` — named feature flags for conditional compilation
    /// (T0-A1, Buff SDK 2.0). Source code gates decls with `@feature(name)`
    /// (T0-B4); codegen emits only those whose name appears in the
    /// resolved feature set. The `default` sub-key lists features
    /// enabled when the user doesn't pass `--features`.
    ///
    /// Forward-compatible: an absent `[features]` section deserialises to
    /// an empty [`FeaturesSection`] (no features defined; `@feature(name)`
    /// always drops — matches Cargo/Rust `#[cfg(feature)]` semantics).
    #[serde(default)]
    pub features: FeaturesSection,
    /// `[lints]` — project-wide lint policy (T0-A1). Currently surfaces
    /// a single `clippy` level (`"deny"|"warn"|"allow"`) consumed by
    /// `buff check`. The section is open-ended — additional lint names
    /// can be added later without breaking the parse (serde ignores
    /// unknown keys per the established forward-compat rule).
    #[serde(default)]
    pub lints: LintsSection,
    /// `[prelude]` — project-wide implicit imports (T0-A1, T0-B3).
    /// Lists module paths whose `export`s become ambient in every
    /// source file of the project (mirrors Rust's `extern_prelude`
    /// concept). Codegen injects the equivalent of `import * from
    /// "<path>"` at the head of each compiled file. Empty by default.
    #[serde(default)]
    pub prelude: PreludeSection,
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
    /// `[workspace.dependencies]` — workspace-level Buff package
    /// declarations inherited by members via `<dep>.workspace = true`
    /// (T0-A3b). Mirrors Cargo's `[workspace.dependencies]` pattern:
    /// declare once at the workspace root, members reference. Prevents
    /// version drift across crates in a monorepo. Member crates still
    /// list the dep in their own `[dependencies]`/`[git-dependencies]`/
    /// `[registry-dependencies]` table — the workspace entry is the
    /// canonical version source when the member's entry is missing or
    /// explicitly carries `workspace = true` (currently informational;
    /// member-side inheritance flag parsing arrives with T1's resolver).
    ///
    /// Stored as a `BTreeMap` so iteration is deterministic — matches
    /// the rest of the manifest's determinism contract.
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    /// `[workspace.extern]` — workspace-level Rust crate (`extern`)
    /// declarations inherited by members (T0-A3b). Same inheritance
    /// shape as [`WorkspaceSection::dependencies`] but for Rust crates
    /// surfaced via Buff `extern` blocks (T119). Each entry is
    /// `name = "version-req"`; members opt in via their `[rust-deps]`
    /// table.
    #[serde(default)]
    pub extern_crates: BTreeMap<String, String>,
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
    /// Optional Buff language edition (e.g. `"0.1"`, `"2026"`). Mirrors
    /// Cargo's `edition` field; absent in legacy manifests. Edition
    /// `"2026"` (T0-A1) is the v2 default for new projects scaffolded
    /// by `buff new`; v1 manifests keep their existing edition.
    #[serde(default)]
    pub edition: Option<String>,
    /// Stability badge consumed by `buff publish` + the registry (T0-G2).
    /// One of `"experimental"`, `"beta"`, `"stable"`, `"locked"`. When
    /// `None`, the registry treats the package as `"experimental"` by
    /// default. Surfaced in search results, package pages, and the
    /// `buff add` resolution log.
    ///
    /// Stored verbatim — the registry enforces the enum at upload time.
    /// See [`Stability`] for the parsed form used by the CLI.
    #[serde(default)]
    pub stability: Option<Stability>,
}

/// Stability badge for a Buff package (T0-G2).
///
/// Surfaces in `buff publish`, the registry, and `buff add` resolution
/// so users can decide whether to depend on a package. The ladder
/// matches the conventions used by the Rust ecosystem (`nightly` →
/// `beta` → `stable`):
///
/// - [`Stability::Experimental`] — API in flux; no compatibility promise.
/// - [`Stability::Beta`] — API mostly settled; minor breakage possible.
/// - [`Stability::Stable`] — SemVer holds; safe to depend on.
/// - [`Stability::Locked`] — Stable + version-pinned downstream
///   (`{ version = "=X.Y.Z" }`) — used by foundational libs.
///
/// Serialises as a lowercase kebab string (`"experimental"`, `"beta"`,
/// `"stable"`, `"locked"`) so the round-trip through `buff.toml` is
/// transparent. Unknown values fall back to [`Stability::Experimental`]
/// at deserialise time (forward-compat — new badges added later don't
/// break older CLIs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stability {
    Experimental,
    #[default]
    Beta,
    Stable,
    Locked,
}

impl Stability {
    /// Lowercase badge string used in user-facing output (e.g. registry
    /// listings, `buff add` resolution log).
    pub fn as_str(&self) -> &'static str {
        match self {
            Stability::Experimental => "experimental",
            Stability::Beta => "beta",
            Stability::Stable => "stable",
            Stability::Locked => "locked",
        }
    }
}

/// `[profile]` table — collection of named build profiles.
///
/// `release` is the v0.5 original. `dev` / `bench` / `test` are added
/// in T0-A1 (Buff SDK 2.0) so the `[profile.*]` table mirrors Cargo's
/// four canonical profiles. Custom profile names remain a v1.18+ concern;
/// serde ignores unknown sub-tables per the forward-compat rule.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Profiles {
    /// `[profile.dev]` — debug-build profile (T0-A1). Selected by
    /// `BUFF_PROFILE=dev` or as the default when no `--release` flag is
    /// passed. Empty/absent → fall back to rustc's built-in dev defaults.
    #[serde(default)]
    pub dev: Option<ProfileOpts>,
    /// `[profile.release]` — optimisation flags for release builds
    /// (v0.5 original). Selected by `--release` or `BUFF_PROFILE=release`.
    #[serde(default)]
    pub release: Option<ProfileOpts>,
    /// `[profile.bench]` — profile applied by `buff bench` (T0-A1,
    /// pairs with the `@bench` attribute from T0-F2). When absent,
    /// `bench` falls back to `release` (matches Cargo).
    #[serde(default)]
    pub bench: Option<ProfileOpts>,
    /// `[profile.test]` — profile applied by `buff test` (T0-A1).
    /// When absent, `test` falls back to `dev` (matches Cargo).
    #[serde(default)]
    pub test: Option<ProfileOpts>,
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
    /// Debug-info level (`0`/`1`/`2`/`"line-tables-only"`). TOML int or
    /// string → string. Mirrors Cargo's `profile.<name>.debug`. (T0-A4)
    #[serde(default, deserialize_with = "deserialize_scalar_string")]
    pub debug: Option<String>,
}

/// `[features]` section of `buff.toml` (T0-A1).
///
/// Buff features are named boolean flags the user toggles at build time
/// to enable optional behaviour. Source code gates decls via
/// `@feature(name)` (T0-B4); codegen emits only those whose name is in
/// the resolved feature set.
///
/// # Layout
///
/// ```toml
/// [features]
/// logging = []
/// json = ["logging"]      # feature can enable other features
/// default = ["logging"]   # enabled unless --no-default-features
/// ```
///
/// `default` is the only reserved key. Other entries map feature name →
/// list of features it implies (transitive enable). Currently the
/// codegen treats `default` + any CLI `--features` list as the
/// resolved set; Cargo-style `feature = ["dep:foo"]` syntax arrives
/// with T1's resolver.
///
/// Stored as `BTreeMap` for deterministic iteration (matches the
/// rest of the manifest's determinism contract).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct FeaturesSection {
    /// Feature name → list of features it transitively enables (Cargo
    /// shape). Empty `Vec` means the feature enables nothing else.
    #[serde(default)]
    pub features: BTreeMap<String, Vec<String>>,
    /// `default = [...]` — features enabled unless the user passes
    /// `--no-default-features`. Stored as a separate field so the
    /// resolution pass can distinguish explicit feature decls from
    /// the default set.
    #[serde(default)]
    pub default: Vec<String>,
}

impl FeaturesSection {
    /// Resolve the effective feature set given a list of explicitly
    /// enabled features (from CLI `--features a,b`) and whether the
    /// default set is included.
    ///
    /// Returns a sorted `Vec<String>` for deterministic codegen. The
    /// transitive closure (feature → implied features) is computed via
    /// a simple BFS — cycles are tolerated (visited-set guard) and
    /// silently broken at first re-visit.
    pub fn resolve(&self, explicit: &[String], include_default: bool) -> Vec<String> {
        use std::collections::BTreeSet;
        let mut enabled: BTreeSet<String> = BTreeSet::new();
        if include_default {
            for d in &self.default {
                enabled.insert(d.clone());
            }
        }
        for e in explicit {
            enabled.insert(e.clone());
        }
        // BFS the implied-features graph. Bounded by `features.len()` so
        // we always terminate (visited-set guard makes cycles benign).
        let mut queue: Vec<String> = enabled.iter().cloned().collect();
        while let Some(name) = queue.pop() {
            if let Some(implies) = self.features.get(&name) {
                for imp in implies {
                    if enabled.insert(imp.clone()) {
                        queue.push(imp.clone());
                    }
                }
            }
        }
        enabled.into_iter().collect()
    }

    /// `true` when `name` is a declared feature (either in
    /// [`FeaturesSection::features`] or [`FeaturesSection::default`]).
    pub fn declares(&self, name: &str) -> bool {
        self.features.contains_key(name) || self.default.iter().any(|d| d == name)
    }
}

/// `[lints]` section of `buff.toml` (T0-A1).
///
/// Project-wide lint policy consumed by `buff check`. The v0.5
/// naming-convention linter (camelCase funcs, etc.) runs unconditionally;
/// entries here override the severity for named lints. Unknown lint
/// names are ignored (forward-compat — new lints added later don't
/// break older manifests).
///
/// # Layout
///
/// ```toml
/// [lints]
/// clippy = "deny"        # "deny" | "warn" | "allow"
/// naming = "warn"
/// ```
///
/// `clippy` is the only key consumed in v1.13; additional lints arrive
/// with future tasks. Stored as `BTreeMap<String, String>` (lowercase
/// lint name → lowercase severity) for deterministic iteration.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct LintsSection {
    /// Lowercase lint name → severity string (`"deny"`/`"warn"`/`"allow"`).
    /// Severities are stored as raw strings rather than an enum so an
    /// unknown value (e.g. `"suggestion"`) survives the round-trip and
    /// can be surfaced by `buff check` rather than rejected at parse
    /// time.
    #[serde(default)]
    pub lints: BTreeMap<String, String>,
}

impl LintsSection {
    /// Severity for `name` if declared, else `None`. Helper used by
    /// `buff check` to look up the policy per-lint.
    pub fn severity(&self, name: &str) -> Option<&str> {
        self.lints.get(name).map(String::as_str)
    }
}

/// `[prelude]` section of `buff.toml` (T0-A1, T0-B3).
///
/// Lists module paths whose `export`s become ambient in every source
/// file of the project — the project-wide analog of the global
/// [`buff_lang_types::prelude`]. The codegen pass injects the
/// equivalent of `import * from "<path>"` at the head of each compiled
/// file, so users can `print(...)` or use `DateTime.now()` without
/// per-file imports.
///
/// # Layout
///
/// ```toml
/// [prelude]
/// modules = ["./src/prelude.buff", "./src/logging.buff"]
/// ```
///
/// Paths are project-relative. The resolver (T1) validates that each
/// path resolves to a Buff source file with at least one `export`.
/// Empty by default — `buff new` does NOT scaffold a project prelude
/// (the global prelude remains ambient regardless).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct PreludeSection {
    /// Module paths (project-relative) whose `export`s become ambient.
    /// Order matters: later modules' exports shadow earlier ones on
    /// name conflict (matches ES6 module semantics).
    #[serde(default)]
    pub modules: Vec<String>,
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

