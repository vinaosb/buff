//! `buff doc` — rustdoc-quality HTML API docs (T56).
//!
//! Replaces the v1.13 placeholder. The generator:
//!
//! 1. Reads `buff.toml` from the target directory for `[package].name`
//!    (single-package mode) or walks `[workspace].members` (workspace mode).
//! 2. Walks `src/` for `.buff` files (recursive) and parses each via the
//!    standard lex + parse pipeline.
//! 3. Extracts `///` doc comments attached to top-level declarations
//!    (`func` / `struct` / `enum` / `trait`). Doc comments are located by
//!    scanning the raw source: the maximal contiguous run of `///` lines
//!    immediately preceding a declaration (skipping any leading `@attr`
//!    attribute lines that belong to the decl). The Buff lexer strips
//!    comments before tokenisation, so the doc text is recovered from the
//!    source bytes rather than the token stream.
//! 4. Emits an HTML documentation tree:
//!    - one page per `.buff` module with rendered signatures + doc text +
//!      cross-references between user-defined type names,
//!    - a per-package `index.html` listing every documented module,
//!    - a top-level workspace `index.html` linking to each package,
//!    - a `search-index.json` describing every documented item (name +
//!      kind + description + URL) for client-side search.
//! 5. `--open` launches the generated top-level `index.html` in the
//!    default browser (mirrors `cargo doc --open`).
//!
//! The output is pure HTML + inline CSS — no JS framework, no external
//! tools, no dependency on rustdoc. `buff doc` is best-effort: lex/parse
//! errors on a single file are skipped (that's `buff check`'s job), never
//! fatal to the whole run.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use buff_lang_ast::{Decl, EnumDecl, FuncDecl, StructDecl, TraitDecl, TypeParam};
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

use crate::config::BuffConfig;

/// Entry point for `buff doc`.
///
/// Reads `buff.toml` from `dir`, walks `src/` for `.buff` files, parses each,
/// extracts `///` doc comments, and emits HTML to `output` (default `doc/`).
/// Idempotent: re-running overwrites the previous output (matches `cargo doc`).
/// When `open` is set, launches the generated top-level `index.html` in the
/// default browser.
pub fn run(dir: &Path, output: Option<&Path>, open: bool) -> Result<()> {
    let manifest_path = dir.join("buff.toml");
    let cfg = BuffConfig::load_from_file(&manifest_path)
        .with_context(|| format!("failed to read manifest at {}", manifest_path.display()))?;

    let docs_root: PathBuf = match output {
        Some(o) => {
            // An explicit `--output` may be relative (to CWD) or absolute.
            let p = PathBuf::from(o);
            if p.is_absolute() {
                p
            } else {
                dir.join(o)
            }
        }
        None => dir.join("doc"),
    };
    fs::create_dir_all(&docs_root)
        .with_context(|| format!("failed to create {}", docs_root.display()))?;

    // Single-package vs workspace mode. Each entry is (package name, package dir).
    let packages: Vec<(String, PathBuf)> = if let Some(p) = &cfg.package {
        vec![(p.name.clone(), dir.to_path_buf())]
    } else if let Some(ws) = &cfg.workspace {
        ws.members
            .iter()
            .filter_map(|m| {
                let member_dir = dir.join(m);
                let member_manifest = member_dir.join("buff.toml");
                BuffConfig::load_from_file(&member_manifest)
                    .ok()
                    .and_then(|c| c.package.map(|p| (p.name, member_dir)))
            })
            .collect()
    } else {
        return Err(anyhow::anyhow!(
            "buff.toml has neither [package] nor [workspace] — nothing to document"
        ));
    };

    // Phase 1: parse every package into structured docs.
    let mut package_docs: Vec<PackageDoc> = Vec::with_capacity(packages.len());
    for (pkg_name, pkg_dir) in &packages {
        let modules = collect_package_modules(pkg_dir);
        package_docs.push(PackageDoc {
            name: pkg_name.clone(),
            modules,
        });
    }

    // Phase 2: build a global symbol index (type name -> URL) for
    // cross-references. First definition wins on duplicate names.
    let symbols = build_symbol_index(&package_docs);

    // Phase 3: render. One subdirectory per package; pages flat inside it.
    let mut search_entries: Vec<SearchEntry> = Vec::new();
    let mut package_index_links: Vec<(String, String)> = Vec::new();

    for pkg in &package_docs {
        let pkg_dir = docs_root.join(&pkg.name);
        fs::create_dir_all(&pkg_dir)
            .with_context(|| format!("failed to create {}", pkg_dir.display()))?;

        for module in &pkg.modules {
            let html = render_module_html(&pkg.name, module, &pkg.modules, &symbols);
            let out = pkg_dir.join(&module.page);
            fs::write(&out, html).with_context(|| format!("failed to write {}", out.display()))?;

            for item in &module.items {
                search_entries.push(SearchEntry {
                    name: item.name.clone(),
                    kind: item.kind.label().to_string(),
                    description: first_doc_paragraph(&item.doc),
                    package: pkg.name.clone(),
                    module: module.rel.clone(),
                    url: format!("{}/{}#{}", pkg.name, module.page, item.anchor),
                });
            }
        }

        // Per-package index.
        let pkg_index_html = render_package_index(pkg);
        let pkg_index_path = pkg_dir.join("index.html");
        fs::write(&pkg_index_path, pkg_index_html)
            .with_context(|| format!("failed to write {}", pkg_index_path.display()))?;
        package_index_links.push((pkg.name.clone(), format!("{}/index.html", pkg.name)));
    }

    // Top-level workspace index linking to each package page.
    let index_html = render_workspace_index(&package_index_links, &package_docs);
    let index_path = docs_root.join("index.html");
    fs::write(&index_path, index_html)
        .with_context(|| format!("failed to write {}", index_path.display()))?;

    // Search index (JSON) at the docs root.
    let search_json = render_search_json(&search_entries);
    let search_path = docs_root.join("search-index.json");
    fs::write(&search_path, search_json)
        .with_context(|| format!("failed to write {}", search_path.display()))?;

    let total_items: usize = package_docs.iter().map(|p| p.item_count()).sum();
    let total_modules: usize = package_docs.iter().map(|p| p.modules.len()).sum();
    eprintln!(
        "Generated API docs for {} package(s), {} module(s), {} documented item(s) under {}",
        package_docs.len(),
        total_modules,
        total_items,
        docs_root.display()
    );
    eprintln!("Open {}/index.html to browse.", docs_root.display());

    if open {
        if let Err(e) = open_in_browser(&index_path) {
            eprintln!("warning: failed to open browser: {e}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// Documentation for a single package (one entry per `[package]`).
struct PackageDoc {
    name: String,
    modules: Vec<ModuleDoc>,
}

impl PackageDoc {
    fn item_count(&self) -> usize {
        self.modules.iter().map(|m| m.items.len()).sum()
    }
}

/// Documentation extracted from a single `.buff` source file.
struct ModuleDoc {
    /// Human-readable relative path (e.g. `math/vector.buff`).
    rel: String,
    /// Flat page filename inside the package docs dir (e.g. `vector.html`).
    page: String,
    /// Documented top-level items, in source order.
    items: Vec<DocItem>,
}

/// A single documented declaration.
struct DocItem {
    kind: ItemKind,
    name: String,
    is_pub: bool,
    /// Rendered Buff signature (e.g. `func greet(name: String) -> String`).
    signature: String,
    /// Joined `///` doc text (raw, unescaped — escaping happens at render).
    doc: String,
    /// HTML anchor id on the module page (e.g. `func-greet`).
    anchor: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    Function,
    Struct,
    Enum,
    Trait,
}

impl ItemKind {
    fn label(self) -> &'static str {
        match self {
            ItemKind::Function => "function",
            ItemKind::Struct => "struct",
            ItemKind::Enum => "enum",
            ItemKind::Trait => "trait",
        }
    }

    fn badge(self) -> &'static str {
        match self {
            ItemKind::Function => "fn",
            ItemKind::Struct => "struct",
            ItemKind::Enum => "enum",
            ItemKind::Trait => "trait",
        }
    }
}

/// One row of the generated `search-index.json`.
struct SearchEntry {
    name: String,
    kind: String,
    description: String,
    package: String,
    module: String,
    url: String,
}

// ---------------------------------------------------------------------------
// Parsing: .buff source -> ModuleDoc
// ---------------------------------------------------------------------------

/// Walk `src/` under `pkg_dir`, parse every `.buff` file, return its docs.
fn collect_package_modules(pkg_dir: &Path) -> Vec<ModuleDoc> {
    let src_dir = pkg_dir.join("src");
    let mut files: Vec<PathBuf> = Vec::new();
    if src_dir.is_dir() {
        walk_buff_files(&src_dir, &mut files);
    }
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let rel = path
                .strip_prefix(&src_dir)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| path.display().to_string());
            parse_module(&path, &rel)
        })
        .collect()
}

