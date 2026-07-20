//! Module system — graph, resolution, cycle detection, visibility (T29).
//!
//! This module implements the REFACTOR target of T29: a **reusable analysis
//! pass** that takes a root `.buff` file (or an in-memory file set via the
//! [`ModuleLoader`] trait) and builds a [`ModuleGraph`] capturing every
//! transitively-imported module, its declared exports, and the import edges
//! between modules.
//!
//! # Pipeline
//!
//! 1. **Path resolution** ([`resolve_path`]): turn an import's string spec
//!    (e.g. `"./hello.buff"`, `"./utils/math"`) into a canonical absolute
//!    file path, resolved relative to the importing file's directory. The
//!    `.buff` extension is auto-appended when missing.
//! 2. **Loading** ([`ModuleLoader`]): the trait abstracts file fetching so
//!    unit tests can supply in-memory file sets without touching disk; the
//!    production code uses [`FsLoader`].
//! 3. **Parsing**: each loaded source is lexed+parsed via the upstream
//!    `buff-lang-lexer` + `buff-lang-parser`. Imports, exports, and
//!    re-exports are extracted from the resulting `Vec<Decl>`.
//! 4. **Cycle detection** (DFS visiting-stack): while walking the import
//!    graph depth-first, a module re-encountered on the current DFS stack
//!    is a circular import → error with a chain like
//!    `"circular import detected: a.buff -> b.buff -> a.buff"`.
//! 5. **Topological sort**: with no cycles, modules are emitted in
//!    dependency order (deps before importers) so downstream codegen can
//!    walk the graph once.
//! 6. **Visibility check**: each named import is verified against the
//!    target module's public exports set — importing a private (non-
//!    `export`ed) symbol is rejected with `"X is not exported from ./mod"`.
//!    Wildcard re-exports (`export * from "..."`) flatten the target's
//!    exports into the re-exporter's.
//!
//! # Limitations (v0.5)
//!
//! - **`std/...` paths**: reserved for the future standard library; v0.5
//!   returns a clear error so users know it isn't a missing file.
//! - **Multi-file codegen**: this module computes the graph; the actual
//!   Rust codegen linking of multiple `.buff` files is a separate (later)
//!   wave. The graph + visibility + cycle checks are testable on their own.
//! - **Default imports**: `import name from "..."` is parsed but, for the
//!   graph's purposes, treated like a named import of `name`. The codegen
//!   wiring of `default` will arrive with multi-file codegen.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use buff_lang_ast::{Decl, ImportDecl, ReexportDecl};
use buff_lang_error::{Diagnostic, ErrorCode, Span, TypeError};

/// A parsed module: its canonical file path, top-level decls, computed
/// exports set, declared imports, and declared re-exports.
#[derive(Debug, Clone)]
pub struct Module {
    /// Canonical absolute path of this module's source file.
    pub path: PathBuf,
    /// Top-level declarations (functions, enums, exports, imports, ...).
    pub decls: Vec<Decl>,
    /// Names of PUBLIC symbols — declared via `export <decl>` (the inner
    /// decl's name) or filled in by resolving `export * from "..."` / named
    /// re-exports. Populated by [`build_graph`].
    pub exports: HashSet<String>,
    /// All `import` declarations in this module. Kept for visibility
    /// checking and downstream codegen.
    pub imports: Vec<ImportDecl>,
    /// All `export * from` / `export { names } from` declarations. The
    /// `from` paths are still string specs (not resolved) — use the graph
    /// edges to find resolved targets.
    pub reexports: Vec<ReexportDecl>,
}

/// The module graph built by [`build_graph`].
///
/// The graph is acyclic — cycles are reported as errors during construction
/// and never appear in a successfully-built graph.
#[derive(Debug, Clone)]
pub struct ModuleGraph {
    /// Canonical module path → module entry. HashMap (unordered); use
    /// [`Self::topo_order`] for dependency-ordered iteration.
    pub modules: HashMap<PathBuf, Module>,
    /// Modules in topological order: dependencies appear before importers.
    /// Useful for codegen (emit each module's items in order; downstream
    /// symbols are always already defined when referenced).
    pub topo_order: Vec<PathBuf>,
    /// The canonical path of the root module (the entry point passed to
    /// [`build_graph`]).
    pub root: PathBuf,
}

