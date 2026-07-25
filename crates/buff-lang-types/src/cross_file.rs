//! Cross-file symbol resolution (T1).

use std::collections::BTreeMap;
use std::path::Path;

use buff_lang_ast::{Decl, EnumDecl, FuncDecl, ImportDecl, StructDecl, TraitDecl, TypeRef};

use crate::project::ParsedProject;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Func,
    Struct,
    Enum,
    Trait,
}

#[derive(Debug, Clone)]
pub struct SymbolSignature {
    pub name: String,
    pub kind: SymbolKind,
    pub params: Vec<TypeRef>,
    pub return_type: Option<TypeRef>,
    pub defining_module: std::path::PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct CrossFileSymbolTable {
    inner: BTreeMap<String, SymbolSignature>,
}

impl CrossFileSymbolTable {
    pub fn from_project(project: &ParsedProject) -> Self {
        let mut table = Self::default();
        for module in project.graph.iter_topo() {
            for decl in &module.decls {
                if let Some(sig) = signature_for_export(decl, &module.path) {
                    table.inner.insert(sig.name.clone(), sig);
                }
            }
        }
        table
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn lookup(&self, name: &str) -> Option<&SymbolSignature> {
        self.inner.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.inner.contains_key(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &SymbolSignature)> {
        self.inner.iter()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn named_imports_for<'a>(
        &'a self,
        importer_module: &'a Path,
        project: &'a ParsedProject,
    ) -> Vec<(String, &'a SymbolSignature)> {
        let Some(module) = project.graph.get(importer_module) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for imp in &module.imports {
            if imp.wildcard || !imp.path.is_empty() {
                // Wildcard and dotted-path imports bring in everything;
                // individual symbols are not named here.
                continue;
            }
            for n in &imp.imports {
                if let Some(sig) = self.lookup(&n.name) {
                    out.push((n.name.clone(), sig));
                }
            }
        }
        out
    }

    pub fn wildcard_active(&self, importer_module: &Path, project: &ParsedProject) -> bool {
        let Some(module) = project.graph.get(importer_module) else {
            return false;
        };
        module
            .imports
            .iter()
            .any(|i| i.wildcard || !i.path.is_empty())
    }

    pub fn all_visible_in<'a>(
        &'a self,
        importer_module: &'a Path,
        project: &'a ParsedProject,
    ) -> Vec<&'a SymbolSignature> {
        let mut out: Vec<&SymbolSignature> = Vec::new();
        for (_, sig) in self.named_imports_for(importer_module, project) {
            out.push(sig);
        }
        if self.wildcard_active(importer_module, project) {
            for sig in self.inner.values() {
                if !out.iter().any(|s| s.name == sig.name) {
                    out.push(sig);
                }
            }
        }
        if let Some(module) = project.graph.get(importer_module) {
            for decl in &module.decls {
                if let Some(name) = exported_decl_name(decl) {
                    if let Some(sig) = self.lookup(&name) {
                        if !out.iter().any(|s| s.name == sig.name) {
                            out.push(sig);
                        }
                    }
                }
            }
        }
        out
    }
}

fn signature_for_export(decl: &Decl, defining_module: &Path) -> Option<SymbolSignature> {
    let inner = match decl {
        Decl::ExportDecl(e) => e.inner.as_ref(),
        _ => return None,
    };
    match inner {
        Decl::FuncDecl(f) => Some(func_signature(f, defining_module)),
        Decl::StructDecl(s) => Some(struct_signature(s, defining_module)),
        Decl::EnumDecl(e) => Some(enum_signature(e, defining_module)),
        Decl::TraitDecl(t) => Some(trait_signature(t, defining_module)),
        _ => None,
    }
}

fn exported_decl_name(decl: &Decl) -> Option<String> {
    match decl {
        Decl::ExportDecl(e) => exported_decl_name(e.inner.as_ref()),
        Decl::FuncDecl(f) => Some(f.name.name.clone()),
        Decl::StructDecl(s) => Some(s.name.name.clone()),
        Decl::EnumDecl(e) => Some(e.name.name.clone()),
        Decl::TraitDecl(t) => Some(t.name.name.clone()),
        _ => None,
    }
}

fn func_signature(f: &FuncDecl, defining_module: &Path) -> SymbolSignature {
    SymbolSignature {
        name: f.name.name.clone(),
        kind: SymbolKind::Func,
        params: f.params.iter().map(|p| p.ty.clone()).collect(),
        return_type: f.return_type.clone(),
        defining_module: defining_module.to_path_buf(),
    }
}

fn struct_signature(s: &StructDecl, defining_module: &Path) -> SymbolSignature {
    SymbolSignature {
        name: s.name.name.clone(),
        kind: SymbolKind::Struct,
        params: Vec::new(),
        return_type: None,
        defining_module: defining_module.to_path_buf(),
    }
}

fn enum_signature(e: &EnumDecl, defining_module: &Path) -> SymbolSignature {
    SymbolSignature {
        name: e.name.name.clone(),
        kind: SymbolKind::Enum,
        params: Vec::new(),
        return_type: None,
        defining_module: defining_module.to_path_buf(),
    }
}

fn trait_signature(t: &TraitDecl, defining_module: &Path) -> SymbolSignature {
    SymbolSignature {
        name: t.name.name.clone(),
        kind: SymbolKind::Trait,
        params: Vec::new(),
        return_type: None,
        defining_module: defining_module.to_path_buf(),
    }
}

pub fn import_includes(imp: &ImportDecl, name: &str) -> bool {
    if imp.wildcard || !imp.path.is_empty() {
        // Wildcard and dotted-path imports include everything.
        return true;
    }
    imp.imports.iter().any(|n| n.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_project_with_loader, MemoryLoader};
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
    fn cross_file_table_collects_exported_funcs() {
        let loader = loader_with(&[
            (
                "/main.buff",
                "import { greet } from \"./hello.buff\"\nfunc main() { return greet() }",
            ),
            ("/hello.buff", "export func greet() { return 1 }"),
        ]);
        let project = parse_project_with_loader(&p("/main.buff"), &loader).expect("parses");
        let table = CrossFileSymbolTable::from_project(&project);
        let greet = table.lookup("greet").expect("greet present");
        assert_eq!(greet.kind, SymbolKind::Func);
        assert_eq!(greet.name, "greet");
        assert_eq!(greet.defining_module, p("/hello.buff"));
        assert!(!table.contains("main"));
    }

    #[test]
    fn cross_file_table_collects_struct_enum_trait() {
        let loader = loader_with(&[
            (
                "/defs.buff",
                "export struct Point { x: Int, y: Int }\n\
                 export enum Color { Red, Green, Blue }\n\
                 export trait Greetable { fn name() -> String }\n",
            ),
            (
                "/main.buff",
                "import { Point } from \"./defs.buff\"\nfunc main() { return 0 }",
            ),
        ]);
        let project = parse_project_with_loader(&p("/main.buff"), &loader).expect("parses");
        let table = CrossFileSymbolTable::from_project(&project);
        assert_eq!(
            table.lookup("Point").map(|s| s.kind),
            Some(SymbolKind::Struct)
        );
        assert_eq!(
            table.lookup("Color").map(|s| s.kind),
            Some(SymbolKind::Enum)
        );
        assert_eq!(
            table.lookup("Greetable").map(|s| s.kind),
            Some(SymbolKind::Trait)
        );
    }

    #[test]
    fn cross_file_table_named_imports_visible_in_importer() {
        let loader = loader_with(&[
            (
                "/main.buff",
                "import { greet } from \"./hello.buff\"\nfunc main() { return greet() }",
            ),
            ("/hello.buff", "export func greet() { return 1 }"),
        ]);
        let project = parse_project_with_loader(&p("/main.buff"), &loader).expect("parses");
        let table = CrossFileSymbolTable::from_project(&project);
        let importer = p("/main.buff");
        let visible: Vec<&str> = table
            .all_visible_in(&importer, &project)
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            visible.contains(&"greet"),
            "greet should be visible in main"
        );
    }

