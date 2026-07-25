//! Integration tests for `buff new` / `buff init` project scaffolding.
//!
//! Two flavours of test:
//!
//! 1. **Pure validation** — exercises [`scaffold::validate_project_name`]
//!    directly. No filesystem, no `rustc`.
//! 2. **Filesystem scaffolding** — invokes [`commands::new::run`] (and
//!    [`commands::init::run`]) inside a per-test temp directory rooted under
//!    `std::env::temp_dir()`. Asserts the expected files exist and have the
//!    expected content.
//!
//! The end-to-end "generated project actually runs" test spawns `rustc` and is
//! therefore gated behind [`rustc_available`].
//!
//! **Parallel-test caveat**: tests that touch the process cwd
//! ([`std::env::set_current_dir`]) are serialized via [`CWD_LOCK`]. `cwd` is
//! process-global; without the lock, parallel tests would race and write each
//! other's files into the wrong directory.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use buff_lang_cli::commands;
use buff_lang_cli::pipeline;
use buff_lang_cli::scaffold::{self, TemplateKind};

/// Convenience wrapper for `commands::run::run` that fills in the
/// post-T55 / T7 / T9 / T113 default values (no incremental, no
/// sccache, default linker/debuginfo/backend, native target, no race
/// detection). Keeps the per-test call sites readable.
fn run_with_defaults(file: &std::path::Path, args: &[String], release: bool) -> anyhow::Result<()> {
    commands::run::run(
        file,
        args,
        release,
        false, // incremental
        true,  // no_incremental (force legacy path)
        false, // sccache
        pipeline::LinkerChoice::default(),
        pipeline::DebugInfoChoice::default(),
        pipeline::BackendChoice::default(),
        None,  // target
        false, // detect_races
    )
}

/// Process-wide mutex serializing tests that call [`std::env::set_current_dir`].
///
/// Rust's default test harness runs tests in parallel on multiple threads, but
/// `current_dir` is a process-global property — without serialization, two
/// `buff new` tests running concurrently would each create their project under
/// whichever workdir happened to win the race.
static CWD_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard: call at the top of any test that needs a stable cwd. Releases
/// automatically on drop (end of test, including via panic).
fn cwd_lock() -> MutexGuard<'static, ()> {
    CWD_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Per-test working directory root: `<temp>/buff-scaffold-tests-<pid>/`.