impl ModuleGraph {
    /// Fetch a module by canonical path. Returns `None` if not in the graph.
    pub fn get(&self, path: &Path) -> Option<&Module> {
        self.modules.get(path)
    }

    /// Iterate modules in topological (deps-first) order.
    pub fn iter_topo(&self) -> impl Iterator<Item = &Module> {
        self.topo_order.iter().filter_map(|p| self.modules.get(p))
    }
}

/// Abstract file-fetching interface so the graph builder can run against
/// either the real filesystem ([`FsLoader`]) or an in-memory file set
/// ([`MemoryLoader`] in tests).
pub trait ModuleLoader {
    /// Return the source text of `path`, or `None` if the file doesn't
    /// exist. The graph builder surfaces a "file not found" diagnostic
    /// when an import's resolved path yields `None` here.
    fn load(&self, path: &Path) -> Option<String>;
}

/// Filesystem-backed loader — the production default.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsLoader;

impl ModuleLoader for FsLoader {
    fn load(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
}

/// In-memory loader — useful for tests and snapshot fixtures without disk I/O.
///
/// Keys are paths; the lookup is a direct `get` (no canonicalization — tests
/// are expected to pre-canonicalize keys or use exact path strings).
#[derive(Debug, Clone, Default)]
pub struct MemoryLoader {
    /// Map of canonical path → source text.
    pub files: HashMap<PathBuf, String>,
}

impl MemoryLoader {
    /// Create an empty loader.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a file. The `path` is stored verbatim — pass canonical paths
    /// if you need cycle detection (which is path-equality based).
    pub fn insert<P: Into<PathBuf>>(&mut self, path: P, src: &str) -> &mut Self {
        self.files.insert(path.into(), src.to_string());
        self
    }
}

impl ModuleLoader for MemoryLoader {
    fn load(&self, path: &Path) -> Option<String> {
        self.files.get(path).cloned()
    }
}

// ---------------------------------------------------------------------------
// Path resolution.
// ---------------------------------------------------------------------------

/// Resolve an import spec (`"./hello.buff"`, `"./utils/math"`) to a canonical
/// absolute file path, relative to `importing` (the file that contains the
/// import).
///
/// # Rules
///
/// - `./foo.buff`     → `importing_dir/foo.buff`
/// - `./foo`          → `importing_dir/foo.buff` (auto-append `.buff`)
/// - `./utils/math`   → `importing_dir/utils/math.buff`
/// - `../sibling.buff`→ `importing_dir/../sibling.buff` (parent dir)
/// - `foo.buff` (no `./`) → TREATED AS RELATIVE for v0.5 ergonomics. (Rust
///   and most languages require explicit `./`; Buff is forgiving.)
/// - `std/...` or any path starting with `std/` → reserved for the future
///   standard library; v0.5 returns a clear "std not yet supported" error.
/// - Absolute paths are returned canonicalized as-is.
///
/// The result is canonicalized (lexically via `std::fs::canonicalize` when
/// the file exists; otherwise via best-effort parent-join) so cycle
/// detection on equal canonical paths is robust across symlinks and
/// relative-spec variations.
///
/// # Errors
///
/// Returns [`TypeError`] with a `"std not yet supported"` message for
/// std-prefixed specs. (Other malformed specs — empty, etc. — surface as
/// a generic resolution error from the graph builder, not here.)
pub fn resolve_path(importing: &Path, spec: &str) -> Result<PathBuf, TypeError> {
    // Reserved std-library namespace.
    let normalized = spec.trim();
    if normalized.is_empty() {
        return Err(TypeError::new(
            Diagnostic::error("import path is empty", Span::dummy())
                .with_code(ErrorCode::ModuleError),
        ));
    }
    if normalized == "std" || normalized.starts_with("std/") || normalized.starts_with("std\\") {
        return Err(TypeError::new(
            Diagnostic::error(
                format!("standard-library import `{normalized}` is not yet supported in v0.5"),
                Span::dummy(),
            )
            .with_code(ErrorCode::ModuleError),
        ));
    }

    // Anchor the spec relative to the importing file's directory.
    let spec_path = Path::new(normalized);
    let resolved = if spec_path.is_absolute() {
        spec_path.to_path_buf()
    } else {
        let base = importing
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(spec_path)
    };

    // Auto-append `.buff` if the spec had no extension.
    let with_ext = match resolved.extension() {
        Some(_) => resolved,
        None => {
            let mut p = resolved.clone();
            p.set_extension("buff");
            p
        }
    };

    // Best-effort canonicalization. If the file exists on disk (FsLoader
    // path), use real canonicalize for symlink robustness; otherwise just
    // normalize `..`/`.` segments lexically so in-memory tests work.
    if with_ext.exists() {
        if let Ok(canon) = std::fs::canonicalize(&with_ext) {
            return Ok(canon);
        }
    }
    Ok(lexical_canonicalize(&with_ext))
}

/// Lexically strip `.` and `..` segments from a path. Used as a fallback
/// when `std::fs::canonicalize` fails (e.g. for in-memory test paths).
fn lexical_canonicalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out: Vec<Component<'_>> = Vec::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => { /* skip `.` */ }
            Component::ParentDir => {
                // Pop the last Normal segment if possible; otherwise keep `..`.
                match out.last() {
                    Some(Component::Normal(_)) => {
                        out.pop();
                    }
                    _ => out.push(comp),
                }
            }
            other => out.push(other),
        }
    }
    let mut ret = PathBuf::new();
    for comp in out {
        ret.push(comp.as_os_str());
    }
    if ret.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        ret
    }
}

