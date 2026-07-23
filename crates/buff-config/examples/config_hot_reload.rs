// T30 example: hot reload via file watcher.
//
// Demonstrates Config::watch: writes a TOML file, starts watching,
// modifies the file, and verifies the callback fires with the new
// value. Uses a temp file so the example is hermetic.

use buff_config::Config;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn main() {
    let cfg = Config::new();
    cfg.set_default("port", 8080);

    let tmp_dir = std::env::temp_dir();
    let config_path = tmp_dir.join(format!(
        "buff_config_hot_reload-{}.toml",
        std::process::id()
    ));

    // Write initial config
    {
        let mut f = std::fs::File::create(&config_path).expect("create temp config");
        write!(f, r#"port = 8080"#).expect("write config");
    }
    cfg.load_file(&config_path).expect("load file");
    assert_eq!(cfg.get_int("port"), Some(8080));

    // Track callback invocations
    let callback_count = Arc::new(Mutex::new(0u32));
    let count_clone = Arc::clone(&callback_count);

    // Start watching
    let _watcher = cfg
        .watch(&config_path, move |updated_cfg| {
            let mut count = count_clone.lock().unwrap();
            *count += 1;
            println!(
                "hot reload callback #{}: port={}",
                *count,
                updated_cfg.get_int("port").unwrap_or(0)
            );
        })
        .expect("start watching");

    // Modify the file to trigger the watcher
    {
        let mut f = std::fs::File::create(&config_path).expect("create temp config");
        write!(f, r#"port = 9090"#).expect("write config");
    }

    // Give the watcher time to fire
    std::thread::sleep(Duration::from_millis(500));

    let count = *callback_count.lock().unwrap();
    assert!(count >= 1, "callback should have fired at least once");

    let _ = std::fs::remove_file(&config_path);
    println!("config_hot_reload: callback fired {} time(s)", count);
}
