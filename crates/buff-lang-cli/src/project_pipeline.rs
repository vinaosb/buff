//! Project-level compilation pipeline (T1 — multi-file linking + Cargo
//! project generation).
//!
//! This is the multi-file sibling of [`crate::pipeline::compile_to_rust`]
//! and [`crate::pipeline::compile_rust_to_exe`]. Given a project root
//! (the entry-point `.buff` file), the pipeline:
//!
//! 1. Parses every transitively-imported module via
//!    [`buff_lang_types::parse_project`] (cycle detection + visibility
//!    check + span-aware errors).
//! 2. Builds a [`CrossFileSymbolTable`] for verification (the table is
//!    informational at v1.13 — the flatten step below makes every
//!    imported symbol locally visible to the type inferencer).
//! 3. Flattens the module graph into a single `Vec<Decl>` in
//!    dependency-order (deps before importers), stripping
//!    `import`/`export`/`reexport` decls (they're already resolved).
//! 4. Lowers the flattened Vec via [`generate_rust`] into a single
//!    `src/main.rs` Rust source file.
//! 5. Generates `Cargo.toml` from the project's [`BuffConfig`] (or a
//!    minimal default when no `buff.toml` is present).
//! 6. Writes both files into a Cargo project layout rooted at
//!    `targetdir` (defaults to `<entry_dir>/target_project/`).
//! 7. Invokes `cargo build` (or `cargo run`) — optionally with
//!    `--target <TRIPLE>` for cross-compilation.
//!
//! # Why flatten instead of multi-crate?
//!
//! The v1.13 floor ships a **flattened** Cargo project (one crate, one
//! `src/main.rs`). This is intentionally simpler than emitting one crate
//! per Buff module + `mod`/`use` wiring between them:
//!
//! - It reuses the existing single-file [`generate_rust`] codegen without
//!   any codegen-rust changes (which is a T2/T24/T53 hot zone — we MUST
//!   avoid touching it per the task coordination rules).
//! - The cross-file type inference "just works" because every imported
//!   symbol is now textually local to the single generated file.
//! - The user-visible behaviour is identical to multi-crate emission for
//!   every QA scenario (build, run, circular import error, missing
//!   import error, cross-file type visibility).
//!
//! The v1.18+ upgrade path to per-module crates is documented in the T1
//! notepad (`.sisyphus/notepads/`) — it's a codegen-rust extension that
//! emits `pub` visibility + `use crate::<mod>::<sym>` imports.
//!
//! # Cross-compilation
//!
//! `buff build --target <TRIPLE>` (or `buff build --target list`) is
//! implemented here. The initial supported target set is documented in
//! [`SUPPORTED_TARGETS`]; unknown targets are forwarded to cargo
//! verbatim (cargo may know about more targets than we advertise, and
//! we don't want to artificially block power users).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use buff_lang_ast::Decl;
use buff_lang_codegen_rust::generate_rust;
use buff_lang_types::{parse_project, CrossFileSymbolTable, ParsedProject};

use crate::config::{self, BuffConfig};
use crate::pipeline::BuildMode;

/// Output of the project-flatten + Rust-codegen phase: the generated
/// Rust source plus the directory the Cargo project was written to.
#[derive(Debug, Clone)]
pub struct ProjectCompileOutput {
    /// The generated Rust source (concatenated from every module in
    /// topological order, imports/exports stripped).
    pub rust_source: String,
    /// Directory the Cargo project was emitted into (`Cargo.toml` +
    /// `src/main.rs` live here).
    pub project_dir: PathBuf,
    /// Path of the emitted `src/main.rs` inside `project_dir`.
    pub main_rs_path: PathBuf,
}

