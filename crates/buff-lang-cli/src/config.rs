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
use std::path::Path;

use serde::Deserialize;
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BuffConfig {
    /// `[package]` — the only required top-level table.
    pub package: PackageSection,
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
    pub fn parse(toml_text: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(toml_text)?)
    }

    /// Load and parse a `buff.toml` file from disk.
    ///
    /// Reads the file synchronously then delegates to [`BuffConfig::parse`].
    /// A missing file surfaces as [`ConfigError::Io`].
    pub fn load_from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        Self::parse(&text)
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
/// The generated `Cargo.toml` includes:
/// - `[package]` section with name, version, edition = "2021"
/// - `[[bin]]` section pointing to `src/main.rs` (the transpiled entry point)
/// - `[dependencies]` section from `BuffConfig::dependencies`
/// - `[dependencies]` entries from `BuffConfig::rust_deps` (T119/T120)
///
/// # Idempotency
///
/// `generate_cargo_toml(cfg) == generate_cargo_toml(cfg)` for any `cfg`.
/// The function uses sorted iteration and a fixed format string — no
/// HashMap iteration, no environment-dependent output.
pub fn generate_cargo_toml(cfg: &BuffConfig) -> String {
    let mut out = String::new();

    // [package]
    out.push_str("[package]\n");
    out.push_str(&format!("name = \"{}\"\n", cfg.package.name));
    out.push_str(&format!("version = \"{}\"\n", cfg.package.version));
    out.push_str("edition = \"2021\"\n");

    // Optional edition override from buff.toml
    if let Some(ed) = &cfg.package.edition {
        // Map Buff edition to Rust edition. For now all Buff editions map to
        // Rust 2021 (the pinned workspace edition).
        out.push_str(&format!("# buff edition: {ed}\n"));
    }

    // [[bin]] — the transpiled entry point
    out.push_str("\n[[bin]]\n");
    out.push_str(&format!("name = \"{}\"\n", cfg.package.name));
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
        assert_eq!(lines, vec!["serde = \"*\"", "tokio = \"*\"", "zlib = \"*\""]);
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
        assert_eq!(cfg.rust_deps.get("serde_json").map(|s| s.as_str()), Some("*"));
        assert_eq!(cfg.rust_deps.get("tokio").map(|s| s.as_str()), Some("1"));
    }

    #[test]
    fn config_rust_deps_defaults_empty_when_absent() {
        let toml = r#"[package]
name = "demo"
version = "0.1.0"
"#;
        let cfg = BuffConfig::parse(toml).expect("minimal manifest must parse");
        assert!(cfg.rust_deps.is_empty(), "absent [rust-deps] defaults to empty");
    }
}