// ---------------------------------------------------------------------------
// Module graph construction.
// ---------------------------------------------------------------------------

/// Build a [`ModuleGraph`] rooted at `root`, loading each transitively-
/// imported module via `loader`.
///
/// Pipeline per module: load → lex+parse → extract imports/exports/reexports
/// → recurse on each import's resolved target → finalize exports (incl.
/// flattening `export * from` re-exports).
///
/// # Errors
///
/// - `file not found: <path>` — when the loader returns `None` for a
///   resolved import path.
/// - `circular import detected: <chain>` — when a module reappears on the
///   active DFS stack.
/// - `lex error` / `parse error` — propagated from the upstream passes,
///   reformatted into a [`TypeError`].
/// - `<sym> is not exported from <mod>` — when a named import references a
///   symbol the target module doesn't export (post-reexport resolution).
pub fn build_graph(root: &Path, loader: &dyn ModuleLoader) -> Result<ModuleGraph, TypeError> {
    // Canonicalize the root path lexically (strip `.`/`..` segments). We
    // intentionally do NOT prepend the current working directory — callers
    // (FsLoader users like the CLI, or MemoryLoader test harnesses) are
    // expected to pass a path that matches their loader's keys. On-disk
    // canonicalization happens lazily inside `resolve_path` for each import.
    let root_canon = lexical_canonicalize(root);

    let mut ctx = BuildCtx {
        loader,
        modules: HashMap::new(),
        stack: Vec::new(),
        visited: HashSet::new(),
        topo_order: Vec::new(),
    };
    let root_canon = process_module(&root_canon, &mut ctx)?;

    // Phase 2: resolve re-exports (`export * from` flattening + named
    // `export { x } from` validation). Done after all modules are parsed
    // so forward chains (A re-exports from B re-exports from C) work.
    resolve_reexports(&mut ctx)?;

    // Phase 3: visibility check — every named import must resolve to an
    // exported symbol in its target module.
    check_visibility(&ctx)?;

    Ok(ModuleGraph {
        modules: ctx.modules,
        topo_order: ctx.topo_order,
        root: root_canon,
    })
}

