//! `deox init` — scaffold a Deox project in the *current* directory.
//!
//! Same files as [`super::new`], but written into `.` instead of `<NAME>/`.
//! The project name is derived from the current directory's name. Refuses to
//! run if `deox.toml` already exists (avoids wiping an existing project).

use std::fs;

use anyhow::{bail, Context, Result};

use crate::scaffold;

/// Entry point for `deox init`.
///
/// Derives the project name from the current directory, validates it, refuses
/// to overwrite an existing `deox.toml`, then writes the scaffold files.
///
/// # Errors
///
/// - The current directory name is not a valid Deox identifier (clear message
///   from [`scaffold::validate_project_name`]).
/// - `deox.toml` already exists in the current directory.
/// - Filesystem errors are wrapped with the offending path.
pub fn run() -> Result<()> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    let dir_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("cannot derive project name from current directory"))?;

    // Validate the derived name up-front so users get a clear error rather
    // than a half-scaffolded project when the directory is e.g. `my app`.
    scaffold::validate_project_name(dir_name).map_err(anyhow::Error::msg)?;

    let manifest_path = cwd.join("deox.toml");
    if manifest_path.exists() {
        bail!("`deox.toml` already exists in the current directory — refusing to overwrite");
    }

    let src_dir = cwd.join("src");
    fs::create_dir_all(&src_dir).context("failed to create `src/` directory")?;

    fs::write(
        &manifest_path,
        scaffold::render_template(scaffold::DEOX_TOML_TEMPLATE, dir_name),
    )
    .with_context(|| format!("failed to write `{}`", manifest_path.display()))?;

    fs::write(
        src_dir.join("main.deox"),
        scaffold::render_template(scaffold::MAIN_DEOX_TEMPLATE, dir_name),
    )
    .with_context(|| format!("failed to write `{}`", src_dir.join("main.deox").display()))?;

    // Only write .gitignore / README if they don't already exist — `init` is
    // idempotent-friendly in a way `new` doesn't need to be (the user may have
    // pre-existing docs/VCS config in the cwd).
    let gitignore_path = cwd.join(".gitignore");
    if !gitignore_path.exists() {
        fs::write(&gitignore_path, scaffold::GITIGNORE_TEMPLATE)
            .with_context(|| format!("failed to write `{}`", gitignore_path.display()))?;
    }

    let readme_path = cwd.join("README.md");
    if !readme_path.exists() {
        fs::write(
            &readme_path,
            scaffold::render_template(scaffold::README_TEMPLATE, dir_name),
        )
        .with_context(|| format!("failed to write `{}`", readme_path.display()))?;
    }

    eprintln!("Initialized Deox project `{dir_name}` in {}", cwd.display());
    eprintln!("Run with: deox run src/main.deox");
    Ok(())
}