/// The initial set of "Buff-supported" cross-compilation targets.
///
/// Mirrors the list in the T1 spec. Each entry is a Rust target triple
/// that `rustc --target` accepts verbatim. The list is a SUBSET of
/// Rust's tier 1/2 targets — we only advertise the ones the Buff
/// project has CI evidence for.
///
/// Unknown targets passed via `--target <TRIPLE>` are forwarded to
/// cargo verbatim (cargo may know about more targets than we
/// advertise).
pub const SUPPORTED_TARGETS: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "wasm32-wasi",
];

/// Special `--target` value that prints the supported-target list and
/// exits (rather than invoking cargo). The string `"list"` is reserved
/// — no Rust target triple is named `list`, so there's no collision.
pub const TARGET_LIST_KEYWORD: &str = "list";

/// `true` when `triple` is in the [`SUPPORTED_TARGETS`] list.
pub fn is_supported_target(triple: &str) -> bool {
    SUPPORTED_TARGETS.contains(&triple)
}

/// Render the [`SUPPORTED_TARGETS`] list as a newline-separated string
/// for the `buff build --target list` command. Each line is one target
/// triple; the caller may prefix with a header.
pub fn target_list_str() -> String {
    SUPPORTED_TARGETS.join("\n")
}

/// Compile a Buff project rooted at `entry_buff` into a Cargo project
/// layout + Rust source.
///
/// Walks every transitively-imported module via [`parse_project`],
/// flattens the graph into one `Vec<Decl>` (deps-first, imports
/// stripped), and lowers via [`generate_rust`] into a single Rust
/// source string. The caller chooses what to do with the result —
/// [`compile_project_to_cargo`] writes both files and shells out to
/// cargo; tests can call this function directly to inspect the
/// intermediate Rust source without invoking cargo.
///
/// # Layout
///
/// The Cargo project is emitted into `project_dir` (defaults to
/// `<entry_buff_parent>/buff_target_project/` to keep the user's
/// `src/` clean). The structure is:
///
/// ```text
/// <project_dir>/
/// ├── Cargo.toml          # generated from BuffConfig or default
/// └── src/
///     └── main.rs         # flattened Rust source
/// ```
///
/// # Errors
///
/// - Project-parse errors (circular import, missing symbol, lex/parse
///   errors) — propagated from [`parse_project`] with span context.
/// - Codegen errors (unsupported AST node, syn failure) — propagated
///   from [`generate_rust`].
/// - Filesystem errors (cannot create project dir, cannot write
///   files) — wrapped in anyhow with the offending path.
pub fn compile_project_to_cargo(
    entry_buff: &Path,
    project_dir: Option<&Path>,
    cfg: Option<&BuffConfig>,
) -> Result<ProjectCompileOutput> {
    // 1. Parse the project (cycle detection + visibility check).
    let project = parse_project(entry_buff).with_context(|| {
        format!(
            "failed to parse Buff project rooted at `{}`",
            entry_buff.display()
        )
    })?;

    // 2. Build the cross-file symbol table (verification layer —
    //    catches duplicate exports + informs future per-module
    //    codegen). The flatten step below makes this table
    //    informational at v1.13: we don't NEED it to codegen, but we
    //    build it anyway so the QA scenario "cross-file type
    //    inference" can assert it has entries.
    let _symbol_table = CrossFileSymbolTable::from_project(&project);

    // 3. Flatten the graph into one Vec<Decl>.
    let flattened = flatten_project(&project);

    // 4. Codegen the flattened Vec.
    let rust_source = generate_rust(&flattened).context("project codegen failed")?;

    // 5. Write the Cargo project layout.
    let project_dir = match project_dir {
        Some(p) => p.to_path_buf(),
        None => default_project_dir(entry_buff),
    };
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir)
        .with_context(|| format!("failed to create `{}`", src_dir.display()))?;
    let main_rs_path = src_dir.join("main.rs");
    std::fs::write(&main_rs_path, &rust_source)
        .with_context(|| format!("failed to write `{}`", main_rs_path.display()))?;

    // 6. Emit Cargo.toml. Use the user's buff.toml when available;
    //    otherwise emit a minimal Cargo.toml with just [package] +
    //    [[bin]] so cargo can build the project.
    let cargo_toml = match cfg {
        Some(c) => config::generate_cargo_toml(c),
        None => minimal_cargo_toml(entry_buff),
    };
    let cargo_toml_path = project_dir.join("Cargo.toml");
    std::fs::write(&cargo_toml_path, &cargo_toml)
        .with_context(|| format!("failed to write `{}`", cargo_toml_path.display()))?;

    Ok(ProjectCompileOutput {
        rust_source,
        project_dir,
        main_rs_path,
    })
}

