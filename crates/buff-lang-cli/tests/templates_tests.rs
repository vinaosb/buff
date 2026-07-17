//! Integration tests for `buff new` template variants (T112).
//!
//! Each test scaffolds a project with a specific [`TemplateKind`] inside a
//! per-test temp directory and asserts the expected files exist with the
//! expected content. Follows the same `cwd_lock` / `unique_dir` / `cleanup`
//! discipline as `scaffold_tests.rs` — `buff new` writes into the process cwd,
//! so tests that chdir must be serialized.
//!
//! Test function names contain `templates` so `cargo test -p buff-lang-cli
//! templates` filters exactly this file.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use buff_lang_cli::commands;
use buff_lang_cli::scaffold::{self, TemplateKind};

/// Process-wide mutex serializing tests that call [`std::env::set_current_dir`].
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn cwd_lock() -> MutexGuard<'static, ()> {
    CWD_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn temp_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("buff-templates-tests-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn unique_dir(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = temp_root().join(format!("{label}-{n}"));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn cleanup(path: &std::path::Path) {
    let _ = fs::remove_dir_all(path);
}

/// Scaffold a project named `name` with `template` inside a unique temp
/// workdir, returning `(workdir, project_dir)`. Panics if the scaffold fails.
fn scaffold_in_temp(name: &str, template: TemplateKind) -> (PathBuf, PathBuf) {
    let _guard = cwd_lock();
    let workdir = unique_dir("templates");

    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&workdir).expect("chdir to workdir");
    let result = commands::new::run(name, template);
    std::env::set_current_dir(&original).expect("restore cwd");
    drop(_guard);

    result.expect("scaffold should succeed for a valid name + template");
    let project_dir = workdir.join(name);
    (workdir, project_dir)
}

// ---------------------------------------------------------------------------
// Per-template acceptance: each variant creates its expected file(s).
// ---------------------------------------------------------------------------

#[test]
fn templates_binary_default_creates_main_buff() {
    let (workdir, project_dir) = scaffold_in_temp("bin_app", TemplateKind::Binary);

    for rel in &["buff.toml", "src/main.buff", ".gitignore", "README.md"] {
        assert!(
            project_dir.join(rel).exists(),
            "binary template should create `{rel}`"
        );
    }
    // Binary must NOT create src/lib.buff.
    assert!(
        !project_dir.join("src/lib.buff").exists(),
        "binary template should NOT create src/lib.buff"
    );

    let main = fs::read_to_string(project_dir.join("src/main.buff")).unwrap_or_default();
    assert!(
        main.contains("func main():"),
        "binary main.buff should declare `func main():`\n{main}"
    );
    let toml = fs::read_to_string(project_dir.join("buff.toml")).unwrap_or_default();
    assert!(
        toml.contains("name = \"bin_app\""),
        "buff.toml should embed the project name; got:\n{toml}"
    );

    cleanup(&project_dir);
    cleanup(&workdir);
}

#[test]
fn templates_lib_creates_lib_buff() {
    // This is the QA case from the task spec: `buff new mylib --lib`.
    let (workdir, project_dir) = scaffold_in_temp("mylib", TemplateKind::Lib);

    assert!(
        project_dir.join("src/lib.buff").exists(),
        "QA case: --lib must create `src/lib.buff`"
    );
    assert!(
        !project_dir.join("src/main.buff").exists(),
        "lib template should NOT create src/main.buff (no `main`)"
    );

    let lib = fs::read_to_string(project_dir.join("src/lib.buff")).unwrap_or_default();
    assert!(
        lib.contains("export func"),
        "lib.buff should contain `export func` (importable surface)\n{lib}"
    );
    assert!(
        !lib.contains("func main():"),
        "lib.buff should NOT define `func main()`\n{lib}"
    );

    let toml = fs::read_to_string(project_dir.join("buff.toml")).unwrap_or_default();
    assert!(
        toml.contains("name = \"mylib\""),
        "buff.toml should embed the project name; got:\n{toml}"
    );

    cleanup(&project_dir);
    cleanup(&workdir);
}

#[test]
fn templates_server_creates_async_main() {
    let (workdir, project_dir) = scaffold_in_temp("srv_app", TemplateKind::Server);

    let main_path = project_dir.join("src/main.buff");
    assert!(
        main_path.exists(),
        "server template must create src/main.buff"
    );

    let main = fs::read_to_string(main_path).unwrap_or_default();
    assert!(
        main.contains("async func"),
        "server main.buff should declare an `async func`\n{main}"
    );
    assert!(
        main.contains("spawn"),
        "server main.buff should demonstrate `spawn` (task scheduling)\n{main}"
    );
    assert!(
        main.contains("func main():"),
        "server main.buff should still have a `func main():` entry point\n{main}"
    );

    cleanup(&project_dir);
    cleanup(&workdir);
}

