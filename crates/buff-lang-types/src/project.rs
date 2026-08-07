//! Project-level parsing + span-aware error formatting (T1 — multi-file linking).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use buff_lang_ast::{Decl, ImportDecl};
use buff_lang_error::{SourceFile, TypeError};

use crate::modules::{build_graph, FsLoader, ModuleGraph, ModuleLoader};

#[derive(Debug, Clone)]
pub struct ParsedProject {
    pub graph: ModuleGraph,
    pub source_files: HashMap<PathBuf, String>,
    pub root: PathBuf,
}

impl ParsedProject {
    pub fn iter_topo_with_source(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.graph.topo_order.iter().filter_map(|p| {
            self.source_files
                .get(p)
                .map(|src| (p.as_path(), src.as_str()))
        })
    }
}

#[derive(Debug, Clone)]
pub struct ProjectError {
    pub message: String,
    pub inner: TypeError,
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.inner)
    }
}

pub fn parse_project(root: &Path) -> Result<ParsedProject, ProjectError> {
    parse_project_with_loader(root, &FsLoader)
}

pub fn parse_project_with_loader(
    root: &Path,
    loader: &dyn ModuleLoader,
) -> Result<ParsedProject, ProjectError> {
    let source_files = snapshot_reachable_sources(root, loader);
    let graph = build_graph(root, loader).map_err(|e| ProjectError {
        message: enrich_error(&e, root, &source_files, loader),
        inner: e,
    })?;
    let mut source_files = source_files;
    for path in graph.modules.keys() {
        if !source_files.contains_key(path) {
            if let Some(src) = loader.load(path) {
                source_files.insert(path.clone(), src);
            }
        }
    }
    Ok(ParsedProject {
        graph,
        source_files,
        root: root.to_path_buf(),
    })
}

fn snapshot_reachable_sources(root: &Path, loader: &dyn ModuleLoader) -> HashMap<PathBuf, String> {
    let mut out = HashMap::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut visited: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    while let Some(path) = stack.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let Some(src) = loader.load(&path) else {
            continue;
        };
        if let Some(imports) = parse_imports_for_snapshot(&path, &src) {
            for imp in imports {
                if let Some(spec) = imp.from_path {
                    if let Ok(target) = crate::modules::resolve_path(&path, &spec) {
                        stack.push(target);
                    }
                } else if !imp.path.is_empty() {
                    // T72: resolve dotted-path imports for snapshot too.
                    let spec = crate::modules::dotted_path_to_spec(&imp.path);
                    if let Ok(target) = crate::modules::resolve_path(&path, &spec) {
                        stack.push(target);
                    }
                }
            }
        }
        out.insert(path, src);
    }
    out
}

fn parse_imports_for_snapshot(path: &Path, src: &str) -> Option<Vec<ImportDecl>> {
    let source_id = buff_lang_error::SourceId(0);
    let tokens = buff_lang_lexer::tokenize(src, source_id).ok()?;
    let decls = buff_lang_parser::parse(&tokens, source_id).ok()?;
    let _ = path;
    Some(
        decls
            .iter()
            .filter_map(|d| match d {
                Decl::ImportDecl(i) => Some(i.clone()),
                _ => None,
            })
            .collect(),
    )
}

fn enrich_error(
    e: &TypeError,
    root: &Path,
    source_files: &HashMap<PathBuf, String>,
    loader: &dyn ModuleLoader,
) -> String {
    let msg = &e.diagnostic.message;
    if let Some(chain_str) = msg.strip_prefix("circular import detected: ") {
        return enrich_cyclic_error(chain_str, source_files);
    }
    if msg.contains("is not exported from") {
        return enrich_missing_export_error(msg, root, source_files, loader);
    }
    msg.clone()
}

fn enrich_cyclic_error(chain_str: &str, source_files: &HashMap<PathBuf, String>) -> String {
    let links: Vec<&str> = chain_str.split(" -> ").collect();
    if links.len() < 2 {
        return format!("circular import detected: {chain_str}");
    }
    let mut annotated: Vec<String> = Vec::with_capacity(links.len());
    for window in links.windows(2) {
        let src_path = PathBuf::from(window[0]);
        let dst_path = PathBuf::from(window[1]);
        annotated.push(format_path_with_import_pos(
            &src_path,
            &dst_path,
            source_files,
        ));
    }
    if let Some(last) = links.last() {
        annotated.push((*last).to_string());
    }
    format!("circular import detected: {}", annotated.join(" -> "))
}

fn enrich_missing_export_error(
    msg: &str,
    root: &Path,
    source_files: &HashMap<PathBuf, String>,
    loader: &dyn ModuleLoader,
) -> String {
    let (name, target_path_str) = match parse_missing_export_message(msg) {
        Some(parts) => parts,
        None => return msg.to_string(),
    };
    let target_path = PathBuf::from(&target_path_str);
    let mut best: Option<(PathBuf, usize, usize)> = None;
    for (path, src) in source_files {
        if let Some(decls) = parse_imports_for_snapshot(path, src) {
            for imp in decls {
                let Some(spec) = &imp.from_path else { continue };
                let Ok(resolved) = crate::modules::resolve_path(path, spec) else {
                    continue;
                };
                if resolved != target_path {
                    continue;
                }
                let matching_ident = imp.imports.iter().find(|n| n.name == name);
                let span = match matching_ident {
                    Some(n) => n.span,
                    None => imp.span,
                };
                if let Some((line, col)) =
                    SourceFile::new(path.clone(), src.clone()).lookup(span.start)
                {
                    let is_better = best.is_none() || path.as_path() == root;
                    if is_better {
                        best = Some((path.clone(), line, col));
                    }
                }
            }
        }
    }
    let _ = loader;
    let Some((importer, line, col)) = best else {
        return msg.to_string();
    };
    format!(
        "no symbol '{name}' exported from {target_path_str} at {}:{line}:{col}",
        importer.display()
    )
}