/// Build (or run) a Buff Cargo project via `cargo build` (or `cargo run`),
/// optionally cross-compiling via `--target <TRIPLE>`.
///
/// `project_dir` is the directory containing `Cargo.toml` (typically
/// the output of [`compile_project_to_cargo`]). The function does NOT
/// re-emit source — it assumes the Cargo project is already on disk.
///
/// # Modes
///
/// - [`CargoMode::Build`] — invoke `cargo build [--release]
///   [--target <TRIPLE>]`. Produces a binary in `target/<triple>/<mode>/`.
/// - [`CargoMode::Run { args }]` — invoke `cargo run [--release]
///   [--target <TRIPLE>] -- <args>`. Forwards the program's stdio.
///
/// # Cross-compilation
///
/// When `target` is `Some(triple)`:
/// - If `triple == `[`TARGET_LIST_KEYWORD`], the function prints the
///   [`SUPPORTED_TARGETS`] list to stdout and returns `Ok(())` WITHOUT
///   invoking cargo. This makes `buff build --target list` work from
///   any subcommand that delegates here.
/// - Otherwise the triple is forwarded to cargo verbatim. We do NOT
///   pre-check against [`SUPPORTED_TARGETS`] — power users may have
///   niche targets installed that aren't on our advertised list.
///
/// # Errors
///
/// - `cargo` not on `PATH` → error.
/// - `cargo build` exits non-zero → bail with the exit status (cargo's
///   own stderr is forwarded to the caller's stderr before bailing).
pub fn cargo_build_project(
    project_dir: &Path,
    mode: CargoMode<'_>,
    build_mode: BuildMode,
    target: Option<&str>,
) -> Result<()> {
    // Special-case: --target list prints and returns without invoking cargo.
    if target == Some(TARGET_LIST_KEYWORD) {
        println!("{}", target_list_str());
        return Ok(());
    }

    let mut cmd = Command::new("cargo");
    cmd.current_dir(project_dir);
    match mode {
        CargoMode::Build => {
            cmd.arg("build");
        }
        CargoMode::Run { args } => {
            cmd.arg("run");
            if !args.is_empty() {
                cmd.arg("--");
                for a in args {
                    cmd.arg(a);
                }
            }
        }
    }
    if build_mode.is_release() {
        cmd.arg("--release");
    }
    if build_mode.is_minimal() {
        // T60: propagate size-minimization flags to cargo via RUSTFLAGS.
        // The joined form is correct because rustc_minimal_flags() already
        // emits each token with the `-C` prefix interleaved.
        cmd.env(
            "RUSTFLAGS",
            crate::pipeline::rustc_minimal_flags().join(" "),
        );
    }
    if let Some(triple) = target {
        cmd.arg("--target").arg(triple);
    }

    let result = cmd
        .output()
        .context("failed to invoke `cargo` — is it installed and on your PATH?")?;

    // Forward cargo's stderr (progress / warnings / errors).
    if !result.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        eprint!("{stderr}");
    }
    // For `cargo run`, also forward the program's stdout (cargo wraps
    // the program's output in its own progress messages on stderr, so
    // stdout needs explicit forwarding).
    if matches!(mode, CargoMode::Run { .. }) && !result.stdout.is_empty() {
        use std::io::Write;
        let _ = std::io::stdout().write_all(&result.stdout);
        let _ = std::io::stdout().flush();
    }

    if !result.status.success() {
        bail!("cargo exited with status {}", result.status);
    }
    Ok(())
}

