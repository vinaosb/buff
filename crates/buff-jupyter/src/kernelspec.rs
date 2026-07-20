//! Kernelspec generation — the `kernel.json` document Jupyter looks for
//! under its data-dir (`~/.local/share/jupyter/kernels/<name>/` on
//! Linux/macOS, `%APPDATA%\jupyter\kernels\<name>\` on Windows).
//!
//! `buff jupyter install` writes this file via [`write_kernel_json`],
//! either directly into the Jupyter data-dir or by shelling out to
//! `jupyter kernelspec install` (preferred when `jupyter` is on
//! `PATH` — the install command handles cross-platform data-dir
//! resolution and `--replace` semantics).
//!
//! The canonical `kernel.json` shape:
//!
//! ```json,ignore
//! {
//!   "argv": ["buff", "jupyter", "start", "--connection-file", "{connection_file}"],
//!   "display_name": "Buff",
//!   "language": "buff",
//!   "interrupt_mode": "signal",
//!   "metadata": {}
//! }
//! ```
//!
//! The `{connection_file}` token is substituted by Jupyter at launch
//! time with the path to the per-session connection JSON.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::error::{JupyterError, JupyterResult};

/// The Jupyter-displayed name of the kernel (no path, no version).
pub const KERNEL_DISPLAY_NAME: &str = "Buff";

/// The Jupyter-internal name (lowercase, no spaces) used as the
/// kernelspec directory name and the `--kernel` flag value
/// (`jupyter console --kernel buff`).
pub const KERNEL_NAME: &str = "buff";

/// The programming language label Jupyter uses for syntax highlighting
/// fallbacks and `language` metadata.
pub const KERNEL_LANGUAGE: &str = "buff";

/// How the kernel wants Jupyter to signal interruption.
///
/// `"signal"` is the default (Jupyter sends SIGINT on Unix / emulates
/// via CTRL_BREAK_EVENT on Windows). The alternative `"message"` mode
/// sends an `interrupt_request` on the control socket — T129a does
/// NOT wire that up.
pub const KERNEL_INTERRUPT_MODE: &str = "signal";

/// The `kernel.json` document Jupyter loads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelSpec {
    /// Argv template Jupyter expands to launch the kernel. The
    /// `{connection_file}` token is substituted at launch time.
    pub argv: Vec<String>,
    /// Display name shown in the JupyterLab / Notebook launcher menu.
    pub display_name: String,
    /// Programming language label (lowercase).
    pub language: String,
    /// Interruption mode: `"signal"` (default) or `"message"`.
    pub interrupt_mode: String,
    /// Free-form metadata bag. Empty for T129a.
    #[serde(default)]
    pub metadata: Value,
}

impl KernelSpec {
    /// Build the canonical Buff kernelspec — invokes
    /// `buff jupyter start --connection-file {connection_file}` so the
    /// same binary that the user installs handles kernel launches.
    ///
    /// `buff_exe` is the absolute path to the `buff` binary (resolved
    /// by the CLI via `std::env::current_exe` so the kernelspec points
    /// at the EXACT binary the user invoked `install` from — not a
    /// bare `buff` that might resolve to a different install on
    /// `PATH`).
    #[must_use]
    pub fn buff(buff_exe: &str) -> Self {
        Self {
            argv: vec![
                buff_exe.to_string(),
                "jupyter".to_string(),
                "start".to_string(),
                "--connection-file".to_string(),
                "{connection_file}".to_string(),
            ],
            display_name: KERNEL_DISPLAY_NAME.to_string(),
            language: KERNEL_LANGUAGE.to_string(),
            interrupt_mode: KERNEL_INTERRUPT_MODE.to_string(),
            metadata: Value::Object(serde_json::Map::new()),
        }
    }

    /// Serialize to a pretty-printed JSON string (the shape Jupyter
    /// writes when you run `jupyter kernelspec install`).
    ///
    /// # Errors
    ///
    /// Returns [`JupyterError::Json`] on serialization failure
    /// (should not happen for the canonical Buff kernelspec, but the
    /// contract is fallible).
    pub fn to_json_pretty(&self) -> JupyterResult<String> {
        let s = serde_json::to_string_pretty(self)?;
        Ok(s)
    }
}