/// Recursive walker: collect every `.buff` file path under `root`.
fn walk_buff_files(current: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_buff_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "buff") {
            out.push(path);
        }
    }
}

/// Parse a single `.buff` file into a [`ModuleDoc`]. Returns `None` on read,
/// lex, or parse failure — `buff doc` is best-effort and never surfaces
/// compile errors (that's `buff check`'s job).
fn parse_module(path: &Path, rel: &str) -> Option<ModuleDoc> {
    let src = fs::read_to_string(path).ok()?;
    let tokens = tokenize(&src, buff_lang_error::SourceId(0)).ok()?;
    let decls = parse(&tokens, buff_lang_error::SourceId(0)).ok()?;
    let lines = LineTable::new(&src);
    let items = extract_items(&decls, &src, &lines);
    let page = page_name_for(rel);
    Some(ModuleDoc {
        rel: rel.to_string(),
        page,
        items,
    })
}

/// Convert a relative source path into a flat HTML page filename.
/// `math/vector.buff` -> `math_vector.html`. Path separators become `_`.
fn page_name_for(rel: &str) -> String {
    let stem = rel.strip_suffix(".buff").unwrap_or(rel);
    let flat: String = stem
        .chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect();
    format!("{flat}.html")
}

/// Walk the top-level declarations of a file and produce [`DocItem`]s for
/// each documented kind (`func` / `struct` / `enum` / `trait`). `export`
/// wrappers are unwrapped and the inner item is marked public.
fn extract_items(decls: &[Decl], src: &str, lines: &LineTable) -> Vec<DocItem> {
    let mut out = Vec::new();
    for decl in decls {
        match decl {
            Decl::ExportDecl(export) => {
                // Unwrap the export and document the inner item as public.
                if let Some(item) = item_from_decl(&export.inner, src, lines, true) {
                    out.push(item);
                }
            }
            other => {
                if let Some(item) = item_from_decl(other, src, lines, false) {
                    out.push(item);
                }
            }
        }
    }
    out
}

/// Build a [`DocItem`] from a declaration node (public if `is_pub`).
/// Returns `None` for non-documented decl kinds (imports, modules, etc.).
fn item_from_decl(decl: &Decl, src: &str, lines: &LineTable, is_pub: bool) -> Option<DocItem> {
    let (kind, name, signature, span) = match decl {
        Decl::FuncDecl(f) => (
            ItemKind::Function,
            f.name.name.clone(),
            render_func_sig(f),
            f.span,
        ),
        Decl::StructDecl(s) => (
            ItemKind::Struct,
            s.name.name.clone(),
            render_struct_sig(s),
            s.span,
        ),
        Decl::EnumDecl(e) => (
            ItemKind::Enum,
            e.name.name.clone(),
            render_enum_sig(e),
            e.span,
        ),
        Decl::TraitDecl(t) => (
            ItemKind::Trait,
            t.name.name.clone(),
            render_trait_sig(t),
            t.span,
        ),
        // Non-documented top-level kinds.
        Decl::ImportDecl(_)
        | Decl::ModuleDecl(_)
        | Decl::ReexportDecl(_)
        | Decl::ExternCrateDecl(_)
        | Decl::ExternFuncDecl(_)
        | Decl::ExtendBlock(_)
        | Decl::ExportDecl(_)
        | Decl::ImplBlock(_) => return None,
    };
    let anchor = format!("{}-{}", kind.label(), name);
    let doc = doc_comment_for(src, lines, span.start);
    Some(DocItem {
        kind,
        name,
        is_pub,
        signature,
        doc,
        anchor,
    })
}