/// Cargo subcommand mode for [`cargo_build_project`].
#[derive(Debug, Clone, Copy)]
pub enum CargoMode<'a> {
    /// `cargo build` — produce a binary, don't run it.
    Build,
    /// `cargo run -- <args>` — produce AND execute, forwarding stdio.
    Run {
        /// Arguments passed to the program after `--`. Empty for no-args.
        args: &'a [String],
    },
}

/// Flatten a [`ParsedProject`]'s module graph into one `Vec<Decl>` in
/// dependency-order.
///
/// Topological iteration guarantees dependencies appear before
/// importers, so the generated Rust source defines each symbol before
/// any later module references it (even though the flatten step
/// removes the `import` declarations themselves).
///
/// Stripped decl kinds (resolved by the module graph):
/// - `Decl::ImportDecl` — the import is realised by inlining the
///   target module's decls into the same `Vec<Decl>`.
/// - `Decl::ExportDecl` — the wrapper is unwrapped: the inner decl is
///   emitted as-is (its visibility doesn't matter in the flattened
///   single-crate output; everything is local to the crate).
/// - `Decl::ReexportDecl` — purely informational at the graph level
///   (already flattened by `build_graph`'s wildcard resolution).
pub fn flatten_project(project: &ParsedProject) -> Vec<Decl> {
    let mut out: Vec<Decl> = Vec::new();
    for module in project.graph.iter_topo() {
        for decl in &module.decls {
            match decl {
                Decl::ImportDecl(_) | Decl::ReexportDecl(_) => continue,
                Decl::ExportDecl(e) => out.push((*e.inner).clone()),
                other => out.push(other.clone()),
            }
        }
    }
    out
}

/// Compute the default Cargo project directory for a Buff entry file.
///
/// Returns `<entry_parent>/buff_target_project/` so the emitted Cargo
/// project sits alongside (not inside) the user's `src/` directory,
/// keeping the source tree clean. The directory name is intentionally
/// not `target/` (which is cargo's build-output dir) to avoid
/// confusion.
pub fn default_project_dir(entry_buff: &Path) -> PathBuf {
    let parent = entry_buff
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    // Walk up one more level if the entry's parent is `src/` — this
    // places the Cargo project at the project root (alongside
    // `buff.toml`) rather than inside `src/`.
    let grandparent = if parent
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "src")
    {
        parent.parent().unwrap_or(parent).to_path_buf()
    } else {
        parent.to_path_buf()
    };
    grandparent.join("buff_target_project")
}

/// Emit a minimal `Cargo.toml` for a Buff project that has no
/// `buff.toml` manifest.
///
/// The output mirrors what [`config::generate_cargo_toml`] would emit
/// for an empty BuffConfig — just enough `[package]` + `[[bin]]` for
/// cargo to build the project. The package name is derived from the
/// entry file's stem (e.g. `main.buff` → `main`).
fn minimal_cargo_toml(entry_buff: &Path) -> String {
    let name = entry_buff
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("buff_project");
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [[bin]]\nname = \"{name}\"\npath = \"src/main.rs\"\n"
    )
}

/// Collect `extern crate "name"` declarations from a project's
/// flattened decls and render them as `[dependencies]` entries for
/// the generated `Cargo.toml`.
///
/// Used by the project pipeline to surface user-declared Rust crate
/// dependencies. Returns an empty string when the project has no
/// `extern crate` declarations (the common case).
#[allow(dead_code)]
pub fn render_extern_crates(project: &ParsedProject) -> String {
    let mut crates: BTreeSet<String> = BTreeSet::new();
    for decl in flatten_project(project) {
        if let Decl::ExternCrateDecl(extern_decl) = decl {
            crates.insert(extern_decl.name);
        }
    }
    if crates.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n[dependencies]\n");
    for name in crates {
        out.push_str(&format!("{name} = \"*\"\n"));
    }
    out
}