fn parse_missing_export_message(msg: &str) -> Option<(String, String)> {
    let prefix = "is not exported from ";
    let backtick_open = msg.find('`')?;
    let backtick_close = msg[backtick_open + 1..].find('`')? + backtick_open + 1;
    let name = msg[backtick_open + 1..backtick_close].to_string();
    let after_name = &msg[backtick_close..];
    let marker = after_name.find(prefix)?;
    let target_start_in_after = marker + prefix.len();
    let target_part = &after_name[target_start_in_after..];
    let target = if let Some(stripped) = target_part.strip_prefix('`') {
        stripped.trim_end_matches('`').to_string()
    } else {
        target_part.trim().to_string()
    };
    Some((name, target))
}

fn format_path_with_import_pos(
    path: &Path,
    dst: &Path,
    source_files: &HashMap<PathBuf, String>,
) -> String {
    let Some(src) = source_files.get(path) else {
        return path.display().to_string();
    };
    let Some(decls) = parse_imports_for_snapshot(path, src) else {
        return path.display().to_string();
    };
    for imp in decls {
        let Some(spec) = &imp.from_path else { continue };
        let Ok(resolved) = crate::modules::resolve_path(path, spec) else {
            continue;
        };
        if resolved == dst {
            let sf = SourceFile::new(path.to_path_buf(), src.clone());
            if let Some((line, col)) = sf.lookup(imp.span.start) {
                return format!("{}:{line}:{col}", path.display());
            }
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryLoader;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn loader_with(files: &[(&str, &str)]) -> MemoryLoader {
        let mut l = MemoryLoader::new();
        for (path, src) in files {
            l.insert(PathBuf::from(*path), src);
        }
        l
    }

    #[test]
    fn parse_project_basic_two_module_chain() {
        let loader = loader_with(&[
            (
                "/main.buff",
                "import { greet } from \"./hello.buff\"\n\nfunc main() { return 0 }",
            ),
            ("/hello.buff", "export func greet() { return 1 }"),
        ]);
        let project = parse_project_with_loader(&p("/main.buff"), &loader).expect("parses");
        assert_eq!(project.graph.modules.len(), 2);
        assert!(project.source_files.contains_key(&p("/main.buff")));
        assert!(project.source_files.contains_key(&p("/hello.buff")));
        assert_eq!(project.root, p("/main.buff"));
    }

    #[test]
    fn parse_project_circular_import_error_includes_lines() {
        let loader = loader_with(&[
            (
                "/a.buff",
                "import { something } from \"./b.buff\"\n\nfunc main() { return 0 }",
            ),
            (
                "/b.buff",
                "import { other } from \"./a.buff\"\n\nexport func something() { return 1 }",
            ),
        ]);
        let err = parse_project_with_loader(&p("/a.buff"), &loader).expect_err("cycle detected");
        assert!(
            err.message.contains("circular import"),
            "missing 'circular import' in: {}",
            err.message
        );
        assert!(
            err.message.contains(":1:"),
            "missing line number for first import in: {}",
            err.message
        );
    }

    #[test]
    fn parse_project_missing_import_error_includes_lines() {
        let loader = loader_with(&[
            (
                "/main.buff",
                "import { nonexistent } from \"./math.buff\"\n\nfunc main() { return 0 }",
            ),
            ("/math.buff", "export func add(a: Int, b: Int) { return a + b }\n"),
        ]);
        let err = parse_project_with_loader(&p("/main.buff"), &loader).expect_err("missing");
        assert!(
            err.message.contains("no symbol 'nonexistent'"),
            "missing QA-spec phrasing in: {}",
            err.message
        );
        assert!(
            err.message.contains(":1:"),
            "missing line number in: {}",
            err.message
        );
    }

    #[test]
    fn parse_project_missing_file_error_carries_path() {
        let loader = loader_with(&[(
            "/main.buff",
            "import { x } from \"./does_not_exist.buff\"\n\nfunc main() { return 0 }",
        )]);
        let err = parse_project_with_loader(&p("/main.buff"), &loader).expect_err("missing file");
        assert!(
            err.message.contains("does_not_exist.buff"),
            "missing file path in: {}",
            err.message
        );
    }

    #[test]
    fn parse_missing_export_message_parses_modules_rs_shape() {
        let (name, path) = parse_missing_export_message(
            "`nonexistent` is not exported from `/proj/src/lib/math.buff`",
        )
        .expect("parses");
        assert_eq!(name, "nonexistent");
        assert_eq!(path, "/proj/src/lib/math.buff");
    }

    #[test]
    fn parsed_project_iter_topo_with_source_yields_in_dep_order() {
        let loader = loader_with(&[
            (
                "/main.buff",
                "import { a } from \"./a.buff\"\nimport { b } from \"./b.buff\"\nfunc main() { return 0 }",
            ),
            ("/a.buff", "export func a() { return 1 }"),
            ("/b.buff", "export func b() { return 2 }"),
        ]);
        let project = parse_project_with_loader(&p("/main.buff"), &loader).expect("parses");
        let order: Vec<&Path> = project.iter_topo_with_source().map(|(p, _)| p).collect();
        assert_eq!(order.last().copied(), Some(p("/main.buff").as_path()));
    }
}