/// Extract the `///` doc-comment block immediately preceding the declaration
/// that starts at byte `decl_start`.
///
/// Walks UP from the line before the decl's first line, collecting contiguous
/// `///` lines. Attribute lines (`@name ...`) that belong to the decl are
/// skipped (so `doc / @attr / func` still attaches the doc). The first line
/// that is neither a `///` doc line nor an `@attr` line stops the search.
///
/// Each collected doc line has its leading `///` (and one optional space)
/// stripped; lines are joined with `\n`.
fn doc_comment_for(src: &str, lines: &LineTable, decl_start: usize) -> String {
    let decl_line = lines.line_of(decl_start);
    let mut collected: Vec<String> = Vec::new();
    let mut i = decl_line;
    while i > 0 {
        i -= 1;
        let text = lines.line_text(src, i);
        let trimmed = text.trim_start();
        if let Some(rest) = trimmed.strip_prefix("///") {
            // Strip exactly one leading space if present (the common
            // `/// text` form). A bare `///` yields an empty line.
            let body = rest.strip_prefix(' ').unwrap_or(rest);
            collected.push(body.to_string());
        } else if trimmed.starts_with('@') {
            // Attribute line belonging to this decl — skip, keep climbing.
            continue;
        } else {
            break;
        }
    }
    collected.reverse();
    collected.join("\n")
}

// ---------------------------------------------------------------------------
// Signature rendering (Buff surface syntax, not the Debug Display)
// ---------------------------------------------------------------------------

fn render_type_params(tps: &[TypeParam]) -> String {
    if tps.is_empty() {
        return String::new();
    }
    let inner: Vec<String> = tps
        .iter()
        .map(|tp| {
            if tp.bounds.is_empty() {
                tp.name.name.clone()
            } else {
                let bounds: Vec<String> = tp.bounds.iter().map(|b| b.to_string()).collect();
                format!("{}: {}", tp.name.name, bounds.join(" + "))
            }
        })
        .collect();
    format!("<{}>", inner.join(", "))
}

fn render_func_sig(f: &FuncDecl) -> String {
    let mut s = String::new();
    for a in &f.attributes {
        s.push_str(&format!("{a} "));
    }
    if f.is_extern {
        s.push_str("extern ");
    }
    if f.is_async {
        s.push_str("async ");
    }
    if f.is_unsafe {
        s.push_str("unsafe ");
    }
    s.push_str("func ");
    s.push_str(&f.name.name);
    s.push_str(&render_type_params(&f.type_params));
    s.push('(');
    let params: Vec<String> = f.params.iter().map(|p| p.to_string()).collect();
    s.push_str(&params.join(", "));
    s.push(')');
    if let Some(rt) = &f.return_type {
        s.push_str(&format!(" -> {rt}"));
    }
    s
}

fn render_struct_sig(s: &StructDecl) -> String {
    let mut out = format!(
        "struct {}{}",
        s.name.name,
        render_type_params(&s.type_params)
    );
    if !s.traits.is_empty() {
        out.push_str(": ");
        out.push_str(
            &s.traits
                .iter()
                .map(|t| t.name.clone())
                .collect::<Vec<_>>()
                .join(" + "),
        );
    }
    out.push_str(" { ");
    let fields: Vec<String> = s.fields.iter().map(|(n, t)| format!("{n}: {t}")).collect();
    out.push_str(&fields.join(", "));
    out.push_str(" }");
    out
}

fn render_enum_sig(e: &EnumDecl) -> String {
    let mut out = format!(
        "enum {}{} {{ ",
        e.name.name,
        render_type_params(&e.type_params)
    );
    let variants: Vec<String> = e.variants.iter().map(|v| v.to_string()).collect();
    out.push_str(&variants.join(", "));
    out.push_str(" }");
    out
}

fn render_trait_sig(t: &TraitDecl) -> String {
    let mut out = format!("trait {}", t.name.name);
    if !t.supertraits.is_empty() {
        out.push_str(": ");
        let sups: Vec<String> = t.supertraits.iter().map(|s| s.to_string()).collect();
        out.push_str(&sups.join(", "));
    }
    let required_count = t.required.len();
    let default_count = t.defaults.len();
    out.push_str(&format!(
        " {{ /* {} required, {} default method(s) */ }}",
        required_count, default_count
    ));
    out
}

// ---------------------------------------------------------------------------
// Symbol index + cross-references
// ---------------------------------------------------------------------------

