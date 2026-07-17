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
}
