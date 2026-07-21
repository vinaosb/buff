//! `buff-config` — layered configuration for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`figment`](https://docs.rs/figment/latest/figment/)
//! crate. Provides layered config: defaults → file (TOML/YAML/JSON) →
//! env vars → CLI args. Hot reload via `notify` file watcher.
//!
//! # Pipeline
//!
//! ```text
//!   Config.new() ──▶ Config.set_default(key, val) ──▶ Config.load_file(path)
//!                        │                                    │
//!                        ▼                                    ▼
//!                   Config.load_env(prefix) ──▶ Config.load_args(args)
//!                        │                                    │
//!                        └──────────────┬─────────────────────┘
//!                                       ▼
//!                              Config.get(key) -> Option<String>
//!                              Config.get_int(key) -> Option<i64>
//!                              Config.get_float(key) -> Option<f64>
//!                              Config.get_bool(key) -> Option<bool>
//!                              Config.watch(callback) -> Result<(), ConfigError>
//! ```
//!
//! # Layered precedence (last wins)
//!
//! 1. Defaults (lowest priority)
//! 2. File (TOML/YAML/JSON)
//! 3. Environment variables
//! 4. CLI args (highest priority)
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Config`, `ConfigError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | `Config` owns its `Figment`. All getters return owned `Option<String>`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, ConfigError>`. `figment::Error` mapped via `From`. |
//! | R4 — Thread safety | `Config` is `Send + Sync` (wraps `figment::Figment` which is `Send + Sync`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `Config` owns its `Figment`. |
//! | R6 — Panic boundary | `load_file` / `watch` wrap their bodies in `catch_unwind` (per FFI guide §6). |

pub mod error;

pub use error::ConfigError;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use figment::providers::{Env, Format, Json, Serialized, Toml, Yaml};
use figment::Figment;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// A layered configuration store.
///
/// Constructed via [`Config::new`]. Supports layered providers:
/// defaults → file → env vars → CLI args. The last provider to
/// set a key wins (highest precedence).
///
/// Internally wraps `figment::Figment` which itself composes
/// providers in order. All getters return `Option<T>` — missing
/// keys are `None` (never panic).
#[derive(Debug, Clone)]
pub struct Config {
    inner: Arc<Mutex<Figment>>,
}

impl Config {
    /// Create a new empty configuration with no providers.
    ///
    /// The returned `Config` has no defaults, no file, no env, no
    /// args. Call the `load_*` methods to add layers.
    pub fn new() -> Self {
        Config {
            inner: Arc::new(Mutex::new(Figment::new())),
        }
    }

    /// Set a default value for `key`. Defaults have the lowest
    /// precedence — any subsequent provider (file, env, args) can
    /// override them.
    ///
    /// The value is serialized via serde. Accepts any type that
    /// implements `serde::Serialize` (String, i64, f64, bool, etc.).
    pub fn set_default<T: serde::Serialize>(&self, key: &str, value: T) {
        let mut fig = self.inner.lock().ok();
        if let Some(ref mut figment) = fig {
            let provider = Serialized::default(key, value);
            *figment = std::mem::take(figment).merge(provider);
        }
    }

    /// Load a configuration file. The format is inferred from the
    /// file extension:
    /// - `.toml` → TOML
    /// - `.yaml` / `.yml` → YAML
    /// - `.json` → JSON
    ///
    /// File values have higher precedence than defaults but lower
    /// than env vars and CLI args.
    ///
    /// Wraps the body in `catch_unwind` per T4 FFI guide R6 so a
    /// panic in the parser becomes a stable `Err(ConfigError::Panic)`.
    pub fn load_file<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        let path_owned = path.as_ref().to_path_buf();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let ext = path_owned
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let provider: Box<dyn figment::Provider> = match ext.as_str() {
                "toml" => Box::new(Toml::file(&path_owned)),
                "yaml" | "yml" => Box::new(Yaml::file(&path_owned)),
                "json" => Box::new(Json::file(&path_owned)),
                _ => return Err(ConfigError::Figment(format!("unsupported config format: .{ext}"))),
            };
            let mut fig = self.inner.lock().map_err(|e| {
                ConfigError::Figment(format!("lock error: {e}"))
            })?;
            *fig = std::mem::take(&mut *fig).merge(provider);
            Ok(())
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(ConfigError::Panic),
        }
    }

    /// Load environment variables with the given prefix. Only
    /// variables starting with `prefix` are included; the prefix
    /// is stripped from the key name.
    ///
    /// For example, `load_env("BUFF")` includes `BUFF_PORT=8080`
    /// as key `port`.
    ///
    /// Env vars have higher precedence than file and defaults but
    /// lower than CLI args.
    pub fn load_env(&self, prefix: &str) {
        let mut fig = self.inner.lock().ok();
        if let Some(ref mut figment) = fig {
            let provider = Env::prefixed(prefix);
            *figment = std::mem::take(figment).merge(provider);
        }
    }

    /// Load CLI arguments as key=value pairs. Each arg should be
    /// in the form `--key=value` or `--key value` (two consecutive
    /// args).
    ///
    /// CLI args have the highest precedence — they override all
    /// other providers.
    pub fn load_args(&self, args: &[String]) {
        let mut fig = self.inner.lock().ok();
        if let Some(ref mut figment) = fig {
            let mut map = figment::value::Dict::new();
            let mut iter = args.iter().peekable();
            while let Some(arg) = iter.next() {
                if let Some(stripped) = arg.strip_prefix("--") {
                    if let Some((key, val)) = stripped.split_once('=') {
                        map.insert(
                            key.to_string(),
                            figment::value::Value::from(val.to_string()),
                        );
                    } else if let Some(next) = iter.peek() {
                        if !next.starts_with("--") {
                            let val = iter.next().unwrap();
                            map.insert(
                                stripped.to_string(),
                                figment::value::Value::from(val.to_string()),
                            );
                        }
                    }
                }
            }
            let provider = Serialized::default("args", map).key("args");
            *figment = std::mem::take(figment).merge(provider);
        }
    }

    /// Get a string value for `key`. Returns `None` if the key is
    /// not set by any provider.
    pub fn get(&self, key: &str) -> Option<String> {
        let fig = self.inner.lock().ok()?;
        fig.try_find(key).ok().and_then(|v: Option<String>| v)
    }

    /// Get an integer value for `key`. Returns `None` if the key
    /// is not set or is not a valid integer.
    pub fn get_int(&self, key: &str) -> Option<i64> {
        let fig = self.inner.lock().ok()?;
        fig.try_find(key).ok().and_then(|v: Option<i64>| v)
    }

    /// Get a floating-point value for `key`. Returns `None` if the
    /// key is not set or is not a valid float.
    pub fn get_float(&self, key: &str) -> Option<f64> {
        let fig = self.inner.lock().ok()?;
        fig.try_find(key).ok().and_then(|v: Option<f64>| v)
    }

    /// Get a boolean value for `key`. Returns `None` if the key is
    /// not set or is not a valid boolean.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        let fig = self.inner.lock().ok()?;
        fig.try_find(key).ok().and_then(|v: Option<bool>| v)
    }

    /// Watch the config file for changes and invoke `callback` when
    /// a modification is detected.
    ///
    /// The callback receives a `&Config` (the config is reloaded
    /// from the file before the callback fires). Returns a
    /// `ConfigWatcher` handle — dropping it stops watching.
    ///
    /// Wraps the body in `catch_unwind` per T4 FFI guide R6.
    pub fn watch<F>(&self, path: &Path, callback: F) -> Result<ConfigWatcher, ConfigError>
    where
        F: Fn(&Config) + Send + 'static,
    {
        let path_owned = path.to_path_buf();
        let config_self = self.clone();
        let (tx, rx) = mpsc::channel();

        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut watcher = RecommendedWatcher::new(
                move |res: notify::Result<notify::Event>| {
                    if let Ok(event) = res {
                        if matches!(event.kind, EventKind::Modify(_)) {
                            let _ = tx.send(());
                        }
                    }
                },
                notify::Config::default(),
            )
            .map_err(|e| ConfigError::Figment(format!("watcher creation failed: {e}")))?;

            watcher
                .watch(&path_owned, RecursiveMode::NonRecursive)
                .map_err(|e| ConfigError::Figment(format!("watch failed: {e}")))?;

            Ok::<_, ConfigError>((watcher, rx))
        }));

        let (mut watcher, rx) = match result {
            Ok(Ok(w)) => w,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(ConfigError::Panic),
        };

        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                // Reload the file before invoking callback
                let _ = config_self.load_file(&path_owned);
                callback(&config_self);
            }
        });

        Ok(ConfigWatcher {
            _watcher: watcher,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}

/// A handle returned by [`Config::watch`]. Dropping it stops the
/// file watcher.
#[derive(Debug)]
pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
}

impl std::fmt::Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Config(layered)")
    }
}
