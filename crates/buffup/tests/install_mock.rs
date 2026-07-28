//! `buffup install` integration test with mocked GitHub Releases.
//!
//! Spins up a [`httpmock::MockServer`], redirects `BUFFUP_GITHUB_API`
//! and `BUFFUP_HOME` so neither the real network nor the user's real
//! `~/.buff/` directory is touched, then drives
//! [`buffup::commands::install::run`] through a full download +
//! extract cycle.
//!
//! # Test isolation
//!
//! All tests in this file mutate the `BUFFUP_HOME` and
//! `BUFFUP_GITHUB_API` env vars (read by `paths::buff_home` and
//! `github::api_base` at call time). The process-wide [`ENV_LOCK`]
//! serializes test bodies so concurrent runs do not race on the env
//! mutation — the same pattern used by the registry CLI tests in
//! `crates/buff-lang-cli/tests/registry_cli_t127.rs`.

// Each `#[tokio::test]` holds the process-wide `ENV_LOCK` guard across the
// `install::run(...).await` call so that `BUFFUP_HOME` /
// `BUFFUP_GITHUB_API` env mutation is serialized for the whole test
// body. The guard is a plain `std::sync::Mutex` and is intentionally
// held across the await point (no deadlock risk — the lock is only
// ever acquired by the test thread).
#![allow(clippy::await_holding_lock)]
#![cfg(test)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use buffup::commands::install;
use buffup::paths;
use httpmock::{Method, MockServer};

/// Process-wide mutex serializing tests that mutate `BUFFUP_HOME` /
/// `BUFFUP_GITHUB_API` env vars. Held for the duration of each test
/// body so concurrent tests do not race.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

struct BuffupEnvGuard {
    home_key: String,
    home_prev: Option<std::ffi::OsString>,
    api_key: String,
    api_prev: Option<std::ffi::OsString>,
    _home_temp: tempfile::TempDir,
    _server: MockServer,
}

impl BuffupEnvGuard {
    fn new() -> Self {
        let home_temp = tempfile::TempDir::new().expect("tempdir");
        let server = MockServer::start();

        let home_key = paths::BUFFUP_HOME_ENV.to_string();
        let home_prev = std::env::var_os(&home_key);
        std::env::set_var(&home_key, home_temp.path());

        let api_key = buffup::GITHUB_API_BASE_ENV.to_string();
        let api_prev = std::env::var_os(&api_key);
        std::env::set_var(&api_key, server.base_url());

        Self {
            home_key,
            home_prev,
            api_key,
            api_prev,
            _home_temp: home_temp,
            _server: server,
        }
    }

    fn server(&self) -> &MockServer {
        &self._server
    }
}

impl Drop for BuffupEnvGuard {
    fn drop(&mut self) {
        match &self.home_prev {
            Some(v) => std::env::set_var(&self.home_key, v),
            None => std::env::remove_var(&self.home_key),
        }
        match &self.api_prev {
            Some(v) => std::env::set_var(&self.api_key, v),
            None => std::env::remove_var(&self.api_key),
        }
    }
}

fn build_gzip_tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, body) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, *name, std::io::Cursor::new(*body))
            .expect("append");
    }
    builder.finish().expect("finish");
    let raw = builder.into_inner().expect("inner");

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&raw).expect("write");
    encoder.finish().expect("finish")
}

#[tokio::test]
async fn install_downloads_and_extracts() {
    let _lock = env_lock();
    let env = BuffupEnvGuard::new();
    let server = env.server();

    let bin_name = paths::binary_name();
    let bin_body: &[u8] = if cfg!(windows) {
        // Minimal "this is a binary" stub — we don't actually exec it.
        b"MZ\x90\x00fake-pe-stub"
    } else {
        b"#!/bin/sh\necho fake-buff\n"
    };

    let tarball = build_gzip_tarball(&[(bin_name, bin_body)]);
    let tarball_path_on_server = "/tarball/v1.0.0";

    let tarball_url = format!("{}{}", server.base_url(), tarball_path_on_server);
    let release_json = serde_json::json!({
        "tag_name": "v1.0.0",
        "tarball_url": tarball_url,
        "zipball_url": format!("{}/zipball/v1.0.0", server.base_url()),
        "assets": [],
    });

    server.mock(|when, then| {
        when.method(Method::GET).path("/releases/tags/v1.0.0");
        then.status(200)
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&release_json).expect("json"));
    });

    server.mock(|when, then| {
        when.method(Method::GET).path(tarball_path_on_server);
        then.status(200)
            .header("content-type", "application/gzip")
            .body(tarball.clone());
    });

    install::run("1.0.0".to_string(), false)
        .await
        .expect("install");

    // Verify the binary landed in the version dir.
    let version_dir = paths::versions_dir().expect("versions_dir").join("1.0.0");
    assert!(version_dir.is_dir(), "version dir should exist");
    let binary_path = version_dir.join(bin_name);
    assert!(
        binary_path.is_file(),
        "binary should exist at {}",
        binary_path.display()
    );
    let body = fs::read(&binary_path).expect("read body");
    assert_eq!(body, bin_body);
}

#[tokio::test]
async fn install_fails_gracefully_on_404() {
    let _lock = env_lock();
    let env = BuffupEnvGuard::new();
    let server = env.server();

    server.mock(|when, then| {
        when.method(Method::GET).path("/releases/tags/v9.9.9");
        then.status(404).body("{\"message\": \"Not Found\"}");
    });

    let err = install::run("9.9.9".to_string(), false)
        .await
        .expect_err("should fail");
    // Could be HttpStatus(404) or another wrapping variant — the key
    // contract is that the error surfaces (and the command prints a
    // helpful "Releases don't exist yet" message to stderr in
    // addition).
    let msg = format!("{err}");
    assert!(
        msg.contains("404") || msg.contains("HTTP"),
        "expected HTTP/404-related error, got: {msg}"
    );

    // No version dir should have been created.
    let versions_dir = paths::versions_dir().expect("versions_dir");
    assert!(
        !versions_dir.join("9.9.9").exists(),
        "no install dir should be left behind on 404"
    );
}

#[tokio::test]
async fn install_rejects_already_installed() {
    let _lock = env_lock();
    let _env = BuffupEnvGuard::new();
    // Pre-populate the version dir.
    let v_dir = paths::versions_dir().expect("versions_dir").join("1.0.0");
    fs::create_dir_all(&v_dir).expect("mkdir");
    // Put a stub binary inside so the dir is "real".
    let mut f = fs::File::create(v_dir.join(paths::binary_name())).expect("create");
    f.write_all(b"stub\n").expect("write");

    let err = install::run("1.0.0".to_string(), false)
        .await
        .expect_err("should fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("already installed"),
        "expected already-installed error, got: {msg}"
    );
}

#[tokio::test]
async fn install_rejects_invalid_version() {
    let _lock = env_lock();
    let _env = BuffupEnvGuard::new();
    let err = install::run("not-a-version".to_string(), false)
        .await
        .expect_err("should fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("invalid version") || msg.contains("Parse") || msg.contains("Unexpected"),
        "expected version-parse error, got: {msg}"
    );
}

/// Sanity-check that the path module respects `BUFFUP_HOME` — this
/// guards against future regressions where the env var name changes
/// but the test helpers above aren't updated.
#[test]
fn buffup_home_env_is_respected() {
    let _lock = env_lock();
    let env = BuffupEnvGuard::new();
    let home = paths::buff_home().expect("buff_home");
    assert_eq!(home, env._home_temp.path());
    let _ = Path::new(""); // mark Path import used
}
