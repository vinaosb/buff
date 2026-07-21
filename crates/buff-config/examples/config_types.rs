// T30 example: config with all value types.
//
// Demonstrates get_int, get_float, get_bool, and get with various
// value types from a JSON config file.

use buff_config::Config;
use std::io::Write;

fn main() {
    let cfg = Config::new();

    let tmp_dir = std::env::temp_dir();
    let config_path = tmp_dir.join(format!("buff_config_types-{}.json", std::process::id()));

    // Write JSON config
    {
        let mut f = std::fs::File::create(&config_path).expect("create temp config");
        write!(
            f,
            r#"{{
    "name": "buff-app",
    "version": 2,
    "pi": 3.14159,
    "enabled": true,
    "tags": ["dev", "test"]
}}"#
        )
        .expect("write config");
    }
    cfg.load_file(&config_path).expect("load file");
    let _ = std::fs::remove_file(&config_path);

    // Read back values
    assert_eq!(cfg.get("name"), Some("buff-app".to_string()));
    assert_eq!(cfg.get_int("version"), Some(2));
    assert!(cfg.get_float("pi").unwrap() - 3.14159 < 0.0001);
    assert_eq!(cfg.get_bool("enabled"), Some(true));

    println!("config_types: all value type assertions passed");
}