/// Internal DFS context.
struct BuildCtx<'a> {
    loader: &'a dyn ModuleLoader,
    modules: HashMap<PathBuf, Module>,
    /// DFS visiting stack (for cycle detection).
    stack: Vec<PathBuf>,
    /// Modules already fully processed (post-order).
    visited: HashSet<PathBuf>,
    /// Topological order built in post-order: deps pushed before importers.
    topo_order: Vec<PathBuf>,
}

/// Process one module: load → parse → record imports/exports → recurse
/// on imports → finalize and store. Detects cycles via `stack`.
fn process_module(path: &Path, ctx: &mut BuildCtx<'_>) -> Result<PathBuf, TypeError> {
    // Cycle: this module is already on the active DFS stack.
    if ctx.stack.iter().any(|p| p == path) {
        let chain = ctx
            .stack
            .iter()
            .map(|p| p.display().to_string())
            .chain(std::iter::once(path.display().to_string()))
            .collect::<Vec<_>>()
            .join(" -> ");
        return Err(TypeError::new(
            Diagnostic::error(format!("circular import detected: {chain}"), Span::dummy())
                .with_code(ErrorCode::ModuleError),
        ));
    }
    // Already done — nothing to do.
    if ctx.visited.contains(path) {
        return Ok(path.to_path_buf());
    }

    // Load.
    let src = ctx.loader.load(path).ok_or_else(|| {
        TypeError::new(
            Diagnostic::error(format!("file not found: {}", path.display()), Span::dummy())
                .with_code(ErrorCode::ModuleError),
        )
    })?;

    // Parse via upstream lexer + parser. We use SourceId(0) for every
    // module; spans inside the graph don't drive diagnostics yet (the
    // CLI's source-map layer will thread real SourceIds in a later wave).
    let source_id = buff_lang_error::SourceId(0);
    let tokens = buff_lang_lexer::tokenize(&src, source_id).map_err(|e| {
        TypeError::new(
            Diagnostic::error(
                format!(
                    "lex error in {}: {}",
                    path.display(),
                    e.inner.diagnostic.message
                ),
                Span::dummy(),
            )
            .with_code(ErrorCode::ModuleError),
        )
    })?;
    let decls = buff_lang_parser::parse(&tokens, source_id).map_err(|e| {
        TypeError::new(
            Diagnostic::error(
                format!(
                    "parse error in {}: {}",
                    path.display(),
                    e.diagnostic.message
                ),
                Span::dummy(),
            )
            .with_code(ErrorCode::ModuleError),
        )
    })?;

    // Categorize decls into imports / reexports / regular (with exports).
    let mut imports: Vec<ImportDecl> = Vec::new();
    let mut reexports: Vec<ReexportDecl> = Vec::new();
    let mut exports: HashSet<String> = HashSet::new();
    for decl in &decls {
        match decl {
            Decl::ImportDecl(imp) => imports.push(imp.clone()),
            Decl::ReexportDecl(r) => {
                // Named re-exports contribute to THIS module's exports
                // immediately (the names are locally declared). Wildcard
                // re-exports need the target's exports — deferred to
                // `resolve_reexports`.
                if !r.wildcard {
                    for n in &r.names {
                        exports.insert(n.name.clone());
                    }
                }
                reexports.push(r.clone());
            }
            Decl::ExportDecl(e) => {
                if let Some(name) = decl_item_name(&e.inner) {
                    exports.insert(name);
                }
            }
            _ => { /* module-private decl — no export */ }
        }
    }

    // Push on stack, recurse on imports, then pop.
    ctx.stack.push(path.to_path_buf());

    // Recurse on each import's target. We need a temp module entry while
    // recursing so the recursion can detect this module on the stack —
    // but cycle detection uses the STACK (not modules map), so we can
    // defer insertion until after recursion completes.
    let mut resolved_import_targets: Vec<(PathBuf, ImportDecl)> = Vec::new();
    for imp in &imports {
        if let Some(spec) = &imp.from_path {
            let target = resolve_path(path, spec)?;
            process_module(&target, ctx)?;
            resolved_import_targets.push((target, imp.clone()));
        }
        // Legacy dotted-path imports (`import a.b.c`) are not part of the
        // v0.5 module system — they're an unused placeholder. Skip.
    }

    // Recurse on re-export targets too (they create real edges).
    for r in &reexports {
        if r.from.is_empty() {
            continue;
        }
        let target = resolve_path(path, &r.from)?;
        process_module(&target, ctx)?;
    }

    ctx.stack.pop();
    ctx.visited.insert(path.to_path_buf());
    ctx.topo_order.push(path.to_path_buf());

    // Insert the parsed module entry.
    ctx.modules.insert(
        path.to_path_buf(),
        Module {
            path: path.to_path_buf(),
            decls,
            exports,
            imports,
            reexports,
        },
    );

    Ok(path.to_path_buf())
}