/// Map user-defined symbol names to their documentation URL (relative to the
/// docs root). First definition wins on duplicate names.
fn build_symbol_index(packages: &[PackageDoc]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for pkg in packages {
        for module in &pkg.modules {
            for item in &module.items {
                let url = format!("{}/{}#{}", pkg.name, module.page, item.anchor);
                map.entry(item.name.clone()).or_insert(url);
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// HTML rendering
// ---------------------------------------------------------------------------

/// Escape `&`, `<`, `>`, `"` for safe HTML text content.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// Render plain `text` as safe HTML, linkifying any identifier that matches a
/// known symbol in `symbols`. The text is tokenised into identifier runs
/// (`[A-Za-z0-9_]+`) and non-identifier runs; identifier runs that exactly
/// match a symbol name become `<a>` links. Both runs are HTML-escaped.
fn linkify(text: &str, symbols: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_alphanumeric() || b == b'_' {
            // identifier run
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];
            if let Some(url) = symbols.get(word) {
                out.push_str(&format!(
                    "<a href=\"{}\">{}</a>",
                    html_escape(url),
                    html_escape(word)
                ));
            } else {
                out.push_str(&html_escape(word));
            }
        } else {
            // non-identifier run
            let start = i;
            while i < bytes.len() && !(bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push_str(&html_escape(&text[start..i]));
        }
    }
    out
}

/// Render doc text into HTML paragraphs. Blank-line-separated blocks become
/// `<p>` elements; identifiers are linkified. Preserves single newlines as
/// `<br>` within a paragraph for list-like content.
fn render_doc_html(doc: &str, symbols: &BTreeMap<String, String>) -> String {
    let trimmed = doc.trim();
    if trimmed.is_empty() {
        return String::from("<p class=\"nodoc\"><em>No documentation.</em></p>");
    }
    let mut html = String::new();
    for paragraph in trimmed.split("\n\n") {
        let ptrimmed = paragraph.trim();
        if ptrimmed.is_empty() {
            continue;
        }
        let lines: Vec<&str> = ptrimmed.lines().collect();
        let joined = lines.join("\n");
        // Linkify per-line then join with <br> to keep line structure.
        let rendered: Vec<String> = joined
            .split('\n')
            .map(|line| linkify(line, symbols))
            .collect();
        html.push_str("<p>");
        html.push_str(&rendered.join("<br>"));
        html.push_str("</p>\n");
    }
    html
}

/// The shared inline `<style>` block for every page. Clean, readable, no JS.
fn stylesheet() -> &'static str {
    r#"
:root {
    --bg: #ffffff;
    --fg: #1f2328;
    --muted: #57606a;
    --accent: #0969da;
    --border: #d0d7de;
    --code-bg: #f6f8fa;
    --pub: #1a7f37;
    --priv: #6e7681;
    --badge-bg: #ddf4ff;
}
* { box-sizing: border-box; }
body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
    color: var(--fg);
    background: var(--bg);
    margin: 0;
    line-height: 1.6;
}
.container { max-width: 980px; margin: 0 auto; padding: 2rem 1.5rem; }
header { border-bottom: 1px solid var(--border); padding-bottom: 1rem; margin-bottom: 2rem; }
header h1 { margin: 0 0 .25rem; font-size: 1.8rem; }
header .crumbs { color: var(--muted); font-size: .9rem; }
header a { color: var(--accent); text-decoration: none; }
header a:hover { text-decoration: underline; }
main h2 {
    border-bottom: 1px solid var(--border);
    padding-bottom: .3rem;
    margin-top: 2.5rem;
}
.item { margin: 1.5rem 0; padding: .5rem 0; border-bottom: 1px solid var(--border); }
.item:last-child { border-bottom: none; }
.item-header { display: flex; align-items: baseline; gap: .5rem; flex-wrap: wrap; }
.item-name { font-size: 1.15rem; font-weight: 600; }
.item-name a { color: var(--fg); text-decoration: none; }
.item-name a:hover { text-decoration: underline; color: var(--accent); }
.badge {
    display: inline-block;
    font-size: .72rem;
    font-weight: 600;
    padding: .05rem .4rem;
    border-radius: 4px;
    background: var(--badge-bg);
    color: var(--accent);
    text-transform: lowercase;
}
.vis {
    display: inline-block;
    font-size: .72rem;
    font-weight: 600;
    padding: .05rem .35rem;
    border-radius: 4px;
}
.vis.pub { background: #dafbe1; color: var(--pub); }
.vis.priv { background: #f6f8fa; color: var(--priv); }
pre.sig {
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: .75rem 1rem;
    overflow-x: auto;
    font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
    font-size: .9rem;
    margin: .5rem 0;
}
.doc p { margin: .4rem 0; }
.doc .nodoc { color: var(--muted); }
ul.module-list, ul.pkg-list { list-style: none; padding-left: 0; }
ul.module-list li, ul.pkg-list li { padding: .35rem 0; }
ul.module-list a, ul.pkg-list a { color: var(--accent); text-decoration: none; font-weight: 500; }
ul.module-list a:hover, ul.pkg-list a:hover { text-decoration: underline; }
.meta { color: var(--muted); font-size: .85rem; }
footer { margin-top: 3rem; padding-top: 1rem; border-top: 1px solid var(--border); color: var(--muted); font-size: .8rem; }
"#
}

fn html_document(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  \
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  \
         <title>{title}</title>\n  <style>{css}</style>\n</head>\n<body>\n\
         <div class=\"container\">\n{body}\n<footer>Generated by <code>buff doc</code>.</footer>\n\
         </div>\n</body>\n</html>\n",
        title = html_escape(title),
        css = stylesheet(),
        body = body,
    )
}

/// Render one module page.
fn render_module_html(
    pkg_name: &str,
    module: &ModuleDoc,
    all_modules: &[ModuleDoc],
    symbols: &BTreeMap<String, String>,
) -> String {
    let mut body = String::new();

    // Header with breadcrumbs + sidebar link to package index.
    body.push_str(&format!(
        "<header>\n  <h1>{name}</h1>\n  \
         <div class=\"crumbs\"><a href=\"../index.html\">docs</a> \
         &rsaquo; <a href=\"index.html\">{pkg}</a> \
         &rsaquo; <code>{rel}</code></div>\n</header>\n",
        name = html_escape(&module.page),
        pkg = html_escape(pkg_name),
        rel = html_escape(&module.rel),
    ));

    if module.items.is_empty() {
        body.push_str(
            "<p class=\"meta\">No documented declarations \
                       (func / struct / enum / trait) found in this module.</p>\n",
        );
    }

    // Group items by kind for a rustdoc-like layout.
    let groups: [(ItemKind, &str); 4] = [
        (ItemKind::Struct, "Structs"),
        (ItemKind::Enum, "Enums"),
        (ItemKind::Trait, "Traits"),
        (ItemKind::Function, "Functions"),
    ];
    for (kind, heading) in groups {
        let items: Vec<&DocItem> = module.items.iter().filter(|it| it.kind == kind).collect();
        if items.is_empty() {
            continue;
        }
        body.push_str(&format!("<main>\n<h2>{heading}</h2>\n"));
        for item in items {
            body.push_str(&render_item_html(item, symbols));
        }
        body.push_str("</main>\n");
    }

    // Cross-links to sibling modules in this package.
    let siblings: Vec<&ModuleDoc> = all_modules.iter().filter(|m| m.rel != module.rel).collect();
    if !siblings.is_empty() {
        body.push_str("<h2>Modules</h2>\n<ul class=\"module-list\">\n");
        for sib in siblings {
            body.push_str(&format!(
                "  <li><a href=\"{page}\">{name}</a> <span class=\"meta\">&mdash; \
                 {rel}</span></li>\n",
                page = html_escape(&sib.page),
                name = html_escape(sib.page.trim_end_matches(".html")),
                rel = html_escape(&sib.rel),
            ));
        }
        body.push_str("</ul>\n");
    }

    html_document(&format!("{} — {} — Buff docs", module.rel, pkg_name), &body)
}

/// Render a single documented item.
fn render_item_html(item: &DocItem, symbols: &BTreeMap<String, String>) -> String {
    let vis_label = if item.is_pub { "pub" } else { "private" };
    let vis_class = if item.is_pub { "pub" } else { "priv" };
    format!(
        "<section class=\"item\" id=\"{anchor}\">\n  \
         <div class=\"item-header\">\n    \
         <span class=\"item-name\"><a href=\"#{anchor}\">{name}</a></span>\n    \
         <span class=\"badge\">{badge}</span>\n    \
         <span class=\"vis {vis_class}\">{vis_label}</span>\n  \
         </div>\n  \
         <pre class=\"sig\">{sig}</pre>\n  \
         <div class=\"doc\">\n{doc}  </div>\n\
         </section>\n",
        anchor = html_escape(&item.anchor),
        name = html_escape(&item.name),
        badge = item.kind.badge(),
        vis_class = vis_class,
        vis_label = vis_label,
        sig = linkify(&item.signature, symbols),
        doc = render_doc_html(&item.doc, symbols),
    )
}

/// Render the per-package `index.html`.
fn render_package_index(pkg: &PackageDoc) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "<header>\n  <h1>{pkg}</h1>\n  \
         <div class=\"crumbs\"><a href=\"../index.html\">docs</a> \
         &rsaquo; {pkg}</div>\n</header>\n",
        pkg = html_escape(&pkg.name),
    ));

    if pkg.modules.is_empty() {
        body.push_str(
            "<p class=\"meta\">No <code>.buff</code> source files found \
                       under <code>src/</code>.</p>\n",
        );
    } else {
        body.push_str("<h2>Modules</h2>\n<ul class=\"module-list\">\n");
        for module in &pkg.modules {
            let count = module.items.len();
            body.push_str(&format!(
                "  <li><a href=\"{page}\">{stem}</a> \
                 <span class=\"meta\">&mdash; {rel} ({count} item{s})</span></li>\n",
                page = html_escape(&module.page),
                stem = html_escape(module.page.trim_end_matches(".html")),
                rel = html_escape(&module.rel),
                count = count,
                s = if count == 1 { "" } else { "s" },
            ));
        }
        body.push_str("</ul>\n");
    }

    // Quick symbol overview (all documented names in this package).
    let mut names: Vec<&DocItem> = Vec::new();
    for m in &pkg.modules {
        names.extend(m.items.iter());
    }
    if !names.is_empty() {
        body.push_str("<h2>All items</h2>\n<ul class=\"module-list\">\n");
        // Stable order: kind, then name.
        names.sort_by(|a, b| a.kind.label().cmp(b.kind.label()).then(a.name.cmp(&b.name)));
        for item in names {
            let module = pkg
                .modules
                .iter()
                .find(|m| m.items.iter().any(|it| it.anchor == item.anchor))
                .map(|m| m.page.as_str())
                .unwrap_or("");
            body.push_str(&format!(
                "  <li><a href=\"{page}#{anchor}\">{name}</a> \
                 <span class=\"badge\">{badge}</span> \
                 <span class=\"meta\">{sig}</span></li>\n",
                page = html_escape(module),
                anchor = html_escape(&item.anchor),
                name = html_escape(&item.name),
                badge = item.kind.badge(),
                sig = html_escape(item.signature.split('\n').next().unwrap_or("")),
            ));
        }
        body.push_str("</ul>\n");
    }

    html_document(&format!("{pkg} — Buff docs", pkg = pkg.name), &body)
}

