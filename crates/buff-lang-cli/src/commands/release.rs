//! `buff release <level>` — release helper (T0-I3 scaffold).
//!
//! v1.13 ships the command scaffold; full publish integration arrives
//! with v1.14+. The scaffold:
//!
//! 1. Verifies the working tree is clean (`git diff --exit-code`).
//! 2. Reads current version from `buff.toml [package].version`.
//! 3. Bumps the version by `<level>` (`patch` | `minor` | `major`).
//! 4. Updates `buff.toml` in place with the new version.
//! 5. Updates `CHANGELOG.md` by prepending a section for the new
//!    version (creates the file if absent).
//! 6. Stages `buff.toml` + `CHANGELOG.md` and creates a git tag
//!    `v<X.Y.Z>`.
//!
//! **Does NOT invoke `buff publish` in v1.13** — that requires registry
//! integration (T126-T127) which arrives with v1.14. The scaffold
//! stops at the git tag; the user runs `buff publish` manually for now.
//!
//! # SemVer
//!
//! Versions follow SemVer 2.0 (`MAJOR.MINOR.PATCH`). Pre-release and
//! build metadata suffixes are preserved across bumps (e.g.
//! `1.2.3-alpha` → patch → `1.2.4-alpha`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::config::BuffConfig;

/// The version-bump level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpLevel {
    Patch,
    Minor,
    Major,
}

impl BumpLevel {
    /// Parse the level from the CLI argument string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "patch" => Some(Self::Patch),
            "minor" => Some(Self::Minor),
            "major" => Some(Self::Major),
            _ => None,
        }
    }
}

/// Entry point for `buff release <level>`.
pub fn run(level: BumpLevel, project_dir: &Path) -> Result<()> {
    // 1. Working tree clean check.
    let git_status = Command::new("git")
        .args(["diff", "--exit-code"])
        .current_dir(project_dir)
        .output()
        .context("failed to invoke `git diff --exit-code`")?;
    if !git_status.status.success() {
        bail!(
            "working tree is not clean — commit or stash your changes \
             before running `buff release`"
        );
    }

    // 2. Read current manifest.
    let manifest_path = project_dir.join("buff.toml");
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let cfg = BuffConfig::parse(&manifest_text)?;
    let pkg = cfg
        .package
        .as_ref()
        .context("buff.toml has no [package] section — nothing to release")?;

    // 3. Bump version.
    let new_version = bump_version(&pkg.version, level)
        .with_context(|| format!("failed to bump version `{}`", pkg.version))?;
    eprintln!("Bumping version: {} -> {new_version}", pkg.version);

    // 4. Rewrite buff.toml with the new version (line-by-line replace
    //    to preserve comments + formatting the user might have).
    let updated_manifest = manifest_text.replacen(
        &format!("version = \"{}\"", pkg.version),
        &format!("version = \"{new_version}\""),
        1,
    );
    if updated_manifest == manifest_text {
        bail!(
            "could not locate `version = \"{}\"` line in buff.toml \
             (was the manifest hand-edited?)",
            pkg.version
        );
    }
    fs::write(&manifest_path, &updated_manifest)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;

    // 5. Update CHANGELOG.md.
    let changelog_path = project_dir.join("CHANGELOG.md");
    let prev_changelog = fs::read_to_string(&changelog_path).unwrap_or_default();
    let new_section = format!("## v{new_version}\n\n- (Add your release notes here.)\n\n");
    let updated_changelog = format!("# Changelog\n\n{new_section}{prev_changelog}");
    fs::write(&changelog_path, &updated_changelog)
        .with_context(|| format!("failed to write {}", changelog_path.display()))?;

    // 6. Stage + git tag. The user reviews + pushes manually.
    Command::new("git")
        .args(["add", "buff.toml", "CHANGELOG.md"])
        .current_dir(project_dir)
        .status()
        .context("failed to `git add` the manifest + changelog")?;
    Command::new("git")
        .args(["tag", &format!("v{new_version}")])
        .current_dir(project_dir)
        .status()
        .context("failed to create git tag")?;

    eprintln!("Created tag v{new_version}.");
    eprintln!(
        "Next steps:\n  \
         1. Edit CHANGELOG.md with the actual release notes.\n  \
         2. `git commit -m \"chore(release): v{new_version}\"`\n  \
         3. `git push && git push --tags`\n  \
         4. `buff publish` (when registry integration lands in v1.14)"
    );
    Ok(())
}