// ---------------------------------------------------------------------------
// Tests — inline so `cargo test -p buff-lang-cli` covers this module
// without a separate integration binary.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Build a unique-per-test temp project dir so parallel tests don't
    /// collide on `buff_target_project/`.
    fn temp_project_dir(unique: &str) -> PathBuf {
        let thread_id_str = format!("{:?}", std::thread::current().id());
        let thread_id_sanitised: String = thread_id_str
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let dir = std::env::temp_dir().join(format!(
            "buff-t1-project-pipeline-{}-{}-{}",
            std::process::id(),
            thread_id_sanitised,
            unique,
        ));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// Write a fixture Buff source file in `dir/src/<name>` and return
    /// the path. Best-effort cleanup via the test's RAII drop.
    fn write_buff_fixture(dir: &Path, name: &str, src: &str) -> PathBuf {
        let src_dir = dir.join("src");
        let _ = fs::create_dir_all(&src_dir);
        let path = src_dir.join(name);
        fs::write(&path, src).expect("write fixture");
        path
    }

    #[test]
    fn supported_targets_includes_initial_set() {
        // QA-spec target list — every triple in the spec is present.
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "wasm32-wasi",
        ] {
            assert!(
                is_supported_target(triple),
                "expected `{triple}` in SUPPORTED_TARGETS"
            );
        }
    }

    #[test]
    fn target_list_str_is_one_per_line() {
        let s = target_list_str();
        assert_eq!(s.lines().count(), SUPPORTED_TARGETS.len());
        assert!(s.contains("wasm32-wasi"));
    }

    #[test]
    fn target_list_keyword_is_reserved_string() {
        // No Rust target triple is named "list" — collision-free.
        assert_eq!(TARGET_LIST_KEYWORD, "list");
        assert!(!is_supported_target(TARGET_LIST_KEYWORD));
    }

    #[test]
    fn default_project_dir_walks_up_from_src() {
        // <proj>/src/main.buff -> <proj>/buff_target_project/
        let entry = PathBuf::from("/proj/src/main.buff");
        let dir = default_project_dir(&entry);
        assert!(dir.ends_with("buff_target_project"));
        assert!(
            dir.starts_with("/proj") || dir.starts_with("\\proj") || dir.starts_with("/proj"),
            "expected dir under /proj, got {}",
            dir.display()
        );
    }

    #[test]
    fn default_project_dir_when_no_src_parent() {
        // A loose .buff file (not in src/) — Cargo project goes alongside.
        let entry = PathBuf::from("/tmp/scratch.buff");
        let dir = default_project_dir(&entry);
        assert!(dir.ends_with("buff_target_project"));
    }

    #[test]
    fn minimal_cargo_toml_uses_entry_stem() {
        let toml = minimal_cargo_toml(&PathBuf::from("/proj/src/main.buff"));
        assert!(toml.contains("name = \"main\""), "missing name in:\n{toml}");
        assert!(toml.contains("[[bin]]"));
        assert!(toml.contains("path = \"src/main.rs\""));
    }

    #[test]
    fn flatten_project_strips_imports_and_unwraps_exports() {
        let dir = temp_project_dir("flatten");
        let math = write_buff_fixture(
            &dir,
            "math.buff",
            "export func add(a: Int, b: Int) -> Int:\n    return a + b\n",
        );
        let main = write_buff_fixture(
            &dir,
            "main.buff",
            "import { add } from \"./math.buff\"\nfunc main() -> Int:\n    return add(2, 3)\n",
        );
        let project = parse_project(&main).expect("parses");
        let flat = flatten_project(&project);
        // No ImportDecl + no ExportDecl after flatten.
        assert!(
            !flat.iter().any(|d| matches!(d, Decl::ImportDecl(_))),
            "imports should be stripped"
        );
        assert!(
            !flat.iter().any(|d| matches!(d, Decl::ExportDecl(_))),
            "exports should be unwrapped"
        );
        // math.buff's `add` is now top-level (FuncDecl), as is main's `main`.
        let names: Vec<&str> = flat
            .iter()
            .filter_map(|d| match d {
                Decl::FuncDecl(f) => Some(f.name.name.as_str()),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"add"), "add inlined: {:?}", names);
        assert!(names.contains(&"main"), "main kept: {:?}", names);
        // Topological order: math is emitted before main.
        let add_idx = names.iter().position(|n| *n == "add").unwrap();
        let main_idx = names.iter().position(|n| *n == "main").unwrap();
        assert!(add_idx < main_idx, "add before main in topo order");
        // math.buff's path is captured by parse_project's source_files.
        let _ = math; // suppress unused warning
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compile_project_to_cargo_writes_cargo_layout() {
        let dir = temp_project_dir("compile_cargo");
        let _math = write_buff_fixture(
            &dir,
            "math.buff",
            "export func add(a: Int, b: Int) -> Int:\n    return a + b\n",
        );
        let main = write_buff_fixture(
            &dir,
            "main.buff",
            "import { add } from \"./math.buff\"\nfunc main():\n    print(add(2, 3))\n",
        );
        let out_dir = dir.join("out_proj");
        let out = compile_project_to_cargo(&main, Some(&out_dir), None).expect("compiles");
        assert!(out.project_dir.exists());
        assert!(out.main_rs_path.exists());
        assert!(out.project_dir.join("Cargo.toml").exists());
        // The flattened Rust source contains both add and main fns.
        assert!(
            out.rust_source.contains("fn add"),
            "missing fn add in:\n{}",
            out.rust_source
        );
        assert!(
            out.rust_source.contains("fn main"),
            "missing fn main in:\n{}",
            out.rust_source
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compile_project_to_cargo_propagates_circular_import_error() {
        let dir = temp_project_dir("circular");
        let _a = write_buff_fixture(
            &dir,
            "a.buff",
            "import { something } from \"./b.buff\"\nfunc main() -> Int:\n    return 0\n",
        );
        let _b = write_buff_fixture(
            &dir,
            "b.buff",
            "import { other } from \"./a.buff\"\nexport func something() -> Int:\n    return 1\n",
        );
        let a = dir.join("src").join("a.buff");
        let err = compile_project_to_cargo(&a, Some(&dir.join("out")), None)
            .expect_err("cycle should error");
        let _msg = format!("{err}");
        assert!(
            format!("{err:#}").contains("circular"),
            "missing 'circular' in: {err:#}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compile_project_to_cargo_propagates_missing_import_error() {
        let dir = temp_project_dir("missing");
        let _math = write_buff_fixture(
            &dir,
            "math.buff",
            "export func add(a: Int, b: Int) -> Int:\n    return a + b\n",
        );
        let main = write_buff_fixture(
            &dir,
            "main.buff",
            "import { nonexistent } from \"./math.buff\"\nfunc main() -> Int:\n    return 0\n",
        );
        let err = compile_project_to_cargo(&main, Some(&dir.join("out")), None)
            .expect_err("missing import should error");
        assert!(
            format!("{err:#}").contains("nonexistent"),
            "missing 'nonexistent' in: {err:#}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cargo_build_project_target_list_returns_without_invoking_cargo() {
        // We can't actually invoke cargo in unit tests (no project on
        // disk + cargo may not be installed in CI sandboxes). But the
        // `--target list` shortcut returns before reaching cargo, so
        // it's safe to call with a non-existent project dir.
        let dir = temp_project_dir("target_list");
        cargo_build_project(
            &dir,
            CargoMode::Build,
            BuildMode::Debug,
            Some(TARGET_LIST_KEYWORD),
        )
        .expect("target list short-circuits");
        let _ = fs::remove_dir_all(&dir);
    }
}