/// Resolve `export * from "..."` re-exports by flattening target exports
/// into re-exporters' exports sets. Verifies that named re-exports
/// (`export { x } from "..."`) reference real exported symbols.
///
/// # Determinism (T29 fix)
///
/// The resolution pass MUST iterate modules in **topological order** (deps
/// before importers). A re-export chain `a → export * from b → export *
/// from c → export deep` only propagates `deep` all the way to `a` if `c`
/// is processed before `b` and `b` before `a`. Iterating `modules.values()`
/// directly would yield a HashMap order that does NOT guarantee this —
/// producing non-deterministic output (the test
/// `types_modules_export_star_chain` was flaky for exactly this reason).
///
/// [`BuildCtx::topo_order`] is computed by `process_module` as a post-order
/// DFS: dependencies are pushed before their importers. Walking it in order
/// guarantees every module's target is finalized before the module's own
/// re-exports are flattened — a single pass suffices, no fixed-point loop
/// required.
fn resolve_reexports(ctx: &mut BuildCtx<'_>) -> Result<(), TypeError> {
    // Snapshot the topological order + each module's re-export decls so we
    // don't hold a borrow across the mutable updates below.
    let topo: Vec<PathBuf> = ctx.topo_order.clone();
    let reexports_per_mod: HashMap<PathBuf, Vec<ReexportDecl>> = ctx
        .modules
        .iter()
        .map(|(p, m)| (p.clone(), m.reexports.clone()))
        .collect();

    for mod_path in &topo {
        let Some(reexports) = reexports_per_mod.get(mod_path) else {
            continue;
        };
        for r in reexports {
            if r.from.is_empty() {
                // `export { names }` without `from` — names must be local
                // decls already in exports (inserted above); nothing more.
                continue;
            }
            let target = resolve_path(mod_path, &r.from)?;
            let target_exports: HashSet<String> = ctx
                .modules
                .get(&target)
                .map(|m| m.exports.clone())
                .unwrap_or_default();
            if r.wildcard {
                // Flatten ALL of the target's exports into this module.
                if let Some(m) = ctx.modules.get_mut(mod_path) {
                    for name in &target_exports {
                        m.exports.insert(name.clone());
                    }
                }
            } else {
                // Named re-export: each name must be in target's exports.
                for n in &r.names {
                    if !target_exports.contains(&n.name) {
                        return Err(TypeError::new(
                            Diagnostic::error(
                                format!("`{}` is not exported from `{}`", n.name, target.display()),
                                Span::dummy(),
                            )
                            .with_code(ErrorCode::ModuleError),
                        ));
                    }
                }
                // Already inserted into this module's exports during the
                // initial pass (in process_module).
            }
        }
    }
    Ok(())
}

