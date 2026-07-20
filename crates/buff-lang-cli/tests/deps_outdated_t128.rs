//! T128 end-to-end CLI test — `buff deps` tree rendering, `--why`
//! chain, and `buff outdated` against an in-process `buff-registry`
//! HTTP server.
//!
//! # Strategy
//!
//! Mirrors `tests/registry_cli_t127.rs`:
//!
//! - Builds [`buff_registry::app`] with a fresh
//!   [`buff_registry::InMemoryStorage`], binds a real
//!   `tokio::net::TcpListener` to `127.0.0.1:0` (ephemeral port),
//!   `axum::serve`s it on a `tokio::spawn`-ed task.
//! - Drives the sync CLI helpers directly (NOT via subprocess — the
//!   binary is too thin to make subprocess worth the overhead).
//!
//! # Coverage
//!
//! - [`deps_tree_renders_all_three_sections`]: `buff deps` on a
//!   synthetic `buff.toml` with all three dependency kinds produces
//!   a tree containing every name + version + source marker.
//! - [`deps_why_chain_explains_rust_dep`] / [`deps_why_chain_explains_git_dep`]
//!   / [`deps_why_chain_explains_registry_dep`]: `--why <PKG>` for
//!   each section.
//! - [`deps_why_unknown_pkg_errors`]: `--why` for an undeclared pkg
//!   returns `Err`.
//! - [`outdated_detects_newer_version`] / [`outdated_pinned_to_latest_reports_up_to_date`]:
//!   `buff outdated` round-trip through the in-process registry.
//! - [`outdated_reports_resolve_errors_as_warnings`]: a missing
//!   package surfaces as a warning, not an abort.

#![allow(clippy::needless_pass_by_value)]
// Each `#[tokio::test]` holds the process-wide `ENV_LOCK` guard across the
// `spawn_registry(...).await` so that `BUFF_REGISTRY_URL` / `BUFF_HOME` env
// mutation is serialized for the whole test body. The guard is a plain
// `std::sync::Mutex` and is intentionally held across the await point.
#![allow(clippy::await_holding_lock)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use buff_lang_cli::commands;
use buff_lang_cli::config::BuffConfig;
use buff_registry::{app, AppState, InMemoryStorage, Storage};

/// Process-wide mutex serialising tests that mutate `BUFF_HOME` /
/// `BUFF_REGISTRY_URL` env vars (reqwest + buff_home_dir both read
/// them at call time, so concurrent tests would race).
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Per-test temp root: `<temp>/buff-t128-<pid>/`.
fn temp_root() -> PathBuf {
    use std::sync::Mutex;
    static ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
    let mut guard = ROOT.lock().expect("temp_root mutex unpoisoned");
    if let Some(p) = guard.as_ref() {
        return p.clone();
    }
    let dir = std::env::temp_dir().join(format!("buff-t128-{}", std::process::id()));
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

/// Hold + restore a single env var. Restores on Drop so tests don't
/// bleed state across each other (and into the rest of the suite).
struct EnvGuard {
    key: &'static str,
    prev: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let prev = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

/// Spin up an in-process registry on an ephemeral port. Returns the
/// base URL once the server is ready to accept connections, plus the
/// shared storage Arc (so tests can seed it via the trait methods).
async fn spawn_registry(token: &str) -> (String, Arc<InMemoryStorage>) {
    let storage = Arc::new(InMemoryStorage::new());
    storage.add_token(token);
    let state = AppState::new(storage.clone() as Arc<dyn Storage>);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("http://{addr}");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app(state)).await;
    });
    (url, storage)
}

