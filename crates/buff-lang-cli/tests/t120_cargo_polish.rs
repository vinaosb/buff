//! T120 — Cargo polish + buff.toml manifest tests.
//!
//! Covers:
//! - `buff clean` and `buff update` dispatch (via `commands::clean::run` and
//!   `commands::update::run` — these shell out to cargo, so we test the
//!   dispatch compiles and the functions exist).
//! - `generate_cargo_toml` idempotency and correctness.
//! - `[rust-deps]` → `[dependencies]` in generated Cargo.toml (T119/T120).

use std::collections::BTreeMap;

use buff_lang_cli::config::{generate_cargo_toml, BuffConfig, PackageSection, Profiles};

/// Helper: build a minimal BuffConfig for testing.
fn minimal_cfg(name: &str) -> BuffConfig {
    BuffConfig {
        package: Some(PackageSection {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            edition: Some("0.1".to_string()),
        }),
        dependencies: BTreeMap::new(),
        profile: Profiles::default(),
        rust_deps: BTreeMap::new(),
        git_dependencies: BTreeMap::new(),
        workspace: None,
    }
}

// ---------------------------------------------------------------------------
// generate_cargo_toml — basic structure
// ---------------------------------------------------------------------------

#[test]
fn generate_cargo_toml_has_package_section() {
    let cfg = minimal_cfg("my_app");
    let toml = generate_cargo_toml(&cfg);
    assert!(toml.starts_with("[package]\n"), "must start with [package]");
    assert!(toml.contains("name = \"my_app\""));
    assert!(toml.contains("version = \"0.1.0\""));
    assert!(toml.contains("edition = \"2021\""));
}

#[test]
fn generate_cargo_toml_has_bin_section() {
    let cfg = minimal_cfg("my_app");
    let toml = generate_cargo_toml(&cfg);
    assert!(toml.contains("[[bin]]"), "must have [[bin]] section");
    assert!(toml.contains("name = \"my_app\""));
    assert!(toml.contains("path = \"src/main.rs\""));
}

#[test]
fn generate_cargo_toml_no_deps_when_empty() {
    let cfg = minimal_cfg("no_deps");
    let toml = generate_cargo_toml(&cfg);
    assert!(
        !toml.contains("[dependencies]"),
        "no deps section when empty"
    );
}

#[test]
fn generate_cargo_toml_includes_dependencies() {
    let mut deps = BTreeMap::new();
    deps.insert("serde".to_string(), "1.0".to_string());
    deps.insert("tokio".to_string(), "1.40".to_string());
    let cfg = BuffConfig {
        dependencies: deps,
        ..minimal_cfg("with_deps")
    };
    let toml = generate_cargo_toml(&cfg);
    assert!(toml.contains("[dependencies]"));
    assert!(toml.contains("serde = \"1.0\""));
    assert!(toml.contains("tokio = \"1.40\""));
}

// ---------------------------------------------------------------------------
// generate_cargo_toml — [rust-deps] → [dependencies] (T119/T120)
// ---------------------------------------------------------------------------

#[test]
fn generate_cargo_toml_includes_rust_deps() {
    let mut rust_deps = BTreeMap::new();
    rust_deps.insert("serde_json".to_string(), "*".to_string());
    rust_deps.insert("tokio".to_string(), "1".to_string());
    let cfg = BuffConfig {
        rust_deps,
        ..minimal_cfg("with_rust_deps")
    };
    let toml = generate_cargo_toml(&cfg);
    assert!(
        toml.contains("[dependencies]"),
        "rust-deps create [dependencies]"
    );
    assert!(toml.contains("serde_json = \"*\""));
    assert!(toml.contains("tokio = \"1\""));
}

#[test]
fn generate_cargo_toml_rust_deps_merged_with_deps() {
    let mut deps = BTreeMap::new();
    deps.insert("serde".to_string(), "1.0".to_string());
    let mut rust_deps = BTreeMap::new();
    rust_deps.insert("serde_json".to_string(), "*".to_string());
    let cfg = BuffConfig {
        dependencies: deps,
        rust_deps,
        ..minimal_cfg("merged")
    };
    let toml = generate_cargo_toml(&cfg);
    // Both should appear under a single [dependencies] section.
    assert!(toml.contains("serde = \"1.0\""));
    assert!(toml.contains("serde_json = \"*\""));
    // Count [dependencies] headers — should be exactly 1.
    assert_eq!(toml.matches("[dependencies]").count(), 1);
}

// ---------------------------------------------------------------------------
// Idempotency — generate_cargo_toml must be deterministic
// ---------------------------------------------------------------------------

#[test]
fn generate_cargo_toml_is_idempotent() {
    let mut deps = BTreeMap::new();
    deps.insert("zlib".to_string(), "1".to_string());
    deps.insert("serde".to_string(), "1.0".to_string());
    let mut rust_deps = BTreeMap::new();
    rust_deps.insert("tokio".to_string(), "1".to_string());
    rust_deps.insert("reqwest".to_string(), "0.12".to_string());
    let cfg = BuffConfig {
        dependencies: deps,
        rust_deps,
        ..minimal_cfg("idempotent")
    };

    let first = generate_cargo_toml(&cfg);
    let second = generate_cargo_toml(&cfg);
    assert_eq!(
        first, second,
        "generate_cargo_toml must be idempotent — same input, same output"
    );
}

#[test]
fn generate_cargo_toml_deterministic_ordering() {
    // Insert deps in reverse alphabetical order — output must still be sorted.
    let mut deps = BTreeMap::new();
    deps.insert("zzz".to_string(), "3".to_string());
    deps.insert("aaa".to_string(), "1".to_string());
    deps.insert("mmm".to_string(), "2".to_string());
    let cfg = BuffConfig {
        dependencies: deps,
        ..minimal_cfg("ordering")
    };
    let toml = generate_cargo_toml(&cfg);

    // Find the deps section and check ordering.
    let deps_start = toml.find("[dependencies]").expect("deps section");
    let deps_block = &toml[deps_start..];
    let aaa_pos = deps_block.find("aaa").expect("aaa present");
    let mmm_pos = deps_block.find("mmm").expect("mmm present");
    let zzz_pos = deps_block.find("zzz").expect("zzz present");
    assert!(aaa_pos < mmm_pos, "aaa must come before mmm");
    assert!(mmm_pos < zzz_pos, "mmm must come before zzz");
}

// ---------------------------------------------------------------------------
// Clean and update dispatch — verify the modules compile and run() exists
// ---------------------------------------------------------------------------

#[test]
fn clean_module_has_run_function() {
    // Compile-time check: the clean module must export a `run()` function.
    // We can't actually invoke `cargo clean` in tests (it would modify the
    // workspace), but we verify the function signature compiles.
    let _ = buff_lang_cli::commands::clean::run;
}

#[test]
fn update_module_has_run_function() {
    let _ = buff_lang_cli::commands::update::run;
}