    #[test]
    fn cross_file_table_wildcard_imports_pulled_in() {
        let loader = loader_with(&[
            (
                "/main.buff",
                "import * from \"./utils.buff\"\nfunc main() { return 0 }",
            ),
            (
                "/utils.buff",
                "export func a() { return 1 }\nexport func b() { return 2 }\n",
            ),
        ]);
        let project = parse_project_with_loader(&p("/main.buff"), &loader).expect("parses");
        let table = CrossFileSymbolTable::from_project(&project);
        let importer = p("/main.buff");
        let visible: Vec<&str> = table
            .all_visible_in(&importer, &project)
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert!(visible.contains(&"a"), "wildcard pulls in a");
        assert!(visible.contains(&"b"), "wildcard pulls in b");
    }

    #[test]
    fn cross_file_table_iter_is_alphabetical() {
        let loader = loader_with(&[
            (
                "/defs.buff",
                "export func zeta() { return 0 }\n\
                 export func alpha() { return 0 }\n\
                 export func mu() { return 0 }\n",
            ),
            (
                "/main.buff",
                "import { zeta } from \"./defs.buff\"\nfunc main() { return 0 }",
            ),
        ]);
        let project = parse_project_with_loader(&p("/main.buff"), &loader).expect("parses");
        let table = CrossFileSymbolTable::from_project(&project);
        let names: Vec<&str> = table.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    }
}