/// Visibility check: every named import must reference a symbol exported
/// by its target module. Wildcard imports (`import * from "..."`) are
/// always "visible" — the wildcard imports everything, so a missing
/// symbol would surface at use-site (deferred to a later name-resolution
/// pass).
fn check_visibility(ctx: &BuildCtx<'_>) -> Result<(), TypeError> {
    for m in ctx.modules.values() {
        for imp in &m.imports {
            let Some(spec) = &imp.from_path else {
                continue;
            };
            if imp.wildcard {
                continue;
            }
            let target = resolve_path(&m.path, spec)?;
            let Some(target_mod) = ctx.modules.get(&target) else {
                // Shouldn't happen — process_module recurses into all
                // imports — but defend against a missing entry.
                return Err(TypeError::new(
                    Diagnostic::error(
                        format!(
                            "import target `{}` not in graph (internal error)",
                            target.display()
                        ),
                        Span::dummy(),
                    )
                    .with_code(ErrorCode::ModuleError),
                ));
            };
            for n in &imp.imports {
                if !target_mod.exports.contains(&n.name) {
                    return Err(TypeError::new(
                        Diagnostic::error(
                            format!("`{}` is not exported from `{}`", n.name, target.display()),
                            Span::dummy(),
                        )
                        .with_code(ErrorCode::ModuleError),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Extract the declared-name string of a top-level item decl (the kind
/// that may be wrapped by `export`). Returns `None` for decls without a
/// name (ModuleDecl, ImportDecl, ReexportDecl, nested ExportDecl).
fn decl_item_name(decl: &Decl) -> Option<String> {
    match decl {
        Decl::FuncDecl(f) => Some(f.name.name.clone()),
        Decl::StructDecl(s) => Some(s.name.name.clone()),
        Decl::EnumDecl(e) => Some(e.name.name.clone()),
        Decl::TraitDecl(t) => Some(t.name.name.clone()),
        Decl::ExportDecl(e) => decl_item_name(&e.inner),
        Decl::ImportDecl(_)
        | Decl::ModuleDecl(_)
        | Decl::ReexportDecl(_)
        | Decl::ExternCrateDecl(_)
        | Decl::ExternFuncDecl(_)
        | Decl::ExtendBlock(_) => None,
    }
}

// ---------------------------------------------------------------------------
// Unit tests (the bulk of the test suite lives in tests/modules.rs).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn resolve_relative_with_extension() {
        let importing = p("/proj/src/main.buff");
        let r = resolve_path(&importing, "./hello.buff").unwrap();
        assert!(r.ends_with("hello.buff"));
    }

    #[test]
    fn resolve_relative_auto_append_extension() {
        let importing = p("/proj/src/main.buff");
        let r = resolve_path(&importing, "./hello").unwrap();
        assert!(r.ends_with("hello.buff"));
    }

    #[test]
    fn resolve_subdir_path() {
        let importing = p("/proj/src/main.buff");
        let r = resolve_path(&importing, "./utils/math.buff").unwrap();
        assert!(r.ends_with("utils/math.buff") || r.ends_with(r"utils\math.buff"));
    }

    #[test]
    fn resolve_std_returns_error() {
        let importing = p("/proj/src/main.buff");
        let err = resolve_path(&importing, "std/io").unwrap_err();
        assert!(err.diagnostic.message.contains("std"));
        assert!(err.diagnostic.message.contains("not yet supported"));
    }

    #[test]
    fn resolve_parent_dir() {
        let importing = p("/proj/src/main.buff");
        let r = resolve_path(&importing, "../sibling.buff").unwrap();
        // Lexically canonical: /proj/sibling.buff
        assert!(r.ends_with("sibling.buff"));
    }

    #[test]
    fn lexical_canonicalize_strips_dotdot() {
        let p = Path::new("/a/b/../c/./d");
        let c = lexical_canonicalize(p);
        assert!(c.ends_with("a/c/d") || c.ends_with(r"a\c\d"));
    }

    #[test]
    fn memory_loader_round_trip() {
        let mut loader = MemoryLoader::new();
        loader.insert("/x.buff", "func f() { }");
        assert_eq!(loader.load(&p("/x.buff")).as_deref(), Some("func f() { }"));
    }
}
