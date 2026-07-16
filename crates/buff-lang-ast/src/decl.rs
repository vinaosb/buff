//! Top-level declaration nodes for the Buff AST.
//!
//! A source file is a sequence of [`Decl`]s (functions, structs, enums,
//! imports, modules, traits). Declarations form the module-level structure.

use std::fmt;

use crate::common::{Block, Ident, Param};
use crate::ty::TypeRef;
use buff_lang_error::Span;

/// A top-level declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    /// A function declaration: `fn name(params) -> ret { body }`.
    FuncDecl(FuncDecl),
    /// A struct declaration.
    StructDecl(StructDecl),
    /// An enum declaration.
    EnumDecl(EnumDecl),
    /// An `import` declaration.
    ImportDecl(ImportDecl),
    /// A `module` declaration.
    ModuleDecl(ModuleDecl),
    /// A trait declaration (used from v0.5 onward; defined now).
    TraitDecl(TraitDecl),
}

impl fmt::Display for Decl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Decl::FuncDecl(d) => write!(f, "{d}"),
            Decl::StructDecl(d) => write!(f, "{d}"),
            Decl::EnumDecl(d) => write!(f, "{d}"),
            Decl::ImportDecl(d) => write!(f, "{d}"),
            Decl::ModuleDecl(d) => write!(f, "{d}"),
            Decl::TraitDecl(d) => write!(f, "{d}"),
        }
    }
}

/// A function declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct FuncDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Block,
    pub is_async: bool,
    pub is_unsafe: bool,
    pub is_extern: bool,
    pub span: Span,
}

impl fmt::Display for FuncDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FuncDecl(")?;
        if self.is_extern {
            f.write_str("extern ")?;
        }
        if self.is_async {
            f.write_str("async ")?;
        }
        if self.is_unsafe {
            f.write_str("unsafe ")?;
        }
        write!(f, "fn {}", self.name)?;
        f.write_str("(")?;
        for (i, p) in self.params.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{p}")?;
        }
        f.write_str(")")?;
        if let Some(rt) = &self.return_type {
            write!(f, " -> {rt}")?;
        }
        write!(f, " {})", self.body)
    }
}

/// A struct declaration: `struct Name { field: Ty, ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: Ident,
    pub fields: Vec<(Ident, TypeRef)>,
    pub traits: Vec<Ident>,
    pub span: Span,
}

impl fmt::Display for StructDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StructDecl({}", self.name)?;
        if !self.traits.is_empty() {
            f.write_str(": ")?;
            for (i, t) in self.traits.iter().enumerate() {
                if i > 0 {
                    f.write_str(" + ")?;
                }
                write!(f, "{t}")?;
            }
        }
        f.write_str(" { ")?;
        for (i, (n, t)) in self.fields.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{n}: {t}")?;
        }
        f.write_str(" })")
    }
}

/// An enum declaration: `enum Name { Variant, Variant(T, U), ... }`.
///
/// # Migration notes (additive AST changes)
///
/// ## T27 — `generics` field
///
/// A `generics: Vec<Ident>` field was **added** in T27 (v0.5) to carry the
/// list of type parameters declared on the enum (e.g. `Result<T, E>` carries
/// `["T", "E"]`). This is a **migration** (not purely additive — a new field
/// was inserted), so every construction site was updated to pass
/// `generics: Vec::new()` for non-generic enums. The Display impl renders
/// `<T, E>` after the name when the list is non-empty, matching Rust syntax.
/// The new field is the LAST field before `span` to keep `span` as the
/// trailing anchor (consistent with the other decl structs). Internal
/// construction sites in this crate's `#[cfg(test)]` blocks build
/// `EnumVariant` (not `EnumDecl`), so no test fixture needed updating; the
/// only external `Decl::EnumDecl` consumer is the Rust codegen, which was
/// upgraded from `Err(unsupported)` to a real `lower_enum_decl` in lockstep.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: Ident,
    /// Type parameters declared on the enum, e.g. `[T, E]` for `Result<T, E>`.
    /// Empty for non-generic enums. T27 (additive).
    pub generics: Vec<Ident>,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

impl fmt::Display for EnumDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EnumDecl({}", self.name)?;
        // T27: render `<T, E>` when generic params are present.
        if !self.generics.is_empty() {
            f.write_str("<")?;
            for (i, g) in self.generics.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{g}")?;
            }
            f.write_str(">")?;
        }
        f.write_str(" { ")?;
        for (i, v) in self.variants.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{v}")?;
        }
        f.write_str(" })")
    }
}

/// A single variant inside an [`EnumDecl`].
#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: Ident,
    /// `Some(T)` style payload — `None` for unit variants.
    pub data: Option<Vec<TypeRef>>,
    pub span: Span,
}

impl fmt::Display for EnumVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if let Some(tys) = &self.data {
            f.write_str("(")?;
            for (i, t) in tys.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{t}")?;
            }
            f.write_str(")")?;
        }
        Ok(())
    }
}

/// An import: `import a.b.c as alias;`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub path: Vec<Ident>,
    pub imports: Vec<Ident>,
    pub alias: Option<Ident>,
    pub span: Span,
}

impl fmt::Display for ImportDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Import(")?;
        for (i, p) in self.path.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            write!(f, "{p}")?;
        }
        if !self.imports.is_empty() {
            f.write_str("::")?;
            for (i, n) in self.imports.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{n}")?;
            }
        }
        if let Some(alias) = &self.alias {
            write!(f, " as {alias}")?;
        }
        f.write_str(")")
    }
}

/// A module declaration: `module name;`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDecl {
    pub name: Ident,
    pub span: Span,
}

impl fmt::Display for ModuleDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ModuleDecl({})", self.name)
    }
}

/// A trait declaration: `trait Name { fn ...; ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    pub name: Ident,
    pub methods: Vec<FuncDecl>,
    pub span: Span,
}

impl fmt::Display for TraitDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TraitDecl({} {{ ", self.name)?;
        for (i, m) in self.methods.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{m}")?;
        }
        f.write_str(" })")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_struct_display() {
        let s = StructDecl {
            name: Ident::new("Foo", Span::dummy()),
            fields: Vec::new(),
            traits: Vec::new(),
            span: Span::dummy(),
        };
        assert_eq!(s.to_string(), "StructDecl(Foo {  })");
    }

    #[test]
    fn enum_unit_variant_display() {
        let v = EnumVariant {
            name: Ident::new("None", Span::dummy()),
            data: None,
            span: Span::dummy(),
        };
        assert_eq!(v.to_string(), "None");
    }

    #[test]
    fn enum_payload_variant_display() {
        let v = EnumVariant {
            name: Ident::new("Some", Span::dummy()),
            data: Some(vec![TypeRef::Named {
                name: Ident::new("T", Span::dummy()),
                span: Span::dummy(),
            }]),
            span: Span::dummy(),
        };
        assert_eq!(v.to_string(), "Some(T)");
    }

    #[test]
    fn module_decl_display() {
        let m = ModuleDecl {
            name: Ident::new("mymod", Span::dummy()),
            span: Span::dummy(),
        };
        assert_eq!(m.to_string(), "ModuleDecl(mymod)");
    }
}
