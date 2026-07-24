//! Cargo.toml generation - extracted from `config.rs` (T106 mechanical split).
//!
//! `generate_cargo_toml` and `generate_workspace_cargo_toml` produce the
//! deterministic `Cargo.toml` (or virtual workspace manifest) from a parsed
//! `BuffConfig`. Pure functions - no I/O, sorted-key output for idempotency.

use super::*;
/// Generate a complete `Cargo.toml` string from a `BuffConfig` manifest.
///
/// The output is deterministic (sorted keys, stable formatting) so that
/// running this function twice on the same config yields byte-identical
/// output (idempotency guarantee).
///
/// # Two emission modes
///
/// - **Single-package mode** (default, v0.1 behaviour): `cfg.workspace`
///   is `None`, `cfg.package` is `Some`. Emits a regular `Cargo.toml`
///   with `[package]` + `[[bin]]` + `[dependencies]`.
/// - **Virtual workspace mode** (T123): `cfg.workspace` is `Some`,
///   `cfg.package` is `None`. Emits a Cargo virtual manifest with
///   `[workspace]` + `members` + `resolver = "2"` and NO `[package]`.
///   `cargo build` / `cargo test` invoked at this root fans out to all
///   members automatically (the whole point of passthrough — Buff does
///   NOT reinvent workspace dep-dedup or shared-`target/`).
///
/// # Idempotency
///
/// `generate_cargo_toml(cfg) == generate_cargo_toml(cfg)` for any `cfg`.
/// The function uses sorted iteration and a fixed format string — no
/// HashMap iteration, no environment-dependent output. Member order in
/// workspace mode is preserved as-declared (see [`WorkspaceSection`]
/// doc for why order matters).
pub fn generate_cargo_toml(cfg: &BuffConfig) -> String {
    // Workspace virtual manifest mode (T123).
    if let Some(ws) = &cfg.workspace {
        return generate_workspace_cargo_toml(ws);
    }

    // Single-package mode (v0.1 behaviour).
    // Defensive: parse() guarantees package is Some when workspace is None,
    // but we never panic — fall through to an empty-comment Cargo.toml so
    // any future edge case surfaces as a build error rather than a crash.
    let package = match &cfg.package {
        Some(p) => p,
        None => {
            return "# ERROR: buff.toml has neither [package] nor [workspace]\n".to_string();
        }
    };

    let mut out = String::new();

    // [package]
    out.push_str("[package]\n");
    out.push_str(&format!("name = \"{}\"\n", package.name));
    out.push_str(&format!("version = \"{}\"\n", package.version));
    out.push_str("edition = \"2021\"\n");

    // Optional edition override from buff.toml
    if let Some(ed) = &package.edition {
        // Map Buff edition to Rust edition. For now all Buff editions map to
        // Rust 2021 (the pinned workspace edition).
        out.push_str(&format!("# buff edition: {ed}\n"));
    }

    // [[bin]] — the transpiled entry point
    out.push_str("\n[[bin]]\n");
    out.push_str(&format!("name = \"{}\"\n", package.name));
    out.push_str("path = \"src/main.rs\"\n");

    // [dependencies] — sorted by key (BTreeMap guarantees this)
    if !cfg.dependencies.is_empty() {
        out.push_str("\n[dependencies]\n");
        for (name, version) in &cfg.dependencies {
            out.push_str(&format!("{name} = \"{version}\"\n"));
        }
    }

    // [rust-deps] entries go into [dependencies] in the generated Cargo.toml
    // (they are Rust crate dependencies, not Buff package deps).
    if !cfg.rust_deps.is_empty() {
        // If [dependencies] section already exists, entries are appended.
        // If not, we need to start the section.
        if cfg.dependencies.is_empty() {
            out.push_str("\n[dependencies]\n");
        }
        for (name, version) in &cfg.rust_deps {
            out.push_str(&format!("{name} = \"{version}\"\n"));
        }
    }

    // [git-dependencies] entries emit as local-path deps pointing at the
    // cloned checkout (T122). The local-path form is preferred over
    // Cargo's native `{ git = "..." }` form because:
    //   - it's offline-friendly (cargo never re-fetches),
    //   - it matches the "clone to ~/.buff/git" design (one canonical
    //     checkout per URL, shared across projects),
    //   - it lets the user inspect / patch the checkout directly.
    // Path resolution is delegated to [`git_checkout_path`], which is
    // deterministic (sha256 of the URL). On the rare case the home dir
    // can't be resolved (env unset), we emit a comment instead of a
    // malformed dep entry so the build breaks loudly rather than silently.
    if !cfg.git_dependencies.is_empty() {
        if cfg.dependencies.is_empty() && cfg.rust_deps.is_empty() {
            out.push_str("\n[dependencies]\n");
        }
        for (name, dep) in &cfg.git_dependencies {
            match git_checkout_path(&dep.git) {
                Ok(path) => {
                    let path_str = path.display().to_string().replace('\\', "/");
                    out.push_str(&format!("{name} = {{ path = \"{path_str}\" }}\n"));
                }
                Err(_) => {
                    out.push_str(&format!(
                        "# WARNING: could not resolve git checkout path for `{name}` \
                         (set USERPROFILE or HOME)\n"
                    ));
                }
            }
        }
    }

    // [registry-dependencies] entries (T127) are recorded in buff.toml
    // but have NO cargo-project wiring yet — the tarball download /
    // unpack / vendor step is deferred (mirrors the v0.5+v1.0
    // "cargo-project wiring is deferred" precedent). We emit a
    // comment-only block so:
    //   - the user sees the dep is recorded,
    //   - the build breaks loudly (no silent skipped dep),
    //   - a future `buff build` extension can replace the comment with
    //     a real `name = { path = "<vendor>/<name>-<version>" }` entry
    //     once vendoring lands.
    if !cfg.registry_dependencies.is_empty() {
        out.push_str("\n# [registry-dependencies] (T127 — cargo wiring TODO)\n");
        for (name, dep) in &cfg.registry_dependencies {
            out.push_str(&format!(
                "# {name} = \"{}\" (resolve + vendor on next `buff build`)\n",
                dep.version
            ));
        }
    }

    out
}

