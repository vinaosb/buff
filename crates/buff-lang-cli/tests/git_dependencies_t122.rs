//! T122 end-to-end CLI test — git dependencies via `buff add git+<URL>`.
//!
//! Verifies the full pipeline against a LOCAL git repo (created in a
//! tempdir on the fly) so the test stays hermetic and network-free:
//!
//! 1. Initialise a git repo at `<tmp>/<unique>/` with a `buff.toml` and
//!    a `src/main.buff`, then `git commit` so it has a real HEAD.
//! 2. Drive [`buff_lang_cli::commands::add::run_with_home`] from a
//!    separate project directory with `git+file:///<repo>` spec.
//! 3. Assert:
//!    - the checkout was created under `<home>/git/<hash>/`,
//!    - the project's `buff.toml` gained the expected `[git-dependencies]`
//!      entry,
//!    - the transitive-dep parse picked up the dependency declared in
//!      the cloned repo's `buff.toml`.
//!
//! If the `git` binary is absent (CI image without git, hostile host),
//! the tests skip gracefully with a diagnostic — never hard-fail.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use buff_lang_cli::commands;
use buff_lang_cli::config::git_checkout_path_for;

/// Process-wide mutex serialising tests that mutate the process cwd or
/// invoke `git` (which reads the local git config from cwd). Mirrors
/// `scaffold_tests.rs`'s `CWD_LOCK` pattern.
static GIT_TEST_LOCK: Mutex<()> = Mutex::new(());

fn git_lock() -> MutexGuard<'static, ()> {
    GIT_TEST_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Per-test temp root: `<temp>/buff-t122-<pid>/`.
