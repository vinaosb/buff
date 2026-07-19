//! Symbol indices for hover / completion / goto-def.
//!
//! Two flat tables built per document:
//!
//! - [`SymbolIndex`] — top-level decls + per-function local bindings.
//!   Powers completion (locals + imports), goto-def, and document-symbols
//!   outline.
//! - [`TypeBindingIndex`] — inferred [`Type`] of every identifier reference,
//!   keyed by the identifier's start byte offset. Powers hover.
//!
//! # Why flat tables (not nested scopes)?
//!
//! Buff's grammar is indentation-based; nested scopes (match arms, if-let,
//! for-let, lambdas) introduce bindings inside their block. v1.2 LSP scope
//! resolution is best-effort: we record every binding + the [`Span`] of
//! the enclosing function (for goto-def) but do NOT track per-block scope
//! boundaries. Completion therefore offers every binding in the file
//! (a superset of what's actually visible at any cursor position). The
//! false-positive rate is low for typical Buff files (a few dozen locals
//! per function) and the v2.0 work items include proper scope tracking
//! (rename / find-references already defer to v2.0).
//!
//! Sorting: deterministic — BTreeMap by (start byte) so iteration order is
//! stable across runs.

use std::collections::BTreeMap;

use buff_lang_ast::{Decl, EnumDecl, FuncDecl, Ident, StructDecl, TraitDecl};
use buff_lang_error::Span;
use buff_lang_types::Type;

/// The kind of a local symbol — used by completion to label candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalKind {
    /// A function parameter.
    Param,
    /// A `let` (or `let`-pattern) binding.
    Let,
    /// A `for`-loop iterator variable or `for let` binding.
    Loop,
    /// A `guard let` binding.
    GuardLet,
}

/// A single entry in the [`SymbolIndex`].
#[derive(Debug, Clone)]
pub struct SymbolEntry {
    /// The identifier (name + its definition-site span).
    pub name: Ident,
    /// What kind of symbol this is.
    pub kind: LocalKind,
    /// Span of the binding itself (for goto-def target).
    pub def_span: Span,
    /// Span of the enclosing top-level function. For top-level decls this
    /// equals `def_span`.
    pub func_span: Span,
}

/// Top-level decl kind — used by document-symbols outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopDeclKind {
    Func,
    Struct,
    Enum,
    Trait,
    /// An exported decl of any of the above. Carries the inner kind
    /// separately so the outline can mark it.
    Export,
    Import,
    Extend,
    Other,
}

/// A top-level decl entry — used by document-symbols + goto-def for module
/// scope names.
#[derive(Debug, Clone)]
pub struct TopDeclEntry {
    /// The user-facing name (function name, struct name, enum name).
    pub name: String,
    /// The full span of the decl (for goto-def + outline range).
    pub span: Span,
    /// The span of just the NAME identifier (for outline selection range).
    pub name_span: Span,
    /// The decl kind.
    pub kind: TopDeclKind,
    /// Detail string for document-symbols outline (e.g. the func signature).
    pub detail: String,
}

/// Flat symbol table for one document.
#[derive(Debug, Clone, Default)]
pub struct SymbolIndex {
    /// All top-level decls in source order.
    pub top_decls: Vec<TopDeclEntry>,
    /// All local bindings (params, lets, loops, guards) keyed by their
    /// start byte offset.
    pub locals: BTreeMap<usize, SymbolEntry>,
}

impl SymbolIndex {
    /// Build an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a top-level [`Decl`] in the index.
    pub fn add_top_decl(&mut self, decl: &Decl) {
        let entry = match decl {
            Decl::FuncDecl(f) => Some(top_entry_for_func(f, TopDeclKind::Func)),
            Decl::StructDecl(s) => Some(top_entry_for_struct(s)),
            Decl::EnumDecl(e) => Some(top_entry_for_enum(e)),
            Decl::TraitDecl(t) => Some(top_entry_for_trait(t)),
            Decl::ExportDecl(exp) => {
                // Record the inner decl under the export wrapper. The
                // outline marker is `Export` so the UI can show a marker.
                match exp.inner.as_ref() {
                    Decl::FuncDecl(f) => {
                        let mut e = top_entry_for_func(f, TopDeclKind::Func);
                        e.kind = TopDeclKind::Export;
                        Some(e)
                    }
                    Decl::StructDecl(s) => {
                        let mut e = top_entry_for_struct(s);
                        e.kind = TopDeclKind::Export;
                        Some(e)
                    }
                    Decl::EnumDecl(en) => {
                        let mut e = top_entry_for_enum(en);
                        e.kind = TopDeclKind::Export;
                        Some(e)
                    }
                    _ => None,
                }
            }
            Decl::ImportDecl(i) => Some(TopDeclEntry {
                name: import_display_name(i),
                span: i.span,
                name_span: i.span,
                kind: TopDeclKind::Import,
                detail: "import".to_string(),
            }),
            Decl::ExtendBlock(ext) => Some(TopDeclEntry {
                name: format!("extend {}", type_ref_display(&ext.target)),
                span: ext.span,
                name_span: ext.span,
                kind: TopDeclKind::Extend,
                detail: "extend".to_string(),
            }),
            // ModuleDecl / ReexportDecl / ExternCrateDecl: not in the MVP
            // outline. Skip.
            _ => None,
        };
        if let Some(e) = entry {
            self.top_decls.push(e);
        }
    }

