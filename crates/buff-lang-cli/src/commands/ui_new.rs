//! `buff ui new --desktop <NAME>` — scaffold a new Tauri 2.0 desktop app
//! project in a fresh `<NAME>/` directory.
//!
//! The scaffolded project contains:
//!
//! ```text
//! <NAME>/
//! ├── buff.toml              # Project manifest with [ui] section
//! ├── .gitignore
//! ├── src/
//! │   └── main.buff          # Buff UI entry point (Dioxus counter)
//! ├── src-tauri/
//! │   ├── Cargo.toml          # Tauri 2.0 Rust project
//! │   ├── tauri.conf.json     # Tauri window + build config
//! │   ├── build.rs            # tauri-build build script
//! │   └── src/
//! │       ├── main.rs         # Tauri entry point
//! │       └── lib.rs          # Tauri app + IPC commands
//! └── static/
//!     └── index.html          # HTML shell that loads the Wasm bundle
//! ```
//!
//! Templates are embedded at compile time via `include_str!` (no TPL engine
//! dep). The `{name}` placeholder is substituted with the project name.
//!
//! Refuses to clobber an existing directory.

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::scaffold;

/// A single scaffolded file: relative path + rendered content.
struct DesktopFile {
    rel_path: &'static str,
    content: String,
}

/// Returns the list of files to write for a desktop Tauri project.
///
/// Each template is embedded via `include_str!` and the `{name}` placeholder
/// is substituted with the project name.
fn desktop_files(name: &str) -> Vec<DesktopFile> {
    let render = |t: &str| t.replace("{name}", name);

    vec![
        DesktopFile {
            rel_path: "buff.toml",
            content: render(include_str!("../../templates/desktop/buff.toml")),
        },
        DesktopFile {
            rel_path: ".gitignore",
            content: render(include_str!("../../templates/desktop/.gitignore")),
        },
        DesktopFile {
            rel_path: "src/main.buff",
            content: render(include_str!("../../templates/desktop/src/main.buff")),
        },
        DesktopFile {
            rel_path: "src-tauri/Cargo.toml",
            content: render(include_str!("../../templates/desktop/src-tauri/Cargo.toml")),
        },
        DesktopFile {
            rel_path: "src-tauri/tauri.conf.json",
            content: render(include_str!(
                "../../templates/desktop/src-tauri/tauri.conf.json"
            )),
        },
        DesktopFile {
            rel_path: "src-tauri/build.rs",
            content: render(include_str!("../../templates/desktop/src-tauri/build.rs")),
        },
        DesktopFile {
            rel_path: "src-tauri/src/main.rs",
            content: render(include_str!(
                "../../templates/desktop/src-tauri/src/main.rs"
            )),
        },
        DesktopFile {
            rel_path: "src-tauri/src/lib.rs",
            content: render(include_str!("../../templates/desktop/src-tauri/src/lib.rs")),
        },
        DesktopFile {
            rel_path: "static/index.html",
            content: render(include_str!("../../templates/desktop/static/index.html")),
        },
    ]
}

