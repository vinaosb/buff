//! T111 — `buff.toml` config parsing + project-layout enforcement.
//!
//! These are the **acceptance-gate** tests for the `config` module: a sample
//! `buff.toml` (with `[package]`, `[dependencies]`, `[profile.release]`) must
//! round-trip into [`BuffConfig`] with name, version, and deps extracted, and
//! malformed input must surface as a structured [`ConfigError`] (never a
//! panic). Layout enforcement is exercised via [`validate_project_layout`].
//!
//! The fixture content is inlined (no on-disk files) for the parsing tests so
//! they remain hermetic and parallel-safe. Layout tests use a unique per-test
//! temp dir to avoid collisions (mirroring `test_command.rs` patterns).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use buff_lang_cli::config::{validate_project_layout, BuffConfig, ConfigError};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_dir(unique: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "buff-lang-cli-config-parsing-{}-{}",
        std::process::id(),
        unique
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn cleanup(dir: &std::path::Path) {
    let _ = fs::remove_dir_all(dir);
}

/// Canonical sample `buff.toml` exercising every supported section.
const SAMPLE_TOML: &str = r#"[package]
name = "my_app"
version = "0.2.1"
edition = "0.1"

[dependencies]
serde = "1.0"
tokio = "1.40"
anyhow = "1"

[profile.release]
opt-level = 3
lto = true
"#;

// ---------------------------------------------------------------------------
// Parsing — happy paths
// ---------------------------------------------------------------------------

#[test]
fn config_parsing_basic() {
    let cfg = BuffConfig::parse(SAMPLE_TOML).expect("sample toml should parse");
    assert_eq!(cfg.package.name, "my_app");
    assert_eq!(cfg.package.version, "0.2.1");
    // Edition is optional in the struct; SAMPLE_TOML sets "0.1".
    assert_eq!(cfg.package.edition.as_deref(), Some("0.1"));
    // Empty deps map when not provided — here we DID provide, so non-empty.
    assert!(
        !cfg.dependencies.is_empty(),
        "deps should be populated from [dependencies]"
    );
    assert!(cfg.profile.release.is_some(), "release profile present");
}

#[test]
fn config_parsing_with_dependencies() {
    let cfg = BuffConfig::parse(SAMPLE_TOML).expect("sample toml should parse");
    let mut expected = BTreeMap::new();
    expected.insert("serde".to_string(), "1.0".to_string());
    expected.insert("tokio".to_string(), "1.40".to_string());
    expected.insert("anyhow".to_string(), "1".to_string());
    assert_eq!(
        cfg.dependencies, expected,
        "dependencies map should match sample verbatim (BTreeMap ordering)"
    );
    // BTreeMap iterates sorted — sanity check ordering is deterministic.
    let keys: Vec<&str> = cfg.dependencies.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["anyhow", "serde", "tokio"]);
}

#[test]
fn config_parsing_profile_release() {
    let cfg = BuffConfig::parse(SAMPLE_TOML).expect("sample toml should parse");
    let release = cfg
        .profile
        .release
        .as_ref()
        .expect("release profile should be parsed");
    // opt-level parses as a number from TOML — we accept either int or string.
    assert_eq!(release.opt_level.as_deref(), Some("3"));
    assert_eq!(release.lto.as_deref(), Some("true"));
}

#[test]
fn config_parsing_no_dependencies_defaults_empty() {
    // A minimal manifest with no [dependencies] section must still parse and
    // yield an empty (not missing) dependencies map.
    let minimal = r#"[package]
name = "tiny"
version = "0.1.0"
"#;
    let cfg = BuffConfig::parse(minimal).expect("minimal manifest should parse");
    assert_eq!(cfg.package.name, "tiny");
    assert!(cfg.dependencies.is_empty(), "no deps => empty map");
    assert!(
        cfg.profile.release.is_none(),
        "no [profile.release] => None"
    );
}

// ---------------------------------------------------------------------------
// Parsing — error paths (must be ConfigError, never panic)
// ---------------------------------------------------------------------------

#[test]
fn config_parsing_missing_package_errors() {
    // No [package] section at all — required field missing.
    let bad = r#"[dependencies]
foo = "1"
"#;
    let err = BuffConfig::parse(bad).expect_err("missing [package] should fail");
    assert!(
        matches!(err, ConfigError::Parse(_)),
        "expected Parse error, got {err:?}"
    );
}