#[test]
fn templates_gpu_creates_gpu_template() {
    let (workdir, project_dir) = scaffold_in_temp("gpu_app", TemplateKind::Gpu);

    let main_path = project_dir.join("src/main.buff");
    assert!(main_path.exists(), "gpu template must create src/main.buff");

    let main = fs::read_to_string(main_path).unwrap_or_default();
    assert!(
        main.contains("@prefer(gpu)"),
        "gpu main.buff should carry an `@prefer(gpu)` dispatch hint\n{main}"
    );
    assert!(
        main.contains("func main():"),
        "gpu main.buff should still have a `func main():` entry point\n{main}"
    );

    cleanup(&project_dir);
    cleanup(&workdir);
}

#[test]
fn templates_workspace_creates_workspace_layout() {
    let (workdir, project_dir) = scaffold_in_temp("ws_app", TemplateKind::Workspace);

    // Root manifest must be a [workspace] (not [package]) and list members.
    let toml = fs::read_to_string(project_dir.join("buff.toml")).unwrap_or_default();
    assert!(
        toml.contains("[workspace]"),
        "workspace buff.toml should contain a `[workspace]` table\n{toml}"
    );
    assert!(
        toml.contains("members"),
        "workspace buff.toml should list `members`\n{toml}"
    );

    // Each member crate should have its own buff.toml + source.
    for rel in &[
        "crates/core/buff.toml",
        "crates/core/src/main.buff",
        "crates/utils/buff.toml",
        "crates/utils/src/lib.buff",
    ] {
        assert!(
            project_dir.join(rel).exists(),
            "workspace should create member file `{rel}`"
        );
    }

    // core is a binary crate; utils is a library crate.
    let core_main =
        fs::read_to_string(project_dir.join("crates/core/src/main.buff")).unwrap_or_default();
    assert!(
        core_main.contains("func main():"),
        "core/src/main.buff should be a binary entry point\n{core_main}"
    );
    let utils_lib =
        fs::read_to_string(project_dir.join("crates/utils/src/lib.buff")).unwrap_or_default();
    assert!(
        utils_lib.contains("export func"),
        "utils/src/lib.buff should export a function\n{utils_lib}"
    );

    cleanup(&project_dir);
    cleanup(&workdir);
}

// ---------------------------------------------------------------------------
// Flag selector: template_from_flags (pure logic, no filesystem).
// ---------------------------------------------------------------------------

#[test]
fn templates_flag_selector_defaults_to_binary() {
    assert_eq!(
        scaffold::template_from_flags(false, false, false, false).unwrap(),
        TemplateKind::Binary
    );
}

#[test]
fn templates_flag_selector_each_flag_maps_to_variant() {
    assert_eq!(
        scaffold::template_from_flags(true, false, false, false).unwrap(),
        TemplateKind::Lib
    );
    assert_eq!(
        scaffold::template_from_flags(false, true, false, false).unwrap(),
        TemplateKind::Server
    );
    assert_eq!(
        scaffold::template_from_flags(false, false, true, false).unwrap(),
        TemplateKind::Gpu
    );
    assert_eq!(
        scaffold::template_from_flags(false, false, false, true).unwrap(),
        TemplateKind::Workspace
    );
}

#[test]
fn templates_flag_selector_rejects_conflicting_flags() {
    let err = scaffold::template_from_flags(true, true, false, false).unwrap_err();
    assert!(
        err.contains("at most one"),
        "expected mutual-exclusion error, got: {err}"
    );
    assert!(
        scaffold::template_from_flags(true, false, true, true).is_err(),
        "3 flags set should also error"
    );
}

// ---------------------------------------------------------------------------
// Cross-template invariants: every variant gets the shared root files.
// ---------------------------------------------------------------------------

#[test]
fn templates_all_variants_get_shared_root_files() {
    for template in [
        TemplateKind::Binary,
        TemplateKind::Lib,
        TemplateKind::Server,
        TemplateKind::Gpu,
        TemplateKind::Workspace,
    ] {
        let (workdir, project_dir) =
            scaffold_in_temp(&format!("shared_{:?}", template).to_lowercase(), template);
        for rel in &["buff.toml", ".gitignore", "README.md"] {
            assert!(
                project_dir.join(rel).exists(),
                "template {template:?} should still create `{rel}`"
            );
        }
        cleanup(&project_dir);
        cleanup(&workdir);
    }
}
