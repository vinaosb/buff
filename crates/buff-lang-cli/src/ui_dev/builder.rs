//! Build pipeline abstraction (T131).
//!
//! When the file watcher fires, the dev server needs to:
//!
//! 1. Re-run the Buff front-end (`pipeline::compile_to_rust`) on each
//!    changed `.buff` file — this catches lex / parse / codegen
//!    errors and regenerates the `.rs` file alongside the source.
//! 2. Shell out to `cargo build --target wasm32-unknown-unknown` to
//!    rebuild the Wasm bundle from the regenerated Rust source.
//! 3. Re-run `wasm-bindgen --target web --out-dir <dir>` to refresh
//!    the served JS+wasm bundle.
//!
//! Steps 2 and 3 are wrapped behind the [`Builder`] trait so unit
//! tests can inject a [`MockBuilder`] (the spec mandates "Mock the
//! file watcher + cargo build in tests (do NOT shell out in unit
//! tests)"). Production code uses [`CargoBuilder`].
//!
//! Step 1 (the Buff front-end) lives in
//! [`crate::pipeline::compile_to_rust`] and is always invoked for
//! real — it's pure Rust, no shell-out, so it's safe to run in tests.
//! The dev server calls it for every changed `.buff` file regardless
//! of the [`Builder`] used for the cargo/wasm-bindgen half.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::ui_dev::error::UiDevError;

/// Outcome of a single [`Builder::build`] invocation. The dev server
/// uses this to decide whether to broadcast a `Reload` or an `Error`
/// message to connected browsers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildOutcome {
    /// The build succeeded. The browser should refresh via
    /// `location.reload()` (LIVE RELOAD — not HMR).
    Ok,
    /// The build failed. The browser should show `message` in a red
    /// banner overlay. The dev server stays up so the user can fix
    /// the error and save again.
    Failed {
        /// Pre-formatted error message (may be multi-line compiler
        /// output). The broadcaster sends it verbatim as the `message`
        /// field of a `{"type":"error", message:"..."}` frame.
        message: String,
    },
}

/// The cargo + wasm-bindgen builder (production path).
///
/// Shells out to `cargo build --target wasm32-unknown-unknown` from
/// `project_root`, then `wasm-bindgen --target web --out-dir
/// <project_root>/target/wasm-bindgen <wasm>`. The exact wasm artifact
/// path is whatever cargo emits for the project's default target
/// (cdylib OR example bin), so we glob the wasm32-unknown-unknown
/// target dir to find the freshest `.wasm` after a successful cargo
/// build. If no `.wasm` is found, we treat it as a no-op success
/// (the project may not yet have any Rust UI code; Buff syntax errors
/// are surfaced separately via [`crate::pipeline`]).
pub struct CargoBuilder {
    project_root: PathBuf,
}