/// Bump a SemVer 2.0 version string by `level`. Returns the new
/// version string (preserves any `-prerelease` or `+build` suffix).
///
/// Errors on malformed input. The parser is intentionally narrow:
/// matches `MAJOR.MINOR.PATCH` with optional `-pre` and `+build`. It
/// does NOT accept ranges (`^1.0`) — bumping is only meaningful on
/// concrete versions.
pub fn bump_version(current: &str, level: BumpLevel) -> Result<String, String> {
    let (core, suffix) = current.split_once('-').unwrap_or((current, ""));
    let (numeric, build) = core.split_once('+').unwrap_or((core, ""));
    let parts: Vec<&str> = numeric.split('.').collect();
    if parts.len() != 3 {
        return Err(format!(
            "version `{current}` is not SemVer (expected MAJOR.MINOR.PATCH)"
        ));
    }
    let mut nums = [0u64; 3];
    for (i, p) in parts.iter().enumerate() {
        nums[i] = p.parse::<u64>().map_err(|_| {
            format!("version segment `{p}` in `{current}` is not a non-negative integer")
        })?;
    }
    match level {
        BumpLevel::Patch => nums[2] += 1,
        BumpLevel::Minor => {
            nums[1] += 1;
            nums[2] = 0;
        }
        BumpLevel::Major => {
            nums[0] += 1;
            nums[1] = 0;
            nums[2] = 0;
        }
    }
    let mut new = format!("{}.{}.{}", nums[0], nums[1], nums[2]);
    if let Some(build) = core.strip_prefix(numeric) {
        // Re-attach +build metadata if it was there.
        if !build.is_empty() {
            new.push_str(build);
        }
    } else if !build.is_empty() {
        new.push('+');
        new.push_str(build);
    }
    if !suffix.is_empty() {
        new.push('-');
        new.push_str(suffix);
    }
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_level_parses_three_levels() {
        assert_eq!(BumpLevel::from_str("patch"), Some(BumpLevel::Patch));
        assert_eq!(BumpLevel::from_str("minor"), Some(BumpLevel::Minor));
        assert_eq!(BumpLevel::from_str("major"), Some(BumpLevel::Major));
        assert_eq!(BumpLevel::from_str("nonsense"), None);
    }

    #[test]
    fn bump_version_patch_increments_third() {
        assert_eq!(bump_version("1.2.3", BumpLevel::Patch).unwrap(), "1.2.4");
    }

    #[test]
    fn bump_version_minor_resets_patch() {
        assert_eq!(bump_version("1.2.3", BumpLevel::Minor).unwrap(), "1.3.0");
    }

    #[test]
    fn bump_version_major_resets_minor_and_patch() {
        assert_eq!(bump_version("1.2.3", BumpLevel::Major).unwrap(), "2.0.0");
    }

    #[test]
    fn bump_version_preserves_prerelease_suffix() {
        assert_eq!(
            bump_version("1.2.3-alpha", BumpLevel::Patch).unwrap(),
            "1.2.4-alpha"
        );
    }

    #[test]
    fn bump_version_rejects_non_semver() {
        assert!(bump_version("1.2", BumpLevel::Patch).is_err());
        assert!(bump_version("1.2.3.4", BumpLevel::Patch).is_err());
        assert!(bump_version("not-a-version", BumpLevel::Patch).is_err());
    }

    #[test]
    fn bump_version_handles_zero() {
        assert_eq!(bump_version("0.0.0", BumpLevel::Patch).unwrap(), "0.0.1");
        assert_eq!(bump_version("0.0.0", BumpLevel::Minor).unwrap(), "0.1.0");
        assert_eq!(bump_version("0.0.0", BumpLevel::Major).unwrap(), "1.0.0");
    }
}