/// Entry point for `buff ui new --desktop <NAME>`.
///
/// Validates the project name, checks the target directory does not exist,
/// then writes all template files into a new `<NAME>/` subdirectory of the
/// current working directory. Parent directories are created on demand.
///
/// # Errors
///
/// - [`scaffold::validate_project_name`] failure (clear message).
/// - The target directory already exists (refuse to overwrite).
/// - Filesystem errors are wrapped with the offending path.
pub fn run(name: &str) -> Result<()> {
    scaffold::validate_project_name(name).map_err(anyhow::Error::msg)?;

    let project_dir = PathBuf::from(name);
    if project_dir.exists() {
        bail!("directory `{name}` already exists");
    }

    for file in desktop_files(name) {
        let full_path = project_dir.join(file.rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory `{}`", parent.display()))?;
        }
        fs::write(&full_path, &file.content)
            .with_context(|| format!("failed to write `{}`", full_path.display()))?;
    }

    eprintln!("Created Tauri desktop project `{name}` in ./{name}/");
    eprintln!("  Next steps:");
    eprintln!("    1. cd {name}");
    eprintln!("    2. buff ui build --desktop   (build the Wasm frontend)");
    eprintln!("    3. cargo tauri build         (build the native binary)");
    eprintln!("  Requires Tauri CLI: cargo install tauri-cli");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_files_contains_all_expected_paths() {
        let files = desktop_files("test_app");
        let paths: Vec<&str> = files.iter().map(|f| f.rel_path).collect();

        assert!(paths.contains(&"buff.toml"));
        assert!(paths.contains(&".gitignore"));
        assert!(paths.contains(&"src/main.buff"));
        assert!(paths.contains(&"src-tauri/Cargo.toml"));
        assert!(paths.contains(&"src-tauri/tauri.conf.json"));
        assert!(paths.contains(&"src-tauri/build.rs"));
        assert!(paths.contains(&"src-tauri/src/main.rs"));
        assert!(paths.contains(&"src-tauri/src/lib.rs"));
        assert!(paths.contains(&"static/index.html"));
        assert_eq!(files.len(), 9, "expected exactly 9 scaffolded files");
    }

    #[test]
    fn desktop_files_substitutes_name() {
        let files = desktop_files("my_desktop_app");
        for file in &files {
            assert!(
                !file.content.contains("{name}"),
                "file `{}` still contains unsubstituted `{{name}}`",
                file.rel_path
            );
        }
        // Check specific files contain the substituted name.
        let toml = files
            .iter()
            .find(|f| f.rel_path == "buff.toml")
            .map(|f| &f.content)
            .expect("buff.toml must exist");
        assert!(
            toml.contains("my_desktop_app"),
            "buff.toml should contain the project name"
        );
    }

    #[test]
    fn desktop_files_tauri_conf_has_valid_json() {
        let files = desktop_files("test");
        let conf = files
            .iter()
            .find(|f| f.rel_path == "src-tauri/tauri.conf.json")
            .map(|f| &f.content)
            .expect("tauri.conf.json must exist");
        // Verify it parses as JSON.
        let parsed: serde_json::Value =
            serde_json::from_str(conf).expect("tauri.conf.json must be valid JSON");
        assert_eq!(parsed["productName"], "test");
        assert!(parsed["app"]["windows"].is_array());
    }

    #[test]
    fn desktop_files_main_rs_references_lib() {
        let files = desktop_files("myapp");
        let main_rs = files
            .iter()
            .find(|f| f.rel_path == "src-tauri/src/main.rs")
            .map(|f| &f.content)
            .expect("main.rs must exist");
        assert!(
            main_rs.contains("myapp_desktop_lib::run()"),
            "main.rs should call the lib's run function"
        );
    }

    #[test]
    fn desktop_files_lib_rs_has_run_function() {
        let files = desktop_files("test");
        let lib_rs = files
            .iter()
            .find(|f| f.rel_path == "src-tauri/src/lib.rs")
            .map(|f| &f.content)
            .expect("lib.rs must exist");
        assert!(
            lib_rs.contains("pub fn run()"),
            "lib.rs should export a run() function"
        );
        assert!(
            lib_rs.contains("tauri::Builder::default()"),
            "lib.rs should use tauri::Builder"
        );
    }

    #[test]
    fn desktop_files_index_html_has_title() {
        let files = desktop_files("myapp");
        let html = files
            .iter()
            .find(|f| f.rel_path == "static/index.html")
            .map(|f| &f.content)
            .expect("index.html must exist");
        assert!(
            html.contains("<title>myapp</title>"),
            "index.html should have the project name as title"
        );
        assert!(
            html.contains("bundle.js"),
            "index.html should reference the Wasm bundle"
        );
    }
}