impl CargoBuilder {
    /// Construct a `CargoBuilder` for the given project root.
    #[must_use]
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Builder for CargoBuilder {
    fn build(&self) -> Result<BuildOutcome, UiDevError> {
        // No Cargo.toml → no Rust side to rebuild. Treat as Ok (the
        // .buff front-end already ran in `run_buff_pipeline`); the
        // browser reloads to fetch whatever static asset changed.
        if !self.project_root.join("Cargo.toml").exists() {
            return Ok(BuildOutcome::Ok);
        }

        // Step 1: cargo build --target wasm32-unknown-unknown.
        let cargo_out = std::process::Command::new("cargo")
            .arg("build")
            .arg("--target")
            .arg("wasm32-unknown-unknown")
            .current_dir(&self.project_root)
            .output()
            .map_err(|e| UiDevError::WasmBuild {
                message: format!("failed to invoke cargo: {e}"),
            })?;

        if !cargo_out.status.success() {
            let stderr = String::from_utf8_lossy(&cargo_out.stderr).to_string();
            return Ok(BuildOutcome::Failed {
                message: format!("cargo build --target wasm32-unknown-unknown failed:\n{stderr}"),
            });
        }

        // Step 2: locate the freshest .wasm artifact under the
        // wasm32-unknown-unknown target dir. We search both `debug/`
        // and `release/` and pick whichever file exists with the
        // latest mtime (cargo emits to one or the other based on
        // --release; we did not pass --release, so debug/ is the
        // default).
        let wasm_dir = self
            .project_root
            .join("target")
            .join("wasm32-unknown-unknown")
            .join("debug");
        let Some(wasm_path) = find_freshest_wasm(&wasm_dir) else {
            // No .wasm artifact found — this is fine if the project
            // has no cdynlib/example to emit. The .buff front-end
            // already ran; the browser reload will pick up static
            // changes.
            return Ok(BuildOutcome::Ok);
        };

        // Step 3: wasm-bindgen --target web --out-dir <dir> <wasm>.
        let out_dir = self.project_root.join("target").join("wasm-bindgen");
        std::fs::create_dir_all(&out_dir).map_err(|e| UiDevError::WasmBuild {
            message: format!(
                "failed to create wasm-bindgen out dir `{}`: {e}",
                out_dir.display()
            ),
        })?;

        let wb_out = std::process::Command::new("wasm-bindgen")
            .arg("--target")
            .arg("web")
            .arg("--out-dir")
            .arg(&out_dir)
            .arg(&wasm_path)
            .output()
            .map_err(|e| UiDevError::WasmBuild {
                message: format!("failed to invoke wasm-bindgen (is it installed?): {e}"),
            })?;

        if !wb_out.status.success() {
            let stderr = String::from_utf8_lossy(&wb_out.stderr).to_string();
            return Ok(BuildOutcome::Failed {
                message: format!("wasm-bindgen failed:\n{stderr}"),
            });
        }

        Ok(BuildOutcome::Ok)
    }
}

/// Find the freshest `.wasm` file under `dir` (recursive). Returns
/// `None` when the directory does not exist or contains no `.wasm`.
///
/// "Freshest" = largest mtime — picks the artifact cargo just wrote.
fn find_freshest_wasm(dir: &Path) -> Option<PathBuf> {
    use std::time::SystemTime;
    if !dir.exists() {
        return None;
    }
    let mut best: Option<(SystemTime, PathBuf)> = None;
    walk_wasm(dir, &mut best);
    best.map(|(_, p)| p)
}

fn walk_wasm(dir: &Path, best: &mut Option<(SystemTime, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_wasm(&path, best);
            continue;
        }
        let is_wasm = path.extension().map(|e| e == "wasm").unwrap_or(false);
        if !is_wasm {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        match best {
            None => *best = Some((mtime, path)),
            Some((cur, _)) if mtime > *cur => *best = Some((mtime, path)),
            _ => {}
        }
    }
}

/// Abstraction over the cargo + wasm-bindgen rebuild path.
///
/// Production code uses [`CargoBuilder`]; unit tests inject a
/// [`MockBuilder`] so no real shell-out happens.
pub trait Builder: Send + Sync {
    /// Rebuild the Wasm bundle. Returns:
    ///
    /// - `Err(UiDevError)` when the build infra itself is broken
    ///   (e.g. cargo binary missing). These are unrecoverable.
    /// - `Ok(BuildOutcome::Failed { message })` when cargo /
    ///   wasm-bindgen ran but failed. Recoverable — the user fixes
    ///   the code and saves again.
    /// - `Ok(BuildOutcome::Ok)` when the build succeeded.
    fn build(&self) -> Result<BuildOutcome, UiDevError>;
}

/// A mock builder that always returns a canned outcome. Used in unit
/// tests to avoid shelling out to cargo / wasm-bindgen.
#[derive(Debug, Clone)]
pub struct MockBuilder {
    outcome: BuildOutcome,
}

impl MockBuilder {
    /// Construct a mock that always returns `Ok(BuildOutcome::Ok)`.
    #[must_use]
    pub fn ok() -> Self {
        Self {
            outcome: BuildOutcome::Ok,
        }
    }

    /// Construct a mock that always returns
    /// `Ok(BuildOutcome::Failed { message })` with the given message.
    #[must_use]
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            outcome: BuildOutcome::Failed {
                message: message.into(),
            },
        }
    }
}

impl Builder for MockBuilder {
    fn build(&self) -> Result<BuildOutcome, UiDevError> {
        Ok(self.outcome.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_ok_returns_ok_outcome() {
        let b = MockBuilder::ok();
        assert_eq!(b.build().unwrap(), BuildOutcome::Ok);
    }

    #[test]
    fn mock_failed_returns_failed_outcome() {
        let b = MockBuilder::failed("oh no");
        let outcome = b.build().unwrap();
        match outcome {
            BuildOutcome::Failed { message } => assert_eq!(message, "oh no"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn build_outcome_implements_eq() {
        // Eq is required so tests can assert outcomes directly.
        assert_eq!(BuildOutcome::Ok, BuildOutcome::Ok);
        assert_ne!(
            BuildOutcome::Ok,
            BuildOutcome::Failed {
                message: "x".into()
            }
        );
    }

    #[test]
    fn find_freshest_wasm_returns_none_for_missing_dir() {
        assert!(find_freshest_wasm(Path::new("this/does/not/exist")).is_none());
    }

    #[test]
    fn find_freshest_wasm_returns_none_for_empty_dir() {
        let tmp = std::env::temp_dir().join("buff-ui-dev-test-empty");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(find_freshest_wasm(&tmp).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_freshest_wasm_finds_a_wasm() {
        let tmp = std::env::temp_dir().join("buff-ui-dev-test-wasm");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let wasm = tmp.join("foo.wasm");
        std::fs::write(&wasm, b"\0asm").unwrap();
        let found = find_freshest_wasm(&tmp).expect("should find one");
        assert_eq!(found.file_name().unwrap(), "foo.wasm");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_freshest_wasm_skips_non_wasm_files() {
        let tmp = std::env::temp_dir().join("buff-ui-dev-test-skip");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("foo.txt"), b"not wasm").unwrap();
        assert!(find_freshest_wasm(&tmp).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