fn temp_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("buff-scaffold-tests-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Unique subdir under [`temp_root`] so parallel tests don't collide.
fn unique_dir(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = temp_root().join(format!("{label}-{n}"));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn rustc_available() -> bool {
    Command::new("rustc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn cleanup(path: &std::path::Path) {
    let _ = fs::remove_dir_all(path);
}

// ---------------------------------------------------------------------------
// Name validation (no filesystem)
// ---------------------------------------------------------------------------

#[test]
fn test_validate_name_accepts_valid() {
    for ok in &["my_app", "app2", "_foo", "CamelCase", "_", "x9_y8"] {
        let r = scaffold::validate_project_name(ok);
        assert!(
            r.is_ok(),
            "expected `{ok}` to be accepted, got {:?}",
            r.err()
        );
    }
}

#[test]
fn test_validate_name_rejects_empty() {
    let err = scaffold::validate_project_name("").unwrap_err();
    assert!(
        err.contains("empty"),
        "expected empty-name error, got: {err}"
    );
}

#[test]
fn test_validate_name_rejects_keyword() {
    for kw in &["func", "let", "if", "match", "async", "unsafe"] {
        let err = scaffold::validate_project_name(kw).unwrap_err();
        assert!(
            err.contains("reserved"),
            "expected reserved-keyword error for `{kw}`, got: {err}"
        );
    }
}

#[test]
fn test_validate_name_rejects_starts_digit() {
    let err = scaffold::validate_project_name("1app").unwrap_err();
    assert!(
        err.contains("letter or underscore"),
        "expected starts-with error, got: {err}"
    );
}

#[test]
fn test_validate_name_rejects_special_chars() {
    for bad in &["my app", "app-2", "app.3", "café", "a/b"] {
        let err = scaffold::validate_project_name(bad).unwrap_err();
        assert!(
            !err.is_empty(),
            "expected non-empty error for invalid name `{bad}`"
        );
    }
}

#[test]
fn test_validate_name_rejects_unicode_first_char() {
    // Non-ASCII first character — must be rejected even though the rest is valid.
    let err = scaffold::validate_project_name("Ωmega").unwrap_err();
    assert!(
        err.contains("letter or underscore") || err.contains("invalid"),
        "expected first-char error for unicode, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// `buff new` — filesystem scaffolding
// ---------------------------------------------------------------------------

#[test]
fn test_new_creates_project_with_all_files() {
    let _guard = cwd_lock();
    let workdir = unique_dir("new_creates");
    let project_name = "test_app";

    // `buff new` writes into the *current* working directory; chdir into the
    // unique workdir for the duration of this test, then restore afterwards.
    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&workdir).expect("chdir to workdir");

    let result = commands::new::run(project_name, TemplateKind::Binary);

    // Restore cwd unconditionally so a failure here can't poison other tests.
    std::env::set_current_dir(&original).expect("restore cwd");
    drop(_guard);

    result.expect("`buff new` should succeed for a valid name");

    let project_dir = workdir.join(project_name);
    for rel in &["buff.toml", "src/main.buff", ".gitignore", "README.md"] {
        let p = project_dir.join(rel);
        assert!(
            p.exists(),
            "expected `{}` to exist after `buff new`",
            p.display()
        );
    }

    // Content sanity-check: buff.toml should reference the project name.
    let toml = fs::read_to_string(project_dir.join("buff.toml")).unwrap_or_default();
    assert!(
        toml.contains("name = \"test_app\""),
        "buff.toml should embed the project name; got:\n{toml}"
    );

    // main.buff must contain a valid `func main():` so it can be run.
    let main_buff = fs::read_to_string(project_dir.join("src/main.buff")).unwrap_or_default();
    assert!(
        main_buff.contains("func main():"),
        "main.buff should declare `func main():`; got:\n{main_buff}"
    );
    assert!(
        main_buff.contains("print("),
        "main.buff should call `print`; got:\n{main_buff}"
    );

    cleanup(&project_dir);
    cleanup(&workdir);
}

#[test]
fn test_new_refuses_existing_directory() {
    let _guard = cwd_lock();
    let workdir = unique_dir("new_refuses");
    let project_name = "already_here";
    let _ = fs::create_dir_all(workdir.join(project_name));

    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&workdir).expect("chdir to workdir");
    let result = commands::new::run(project_name, TemplateKind::Binary);
    std::env::set_current_dir(&original).expect("restore cwd");
    drop(_guard);

    let err = result.unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("already exists"),
        "expected already-exists error, got: {msg}"
    );

    cleanup(&workdir);
}

#[test]
fn test_new_rejects_invalid_name() {
    let _guard = cwd_lock();
    let workdir = unique_dir("new_invalid_name");

    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&workdir).expect("chdir to workdir");
    let result = commands::new::run("1bad", TemplateKind::Binary);
    std::env::set_current_dir(&original).expect("restore cwd");
    drop(_guard);

    let err = result.unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("letter or underscore"),
        "expected name-validation error, got: {msg}"
    );

    // No directory should have been created.
    assert!(
        !workdir.join("1bad").exists(),
        "invalid-name `buff new` should not create a directory"
    );

    cleanup(&workdir);
}

// ---------------------------------------------------------------------------
// `buff init` — current directory scaffolding
// ---------------------------------------------------------------------------