/// Emit a Cargo virtual workspace manifest from a [`WorkspaceSection`] (T123).
///
/// Produces:
///
/// ```toml
/// [workspace]
/// resolver = "2"
///
/// members = [
///     "pkg-a",
///     "pkg-b",
/// ]
/// ```
///
/// - `resolver` defaults to `"2"` (the modern resolver; recommended for
///   edition 2021+) when the user left it unset in `buff.toml`.
/// - `members` preserves declared order (see [`WorkspaceSection`] doc).
/// - Empty `members` emits `members = []` on a single line (degenerate
///   but syntactically valid; guards against a panic-on-empty regression).
pub(super) fn generate_workspace_cargo_toml(ws: &WorkspaceSection) -> String {
    let mut out = String::new();
    out.push_str("[workspace]\n");
    let resolver = ws.resolver.as_deref().unwrap_or("2");
    out.push_str(&format!("resolver = \"{resolver}\"\n"));

    if ws.members.is_empty() {
        out.push_str("members = []\n");
    } else {
        out.push_str("\nmembers = [\n");
        for member in &ws.members {
            out.push_str(&format!("    \"{member}\",\n"));
        }
        out.push_str("]\n");
    }

    // T0-A3b: [workspace.dependencies] — workspace-level dep declarations
    // inherited by members via `<dep>.workspace = true`. Emitted as the
    // Cargo-equivalent block so a single `cargo build` at the workspace
    // root resolves all members uniformly. BTreeMap iteration is sorted
    // → byte-deterministic output.
    if !ws.dependencies.is_empty() {
        out.push_str("\n[workspace.dependencies]\n");
        for (name, version) in &ws.dependencies {
            out.push_str(&format!("{name} = \"{version}\"\n"));
        }
    }

    // T0-A3b: [workspace.extern] — workspace-level Rust crate (`extern`)
    // declarations inherited by members. Emit under the same
    // `[workspace.dependencies]` block (Cargo doesn't distinguish Buff
    // deps from Rust deps — both are crate deps from cargo's POV).
    if !ws.extern_crates.is_empty() {
        if ws.dependencies.is_empty() {
            out.push_str("\n[workspace.dependencies]\n");
        }
        for (name, version) in &ws.extern_crates {
            out.push_str(&format!("{name} = \"{version}\"\n"));
        }
    }

    out
}
