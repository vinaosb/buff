//! T123 — Workspace support (`[workspace]` section in `buff.toml`).
//!
//! Cargo-workspace passthrough: a workspace `buff.toml` is a VIRTUAL
//! manifest (has `[workspace]` with `members`, NO `[package]`).
//! `generate_cargo_toml` emits a matching virtual `Cargo.toml`, and
//! `buff build` / `buff test` shell out to `cargo build` / `cargo test`
//! at the workspace root — cargo fans out to members automatically.
//!
//! ## Test split
//!
//! - **CI-safe unit tests** (always run): parse workspace buff.toml,
//!   emit workspace Cargo.toml, validate structure. NO cargo invocation.
//! - **`#[ignore]` integration test** (opt-in via `--ignored`): creates
//!   a real 2-member workspace on disk, invokes `commands::build::run`
//!   via the library API, asserts both members' binaries were produced.
//!   Gated because it shells out to `cargo build` (slow, network-heavy).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use buff_lang_cli::config::{generate_cargo_toml, BuffConfig, WorkspaceSection};

// ---------------------------------------------------------------------------
// Test infra — temp dir helpers (mirror git_dependencies_t122.rs pattern)
// ---------------------------------------------------------------------------

/// Process-wide mutex serialising tests that mutate the process cwd or
/// invoke `cargo` (which reads the local env from cwd).
static WS_TEST_LOCK: Mutex<()> = Mutex::new(());

fn ws_lock() -> MutexGuard<'static, ()> {
    WS_TEST_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Per-test temp root: `<temp>/buff-t123-<pid>/`. Wipe-once-on-first-call