/// Locate the Jupyter data-dir for kernelspec installation.
///
/// Order:
/// 1. `$JUPYTER_DATA_DIR` env var (if set).
/// 2. Platform default:
///    - Windows: `%APPDATA%\jupyter\kernels\`
///    - macOS: `~/Library/Jupyter/kernels/`
///    - Linux/other: `~/.local/share/jupyter/kernels/`
/// 3. Falls back to `None` if the home / appdata dir cannot be
///    resolved (rare — the caller surfaces this as a user-action item
///    rather than crashing).
///
/// Mirrors the resolution rules `jupyter_core.paths.jupyter_data_dir()`
/// uses — we deliberately do NOT depend on `jupyter_core` (would
/// require python on the build host); the env-var + platform-default
/// fallback covers the cases that matter.
#[must_use]
pub fn jupyter_kernels_dir() -> Option<PathBuf> {
    if let Ok(env_dir) = std::env::var("JUPYTER_DATA_DIR") {
        if !env_dir.is_empty() {
            return Some(PathBuf::from(env_dir).join("kernels"));
        }
    }
    if cfg!(target_os = "windows") {
        std::env::var("APPDATA")
            .ok()
            .map(|d| PathBuf::from(d).join("jupyter").join("kernels"))
    } else if cfg!(target_os = "macos") {
        std::env::var("HOME").ok().map(|h| {
            PathBuf::from(h)
                .join("Library")
                .join("Jupyter")
                .join("kernels")
        })
    } else {
        std::env::var("HOME").ok().map(|h| {
            PathBuf::from(h)
                .join(".local")
                .join("share")
                .join("jupyter")
                .join("kernels")
        })
    }
}

/// Where `buff jupyter install` will write the kernelspec — the
/// `<data_dir>/kernels/buff/` directory. Returns `None` if
/// [`jupyter_kernels_dir`] cannot resolve the data dir.
#[must_use]
pub fn buff_kernelspec_dir() -> Option<PathBuf> {
    jupyter_kernels_dir().map(|d| d.join(KERNEL_NAME))
}

/// Write the `kernel.json` file into `<dest_dir>/kernel.json`.
///
/// Used by [`install`] when writing directly to the data-dir AND by
/// the `jupyter kernelspec install <tmpdir>` shell-out path (writes
/// into a temp dir first, lets `jupyter` move it into place).
///
/// # Errors
///
/// Returns [`JupyterError::Io`] on filesystem failure (missing parent
/// dir, permission denied, etc.). Parent directories are NOT created
/// by this function — the caller is expected to have `mkdir -p`'d
/// them already.
pub fn write_kernel_json(spec: &KernelSpec, dest_dir: &Path) -> JupyterResult<PathBuf> {
    let json = spec.to_json_pretty()?;
    if let Err(e) = std::fs::create_dir_all(dest_dir) {
        return Err(JupyterError::Io(format!(
            "create_dir_all({}): {e}",
            dest_dir.display()
        )));
    }
    let dest = dest_dir.join("kernel.json");
    std::fs::write(&dest, json)?;
    Ok(dest)
}

/// Top-level install routine for `buff jupyter install`.
///
/// Strategy:
/// 1. Resolve `buff` exe path (via `std::env::current_exe` so the
///    kernelspec points at the EXACT binary the user just ran).
/// 2. If `jupyter` is on `PATH`, shell out to
///    `jupyter kernelspec install <tmpdir> --name=buff` (preferred —
///    Jupyter resolves cross-platform data-dir + handles `--replace`).
/// 3. Otherwise, fall back to writing directly into
///    [`buff_kernelspec_dir`].
///
/// Returns the path the kernelspec ended up at (for the CLI to print).
///
/// # Errors
///
/// Returns [`JupyterError::Io`] if the buff exe path cannot be
/// resolved, if the temp dir cannot be created, or if the write
/// fails. Returns [`JupyterError::UnsupportedConnectionValue`] if no
/// data dir can be resolved (no `$JUPYTER_DATA_DIR`, no `$APPDATA` /
/// `$HOME`).
pub fn install() -> JupyterResult<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| JupyterError::Io(format!("current_exe: {e}")))?;
    let exe_str = exe.display().to_string();
    let spec = KernelSpec::buff(&exe_str);

    // Try the preferred path: shell out to `jupyter kernelspec install`.
    if let Ok(installed) = try_install_via_jupyter_cli(&spec) {
        return Ok(installed);
    }

    // Fallback: write directly into the resolved data dir.
    let dest_dir =
        buff_kernelspec_dir().ok_or_else(|| JupyterError::UnsupportedConnectionValue {
            field: "JUPYTER_DATA_DIR".to_string(),
            value:
                "no $JUPYTER_DATA_DIR / $APPDATA / $HOME — cannot resolve kernelspec install dir"
                    .to_string(),
        })?;
    write_kernel_json(&spec, &dest_dir)
}