/// Run a blocking closure on a FRESH OS thread (no ambient tokio
/// runtime), returning its result.
///
/// Required because `reqwest::blocking` internally builds a tokio
/// runtime, and that runtime's drop panics if it's created within a
/// context where blocking is not allowed (i.e. inside a `#[tokio::test]`
/// worker thread). Spawning a fresh thread isolates the blocking
/// runtime from the test's multi-thread runtime so neither's drop
/// witnesses the other.
fn run_blocking<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(f)
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Write a minimal valid `buff.toml` + `src/main.buff` so
/// `buff publish` has something to pack. Used to seed registry
/// packages before the `buff outdated` round-trip.
fn write_project(root: &Path, name: &str, version: &str) {
    fs::create_dir_all(root.join("src")).expect("mkdir src");
    fs::write(
        root.join("buff.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n"),
    )
    .expect("write buff.toml");
    fs::write(
        root.join("src").join("main.buff"),
        "func main():\n    print(\"hello\")\n",
    )
    .expect("write main.buff");
}

/// A consumer `buff.toml` with all three dependency kinds declared,
/// for the tree-rendering assertions.
const CONSUMER_TOML_WITH_ALL_KINDS: &str = "[package]\n\
name = \"consumer-app\"\n\
version = \"0.3.2\"\n\
\n\
[rust-deps]\n\
serde = \"1.0\"\n\
tokio = \"1\"\n\
\n\
[git-dependencies]\n\
lib = { git = \"https://example/lib.buff\", branch = \"dev\" }\n\
\n\
[registry-dependencies]\n\
pkg-cycle = { version = \"^1.0.0\" }\n\
pkg-req = { version = \"*\" }\n";

// -----------------------------------------------------------------------
// `buff deps` tree rendering — pure helper, no network needed.
// -----------------------------------------------------------------------

#[test]
fn deps_tree_renders_all_three_sections() {
    let cfg = BuffConfig::parse(CONSUMER_TOML_WITH_ALL_KINDS).expect("parse consumer toml");
    let tree = commands::deps::render_tree(&cfg);

    // Root line: project name + version.
    assert!(
        tree.contains("consumer-app v0.3.2"),
        "root line missing: {tree}"
    );

    // All three section headers.
    assert!(tree.contains("[rust-deps]"), "rust-deps header: {tree}");
    assert!(
        tree.contains("[git-dependencies]"),
        "git-dependencies header: {tree}"
    );
    assert!(
        tree.contains("[registry-dependencies]"),
        "registry-dependencies header: {tree}"
    );

    // Names + versions for each section.
    assert!(tree.contains("serde v1.0"), "rust dep serde: {tree}");
    assert!(tree.contains("tokio v1"), "rust dep tokio: {tree}");
    assert!(
        tree.contains("lib v*"),
        "git dep name + wildcard version: {tree}"
    );
    assert!(
        tree.contains("branch=dev"),
        "git dep branch qualifier: {tree}"
    );
    assert!(
        tree.contains("https://example/lib.buff"),
        "git dep url: {tree}"
    );
    assert!(
        tree.contains("pkg-cycle v^1.0.0"),
        "registry dep semver req: {tree}"
    );
    assert!(
        tree.contains("(registry)"),
        "registry dep source marker: {tree}"
    );

    // Section ordering: rust-deps < git-dependencies < registry-dependencies.
    let rust_pos = tree.find("[rust-deps]").expect("rust-deps section");
    let git_pos = tree
        .find("[git-dependencies]")
        .expect("git-dependencies section");
    let reg_pos = tree
        .find("[registry-dependencies]")
        .expect("registry-dependencies section");
    assert!(rust_pos < git_pos, "rust before git: {tree}");
    assert!(git_pos < reg_pos, "git before registry: {tree}");

    // The last section's header should use the `└──` branch (not `├──`).
    let reg_line = tree
        .lines()
        .find(|l| l.contains("[registry-dependencies]"))
        .expect("registry line");
    assert!(
        reg_line.starts_with("└──"),
        "last section uses └── branch: {reg_line}"
    );
}