/// via `Mutex<Option<PathBuf>>` to avoid sibling-test race-deletion.
fn temp_root() -> PathBuf {
    static ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
    let mut guard = ROOT.lock().expect("temp_root mutex unpoisoned");
    if let Some(p) = guard.as_ref() {
        return p.clone();
    }
    let dir = std::env::temp_dir().join(format!("buff-t123-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::create_dir_all(&dir);
    *guard = Some(dir.clone());
    dir
}

/// Unique subdir under [`temp_root`] so parallel tests don't collide.
fn unique_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = temp_root().join(format!("{label}-{n}"));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn cleanup(path: &std::path::Path) {
    let _ = fs::remove_dir_all(path);
}

/// True when the `cargo` binary is on PATH and invokable.
fn cargo_available() -> bool {
    Command::new("cargo")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

// ---------------------------------------------------------------------------
// CI-safe unit tests — parse + emission (NO cargo invocation)
// ---------------------------------------------------------------------------

#[test]
fn parses_workspace_manifest_with_members() {
    let toml = r#"[workspace]
members = ["pkg-a", "pkg-b"]
"#;
    let cfg = BuffConfig::parse(toml).expect("workspace manifest must parse");
    let ws = cfg.workspace.expect("workspace section present");
    assert_eq!(ws.members, vec!["pkg-a".to_string(), "pkg-b".to_string()]);
    // package MUST be absent on a virtual workspace manifest.
    assert!(
        cfg.package.is_none(),
        "virtual workspace manifest must not have [package]"
    );
}

#[test]
fn parses_workspace_manifest_with_resolver() {
    let toml = r#"[workspace]
members = ["crates/core"]
resolver = "2"
"#;
    let cfg = BuffConfig::parse(toml).expect("workspace with resolver must parse");
    let ws = cfg.workspace.expect("workspace present");
    assert_eq!(ws.resolver.as_deref(), Some("2"));
}

#[test]
fn workspace_defaults_empty_when_no_workspace_section() {
    // Regular single-package manifest: no [workspace], workspace must be None.
    let toml = r#"[package]
name = "demo"
version = "0.1.0"
"#;
    let cfg = BuffConfig::parse(toml).expect("single-package manifest must parse");
    assert!(
        cfg.workspace.is_none(),
        "absent [workspace] defaults to None"
    );
    assert!(cfg.package.is_some(), "[package] present");
}

#[test]
fn rejects_manifest_with_neither_package_nor_workspace() {
    let toml = r#"[dependencies]
serde = "1.0"
"#;
    let err = BuffConfig::parse(toml).expect_err("must reject ambiguous manifest");
    let msg = format!("{err}");
    assert!(
        msg.contains("package") || msg.contains("workspace"),
        "error must mention package/workspace: {msg}"
    );
}

#[test]
fn rejects_manifest_with_both_package_and_workspace() {
    // Ambiguous: a virtual workspace manifest must omit [package].
    let toml = r#"[package]
name = "demo"
version = "0.1.0"

[workspace]
members = ["pkg-a"]
"#;
    let err = BuffConfig::parse(toml).expect_err("must reject ambiguous manifest");
    let msg = format!("{err}");
    assert!(
        msg.contains("ambiguous") || msg.contains("both"),
        "error must mention ambiguity: {msg}"
    );
}

#[test]
fn generate_cargo_toml_emits_virtual_workspace_manifest() {
    let cfg = BuffConfig {
        package: None,
        dependencies: BTreeMap::new(),
        profile: Default::default(),
        rust_deps: BTreeMap::new(),
        git_dependencies: BTreeMap::new(),
        registry_dependencies: BTreeMap::new(),
        workspace: Some(WorkspaceSection {
            members: vec!["pkg-a".to_string(), "pkg-b".to_string()],
            resolver: None,
        }),
    };
    let cargo = generate_cargo_toml(&cfg);
    // Virtual manifest: NO [package] section.
    assert!(
        !cargo.contains("[package]"),
        "virtual workspace Cargo.toml must not contain [package]: {cargo}"
    );
    // Must contain [workspace] section.
    assert!(
        cargo.contains("[workspace]"),
        "missing [workspace] section: {cargo}"
    );
    // Members must be listed.
    assert!(cargo.contains("\"pkg-a\""), "missing pkg-a: {cargo}");
    assert!(cargo.contains("\"pkg-b\""), "missing pkg-b: {cargo}");
    // Resolver must default to "2" when unset.
    assert!(
        cargo.contains("resolver = \"2\""),
        "missing default resolver: {cargo}"
    );
}

#[test]
fn generate_cargo_toml_workspace_respects_custom_resolver() {
    let cfg = BuffConfig {
        package: None,
        dependencies: BTreeMap::new(),
        profile: Default::default(),
        rust_deps: BTreeMap::new(),
        git_dependencies: BTreeMap::new(),
        registry_dependencies: BTreeMap::new(),
        workspace: Some(WorkspaceSection {
            members: vec!["only".to_string()],
            resolver: Some("1".to_string()),
        }),
    };
    let cargo = generate_cargo_toml(&cfg);
    assert!(
        cargo.contains("resolver = \"1\""),
        "custom resolver must be honoured: {cargo}"
    );
    assert!(
        !cargo.contains("resolver = \"2\""),
        "default resolver must NOT appear when custom set: {cargo}"
    );
}

#[test]
fn generate_cargo_toml_workspace_is_deterministic() {
    let mk = || BuffConfig {
        package: None,
        dependencies: BTreeMap::new(),
        profile: Default::default(),
        rust_deps: BTreeMap::new(),
        git_dependencies: BTreeMap::new(),
        registry_dependencies: BTreeMap::new(),
        workspace: Some(WorkspaceSection {
            members: vec!["zpkg".to_string(), "apkg".to_string()],
            resolver: None,
        }),
    };
    let a = generate_cargo_toml(&mk());
    let b = generate_cargo_toml(&mk());
    assert_eq!(a, b, "workspace Cargo.toml emission must be idempotent");
}

#[test]
fn generate_cargo_toml_workspace_preserves_member_order() {
    // Members are a Vec — order is the user's declared order (NOT sorted).
    // Cargo accepts members in any order; we preserve user intent.
    let cfg = BuffConfig {
        package: None,
        dependencies: BTreeMap::new(),
        profile: Default::default(),
        rust_deps: BTreeMap::new(),
        git_dependencies: BTreeMap::new(),
        registry_dependencies: BTreeMap::new(),
        workspace: Some(WorkspaceSection {
            members: vec!["zeta".to_string(), "alpha".to_string(), "mid".to_string()],
            resolver: None,
        }),
    };
    let cargo = generate_cargo_toml(&cfg);
    let zeta_pos = cargo.find("\"zeta\"").expect("zeta emitted");
    let alpha_pos = cargo.find("\"alpha\"").expect("alpha emitted");
    let mid_pos = cargo.find("\"mid\"").expect("mid emitted");
    assert!(zeta_pos < alpha_pos, "declared order preserved: zeta<alpha");
    assert!(alpha_pos < mid_pos, "declared order preserved: alpha<mid");
}

#[test]
fn generate_cargo_toml_workspace_empty_members_still_emits_header() {
    // Edge case: a workspace with zero members is degenerate but should
    // still produce a syntactically valid virtual Cargo.toml. This guards
    // against a panic-on-empty regression.
    let cfg = BuffConfig {
        package: None,
        dependencies: BTreeMap::new(),
        profile: Default::default(),
        rust_deps: BTreeMap::new(),
        git_dependencies: BTreeMap::new(),
        registry_dependencies: BTreeMap::new(),
        workspace: Some(WorkspaceSection {
            members: vec![],
            resolver: None,
        }),
    };
    let cargo = generate_cargo_toml(&cfg);
    assert!(cargo.contains("[workspace]"));
    assert!(cargo.contains("resolver = \"2\""));
    assert!(cargo.contains("members = []"));
}

// ---------------------------------------------------------------------------
// Integration test — 2-member workspace, end-to-end build via cargo
// ---------------------------------------------------------------------------

/// Scaffold a member crate: writes `<member>/buff.toml`, `<member>/src/main.buff`,
/// and the transpiled stub `<member>/src/main.rs` (so cargo build doesn't need
/// the Buff pipeline to run end-to-end). This isolates the test from the
/// transpiler — we're testing workspace plumbing, not Buff codegen.
fn scaffold_member(root: &std::path::Path, name: &str) -> PathBuf {
    let member_dir = root.join(name);
    fs::create_dir_all(member_dir.join("src")).expect("create member src");

    // Member buff.toml.
    let buff_toml = format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n");
    fs::write(member_dir.join("buff.toml"), buff_toml).expect("write member buff.toml");

    // Member main.buff (present so the workspace is layout-valid).
    fs::write(
        member_dir.join("src").join("main.buff"),
        "func main():\n    print(\"hello\")\n",
    )
    .expect("write member main.buff");

    // Pre-transpiled main.rs so cargo build succeeds without invoking the
    // Buff pipeline. We use a minimal std-only program (no extern deps).
    let main_rs = format!("fn main() {{\n    println!(\"hello from {name}\");\n}}\n");
    fs::write(member_dir.join("src").join("main.rs"), main_rs).expect("write main.rs");

    // Member Cargo.toml — points [[bin]] at src/main.rs. cargo requires this
    // for each workspace member.
    let cargo_toml = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [[bin]]\nname = \"{name}\"\npath = \"src/main.rs\"\n"
    );
    fs::write(member_dir.join("Cargo.toml"), cargo_toml).expect("write member Cargo.toml");

    member_dir
}

#[test]
#[ignore = "shells out to cargo build (slow, may hit network); run with --ignored"]
fn integration_builds_two_member_workspace() {
    let _guard = ws_lock();
    if !cargo_available() {
        eprintln!("skip: cargo binary not on PATH");
        return;
    }

    let ws_root = unique_dir("ws-root-build");
    scaffold_member(&ws_root, "pkg-a");
    scaffold_member(&ws_root, "pkg-b");

    // Workspace buff.toml (virtual manifest).
    let buff_toml = "[workspace]\nmembers = [\"pkg-a\", \"pkg-b\"]\n";
    fs::write(ws_root.join("buff.toml"), buff_toml).expect("write workspace buff.toml");

    // chdir into workspace root (commands::build::run reads cwd).
    let prev = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&ws_root).expect("chdir ws root");
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _cwd_guard = CwdGuard(prev.clone());

    // Drive the library API: buff build (no file → workspace mode).
    buff_lang_cli::commands::build::run(None, None, false).expect("build must succeed");

    // Assert both member binaries were compiled. cargo build places
    // binaries at target/debug/<name>(.exe).
    let target_debug = ws_root.join("target").join("debug");
    assert!(target_debug.is_dir(), "target/debug must exist after build");

    let bin_a = target_debug
        .join("pkg-a")
        .with_extension(std::env::consts::EXE_EXTENSION);
    let bin_b = target_debug
        .join("pkg-b")
        .with_extension(std::env::consts::EXE_EXTENSION);
    assert!(
        bin_a.is_file(),
        "pkg-a binary must exist: {}",
        bin_a.display()
    );
    assert!(
        bin_b.is_file(),
        "pkg-b binary must exist: {}",
        bin_b.display()
    );

    // Assert the shared target/ dir is at workspace root (not per-member).
    // If target/ were per-member, pkg-a/target would also exist.
    assert!(
        !ws_root.join("pkg-a").join("target").is_dir(),
        "cargo workspace must share target/ at root, not per-member"
    );

    cleanup(&ws_root);
}

#[test]
#[ignore = "shells out to cargo test (slow); run with --ignored"]
fn integration_runs_cargo_test_at_workspace_root() {
    let _guard = ws_lock();
    if !cargo_available() {
        eprintln!("skip: cargo binary not on PATH");
        return;
    }

    let ws_root = unique_dir("ws-root-test");
    scaffold_member(&ws_root, "pkg-a");
    scaffold_member(&ws_root, "pkg-b");

    // Add a Rust unit test to pkg-a's main.rs (overwrite).
    fs::write(
        ws_root.join("pkg-a").join("src").join("main.rs"),
        "#[test]\nfn pkg_a_smoke() {\n    assert_eq!(2 + 2, 4);\n}\n\n\
         fn main() {\n    println!(\"hello from pkg-a\");\n}\n",
    )
    .expect("write pkg-a test main.rs");

    let buff_toml = "[workspace]\nmembers = [\"pkg-a\", \"pkg-b\"]\n";
    fs::write(ws_root.join("buff.toml"), buff_toml).expect("write workspace buff.toml");

    let prev = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&ws_root).expect("chdir ws root");
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _cwd_guard = CwdGuard(prev);

    // buff test (no file → workspace mode).
    buff_lang_cli::commands::test::run(None, None).expect("cargo test must succeed");

    cleanup(&ws_root);
}