#[test]
fn test_init_scaffolds_current_directory() {
    let _guard = cwd_lock();
    let workdir = unique_dir("init_works");
    // The directory name must itself be a valid project name — rename it.
    let init_dir = workdir.join("init_project");
    let _ = fs::create_dir_all(&init_dir);

    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&init_dir).expect("chdir to init_dir");
    let result = commands::init::run();
    std::env::set_current_dir(&original).expect("restore cwd");
    drop(_guard);

    result.expect("`buff init` should succeed in a clean dir");

    for rel in &["buff.toml", "src/main.buff", ".gitignore", "README.md"] {
        let p = init_dir.join(rel);
        assert!(
            p.exists(),
            "expected `{}` to exist after `buff init`",
            p.display()
        );
    }

    // The manifest should pick up the directory name.
    let toml = fs::read_to_string(init_dir.join("buff.toml")).unwrap_or_default();
    assert!(
        toml.contains("name = \"init_project\""),
        "init should derive name from directory; got:\n{toml}"
    );

    cleanup(&workdir);
}

#[test]
fn test_init_refuses_existing_manifest() {
    let _guard = cwd_lock();
    let workdir = unique_dir("init_refuses");
    let init_dir = workdir.join("init_already");
    let _ = fs::create_dir_all(&init_dir);
    fs::write(init_dir.join("buff.toml"), "[package]\nname = \"old\"\n")
        .unwrap_or_else(|e| panic!("write: {e}"));

    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&init_dir).expect("chdir to init_dir");
    let result = commands::init::run();
    std::env::set_current_dir(&original).expect("restore cwd");
    drop(_guard);

    let err = result.unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("already exists"),
        "expected already-exists error for existing buff.toml, got: {msg}"
    );

    cleanup(&workdir);
}

// ---------------------------------------------------------------------------
// T31: Cargo.lock generation for scaffolded projects.
// ---------------------------------------------------------------------------

#[test]
fn test_new_generates_cargo_lock() {
    let _guard = cwd_lock();
    let workdir = unique_dir("new_cargo_lock");
    let project_name = "lock_test";

    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&workdir).expect("chdir to workdir");
    commands::new::run(project_name, TemplateKind::Binary).expect("scaffold");
    std::env::set_current_dir(&original).expect("restore cwd");
    drop(_guard);

    let project_dir = workdir.join(project_name);

    // Cargo.toml should exist (generated from buff.toml).
    let cargo_toml = project_dir.join("Cargo.toml");
    assert!(
        cargo_toml.exists(),
        "expected Cargo.toml to exist after `buff new`"
    );

    // Cargo.lock should exist (generated by cargo generate-lockfile).
    let cargo_lock = project_dir.join("Cargo.lock");
    assert!(
        cargo_lock.exists(),
        "expected Cargo.lock to exist after `buff new`"
    );

    // Cargo.lock should be non-empty.
    let lock_content = fs::read_to_string(&cargo_lock).unwrap_or_default();
    assert!(!lock_content.is_empty(), "Cargo.lock should not be empty");

    cleanup(&project_dir);
    cleanup(&workdir);
}

// ---------------------------------------------------------------------------
// End-to-end (rustc required): the scaffolded project actually runs.
// ---------------------------------------------------------------------------

#[test]
fn test_new_generated_project_runs() {
    if !rustc_available() {
        eprintln!("skipping test_new_generated_project_runs: rustc not on PATH");
        return;
    }

    let _guard = cwd_lock();
    let workdir = unique_dir("new_runs");
    let project_name = "hello_runner";

    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&workdir).expect("chdir to workdir");
    commands::new::run(project_name, TemplateKind::Binary).expect("scaffold");
    let main_path = workdir.join(project_name).join("src/main.buff");
    std::env::set_current_dir(&original).expect("restore cwd");
    drop(_guard);

    // Compile + run the scaffolded program, capturing stdout.
    // Using commands::run::run forwards stdout to the test process, which
    // doesn't let us assert — so spawn the `buff` lib pipeline via a captured
    // Command isn't trivial; instead we call the run entrypoint and rely on
    // the fact that it prints to stdout that the test runner captures.
    let result = run_with_defaults(&main_path, &[], false);
    cleanup(&workdir);

    result.expect("generated `buff new` project should run end-to-end");
}
