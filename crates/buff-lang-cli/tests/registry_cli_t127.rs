//! T127 end-to-end CLI test — `buff login` / `buff publish` / `buff add
//! <name>` / `buff install` against an in-process `buff-registry` HTTP
//! server.
//!
//! # Strategy
//!
//! Builds the [`buff_registry::app`] Router with a fresh
//! [`buff_registry::InMemoryStorage`] seeded with a test token, binds a
//! real `tokio::net::TcpListener` to `127.0.0.1:0` (ephemeral port),
//! and `axum::serve`s it on a `tokio::spawn`-ed task. The CLI commands
//! are sync (they use `reqwest::blocking`), but the multi-thread test
//! runtime lets the server respond while the test thread blocks in
//! reqwest.
//!
//! Each test:
//!
//! 1. Sets `BUFF_REGISTRY_URL` to the bound URL.
//! 2. Sets `BUFF_HOME` to a per-test tempdir (so credentials +
//!    install dirs don't pollute the user's `~/.buff`).
//! 3. Calls the sync CLI helpers directly (NOT via subprocess — the
//!    binary is too thin to make subprocess worth the overhead).
//!
//! If the registry's bind fails (rare — port exhaustion), the test
//! fails; we never skip a smoke this central to T127.
//!
//! # Coverage
//!
//! - [`login_stores_token`]: `buff login <TOKEN>` writes the file.
//! - [`publish_uploads_tarball`]: `buff publish` round-trips a real
//!   tarball to the registry; subsequent GET confirms it.
//! - [`login_then_publish_then_add_round_trip`]: the full
//!   publish→consume cycle. Login stores the token, publish uploads
//!   `pkg-a`, `buff add pkg-a` resolves + records in `buff.toml`.
//! - [`install_downloads_and_unpacks_tarball`]: `buff install <name>`
//!   downloads + unpacks into `<buff_home>/install/<name>/<version>/`.

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

/// Per-test temp root: `<temp>/buff-t127-<pid>/`.
fn temp_root() -> PathBuf {
    use std::sync::Mutex;
    static ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
    let mut guard = ROOT.lock().expect("temp_root mutex unpoisoned");
    if let Some(p) = guard.as_ref() {
        return p.clone();
    }
    let dir = std::env::temp_dir().join(format!("buff-t127-{}", std::process::id()));
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
/// base URL once the server is ready to accept connections.
///
/// The returned guard keeps the spawned task alive until the end of
/// the test (dropping it does NOT abort the task — tokio's runtime
/// teardown at the end of the `#[tokio::test]` does — but binding the
/// storage Arc to the test scope prevents accidental reuse across
/// tests).
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

/// Write a minimal valid `buff.toml` + `src/main.buff` so `buff
/// publish` has something to pack.
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

// ---------------------------------------------------------------------------
// Acceptance bullet: `buff login <TOKEN>` stores credentials.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_stores_token() {
    let _env_guard = env_lock();
    let home = unique_dir("home-login");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    let (url, _storage) = spawn_registry("ignored-token").await;

    let url_clone = url.clone();
    run_blocking(move || {
        commands::login::run_with_token("my-test-token", &url_clone).expect("login");
    });

    let creds_path = home.join("credentials");
    assert!(creds_path.is_file(), "credentials file must exist");
    let text = fs::read_to_string(&creds_path).expect("read creds");
    assert!(
        text.contains("my-test-token"),
        "credentials file must contain the token: {text}"
    );

    let _ = fs::remove_dir_all(&home);
}

// ---------------------------------------------------------------------------
// Acceptance bullet: `buff publish` uploads a tarball that can be
// downloaded back.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publish_uploads_tarball() {
    let _env_guard = env_lock();
    let home = unique_dir("home-publish");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    let (url, _storage) = spawn_registry("pub-token").await;

    // Build a project, then publish.
    let project = unique_dir("project-publish");
    write_project(&project, "pub-pkg", "0.1.0");

    let url_clone = url.clone();
    let project_clone = project.clone();
    run_blocking(move || {
        // Store credentials so publish can read them.
        commands::login::run_with_token("pub-token", &url_clone).expect("login");
        let cfg = BuffConfig::load_from_file(&project_clone.join("buff.toml")).expect("load cfg");
        let response =
            commands::publish::publish_project(&cfg, &project_clone, &url_clone, "pub-token")
                .expect("publish");
        assert_eq!(response.name, "pub-pkg");
        assert_eq!(response.version, "0.1.0");
    });

    // Download back and verify it's a valid tar (smoke). Use a fresh
    // blocking thread so reqwest::blocking doesn't share the test's
    // tokio runtime.
    let url_for_download = url.clone();
    let bytes = run_blocking(move || {
        commands::registry::download_tarball(&url_for_download, "pub-pkg", "0.1.0")
            .expect("download")
    });
    assert!(!bytes.is_empty(), "downloaded tarball must be non-empty");
    let mut archive = tar::Archive::new(&bytes[..]);
    let entries: Vec<Result<tar::Entry<_>, _>> = archive.entries().expect("entries").collect();
    assert!(
        !entries.is_empty(),
        "tarball must contain at least one entry"
    );

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&project);
}

