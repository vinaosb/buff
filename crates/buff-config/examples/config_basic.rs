// T30 example: basic config with defaults + file + env + args.
//
// Demonstrates the layered config pipeline: set defaults, load a
// TOML file, load env vars, load CLI args, then read values back.
// Uses a temp file so the example is hermetic (no fixture needed).

use buff_config::Config;
use std::io::Write;

fn main() {
    let cfg = Config::new();

    // Layer 1: defaults (lowest precedence)
    cfg.set_default("port", 8080);
    cfg.set_default("host", "localhost");
    cfg.set_default("debug", false);

    // Layer 2: file (TOML)
    let tmp_dir = std::env::temp_dir();
    let config_path = tmp_dir.join(format!("buff_config_basic-{}.toml", std::process::id()));
    {
        let mut f = std::fs::File::create(&config_path).expect("create temp config");
        write!(f, r#"port = 9090"#).expect("write config");
    }
    cfg.load_file(&config_path).expect("load file");
    let _ = std::fs::remove_file(&config_path);

    // Layer 3: env vars (simulated via load_args for hermetic test)
    cfg.load_env("BUFF");

    // Layer 4: CLI args (highest precedence)
    cfg.load_args(&["--host=0.0.0.0".to_string()]);

    // Verify layered precedence
    // port=9090 from file overrides default 8080
    assert_eq!(cfg.get_int("port"), Some(9090));
    // host=0.0.0.0 from CLI args overrides default localhost
    assert_eq!(cfg.get("host"), Some("0.0.0.0".to_string()));
    // debug=false from default (not overridden)
    assert_eq!(cfg.get_bool("debug"), Some(false));
    // missing key returns None
    assert_eq!(cfg.get("missing_key"), None);

    println!("config_basic: all assertions passed");
}