/// Render the top-level workspace `index.html`.
fn render_workspace_index(package_links: &[(String, String)], packages: &[PackageDoc]) -> String {
    let mut body = String::new();
    body.push_str(
        "<header>\n  <h1>Buff docs</h1>\n  <div class=\"crumbs\">workspace</div>\n</header>\n",
    );
    if package_links.is_empty() {
        body.push_str("<p class=\"meta\">No packages documented.</p>\n");
    } else {
        body.push_str("<h2>Packages</h2>\n<ul class=\"pkg-list\">\n");
        for (name, href) in package_links {
            let modules = packages
                .iter()
                .find(|p| &p.name == name)
                .map(|p| p.modules.len())
                .unwrap_or(0);
            let items = packages
                .iter()
                .find(|p| &p.name == name)
                .map(|p| p.item_count())
                .unwrap_or(0);
            body.push_str(&format!(
                "  <li><a href=\"{href}\">{name}</a> \
                 <span class=\"meta\">&mdash; {modules} module{s1}, {items} item{s2}</span></li>\n",
                href = html_escape(href),
                name = html_escape(name),
                modules = modules,
                s1 = if modules == 1 { "" } else { "s" },
                items = items,
                s2 = if items == 1 { "" } else { "s" },
            ));
        }
        body.push_str("</ul>\n");
        body.push_str(
            "<p class=\"meta\">A <code>search-index.json</code> was also generated for \
             client-side search integration.</p>\n",
        );
    }
    html_document("Buff docs — workspace index", &body)
}