// ---------------------------------------------------------------------------
// Acceptance bullet + QA scenario: full publish → consume cycle.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_then_publish_then_add_round_trip() {
    let _env_guard = env_lock();
    let home = unique_dir("home-cycle");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    let (url, _storage) = spawn_registry("cycle-token").await;

    // Step 1: publish `pkg-cycle`.
    let publisher_project = unique_dir("publisher");
    write_project(&publisher_project, "pkg-cycle", "1.0.0");

    let url_clone = url.clone();
    let publisher_project_clone = publisher_project.clone();
    run_blocking(move || {
        commands::login::run_with_token("cycle-token", &url_clone).expect("login");
        let pub_cfg = BuffConfig::load_from_file(&publisher_project_clone.join("buff.toml"))
            .expect("load pub cfg");
        commands::publish::publish_project(
            &pub_cfg,
            &publisher_project_clone,
            &url_clone,
            "cycle-token",
        )
        .expect("publish pkg-cycle");
    });

    // Step 2: in a separate consumer project, `buff add pkg-cycle`.
    let consumer_project = unique_dir("consumer");
    fs::write(
        consumer_project.join("buff.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write consumer buff.toml");

    let url_for_add = url.clone();
    let consumer_for_add = consumer_project.clone();
    run_blocking(move || {
        // `buff add` reads buff.toml from cwd, so chdir.
        let prev_cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&consumer_for_add).expect("chdir consumer");
        struct CwdGuard(PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _cwd_guard = CwdGuard(prev_cwd);

        // Point BUFF_REGISTRY_URL at our ephemeral registry (the
        // `commands::add::run` helper reads it via
        // `commands::registry::registry_url`).
        let _url_guard = EnvGuard::set("BUFF_REGISTRY_URL", &url_for_add);
        commands::add::run("pkg-cycle", None, None, None).expect("buff add pkg-cycle");
    });

    // Verify the buff.toml gained a `[registry-dependencies]` entry.
    let written = fs::read_to_string(consumer_project.join("buff.toml")).expect("read toml");
    assert!(
        written.contains("registry-dependencies"),
        "missing registry-dependencies section: {written}"
    );
    assert!(
        written.contains("pkg-cycle"),
        "missing pkg-cycle entry: {written}"
    );
    let cfg = BuffConfig::parse(&written).expect("round-trip parse");
    let entry = cfg
        .registry_dependencies
        .get("pkg-cycle")
        .expect("pkg-cycle entry");
    assert_eq!(entry.version, "*", "default req is *");

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&publisher_project);
    let _ = fs::remove_dir_all(&consumer_project);
}

// ---------------------------------------------------------------------------
// `buff add <name>@<req>` with a specific semver requirement.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_with_explicit_version_req_records_the_req() {
    let _env_guard = env_lock();
    let home = unique_dir("home-req");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    let (url, _storage) = spawn_registry("req-token").await;

    // Publish two versions so a `^1.0.0` req resolves.
    let publisher_project = unique_dir("publisher-req");
    write_project(&publisher_project, "pkg-req", "1.0.0");
    let publisher_project_v2 = unique_dir("publisher-req-v2");
    write_project(&publisher_project_v2, "pkg-req", "1.2.0");

    let url_clone = url.clone();
    let p1 = publisher_project.clone();
    let p2 = publisher_project_v2.clone();
    run_blocking(move || {
        commands::login::run_with_token("req-token", &url_clone).expect("login");
        let cfg_v1 = BuffConfig::load_from_file(&p1.join("buff.toml")).expect("load v1 cfg");
        commands::publish::publish_project(&cfg_v1, &p1, &url_clone, "req-token")
            .expect("publish v1");
        let cfg_v2 = BuffConfig::load_from_file(&p2.join("buff.toml")).expect("load v2 cfg");
        commands::publish::publish_project(&cfg_v2, &p2, &url_clone, "req-token")
            .expect("publish v2");
    });

    // Now consume with an explicit req.
    let consumer = unique_dir("consumer-req");
    fs::write(
        consumer.join("buff.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write consumer buff.toml");

    let url_for_add = url.clone();
    let consumer_for_add = consumer.clone();
    run_blocking(move || {
        let prev_cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&consumer_for_add).expect("chdir consumer");
        struct CwdGuard(PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _cwd_guard = CwdGuard(prev_cwd);
        let _url_guard = EnvGuard::set("BUFF_REGISTRY_URL", &url_for_add);
        commands::add::run("pkg-req@^1.0.0", None, None, None).expect("buff add");
    });

    let written = fs::read_to_string(consumer.join("buff.toml")).expect("read toml");
    let cfg = BuffConfig::parse(&written).expect("round-trip parse");
    let entry = cfg.registry_dependencies.get("pkg-req").expect("entry");
    assert_eq!(entry.version, "^1.0.0", "req must be preserved verbatim");

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&publisher_project);
    let _ = fs::remove_dir_all(&publisher_project_v2);
    let _ = fs::remove_dir_all(&consumer);
}