#[test]
fn deps_tree_empty_manifest_says_no_dependencies() {
    let cfg =
        BuffConfig::parse("[package]\nname = \"x\"\nversion = \"0.1.0\"\n").expect("parse minimal");
    let tree = commands::deps::render_tree(&cfg);
    assert!(
        tree.contains("(no dependencies declared)"),
        "placeholder missing: {tree}"
    );
}

// -----------------------------------------------------------------------
// `buff deps --why <PKG>` — pure helper, no network needed.
// -----------------------------------------------------------------------

#[test]
fn deps_why_chain_explains_rust_dep() {
    let cfg = BuffConfig::parse(CONSUMER_TOML_WITH_ALL_KINDS).expect("parse");
    let chain = commands::deps::render_why_chain(&cfg, "serde").expect("serde chain");
    assert!(chain.contains("Why is `serde`"), "question header: {chain}");
    assert!(
        chain.contains("[rust-deps]"),
        "section attribution: {chain}"
    );
    assert!(chain.contains("v1.0"), "version preserved: {chain}");
    assert!(
        chain.contains("required by: consumer-app"),
        "root attribution: {chain}"
    );
}

#[test]
fn deps_why_chain_explains_git_dep() {
    let cfg = BuffConfig::parse(CONSUMER_TOML_WITH_ALL_KINDS).expect("parse");
    let chain = commands::deps::render_why_chain(&cfg, "lib").expect("lib chain");
    assert!(
        chain.contains("[git-dependencies]"),
        "git section attribution: {chain}"
    );
    assert!(
        chain.contains("https://example/lib.buff"),
        "url preserved: {chain}"
    );
    assert!(
        chain.contains("branch=dev"),
        "branch qualifier preserved: {chain}"
    );
}

#[test]
fn deps_why_chain_explains_registry_dep() {
    let cfg = BuffConfig::parse(CONSUMER_TOML_WITH_ALL_KINDS).expect("parse");
    let chain = commands::deps::render_why_chain(&cfg, "pkg-cycle").expect("pkg-cycle chain");
    assert!(
        chain.contains("[registry-dependencies]"),
        "registry section attribution: {chain}"
    );
    assert!(chain.contains("v^1.0.0"), "version preserved: {chain}");
}

#[test]
fn deps_why_unknown_pkg_errors() {
    let cfg = BuffConfig::parse(CONSUMER_TOML_WITH_ALL_KINDS).expect("parse");
    let result = commands::deps::render_why_chain(&cfg, "not-declared");
    assert!(
        result.is_err(),
        "undeclared pkg must Err, not silently emit empty chain"
    );
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("not-declared"),
        "error mentions pkg name: {msg}"
    );
}

// -----------------------------------------------------------------------
// `buff deps` end-to-end via run() — chdir into a tempdir, assert
// stdout goes nowhere we can't capture easily, but at least exercise
// the disk path (proves `BuffConfig::load_from_file` is wired up).
// -----------------------------------------------------------------------

#[test]
fn deps_run_loads_buff_toml_from_cwd() {
    let _env_guard = env_lock();
    let project = unique_dir("project-deps-run");
    fs::write(
        project.join("buff.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n\
         [rust-deps]\nserde = \"1.0\"\n",
    )
    .expect("write buff.toml");

    // chdir to the project root for the duration of the call.
    let prev_cwd = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&project).expect("chdir");
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _cwd_guard = CwdGuard(prev_cwd);

    // `buff deps` writes to stdout; we only assert it does NOT error
    // (the rendering assertions live in the unit tests above).
    let result = commands::deps::run(None);
    assert!(
        result.is_ok(),
        "deps run should succeed: {:?}",
        result.err()
    );

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn deps_run_errors_when_no_buff_toml() {
    let _env_guard = env_lock();
    let project = unique_dir("project-no-toml");
    // Deliberately do NOT write buff.toml.

    let prev_cwd = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&project).expect("chdir");
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _cwd_guard = CwdGuard(prev_cwd);

    let result = commands::deps::run(None);
    assert!(result.is_err(), "missing buff.toml must Err, not panic");

    let _ = fs::remove_dir_all(&project);
}