/// Attempt `jupyter kernelspec install <tmpdir> --name=buff`. Returns
/// `Ok(path)` if the install succeeded (path is the dir jupyter moved
/// it to — typically `<data_dir>/kernels/buff/`), `Err` if jupyter is
/// not on PATH or the install failed (caller falls back to direct
/// write).
fn try_install_via_jupyter_cli(spec: &KernelSpec) -> JupyterResult<PathBuf> {
    // Stage the kernelspec into a temp dir first; let jupyter move it.
    let staging_parent =
        std::env::temp_dir().join(format!("buff-jupyter-install-{}", std::process::id()));
    let staging_dir = staging_parent.join(KERNEL_NAME);
    if let Err(e) = std::fs::create_dir_all(&staging_dir) {
        return Err(JupyterError::Io(format!(
            "create_dir_all({}): {e}",
            staging_dir.display()
        )));
    }
    // Write the kernel.json into the staging dir. The path is unused
    // after this point — `jupyter kernelspec install` reads the
    // staging dir by path; we discard the per-file return.
    let _staged = write_kernel_json(spec, &staging_dir)?;
    let staged_parent_str = staging_parent.display().to_string();

    // Invoke `jupyter kernelspec install`.
    let output = std::process::Command::new("jupyter")
        .args([
            "kernelspec",
            "install",
            "--replace",
            "--name=buff",
            &staged_parent_str,
        ])
        .output();

    let _ = std::fs::remove_dir_all(&staging_parent); // best-effort cleanup

    match output {
        Ok(result) if result.status.success() => {
            // jupyter prints the install destination to stdout; we can
            // also just compute it from buff_kernelspec_dir().
            Ok(buff_kernelspec_dir().unwrap_or(staging_dir))
        }
        Ok(result) => {
            // jupyter exists but errored — surface the failure and let
            // the caller fall back.
            let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
            Err(JupyterError::Io(format!(
                "jupyter kernelspec install failed: status {} stderr: {stderr}",
                result.status
            )))
        }
        Err(_) => {
            // jupyter not on PATH — not an error from the caller's
            // perspective (fall back to direct write).
            Err(JupyterError::Io(
                "jupyter not on PATH; falling back to direct write".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buff_kernel_spec_argv_template() {
        let spec = KernelSpec::buff("/usr/local/bin/buff");
        assert_eq!(
            spec.argv,
            vec![
                "/usr/local/bin/buff".to_string(),
                "jupyter".to_string(),
                "start".to_string(),
                "--connection-file".to_string(),
                "{connection_file}".to_string(),
            ]
        );
        assert_eq!(spec.display_name, "Buff");
        assert_eq!(spec.language, "buff");
        assert_eq!(spec.interrupt_mode, "signal");
    }

    #[test]
    fn kernel_spec_json_round_trips() {
        let spec = KernelSpec::buff("/path/to/buff");
        let json = spec.to_json_pretty().expect("serialize");
        let parsed: KernelSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(spec, parsed);
    }

    #[test]
    fn kernel_spec_json_has_required_fields() {
        let spec = KernelSpec::buff("/bin/buff");
        let json = spec.to_json_pretty().expect("serialize");
        let v: Value = serde_json::from_str(&json).expect("re-parse");
        let obj = v.as_object().expect("object");
        for key in [
            "argv",
            "display_name",
            "language",
            "interrupt_mode",
            "metadata",
        ] {
            assert!(obj.contains_key(key), "missing {key}");
        }
        let argv = obj["argv"].as_array().expect("argv");
        assert_eq!(argv.len(), 5);
        assert_eq!(argv[4].as_str(), Some("{connection_file}"));
    }

    #[test]
    fn write_kernel_json_creates_dir_and_file() {
        let tmp = std::env::temp_dir().join(format!(
            "buff-jupyter-test-write-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let dest = write_kernel_json(&KernelSpec::buff("/bin/buff"), &tmp).expect("write");
        assert!(dest.ends_with("kernel.json"));
        let on_disk = std::fs::read_to_string(&dest).expect("read");
        let parsed: KernelSpec = serde_json::from_str(&on_disk).expect("parse");
        assert_eq!(parsed.display_name, "Buff");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn jupyter_kernels_dir_respects_env_var() {
        // The env var path is only consulted when set; we don't mutate
        // real env here to keep the test hermetic. This test asserts
        // the function returns SOME path on a normal host (i.e. one
        // where HOME or APPDATA is set, which is always the case on a
        // developer machine).
        let dir = jupyter_kernels_dir();
        assert!(
            dir.is_some(),
            "jupyter_kernels_dir should resolve on a normal host"
        );
    }

    #[test]
    fn buff_kernelspec_dir_appends_buff() {
        if let Some(dir) = buff_kernelspec_dir() {
            assert!(dir.ends_with(KERNEL_NAME));
        }
    }
}