// ---------------------------------------------------------------------------
// `buff install <name>` downloads + unpacks into <buff_home>/install.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn install_downloads_and_unpacks_tarball() {
    let _env_guard = env_lock();
    let home = unique_dir("home-install");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    let (url, _storage) = spawn_registry("install-token").await;

    // Publish a package.
    let publisher = unique_dir("publisher-install");
    write_project(&publisher, "pkg-install", "0.2.0");

    let url_clone = url.clone();
    let publisher_clone = publisher.clone();
    run_blocking(move || {
        commands::login::run_with_token("install-token", &url_clone).expect("login");
        let cfg = BuffConfig::load_from_file(&publisher_clone.join("buff.toml")).expect("load cfg");
        commands::publish::publish_project(&cfg, &publisher_clone, &url_clone, "install-token")
            .expect("publish pkg-install");
    });

    // Install into a per-test install root (under BUFF_HOME).
    let install_root = home.join("install").join("pkg-install");
    let url_for_install = url.clone();
    let install_root_clone = install_root.clone();
    let target = run_blocking(move || {
        commands::install::install_latest("pkg-install", &url_for_install, &install_root_clone)
            .expect("install")
    });
    assert!(target.is_dir(), "install target dir must exist");
    // Tarball layout is `package/<file>` — unpack must populate it.
    let unpacked_package = target.join("package");
    assert!(
        unpacked_package.is_dir(),
        "unpacked package/ dir must exist at {}",
        unpacked_package.display()
    );
    assert!(
        unpacked_package.join("main.buff").is_file(),
        "unpacked main.buff must exist"
    );

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&publisher);
}

// ---------------------------------------------------------------------------
// Registry unreachable → `buff add <name>` falls back gracefully with
// an `Err` (so the user knows to start the registry). The git fallback
// is exercised separately by the T122 suite.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_errors_when_registry_unreachable() {
    let _env_guard = env_lock();
    let home = unique_dir("home-unreachable");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    let consumer = unique_dir("consumer-unreachable");
    fs::write(
        consumer.join("buff.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write consumer buff.toml");

    let consumer_clone = consumer.clone();
    let result = run_blocking(move || {
        // Point at a port that's almost certainly closed (the OS reserves
        // the very first ephemeral ports for bind() but rarely serves on
        // them; port 1 is reserved and unused in practice).
        let _url_guard = EnvGuard::set("BUFF_REGISTRY_URL", "http://127.0.0.1:1");

        let prev_cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&consumer_clone).expect("chdir consumer");
        struct CwdGuard(PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _cwd_guard = CwdGuard(prev_cwd);

        commands::add::run("nonexistent-pkg", None, None, None)
    });
    assert!(
        result.is_err(),
        "registry-unreachable add must Err, not panic"
    );

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&consumer);
}

// ---------------------------------------------------------------------------
// Publish without stored credentials → helpful error.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publish_without_credentials_errors_with_helpful_message() {
    let _env_guard = env_lock();
    let home = unique_dir("home-no-creds");
    let _home_guard = EnvGuard::set("BUFF_HOME", &home.display().to_string());

    // No `buff login` — credentials file does NOT exist.
    let project = unique_dir("project-no-creds");
    write_project(&project, "pkg-no-creds", "0.1.0");

    let require_result = run_blocking(commands::registry::require_token);
    assert!(
        require_result.is_err(),
        "require_token must Err when no creds file"
    );
    let msg = format!("{}", require_result.unwrap_err());
    assert!(
        msg.contains("buff login") || msg.contains("credentials"),
        "error must point at `buff login`: {msg}"
    );

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&project);
}
