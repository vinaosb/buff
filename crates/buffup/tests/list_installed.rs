//! `buffup list` integration test.
//!
//! Verifies the `list` command discovers installed versions and
//! correctly marks the active one. Runs fully hermetic by pointing
//! `BUFFUP_HOME` at a per-test `tempfile::TempDir`.
//!
//! Tests in this file mutate `BUFFUP_HOME` so the process-wide
//! [`ENV_LOCK`] serializes them (mirrors `tests/install_mock.rs`).

#![cfg(test)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use buffup::commands::list;
use buffup::paths;

/// Process-wide mutex serializing tests that mutate `BUFFUP_HOME`.
/// Held for the duration of each test body so concurrent runs do not
/// race on env mutation.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Helper: hold a BUFFUP_HOME override for the duration of a test.
/// Restores the previous value (if any) on drop so parallel tests
/// don't leak state.
struct BuffupHomeGuard {
    key: String,
    prev: Option<std::ffi::OsString>,
    _temp: tempfile::TempDir,
}

impl BuffupHomeGuard {
    fn new(label: &str) -> Self {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let key = paths::BUFFUP_HOME_ENV.to_string();
        let prev = std::env::var_os(&key);
        std::env::set_var(&key, temp.path());
        let _ = label; // mark used
        Self {
            key,
            prev,
            _temp: temp,
        }
    }
}

impl Drop for BuffupHomeGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var(&self.key, v),
            None => std::env::remove_var(&self.key),
        }
    }
}

fn make_buff_binary(dir: &Path) {
    fs::create_dir_all(dir).expect("mkdir");
    let bin = dir.join(paths::binary_name());
    let mut f = fs::File::create(&bin).expect("create");
    f.write_all(b"#!fake-binary\n").expect("write");
}

#[test]
fn empty_when_no_versions_dir() {
    let _lock = env_lock();
    let _g = BuffupHomeGuard::new("empty");
    let entries = list::collect().expect("collect");
    assert!(entries.is_empty(), "expected empty when no versions/ dir");
}

#[test]
fn lists_versions_in_ascending_order() {
    let _lock = env_lock();
    let _g = BuffupHomeGuard::new("ascending");
    let versions_dir = paths::versions_dir().expect("versions_dir");
    make_buff_binary(&versions_dir.join("1.1.0"));
    make_buff_binary(&versions_dir.join("1.0.0"));
    make_buff_binary(&versions_dir.join("2.0.0"));

    let entries = list::collect().expect("collect");
    let versions: Vec<String> = entries.iter().map(|e| e.version.to_string()).collect();
    assert_eq!(versions, vec!["1.0.0", "1.1.0", "2.0.0"]);
    assert!(entries.iter().all(|e| !e.active), "none active yet");
}

#[test]
fn marks_active_version() {
    let _lock = env_lock();
    let _g = BuffupHomeGuard::new("active");
    let versions_dir = paths::versions_dir().expect("versions_dir");
    let v1 = versions_dir.join("1.0.0");
    let v2 = versions_dir.join("1.1.0");
    make_buff_binary(&v1);
    make_buff_binary(&v2);

    // Set v1 as active via the default command (this exercises the
    // real symlink path too).
    buffup::commands::default_cmd::run("1.0.0".to_string()).expect("default v1");

    let entries = list::collect().expect("collect");
    let active: Vec<_> = entries.iter().filter(|e| e.active).collect();
    assert_eq!(active.len(), 1, "exactly one active version");
    assert_eq!(active[0].version.to_string(), "1.0.0");
}

#[test]
fn skips_non_semver_dirs() {
    let _lock = env_lock();
    let _g = BuffupHomeGuard::new("skip-non-semver");
    let versions_dir = paths::versions_dir().expect("versions_dir");
    make_buff_binary(&versions_dir.join("1.0.0"));
    make_buff_binary(&versions_dir.join("staging"));
    make_buff_binary(&versions_dir.join("old"));

    let entries = list::collect().expect("collect");
    let versions: Vec<String> = entries.iter().map(|e| e.version.to_string()).collect();
    assert_eq!(versions, vec!["1.0.0"]);
}
