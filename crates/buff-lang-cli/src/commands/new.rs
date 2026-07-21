//! `buff new <NAME>` — scaffold a new Buff project in a fresh `<NAME>/`
//! directory.
//!
//! Default layout (Binary template, no flag — v0.1 behavior):
//!
//! ```text
//! <NAME>/
//! ├── buff.toml
//! ├── .gitignore
//! ├── README.md
//! └── src/
//!     └── main.buff
//! ```
//!
//! Template variants (T112):
//!
//! - `--lib`      → `src/lib.buff` (library; no `main`).
//! - `--server`   → `src/main.buff` with an async-server starter.
//! - `--gpu`      → `src/main.buff` with a `@prefer(gpu)` GPU starter.
//! - `--workspace`→ `crates/{core,utils}/...` multi-crate layout.
//!
//! Refuses to clobber an existing directory. All templates + validation live
//! in [`crate::scaffold`].

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::scaffold::{self, TemplateKind};

/// Entry point for `buff new <NAME> [--lib | --server | --gpu | --workspace]`.
///
/// Validates the name, computes the file set for `template` via
/// [`scaffold::files_for_template`], then writes each file into a new
/// `<NAME>/` subdirectory of the current working directory. Parent
/// directories (e.g. `src/`, `crates/core/src/`) are created on demand.
///
/// # Errors
///
/// - [`scaffold::validate_project_name`] failure (clear message).
/// - The target directory already exists (refuse to overwrite).
/// - Filesystem errors are wrapped with the offending path.
pub fn run(name: &str, template: TemplateKind) -> Result<()> {
    scaffold::validate_project_name(name).map_err(anyhow::Error::msg)?;

    let project_dir = PathBuf::from(name);
    if project_dir.exists() {
        bail!("directory `{name}` already exists");
    }

    for (rel_path, content) in scaffold::files_for_template(template, name) {
        let full_path = project_dir.join(rel_path);
        // Create any missing parent directories (src/, crates/core/src/, ...).
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory `{}`", parent.display()))?;
        }
        fs::write(&full_path, content)
            .with_context(|| format!("failed to write `{}`", full_path.display()))?;
    }

    eprintln!(
        "Created Buff project `{name}` ({}) in ./{name}/",
        template.as_kebab()
    );
    match template {
        TemplateKind::Binary | TemplateKind::Server | TemplateKind::Gpu => {
            eprintln!("Run with: buff run {name}/src/main.buff");
        }
        TemplateKind::Lib => {
            eprintln!("Library at: {name}/src/lib.buff");
        }
        TemplateKind::Workspace => {
            eprintln!("Workspace members: crates/core, crates/utils");
        }
        TemplateKind::Web | TemplateKind::Ml | TemplateKind::Game | TemplateKind::Pipeline => {
            // T0-C1: v2 templates — runtime arrives with the matching
            // framework wave (v1.14-v1.17). Today the scaffolded file is
            // a starter; `buff check` accepts the imports today, `buff run`
            // resolves once the framework crate lands.
            eprintln!("Run with: buff run {name}/src/main.buff (requires matching framework crate)");
            eprintln!("Tests at: {name}/tests/test_hello.buff");
            eprintln!("Examples at: {name}/examples/hello.buff");
        }
    }
    Ok(())
}