/// Render `search-index.json` as pretty JSON.
fn render_search_json(entries: &[SearchEntry]) -> String {
    // serde_json is already a workspace dep of this crate — use it for
    // robust, escaping-correct JSON (no hand-rolling strings).
    let arr: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "kind": e.kind,
                "description": e.description,
                "package": e.package,
                "module": e.module,
                "url": e.url,
            })
        })
        .collect();
    // Pretty-print for readability / diff-friendliness.
    serde_json::to_string_pretty(&serde_json::Value::Array(arr))
        .unwrap_or_else(|_| String::from("[]\n"))
}

/// First non-empty line of a doc block, trimmed, for the search description.
fn first_doc_paragraph(doc: &str) -> String {
    for line in doc.lines() {
        let t = line.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Line table: byte offset <-> line index
// ---------------------------------------------------------------------------

/// Precomputed line-start byte offsets for a source string. Lets `doc_comment_for`
/// map a declaration's span.start byte to its line in O(log n).
struct LineTable {
    /// Byte offset of the start of each line (line 0 starts at byte 0).
    starts: Vec<usize>,
}

impl LineTable {
    fn new(src: &str) -> Self {
        let mut starts = vec![0usize];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { starts }
    }

    /// 0-based index of the line containing `byte`.
    fn line_of(&self, byte: usize) -> usize {
        // Largest i with starts[i] <= byte.
        self.starts
            .partition_point(|&s| s <= byte)
            .saturating_sub(1)
    }

    /// Text of line `idx` (without the trailing newline), borrowed from `src`.
    fn line_text<'a>(&self, src: &'a str, idx: usize) -> &'a str {
        let start = self.starts[idx];
        let end = if idx + 1 < self.starts.len() {
            // Exclude the trailing newline.
            self.starts[idx + 1] - 1
        } else {
            src.len()
        };
        // Also strip a trailing '\r' (CRLF) for clean trimming.
        let slice = &src[start..end.min(src.len())];
        slice.strip_suffix('\r').unwrap_or(slice)
    }
}

// ---------------------------------------------------------------------------
// Browser launch (--open)
// ---------------------------------------------------------------------------