#[test]
fn config_parsing_missing_name_field_errors() {
    // [package] present but `name` missing — required field missing.
    let bad = r#"[package]
version = "0.1.0"
"#;
    let err = BuffConfig::parse(bad).expect_err("missing name should fail");
    assert!(
        matches!(err, ConfigError::Parse(_)),
        "expected Parse error, got {err:?}"
    );
}

#[test]
fn config_parsing_malformed_toml_errors() {
    // Genuinely malformed TOML — unbalanced quotes.
    let bad = "[package]\nname = \"unterminated\n";
    let err = BuffConfig::parse(bad).expect_err("malformed toml should fail");
    assert!(
        matches!(err, ConfigError::Parse(_)),
        "expected Parse error, got {err:?}"
    );
}

#[test]
fn config_parsing_wrong_type_errors() {
    // `name` declared as an integer instead of a string — serde type mismatch.
    let bad = "[package]\nname = 42\nversion = \"0.1.0\"\n";
    let err = BuffConfig::parse(bad).expect_err("wrong type should fail");
    assert!(
        matches!(err, ConfigError::Parse(_)),
        "expected Parse error, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// load_from_file — I/O wrapper
// ---------------------------------------------------------------------------

#[test]
fn config_parsing_load_from_file() {
    let dir = temp_dir("load_from_file");
    let path = dir.join("buff.toml");
    fs::write(&path, SAMPLE_TOML).expect("write fixture");
    let cfg = BuffConfig::load_from_file(&path).expect("file load should succeed");
    assert_eq!(cfg.package.name, "my_app");
    assert_eq!(cfg.package.version, "0.2.1");
    cleanup(&dir);
}

#[test]
fn config_parsing_load_missing_file_errors() {
    let dir = temp_dir("missing_file");
    let path = dir.join("does-not-exist.toml");
    let err = BuffConfig::load_from_file(&path).expect_err("missing file should fail");
    assert!(
        matches!(err, ConfigError::Io(_)),
        "expected Io error, got {err:?}"
    );
    cleanup(&dir);
}

// ---------------------------------------------------------------------------
// validate_project_layout — src/ + tests/ enforcement
// ---------------------------------------------------------------------------

#[test]
fn config_parsing_validate_layout_ok() {
    let dir = temp_dir("layout_ok");
    let _ = fs::create_dir_all(dir.join("src"));
    let _ = fs::create_dir_all(dir.join("tests"));
    // `src/main.buff` is the v0.1 convention; presence is recommended.
    fs::write(dir.join("src").join("main.buff"), "// stub\n").expect("write stub");
    validate_project_layout(&dir).expect("valid layout should pass");
    cleanup(&dir);
}

#[test]
fn config_parsing_validate_layout_missing_src() {
    let dir = temp_dir("layout_missing_src");
    let _ = fs::create_dir_all(&dir); // empty — no src/
    let err = validate_project_layout(&dir).expect_err("missing src/ should fail");
    assert!(
        matches!(err, ConfigError::Layout(_)),
        "expected Layout error, got {err:?}"
    );
    assert!(
        err.to_string().to_lowercase().contains("src"),
        "error message should mention `src`: {err}"
    );
    cleanup(&dir);
}

#[test]
fn config_parsing_validate_layout_missing_tests_warns_or_errs() {
    // `tests/` is RECOMMENDED but the contract is at least a clear signal
    // when it's absent. We accept either Ok-without-tests or a Layout error
    // that mentions `tests` — what we DON'T accept is a panic.
    let dir = temp_dir("layout_missing_tests");
    let _ = fs::create_dir_all(dir.join("src"));
    fs::write(dir.join("src").join("main.buff"), "// stub\n").expect("write stub");
    let result = validate_project_layout(&dir);
    match result {
        Ok(()) => { /* tests/ treated as optional — acceptable */ }
        Err(ConfigError::Layout(msg)) => {
            assert!(
                msg.to_lowercase().contains("tests"),
                "missing-tests message should mention `tests`: {msg}"
            );
        }
        Err(other) => panic!("unexpected error variant for missing tests/: {other:?}"),
    }
    cleanup(&dir);
}