// -----------------------------------------------------------------------
// `buff outdated` — in-process registry round-trip.
// -----------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outdated_detects_newer_version() {
    let _env_guard = env_lock();
    let home = unique_dir("home-outdated-detect");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    let (url, _storage) = spawn_registry("outdated-token").await;

    // Publish pkg-outdated @ 1.0.0 then @ 1.2.0 so a consumer pinning
    // `^1.0.0` resolves to 1.0.0... wait, `^1.0.0` matches 1.2.0 too.
    // Use `=1.0.0` to force the pinned resolution to the OLDER one.
    let publisher_v1 = unique_dir("publisher-outdated-v1");
    write_project(&publisher_v1, "pkg-outdated", "1.0.0");
    let publisher_v2 = unique_dir("publisher-outdated-v2");
    write_project(&publisher_v2, "pkg-outdated", "1.2.0");

    let url_clone = url.clone();
    let p1 = publisher_v1.clone();
    let p2 = publisher_v2.clone();
    run_blocking(move || {
        commands::login::run_with_token("outdated-token", &url_clone).expect("login");
        let cfg_v1 = BuffConfig::load_from_file(&p1.join("buff.toml")).expect("load v1 cfg");
        commands::publish::publish_project(&cfg_v1, &p1, &url_clone, "outdated-token")
            .expect("publish v1");
        let cfg_v2 = BuffConfig::load_from_file(&p2.join("buff.toml")).expect("load v2 cfg");
        commands::publish::publish_project(&cfg_v2, &p2, &url_clone, "outdated-token")
            .expect("publish v2");
    });

    // Consumer pins `=1.0.0` while latest is 1.2.0.
    let consumer = unique_dir("consumer-outdated-detect");
    fs::write(
        consumer.join("buff.toml"),
        "[package]\n\
         name = \"app\"\n\
         version = \"0.1.0\"\n\
         \n\
         [registry-dependencies]\n\
         pkg-outdated = { version = \"=1.0.0\" }\n",
    )
    .expect("write consumer buff.toml");

    let url_for_check = url.clone();
    let consumer_for_check = consumer.clone();
    let rows = run_blocking(move || {
        let _url_guard = EnvGuard::set("BUFF_REGISTRY_URL", &url_for_check);
        let cfg = BuffConfig::load_from_file(&consumer_for_check.join("buff.toml"))
            .expect("load consumer");
        commands::outdated::check_outdated(&cfg, &url_for_check).expect("check_outdated")
    });

    assert_eq!(rows.len(), 1, "one registry dep: {rows:?}");
    let row = &rows[0];
    assert_eq!(row.name, "pkg-outdated");
    assert_eq!(row.pinned_req, "=1.0.0");
    assert_eq!(
        row.current.as_deref(),
        Some("1.0.0"),
        "pinned resolution: {row:?}"
    );
    assert_eq!(
        row.latest.as_deref(),
        Some("1.2.0"),
        "latest resolution: {row:?}"
    );
    assert!(row.is_outdated(), "must flag as outdated: {row:?}");

    // Rendered report contains the outdated marker + both versions.
    let report = commands::outdated::render_report(&rows);
    assert!(report.contains("pkg-outdated"), "name in report: {report}");
    assert!(report.contains("1.0.0"), "current in report: {report}");
    assert!(report.contains("1.2.0"), "latest in report: {report}");
    // The `*` marker is column-padded after the latest version
    // (`{:<16}{}` format), so assert the row line ends with `*`
    // rather than looking for `1.2.0 *` as a substring.
    let pkg_line = report
        .lines()
        .find(|l| l.contains("pkg-outdated"))
        .expect("pkg-outdated line present");
    assert!(
        pkg_line.trim_end().ends_with('*'),
        "row ends with outdated `*` marker: {pkg_line:?}"
    );
    assert!(
        report.contains("Outdated — newer version available"),
        "outdated legend: {report}"
    );

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&publisher_v1);
    let _ = fs::remove_dir_all(&publisher_v2);
    let _ = fs::remove_dir_all(&consumer);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outdated_pinned_to_latest_reports_up_to_date() {
    let _env_guard = env_lock();
    let home = unique_dir("home-outdated-uptodate");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    let (url, _storage) = spawn_registry("uptodate-token").await;

    // Publish pkg-uptodate @ 2.0.0 only.
    let publisher = unique_dir("publisher-uptodate");
    write_project(&publisher, "pkg-uptodate", "2.0.0");

    let url_clone = url.clone();
    let p = publisher.clone();
    run_blocking(move || {
        commands::login::run_with_token("uptodate-token", &url_clone).expect("login");
        let cfg = BuffConfig::load_from_file(&p.join("buff.toml")).expect("load cfg");
        commands::publish::publish_project(&cfg, &p, &url_clone, "uptodate-token")
            .expect("publish");
    });

    // Consumer uses `*` so the pinned resolution equals the latest.
    let consumer = unique_dir("consumer-uptodate");
    fs::write(
        consumer.join("buff.toml"),
        "[package]\n\
         name = \"app\"\n\
         version = \"0.1.0\"\n\
         \n\
         [registry-dependencies]\n\
         pkg-uptodate = { version = \"*\" }\n",
    )
    .expect("write consumer buff.toml");

    let url_for_check = url.clone();
    let consumer_for_check = consumer.clone();
    let rows = run_blocking(move || {
        let _url_guard = EnvGuard::set("BUFF_REGISTRY_URL", &url_for_check);
        let cfg = BuffConfig::load_from_file(&consumer_for_check.join("buff.toml"))
            .expect("load consumer");
        commands::outdated::check_outdated(&cfg, &url_for_check).expect("check_outdated")
    });

    assert_eq!(rows.len(), 1, "one dep: {rows:?}");
    let row = &rows[0];
    assert_eq!(row.current.as_deref(), Some("2.0.0"));
    assert_eq!(row.latest.as_deref(), Some("2.0.0"));
    assert!(!row.is_outdated(), "must NOT flag as outdated: {row:?}");

    let report = commands::outdated::render_report(&rows);
    assert!(
        report.contains("All registry dependencies are up to date"),
        "up-to-date legend: {report}"
    );
    // No `*` marker terminates any row in the up-to-date case.
    let pkg_line = report
        .lines()
        .find(|l| l.contains("pkg-uptodate"))
        .expect("pkg-uptodate line present");
    assert!(
        !pkg_line.trim_end().ends_with('*'),
        "no outdated `*` marker on up-to-date row: {pkg_line:?}"
    );

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&publisher);
    let _ = fs::remove_dir_all(&consumer);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outdated_reports_resolve_errors_as_warnings() {
    let _env_guard = env_lock();
    let home = unique_dir("home-outdated-warn");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    let (url, _storage) = spawn_registry("warn-token").await;

    // Consumer references a package the registry has NEVER seen.
    let consumer = unique_dir("consumer-outdated-warn");
    fs::write(
        consumer.join("buff.toml"),
        "[package]\n\
         name = \"app\"\n\
         version = \"0.1.0\"\n\
         \n\
         [registry-dependencies]\n\
         never-published = { version = \"^1.0.0\" }\n",
    )
    .expect("write consumer buff.toml");

    let url_for_check = url.clone();
    let consumer_for_check = consumer.clone();
    let rows = run_blocking(move || {
        let _url_guard = EnvGuard::set("BUFF_REGISTRY_URL", &url_for_check);
        let cfg = BuffConfig::load_from_file(&consumer_for_check.join("buff.toml"))
            .expect("load consumer");
        commands::outdated::check_outdated(&cfg, &url_for_check).expect("check_outdated")
    });

    assert_eq!(rows.len(), 1, "row emitted even on error: {rows:?}");
    let row = &rows[0];
    assert_eq!(row.name, "never-published");
    assert!(row.current.is_none(), "no current: {row:?}");
    assert!(row.latest.is_none(), "no latest: {row:?}");
    assert!(row.error.is_some(), "error captured: {row:?}");
    assert!(
        !row.is_outdated(),
        "must NOT flag as outdated when resolve failed: {row:?}"
    );

    let report = commands::outdated::render_report(&rows);
    assert!(report.contains("warning:"), "warning prefix: {report}");
    // The dashed column row still prints, with the name in column 1
    // and `-` placeholders in the version columns.
    assert!(
        report.contains("never-published"),
        "name in table even on failure: {report}"
    );

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&consumer);
}