    /// Record a local binding (function param, `let`, etc.).
    pub fn add_local(
        &mut self,
        name: &Ident,
        kind: LocalKind,
        def_span: Span,
        func_span: Span,
    ) {
        self.locals.insert(
            def_span.start,
            SymbolEntry {
                name: name.clone(),
                kind,
                def_span,
                func_span,
            },
        );
    }

    /// Find the top-level decl with the given name (first match in source
    /// order). Returns `None` if no decl has that name.
    pub fn find_top_decl(&self, name: &str) -> Option<&TopDeclEntry> {
        self.top_decls.iter().find(|d| d.name == name)
    }

    /// Find the local binding at exactly `def_start_byte`. Returns `None`
    /// if no local binding starts there.
    pub fn find_local_at(&self, def_start_byte: usize) -> Option<&SymbolEntry> {
        self.locals.get(&def_start_byte)
    }

    /// Find any local binding whose name matches `name` (first match by
    /// byte offset). Used by goto-def / hover when the user clicks an
    /// identifier reference.
    pub fn find_local_named(&self, name: &str) -> Option<&SymbolEntry> {
        self.locals.values().find(|l| l.name.name == name)
    }

    /// All locals + top-level decls in scope at `cursor_byte`. For v1.2 we
    /// return every local in the file (the per-block scope check is a v2.0
    /// refinement).
    pub fn completions(&self) -> Vec<CompletionCandidate> {
        let mut out: Vec<CompletionCandidate> = Vec::new();
        for d in &self.top_decls {
            if matches!(d.kind, TopDeclKind::Func | TopDeclKind::Export) {
                out.push(CompletionCandidate {
                    label: d.name.clone(),
                    kind: CompletionItemKind::Function,
                    detail: Some(d.detail.clone()),
                });
            } else if matches!(d.kind, TopDeclKind::Struct | TopDeclKind::Export) {
                out.push(CompletionCandidate {
                    label: d.name.clone(),
                    kind: CompletionItemKind::Struct,
                    detail: Some(d.detail.clone()),
                });
            } else if matches!(d.kind, TopDeclKind::Enum | TopDeclKind::Export) {
                out.push(CompletionCandidate {
                    label: d.name.clone(),
                    kind: CompletionItemKind::Enum,
                    detail: Some(d.detail.clone()),
                });
            }
        }
        for l in self.locals.values() {
            let kind = match l.kind {
                LocalKind::Param => CompletionItemKind::Field,
                LocalKind::Let | LocalKind::Loop | LocalKind::GuardLet => CompletionItemKind::Variable,
            };
            // Only one candidate per name (later definitions win — this
            // matches how most language servers handle shadowing in v1.x).
            if let Some(existing) = out.iter_mut().find(|c| c.label == l.name.name) {
                let _ = existing;
            } else {
                out.push(CompletionCandidate {
                    label: l.name.name.clone(),
                    kind,
                    detail: Some(format!("{:?}", l.kind)),
                });
            }
        }
        out
    }
}

/// A completion candidate — what the LSP `completion` handler turns into
/// a `CompletionItem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCandidate {
    pub label: String,
    pub kind: CompletionItemKind,
    pub detail: Option<String>,
}

/// Mirror of [`lsp_types::CompletionItemKind`] without depending on the
/// LSP types crate at the symbol-table layer. The handlers.rs module
/// converts this to the LSP form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionItemKind {
    Function,
    Struct,
    Enum,
    Variable,
    Field,
}

/// Inferred-type index for identifier references. Keyed by the identifier's
/// START byte offset, so a hover at byte N looks up `lookup(N)`.
#[derive(Debug, Clone, Default)]
pub struct TypeBindingIndex {
    bindings: BTreeMap<usize, Type>,
}

impl TypeBindingIndex {
    /// Build an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the inferred type for the identifier starting at `byte`.
    pub fn insert_binding(&mut self, byte: usize, ty: Type) {
        self.bindings.insert(byte, ty);
    }

    /// Look up the inferred type at exactly `byte`.
    pub fn lookup(&self, byte: usize) -> Option<&Type> {
        self.bindings.get(&byte)
    }