///
/// The wipe-on-first-call guarantee is implemented via a `Mutex<Option<PathBuf>>`:
/// the first caller (across all parallel tests in this binary) wipes any
/// stale directory from a previous run, then creates the root. Subsequent
/// callers reuse the same root WITHOUT wiping — otherwise sibling tests'
/// `unique_dir` allocations would race-delete each other.
fn temp_root() -> PathBuf {
    use std::sync::Mutex;
    static ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
    let mut guard = ROOT.lock().expect("temp_root mutex unpoisoned");
    if let Some(p) = guard.as_ref() {
        return p.clone();
    }
    let dir = std::env::temp_dir().join(format!("buff-t122-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::create_dir_all(&dir);
    *guard = Some(dir.clone());
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

/// True when the `git` binary is on PATH and invokable.
fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn cleanup(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

/// Create a local git repo at `repo_dir` with a Buff-shaped layout:
/// `buff.toml`, `src/main.buff`, and one git commit. Returns the file://
/// URL suitable for `git+file://...` specs.
fn make_local_git_repo(repo_dir: &Path, lib_name: &str, with_dep: Option<(&str, &str)>) -> PathBuf {
    fs::create_dir_all(repo_dir.join("src")).expect("create src");

    let mut buff_toml =
        format!("[package]\nname = \"{lib_name}\"\nversion = \"0.1.0\"\n\n[dependencies]\n");
    if let Some((dep, ver)) = with_dep {
        buff_toml.push_str(&format!("{dep} = \"{ver}\"\n"));
    }
    fs::write(repo_dir.join("buff.toml"), buff_toml).expect("write buff.toml");
    fs::write(
        repo_dir.join("src").join("main.buff"),
        "func main():\n    print(\"hello from lib\")\n",
    )
    .expect("write main.buff");

    // git init + commit (config user locally to avoid touching global git config).
    let status = Command::new("git")
        .arg("init")
        .current_dir(repo_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git init");
    assert!(status.success(), "git init failed");

    for (k, v) in &[
        ("user.name", "buff-test"),
        ("user.email", "test@buff.local"),
    ] {
        let status = Command::new("git")
            .args(["config", k, v])
            .current_dir(repo_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git config");
        assert!(status.success(), "git config {k} failed");
    }

    let status = Command::new("git")
        .args(["add", "."])
        .current_dir(repo_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git add");
    assert!(status.success(), "git add failed");

    let status = Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(repo_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git commit");
    assert!(status.success(), "git commit failed");

    repo_dir.to_path_buf()
}

/// Build a `git+file://...` spec for a local repo dir. Forward-slash
/// form is required so git treats it as a URL (Windows backslashes
/// confuse git's URL parser). We deliberately avoid `canonicalize()`
/// here — on Windows it returns a `\\?\` UNC-prefixed path that
/// confuses git's URL parser, and `std::env::temp_dir()` already gives
/// us an absolute path.
fn file_spec(repo_dir: &Path) -> String {
    let abs = repo_dir.display().to_string();
    let forward = abs.replace('\\', "/");
    // `file:///<drive>:/path` is the canonical Windows file URL;
    // unix paths already start with `/`, producing `file:///path/...`.
    format!("git+file://{forward}")
}

// ---------------------------------------------------------------------------
// Happy-path: clone + buff.toml upsert + transitive-dep parse
// ---------------------------------------------------------------------------

#[test]
fn add_clones_repo_and_records_entry() {
    let _guard = git_lock();
    if !git_available() {
        eprintln!("skip: git binary not on PATH");
        return;
    }

    let home = unique_dir("home-add");
    let project = unique_dir("project-add");
    // Name the repo dir after the lib so `derive_dep_name` (which takes
    // the URL's last path segment) yields `libbuff` rather than the
    // unique-dir label.
    let repo = unique_dir("repo-add").join("libbuff");

    make_local_git_repo(&repo, "libbuff", Some(("serde", "1.0")));

    // Project pre-existing buff.toml.
    fs::write(
        project.join("buff.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write project buff.toml");

    let _prev = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&project).expect("chdir project");
    // Restore cwd on exit, even on panic.
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _cwd_guard = CwdGuard(_prev.clone());

    let spec = file_spec(&repo);
    commands::add::run_with_home(&spec, None, None, None, &home).expect("add must succeed");

    // 1. Checkout created at the expected path.
    let stripped = spec.strip_prefix("git+").expect("strip prefix");
    let expected_checkout = git_checkout_path_for(stripped, &home);
    assert!(
        expected_checkout.is_dir(),
        "checkout dir not created at {}",
        expected_checkout.display()
    );
    assert!(
        expected_checkout.join("buff.toml").is_file(),
        "checkout must contain buff.toml"
    );

    // 2. Project buff.toml gained [git-dependencies] entry.
    let written = fs::read_to_string(project.join("buff.toml")).expect("read buff.toml");
    assert!(
        written.contains("git-dependencies"),
        "missing section: {written}"
    );
    assert!(written.contains("libbuff"), "missing dep name: {written}");

    // 3. Re-parsing round-trips the new entry.
    let cfg = buff_lang_cli::config::BuffConfig::parse(&written).expect("round-trip parse");
    let dep = cfg.git_dependencies.get("libbuff").expect("libbuff entry");
    assert!(
        dep.git.contains("libbuff") || dep.git.starts_with("file:"),
        "git URL field populated: {}",
        dep.git
    );

    // 4. Idempotent: re-running doesn't error (checkout reuse path).
    commands::add::run_with_home(&spec, None, None, None, &home)
        .expect("second add must succeed (reuse)");

    cleanup(&home);
    cleanup(&project);
    cleanup(repo.parent().expect("repo parent"));
}

// ---------------------------------------------------------------------------
// Transitive-dep parse: read_transitive_deps finds cloned buff.toml
// ---------------------------------------------------------------------------

#[test]
fn add_reads_transitive_deps_from_cloned_buff_toml() {
    let _guard = git_lock();
    if !git_available() {
        eprintln!("skip: git binary not on PATH");
        return;
    }

    let home = unique_dir("home-trans");
    let project = unique_dir("project-trans");
    let repo = unique_dir("repo-trans").join("libbuff");

    make_local_git_repo(&repo, "libbuff", Some(("serde", "1.0")));

    fs::write(
        project.join("buff.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write project buff.toml");

    let prev = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&project).expect("chdir");
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _cwd_guard = CwdGuard(prev.clone());

    let spec = file_spec(&repo);
    commands::add::run_with_home(&spec, None, None, None, &home).expect("add must succeed");

    // Inspect the cloned buff.toml directly via read_transitive_deps.
    let stripped = spec.strip_prefix("git+").expect("strip");
    let checkout = git_checkout_path_for(stripped, &home);
    let transitive = commands::add::read_transitive_deps(&checkout).expect("transitive parse");
    let cfg = transitive.expect("cloned buff.toml must parse");
    assert_eq!(cfg.package.name, "libbuff");
    assert_eq!(
        cfg.dependencies.get("serde").map(|s| s.as_str()),
        Some("1.0")
    );

    cleanup(&home);
    cleanup(&project);
    cleanup(repo.parent().expect("repo parent"));
}

// ---------------------------------------------------------------------------
// Spec validation: missing `git+` prefix errors
// ---------------------------------------------------------------------------

#[test]
fn add_rejects_spec_without_git_prefix() {
    let _guard = git_lock();
    let home = unique_dir("home-prefix");
    let project = unique_dir("project-prefix");

    fs::write(
        project.join("buff.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write project buff.toml");

    let prev = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&project).expect("chdir");
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _cwd_guard = CwdGuard(prev);

    let res =
        commands::add::run_with_home("https://no-prefix.example/x.buff", None, None, None, &home);
    assert!(res.is_err(), "spec without git+ must error");

    cleanup(&home);
    cleanup(&project);
}

// ---------------------------------------------------------------------------
// Branch qualifier: clone via --branch
// ---------------------------------------------------------------------------

#[test]
fn add_with_branch_qualifier() {
    let _guard = git_lock();
    if !git_available() {
        eprintln!("skip: git binary not on PATH");
        return;
    }

    let home = unique_dir("home-branch");
    let project = unique_dir("project-branch");
    let repo = unique_dir("repo-branch").join("libbuff");

    make_local_git_repo(&repo, "libbuff", None);

    // Create a branch in the repo so `--branch dev` works.
    let status = Command::new("git")
        .args(["checkout", "-b", "dev"])
        .current_dir(&repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git checkout -b");
    assert!(status.success(), "git checkout -b dev failed");

    fs::write(
        project.join("buff.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write project buff.toml");

    let prev = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&project).expect("chdir");
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _cwd_guard = CwdGuard(prev);

    let spec = file_spec(&repo);
    commands::add::run_with_home(&spec, Some("dev"), None, None, &home)
        .expect("add with --branch must succeed");

    let written = fs::read_to_string(project.join("buff.toml")).expect("read buff.toml");
    let cfg = buff_lang_cli::config::BuffConfig::parse(&written).expect("round-trip");
    let dep = cfg.git_dependencies.get("libbuff").expect("entry");
    assert_eq!(
        dep.branch.as_deref(),
        Some("dev"),
        "branch qualifier preserved"
    );

    cleanup(&home);
    cleanup(&project);
    cleanup(repo.parent().expect("repo parent"));
}
