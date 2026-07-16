//! `buff new <NAME>` — scaffold a new Buff project in a fresh `<NAME>/`
//! directory.
//!
//! Layout produced:
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
//! Refuses to clobber an existing directory. All templates + validation live
//! in [`crate::scaffold`].

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::scaffold;

/// Entry point for `buff new <NAME>`.
///
/// Validates the name, then writes the four scaffold files into a new
/// `<NAME>/` subdirectory of the current working directory.
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

    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)
        .with_context(|| format!("failed to create project directory `{name}`"))?;

    fs::write(
        project_dir.join("buff.toml"),
        scaffold::render_template(scaffold::BUFF_TOML_TEMPLATE, name),
    )
    .with_context(|| format!("failed to write `{}/buff.toml`", name))?;

    fs::write(
        src_dir.join("main.buff"),
        scaffold::render_template(scaffold::MAIN_BUFF_TEMPLATE, name),
    )
    .with_context(|| format!("failed to write `{}/src/main.buff`", name))?;

    fs::write(project_dir.join(".gitignore"), scaffold::GITIGNORE_TEMPLATE)
        .with_context(|| format!("failed to write `{}/.gitignore`", name))?;

    fs::write(
        project_dir.join("README.md"),
        scaffold::render_template(scaffold::README_TEMPLATE, name),
    )
    .with_context(|| format!("failed to write `{}/README.md`", name))?;

    eprintln!("Created Buff project `{name}` in ./{name}/");
    eprintln!("Run with: buff run {name}/src/main.buff");
    Ok(())
}