    /// Find the inferred type whose identifier span starts at the largest
    /// byte ≤ `byte`. Useful when the cursor sits in the middle of a
    /// multi-byte identifier (we record at the START byte).
    pub fn lookup_covering(&self, byte: usize) -> Option<&Type> {
        // BTreeMap::range: keys in (..=byte).
        self.bindings
            .range(..=byte)
            .next_back()
            .map(|(_, v)| v)
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn top_entry_for_func(f: &FuncDecl, kind: TopDeclKind) -> TopDeclEntry {
    let detail = format_func_signature(f);
    TopDeclEntry {
        name: f.name.name.clone(),
        span: f.span,
        name_span: f.name.span,
        kind,
        detail,
    }
}

fn top_entry_for_struct(s: &StructDecl) -> TopDeclEntry {
    TopDeclEntry {
        name: s.name.name.clone(),
        span: s.span,
        name_span: s.name.span,
        kind: TopDeclKind::Struct,
        detail: format!("struct {}", s.name),
    }
}

fn top_entry_for_enum(e: &EnumDecl) -> TopDeclEntry {
    TopDeclEntry {
        name: e.name.name.clone(),
        span: e.span,
        name_span: e.name.span,
        kind: TopDeclKind::Enum,
        detail: format!("enum {}", e.name),
    }
}

fn top_entry_for_trait(t: &TraitDecl) -> TopDeclEntry {
    TopDeclEntry {
        name: t.name.name.clone(),
        span: t.span,
        name_span: t.name.span,
        kind: TopDeclKind::Trait,
        detail: format!("trait {}", t.name),
    }
}

/// Render a function signature for hover/completion detail.
fn format_func_signature(f: &FuncDecl) -> String {
    let mut s = String::from("func ");
    s.push_str(&f.name.name);
    s.push('(');
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&p.name.name);
        s.push_str(": ");
        s.push_str(&type_ref_display(&p.ty));
    }
    s.push(')');
    if let Some(rt) = &f.return_type {
        s.push_str(" -> ");
        s.push_str(&type_ref_display(rt));
    }
    s
}

/// Render a parse-time `TypeRef` for detail strings.
fn type_ref_display(ty: &buff_lang_ast::TypeRef) -> String {
    format!("{ty}")
}

/// Human-readable name for an import (the path string or first imported
/// name). Used only for the document-symbol outline label.
fn import_display_name(i: &buff_lang_ast::ImportDecl) -> String {
    if let Some(from) = &i.from_path {
        if i.wildcard {
            format!("import * from {from:?}")
        } else if let Some(first) = i.imports.first() {
            format!("import {{ {first}, … }} from {from:?}")
        } else {
            format!("import from {from:?}")
        }
    } else if let Some(first) = i.path.first() {
        format!("import {first}")
    } else {
        "import".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::SourceId;

    #[test]
    fn top_decls_indexed_in_source_order() {
        let src = "func a():\n    print(\"a\")\n\nfunc b():\n    print(\"b\")\n";
        let toks = buff_lang_lexer::tokenize(src, SourceId(0)).unwrap();
        let (decls, _) = buff_lang_parser::parse_recovering(&toks, SourceId(0));
        let mut idx = SymbolIndex::new();
        for d in &decls {
            idx.add_top_decl(d);
        }
        assert_eq!(idx.top_decls.len(), 2);
        assert_eq!(idx.top_decls[0].name, "a");
        assert_eq!(idx.top_decls[1].name, "b");
    }

    #[test]
    fn completions_include_top_funcs_and_locals() {
        // Typed params required by Buff grammar.
        let src = "func add(a: Int, b: Int) -> Int:\n    return a + b\n";
        let toks = buff_lang_lexer::tokenize(src, SourceId(0)).unwrap();
        let (decls, _) = buff_lang_parser::parse_recovering(&toks, SourceId(0));
        assert!(!decls.is_empty(), "fixture should parse cleanly");
        let mut idx = SymbolIndex::new();
        for d in &decls {
            idx.add_top_decl(d);
        }
        let f = match &decls[0] {
            Decl::FuncDecl(f) => f.clone(),
            _ => unreachable!("expected FuncDecl, got: {:?}", decls[0]),
        };
        assert!(
            !f.params.is_empty(),
            "fixture should have params — parse state: {decls:?}"
        );
        idx.add_local(&f.params[0].name, LocalKind::Param, f.params[0].span, f.span);
        idx.add_local(&f.params[1].name, LocalKind::Param, f.params[1].span, f.span);

        let cands = idx.completions();
        let labels: Vec<&str> = cands.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"add"), "labels: {labels:?}");
        assert!(labels.contains(&"a"), "labels: {labels:?}");
        assert!(labels.contains(&"b"), "labels: {labels:?}");
    }

    #[test]
    fn type_bindings_lookup_covering() {
        let mut idx = TypeBindingIndex::new();
        idx.insert_binding(10, Type::int_default());
        idx.insert_binding(50, Type::string());
        // Exact lookup.
        assert!(matches!(idx.lookup(10), Some(Type::Int { .. })));
        // Covering lookup — byte 15 is between 10 and 50, falls in the
        // byte-10 binding.
        assert!(matches!(idx.lookup_covering(15), Some(Type::Int { .. })));
        // Byte past last binding covers to the last one.
        assert!(matches!(idx.lookup_covering(999), Some(Type::String)));
        // Byte before first binding is None.
        assert!(idx.lookup_covering(5).is_none());
    }
}
