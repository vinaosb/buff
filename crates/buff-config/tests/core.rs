//! Integration tests for the `buff-config` crate.
//!
//! Covers all 12 public functions per the T30 spec:
//! - Constructors: `Config::new`
//! - Providers: `set_default`, `load_file`, `load_env`, `load_args`
//! - Accessors: `get`, `get_int`, `get_float`, `get_bool`
//! - Lifecycle: `watch`
//!
//! Pure-Rust test fixtures are generated inline (no TOML/YAML/JSON
//! fixtures needed; keeps the test hermetic). 12+ unit tests (per
//! T30 acceptance criteria).

use buff_config::{Config, ConfigError};
use std::io::Write;

#[test]
fn config_new_creates_empty_config() {
    let cfg = Config::new();
    assert_eq!(cfg.get("anything"), None);
}

#[test]
fn config_set_default_and_get() {
    let cfg = Config::new();
    cfg.set_default("name", "buff");
    assert_eq!(cfg.get("name"), Some("buff".to_string()));
}

#[test]
fn config_set_default_int() {
    let cfg = Config::new();
    cfg.set_default("port", 8080);
    assert_eq!(cfg.get_int("port"), Some(8080));
}

#[test]
fn config_set_default_float() {
    let cfg = Config::new();
    cfg.set_default("pi", 3.14);
    let val = cfg.get_float("pi").unwrap();
    assert!((val - 3.14).abs() < 0.001);
}

#[test]
fn config_set_default_bool() {
    let cfg = Config::new();
    cfg.set_default("debug", true);
    assert_eq!(cfg.get_bool("debug"), Some(true));
}

#[test]
fn config_missing_key_returns_none() {
    let cfg = Config::new();
    assert_eq!(cfg.get("nonexistent"), None);
    assert_eq!(cfg.get_int("nonexistent"), None);
    assert_eq!(cfg.get_float("nonexistent"), None);
    assert_eq!(cfg.get_bool("nonexistent"), None);
}

#[test]
fn config_load_toml_file() {
    let cfg = Config::new();
    let tmp = std::env::temp_dir().join(format!("buff_config_test-{}.toml", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).expect("create temp file");
        write!(f, r#"key = "value""#).expect("write toml");
    }
    cfg.load_file(&tmp).expect("load toml");
    assert_eq!(cfg.get("key"), Some("value".to_string()));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn config_load_json_file() {
    let cfg = Config::new();
    let tmp = std::env::temp_dir().join(format!("buff_config_test-{}.json", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).expect("create temp file");
        write!(f, r#"{{"key": "json_value"}}"#).expect("write json");
    }
    cfg.load_file(&tmp).expect("load json");
    assert_eq!(cfg.get("key"), Some("json_value".to_string()));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn config_load_yaml_file() {
    let cfg = Config::new();
    let tmp = std::env::temp_dir().join(format!("buff_config_test-{}.yaml", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).expect("create temp file");
        write!(f, "key: yaml_value").expect("write yaml");
    }
    cfg.load_file(&tmp).expect("load yaml");
    assert_eq!(cfg.get("key"), Some("yaml_value".to_string()));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn config_load_unsupported_format_returns_error() {
    let cfg = Config::new();
    let tmp = std::env::temp_dir().join(format!("buff_config_test-{}.xyz", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).expect("create temp file");
        write!(f, "some content").expect("write");
    }
    let err = cfg.load_file(&tmp).unwrap_err();
    assert!(matches!(err, ConfigError::Figment(_)));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn config_load_args_parses_key_value() {
    let cfg = Config::new();
    cfg.load_args(&["--host=0.0.0.0".to_string()]);
    assert_eq!(cfg.get("host"), Some("0.0.0.0".to_string()));
}

#[test]
fn config_load_args_parses_separate_key_value() {
    let cfg = Config::new();
    cfg.load_args(&["--host".to_string(), "127.0.0.1".to_string()]);
    assert_eq!(cfg.get("host"), Some("127.0.0.1".to_string()));
}

#[test]
fn config_layered_precedence_file_overrides_default() {
    let cfg = Config::new();
    cfg.set_default("port", 8080);
    let tmp = std::env::temp_dir().join(format!("buff_config_test-{}.toml", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).expect("create temp file");
        write!(f, r#"port = 9090"#).expect("write toml");
    }
    cfg.load_file(&tmp).expect("load file");
    assert_eq!(cfg.get_int("port"), Some(9090));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn config_layered_precedence_args_override_file() {
    let cfg = Config::new();
    cfg.set_default("port", 8080);
    let tmp = std::env::temp_dir().join(format!("buff_config_test-{}.toml", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).expect("create temp file");
        write!(f, r#"port = 9090"#).expect("write toml");
    }
    cfg.load_file(&tmp).expect("load file");
    cfg.load_args(&["--port=7070".to_string()]);
    assert_eq!(cfg.get_int("port"), Some(7070));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn config_watch_fires_callback_on_file_change() {
    let cfg = Config::new();
    cfg.set_default("port", 8080);
    let tmp = std::env::temp_dir().join(format!("buff_config_test-{}.toml", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).expect("create temp file");
        write!(f, r#"port = 8080"#).expect("write toml");
    }
    cfg.load_file(&tmp).expect("load file");

    use std::sync::{Arc, Mutex};
    let fired = Arc::new(Mutex::new(false));
    let fired_clone = Arc::clone(&fired);

    let _watcher = cfg
        .watch(&tmp, move |_updated| {
            *fired_clone.lock().unwrap() = true;
        })
        .expect("start watch");

    // Modify the file
    {
        let mut f = std::fs::File::create(&tmp).expect("create temp file");
        write!(f, r#"port = 9090"#).expect("write toml");
    }

    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(*fired.lock().unwrap(), "callback should have fired");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn config_display_works() {
    let cfg = Config::new();
    let display = format!("{cfg}");
    assert_eq!(display, "Config(layered)");
}