// -----------------------------------------------------------------------
// `buff outdated` end-to-end via run() — exercises the cwd-load +
// registry_url() resolution + print path. Captures stdout via a
// swallow (we don't assert on the printed text, only that run()
// succeeds; the row assertions above are the contract gate).
// -----------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outdated_run_end_to_end_succeeds() {
    let _env_guard = env_lock();
    let home = unique_dir("home-outdated-run");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    let (url, _storage) = spawn_registry("run-token").await;

    // Publish pkg-run @ 1.0.0.
    let publisher = unique_dir("publisher-run");
    write_project(&publisher, "pkg-run", "1.0.0");

    let url_clone = url.clone();
    let p = publisher.clone();
    run_blocking(move || {
        commands::login::run_with_token("run-token", &url_clone).expect("login");
        let cfg = BuffConfig::load_from_file(&p.join("buff.toml")).expect("load cfg");
        commands::publish::publish_project(&cfg, &p, &url_clone, "run-token").expect("publish");
    });

    // Consumer pins `*` — up-to-date case so the report prints the
    // "all up-to-date" footer.
    let consumer = unique_dir("consumer-run");
    fs::write(
        consumer.join("buff.toml"),
        "[package]\n\
         name = \"app\"\n\
         version = \"0.1.0\"\n\
         \n\
         [registry-dependencies]\n\
         pkg-run = { version = \"*\" }\n",
    )
    .expect("write consumer buff.toml");

    let url_for_run = url.clone();
    let consumer_for_run = consumer.clone();
    let result = run_blocking(move || {
        let _url_guard = EnvGuard::set("BUFF_REGISTRY_URL", &url_for_run);
        let prev_cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&consumer_for_run).expect("chdir consumer");
        struct CwdGuard(PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _cwd_guard = CwdGuard(prev_cwd);
        commands::outdated::run()
    });
    assert!(
        result.is_ok(),
        "outdated run should succeed: {:?}",
        result.err()
    );

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&publisher);
    let _ = fs::remove_dir_all(&consumer);
}

// -----------------------------------------------------------------------
// `buff outdated` with NO registry deps → friendly empty message,
// no network touched. Exercises the run() path that prints the
// "No registry dependencies declared" footer.
// -----------------------------------------------------------------------

#[test]
fn outdated_run_empty_message_when_no_registry_deps() {
    let _env_guard = env_lock();
    let project = unique_dir("project-empty-reg");
    fs::write(
        project.join("buff.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n\
         [rust-deps]\nserde = \"1.0\"\n",
    )
    .expect("write buff.toml");

    let prev_cwd = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&project).expect("chdir");
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _cwd_guard = CwdGuard(prev_cwd);

    // Point BUFF_REGISTRY_URL at an unreachable address to PROVE
    // no network call happens when the dep section is empty.
    let _url_guard = EnvGuard::set("BUFF_REGISTRY_URL", "http://127.0.0.1:1");
    let result = commands::outdated::run();
    assert!(
        result.is_ok(),
        "no network call for empty reg deps: {:?}",
        result.err()
    );

    let _ = fs::remove_dir_all(&project);
}