/// Open `path` in the default browser, platform-specific. Mirrors
/// `cargo doc --open`. Errors are non-fatal (the docs are already written).
fn open_in_browser(path: &Path) -> Result<()> {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let display = abs.display().to_string();
    match std::env::consts::OS {
        "windows" => {
            // `start` is a cmd builtin; invoke via cmd /C. The empty title
            // arg `""` is required when the path contains spaces.
            std::process::Command::new("cmd")
                .args(["/C", "start", "", &display])
                .spawn()
                .with_context(|| format!("failed to open browser for {display}"))?;
        }
        "macos" => {
            std::process::Command::new("open")
                .arg(&display)
                .spawn()
                .with_context(|| format!("failed to open browser for {display}"))?;
        }
        _ => {
            // Linux / *BSD / others: xdg-open.
            std::process::Command::new("xdg-open")
                .arg(&display)
                .spawn()
                .with_context(|| format!("failed to open browser for {display}"))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// T102: `buff doc --serve` — local HTTP server with live reload
// ---------------------------------------------------------------------------

/// Entry point for `buff doc --serve`.
///
/// 1. Generates docs via [`run`] (the normal doc pipeline).
/// 2. Starts a minimal HTTP server on `127.0.0.1:<port>` serving the
///    generated HTML from `docs_root`.
/// 3. Polls `.buff` source files for mtime changes (reuses T97's polling
///    approach — no `notify` dependency).
/// 4. On change: regenerates docs + broadcasts an SSE `reload` event to
///    all connected browsers.
///
/// Blocks until Ctrl-C / SIGINT.
pub fn run_serve(dir: &Path, output: Option<&Path>, port: u16) -> Result<()> {
    // Phase 1: generate docs first so the server has something to serve.
    run(dir, output, false)?;

    let docs_root: PathBuf = match output {
        Some(o) => {
            let p = PathBuf::from(o);
            if p.is_absolute() {
                p
            } else {
                dir.join(o)
            }
        }
        None => dir.join("doc"),
    };

    let addr = format!("127.0.0.1:{port}");
    let listener =
        std::net::TcpListener::bind(&addr).with_context(|| format!("failed to bind to {addr}"))?;
    listener
        .set_nonblocking(true)
        .context("failed to set non-blocking")?;

    eprintln!("buff doc: serving docs at http://{addr}/ — Ctrl-C to exit",);

    // Collect .buff source files for mtime polling.
    let source_files: Vec<PathBuf> = collect_source_files(dir);

    // Shared state: last mtime per source file + SSE clients.
    let last_mtimes: std::sync::Arc<std::sync::Mutex<Vec<(PathBuf, std::time::SystemTime)>>> =
        std::sync::Arc::new(std::sync::Mutex::new(
            source_files
                .iter()
                .map(|p| {
                    let mtime = std::fs::metadata(p)
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    (p.clone(), mtime)
                })
                .collect(),
        ));

    let sse_clients: std::sync::Arc<std::sync::Mutex<Vec<std::net::TcpStream>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    let sse_clients_watcher = sse_clients.clone();
    let last_mtimes_watcher = last_mtimes.clone();
    let _docs_root_watcher = docs_root.clone();
    let dir_watcher = dir.to_path_buf();
    let output_watcher = output.map(|p| p.to_path_buf());

    // Spawn a polling watcher thread (reuses T97's polling approach).
    std::thread::Builder::new()
        .name("buff-doc-watcher".into())
        .spawn(move || {
            let poll_interval = std::time::Duration::from_millis(500);
            loop {
                std::thread::sleep(poll_interval);

                let mut mtimes = match last_mtimes_watcher.lock() {
                    Ok(g) => g,
                    Err(_) => break,
                };

                let mut changed = false;
                for (path, last) in mtimes.iter_mut() {
                    let current = std::fs::metadata(path)
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    if current > *last {
                        *last = current;
                        changed = true;
                    }
                }

                if changed {
                    eprintln!("buff doc: source changed — regenerating docs");
                    // Regenerate docs (best-effort — errors are logged, not fatal).
                    if let Err(e) = run(&dir_watcher, output_watcher.as_deref(), false) {
                        eprintln!("buff doc: regeneration error: {e:#}");
                    } else {
                        eprintln!("buff doc: docs regenerated — notifying browsers");
                    }

                    // Notify all SSE clients.
                    let mut clients = match sse_clients_watcher.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    let msg = "data: reload\n\n";
                    clients.retain(|stream| {
                        // Try to send; drop broken connections.
                        let mut s = stream.try_clone().ok();
                        match s.as_mut() {
                            Some(s) => {
                                use std::io::Write;
                                if write!(s, "{msg}").is_err() {
                                    false
                                } else {
                                    s.flush().ok();
                                    true
                                }
                            }
                            None => false,
                        }
                    });
                }
            }
        })
        .context("failed to spawn watcher thread")?;

    // Main accept loop: handle HTTP requests + SSE connections.
    let mut incoming = listener.incoming();
    loop {
        match incoming.next() {
            Some(Ok(stream)) => {
                if let Err(e) = handle_connection(stream, &docs_root, &sse_clients) {
                    eprintln!("buff doc: request error: {e:#}");
                }
            }
            Some(Err(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No pending connection — yield briefly.
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Some(Err(e)) => {
                eprintln!("buff doc: accept error: {e}");
            }
            None => break,
        }
    }

    Ok(())
}

/// Collect all `.buff` source files under `dir/src/` recursively.
fn collect_source_files(dir: &Path) -> Vec<PathBuf> {
    let src_dir = dir.join("src");
    let mut files = Vec::new();
    if src_dir.is_dir() {
        walk_buff_files(&src_dir, &mut files);
    }
    files
}

/// Handle a single HTTP connection: parse the request, serve the file, or
/// handle the SSE `/live-reload` endpoint.
fn handle_connection(
    mut stream: std::net::TcpStream,
    docs_root: &Path,
    sse_clients: &std::sync::Arc<std::sync::Mutex<Vec<std::net::TcpStream>>>,
) -> Result<()> {
    use std::io::{BufRead, Write};

    // Read the request line + headers (small read — we only need the path).
    let mut reader = std::io::BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let request_line = request_line.trim().to_string();

    // Read remaining headers (we don't need them, but consume them so the
    // connection doesn't stall).
    let mut header = String::new();
    loop {
        header.clear();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
    }

    // Parse the request path from "GET /path HTTP/1.1".
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");

    match path {
        "/live-reload" => {
            // SSE endpoint: register the client and keep the connection open.
            let peer = stream.peer_addr().ok();
            eprintln!(
                "buff doc: SSE client connected{}",
                peer.map(|p| format!(" from {p}")).unwrap_or_default()
            );

            // Send SSE headers.
            write!(
                stream,
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: text/event-stream\r\n\
                 Cache-Control: no-cache\r\n\
                 Connection: keep-alive\r\n\
                 Access-Control-Allow-Origin: *\r\n\
                 \r\n"
            )?;
            stream.flush()?;

            // Register this client.
            {
                let mut clients = sse_clients.lock().unwrap();
                clients.push(stream.try_clone()?);
            }

            // Keep the connection open (block on read until the client
            // disconnects). The watcher thread writes to the stream.
            use std::io::Read;
            let mut buf = [0u8; 1024];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }

            // Client disconnected — remove from the list.
            {
                let mut clients = sse_clients.lock().unwrap();
                clients.retain(|c| c.peer_addr().ok() != stream.peer_addr().ok());
            }
            eprintln!("buff doc: SSE client disconnected");
        }
        _ => {
            // Serve a static file from the docs root.
            let file_path = if path == "/" || path == "/index.html" {
                docs_root.join("index.html")
            } else {
                // Strip leading slash and serve relative to docs_root.
                let rel = path.trim_start_matches('/');
                docs_root.join(rel)
            };

            let canonical = file_path.canonicalize().unwrap_or(file_path.clone());
            // Security: ensure the resolved path is inside docs_root.
            let docs_canon = docs_root
                .canonicalize()
                .unwrap_or_else(|_| docs_root.to_path_buf());
            if !canonical.starts_with(&docs_canon) {
                // Path traversal attempt — 404.
                serve_404(&mut stream, &request_line)?;
                return Ok(());
            }

            match std::fs::read(&canonical) {
                Ok(body) => {
                    let ext = canonical.extension().and_then(|e| e.to_str()).unwrap_or("");
                    let content_type = mime_type_for(ext);

                    // Inject SSE live-reload script into HTML pages.
                    let response = if ext == "html" {
                        let body_str = String::from_utf8_lossy(&body);
                        let injected = inject_live_reload_script(&body_str);
                        format!(
                            "HTTP/1.1 200 OK\r\n\
                             Content-Length: {}\r\n\
                             Content-Type: {}\r\n\
                             \r\n\
                             {}",
                            injected.len(),
                            content_type,
                            injected,
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\n\
                             Content-Length: {}\r\n\
                             Content-Type: {}\r\n\
                             \r\n",
                            body.len(),
                            content_type,
                        )
                    };

                    let mut response_bytes = response.into_bytes();
                    if ext != "html" {
                        response_bytes.extend_from_slice(&body);
                    }

                    use std::io::Write;
                    stream.write_all(&response_bytes)?;
                    stream.flush()?;
                }
                Err(_) => {
                    serve_404(&mut stream, &request_line)?;
                }
            }
        }
    }

    Ok(())
}

/// Serve a 404 response.
fn serve_404(stream: &mut std::net::TcpStream, _request_line: &str) -> Result<()> {
    use std::io::Write;
    let body = "<!DOCTYPE html><html><body><h1>404 — Not Found</h1></body></html>";
    let response = format!(
        "HTTP/1.1 404 Not Found\r\n\
         Content-Length: {}\r\n\
         Content-Type: text/html\r\n\
         \r\n\
         {}",
        body.len(),
        body,
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// Map a file extension to a MIME type.
fn mime_type_for(ext: &str) -> &'static str {
    match ext {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

/// Inject a small SSE-based live-reload script just before `</body>` in an
/// HTML page. The script opens an `EventSource` to `/live-reload` and calls
/// `location.reload()` on every `reload` event.
fn inject_live_reload_script(html: &str) -> String {
    let script = r#"<script>
(function(){var s=new EventSource("/live-reload");s.addEventListener("reload",function(){location.reload()});s.addEventListener("error",function(){s.close();setTimeout(function(){location.reload()},3000)})})();
</script>"#;
    if let Some(pos) = html.rfind("</body>") {
        let mut result = String::with_capacity(html.len() + script.len());
        result.push_str(&html[..pos]);
        result.push_str(script);
        result.push_str(&html[pos..]);
        result
    } else {
        // No </body> tag — append the script at the end.
        format!("{html}\n{script}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_name_flattens_separators() {
        assert_eq!(page_name_for("vector.buff"), "vector.html");
        assert_eq!(page_name_for("math/vector.buff"), "math_vector.html");
        assert_eq!(page_name_for("a/b/c.buff"), "a_b_c.html");
        assert_eq!(page_name_for("noext"), "noext.html");
    }

    #[test]
    fn line_table_maps_offsets() {
        let src = "abc\ndef\nghi";
        let t = LineTable::new(src);
        assert_eq!(t.line_of(0), 0);
        assert_eq!(t.line_of(3), 0); // 'c'
        assert_eq!(t.line_of(4), 1); // start of "def"
        assert_eq!(t.line_of(8), 2); // 'h'
        assert_eq!(t.line_text(src, 0), "abc");
        assert_eq!(t.line_text(src, 1), "def");
        assert_eq!(t.line_text(src, 2), "ghi");
    }

    #[test]
    fn line_table_handles_crlf() {
        let src = "a\r\nb\r\nc";
        let t = LineTable::new(src);
        assert_eq!(t.line_text(src, 0), "a");
        assert_eq!(t.line_text(src, 1), "b");
        assert_eq!(t.line_text(src, 2), "c");
    }

    #[test]
    fn doc_comment_directly_above() {
        let src = "/// Hello.\n/// World.\nfunc greet() {}\n";
        let t = LineTable::new(src);
        // decl starts at byte of 'func' (line index 2).
        let func_byte = src.find("func").unwrap();
        let doc = doc_comment_for(src, &t, func_byte);
        assert_eq!(doc, "Hello.\nWorld.");
    }

    #[test]
    fn doc_comment_skips_attribute_lines() {
        let src = "/// Doc.\n@test\nfunc tagged() {}\n";
        let t = LineTable::new(src);
        let func_byte = src.find("func").unwrap();
        let doc = doc_comment_for(src, &t, func_byte);
        assert_eq!(doc, "Doc.");
    }

    #[test]
    fn doc_comment_stops_at_blank_line() {
        let src = "/// First.\n\n/// Second.\nfunc f() {}\n";
        let t = LineTable::new(src);
        let func_byte = src.find("func").unwrap();
        let doc = doc_comment_for(src, &t, func_byte);
        assert_eq!(doc, "Second.");
    }

    #[test]
    fn doc_comment_empty_when_none() {
        let src = "func nodoc() {}\n";
        let t = LineTable::new(src);
        let func_byte = src.find("func").unwrap();
        let doc = doc_comment_for(src, &t, func_byte);
        assert_eq!(doc, "");
    }

    #[test]
    fn html_escape_replaces_special_chars() {
        assert_eq!(
            html_escape("a < b & c > d \" e"),
            "a &lt; b &amp; c &gt; d &quot; e"
        );
    }

    #[test]
    fn linkify_links_known_symbols() {
        let mut syms = BTreeMap::new();
        syms.insert("Foo".to_string(), "pkg/foo.html#struct-Foo".to_string());
        let out = linkify("returns Foo or Bar", &syms);
        assert!(out.contains("<a href=\"pkg/foo.html#struct-Foo\">Foo</a>"));
        // Unknown symbol Bar stays plain (escaped but no link).
        assert!(out.contains("Bar"));
        assert!(!out.contains("<a href=\"Bar"));
    }

    #[test]
    fn render_doc_html_splits_paragraphs() {
        let doc = "First para.\n\nSecond para.";
        let syms = BTreeMap::new();
        let html = render_doc_html(doc, &syms);
        assert!(html.contains("<p>First para.</p>"));
        assert!(html.contains("<p>Second para.</p>"));
    }

    #[test]
    fn render_doc_html_nodoc_marker() {
        let syms = BTreeMap::new();
        let html = render_doc_html("", &syms);
        assert!(html.contains("No documentation"));
    }

    #[test]
    fn first_doc_paragraph_skips_blanks() {
        assert_eq!(first_doc_paragraph("\n\nHello.\nMore."), "Hello.");
        assert_eq!(first_doc_paragraph(""), "");
    }

    #[test]
    fn parse_module_extracts_documented_items() {
        let tmp = std::env::temp_dir().join("buff_doc_test_item.buff");
        let src = "/// A greeting.\nexport func greet(name: String) -> String {\n    name\n}\n\n\
                   struct Point {\n    x: Int,\n    y: Int,\n}\n";
        std::fs::write(&tmp, src).unwrap();
        let module = parse_module(&tmp, "test.buff").expect("parse should succeed");
        assert_eq!(module.page, "test.html");
        // greet (exported) + Point (private struct).
        assert_eq!(module.items.len(), 2);
        let greet = module.items.iter().find(|i| i.name == "greet").unwrap();
        assert_eq!(greet.kind, ItemKind::Function);
        assert!(greet.is_pub);
        assert_eq!(greet.doc, "A greeting.");
        assert!(greet
            .signature
            .contains("func greet(name: String) -> String"));
        let point = module.items.iter().find(|i| i.name == "Point").unwrap();
        assert_eq!(point.kind, ItemKind::Struct);
        assert!(!point.is_pub);
        assert!(point.signature.contains("struct Point"));
        assert!(point.signature.contains("x: Int"));
        let _ = std::fs::remove_file(&tmp);
    }
}
