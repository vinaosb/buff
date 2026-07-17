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
    /// An `export <decl>` wrapper: `export func foo() { ... }`,
    /// `export enum Color { ... }`, etc. (T29).
    ///
    /// The wrapped [`Decl`] is the inner item (currently
    /// [`FuncDecl`](Decl::FuncDecl), [`StructDecl`](Decl::StructDecl),
    /// [`EnumDecl`](Decl::EnumDecl)); other inner kinds are a parse error.
    /// The module-graph pass treats the inner item as PUBLIC (visible to
    /// importers) — non-wrapped decls stay module-private.
    ExportDecl(ExportDecl),
    /// A `export * from "./path"` (or `export { a, b } from "./path"`)
    /// re-export declaration (T29).
    ///
    /// `wildcard = true` re-exports ALL public symbols from the target
    /// module; otherwise `names` lists the specific symbols re-exported.
    ReexportDecl(ReexportDecl),
    /// An `extern crate "name"` declaration (T32 — FFI basics).
    ///
    /// Records that the generated Rust crate must depend on the named
    /// crates.io/Rust crate AND that a `use <name>;` item should be
    /// emitted at the top of the generated source. The RustCodegen
    /// collects these into a `BTreeSet<String>` (exposed via
    /// [`RustCodegen::extern_crates`](../../buff_lang_codegen_rust/struct.RustCodegen.html))
    /// so the CLI/pipeline can write them into the generated `Cargo.toml`
    /// when full Cargo-project wiring lands.
    ExternCrateDecl(ExternCrateDecl),
    /// An `extend TYPE { fn ...; ... }` extension-method block (T75).
    ///
    /// Adds methods to an existing type (primitive or user-defined). The
    /// target type is stored as a [`TypeRef`] (today always a
    /// [`TypeRef::Named`]); the methods reuse the existing [`FuncDecl`]
    /// shape (the parser routes each `fn` inside the block through
    /// [`parse_func_decl`](../../buff_lang_parser/fn.parse_func_decl.html)).
    ///
    /// The codegen lowers this to a Rust "extension trait" + blanket-free
    /// impl — the standard Rust extension-trait pattern:
    ///
    /// ```ignore
    /// // Buff:  extend String { fn shout(self) -> String { ... } }
    /// // Rust:
    /// trait BuffExtString {
    ///     fn shout(self) -> String;
    /// }
    /// impl BuffExtString for String {
    ///     fn shout(self) -> String { ... }
    /// }
    /// ```
    ///
    /// The trait name is derived from the target type as `BuffExt{Type}`
    /// (e.g. `extend String` → `BuffExtString`). This is the FIRST `Decl`
    /// variant that lowers to TWO `syn::Item`s (the trait + the impl);
    /// [`RustCodegen::generate`] handles the multi-item emission by
    /// extending the items `Vec` rather than pushing a single item.
    ///
    /// v0.5 single extend-block per target type is the common case.
    /// Multi-block merging (two `extend String { ... }` blocks for the
    /// same target) and generic targets (`extend Vector<T>`) are deferred.
    ExtendBlock(ExtendBlock),
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
            Decl::ExportDecl(d) => write!(f, "{d}"),
            Decl::ReexportDecl(d) => write!(f, "{d}"),
            Decl::ExternCrateDecl(d) => write!(f, "{d}"),
            Decl::ExtendBlock(d) => write!(f, "{d}"),
        }
    }
}

/// A function declaration.
///
/// # Migration notes (additive AST changes)
///
/// ## T35 — `attributes` field
///
/// An `attributes: Vec<Attribute>` field was **added** in T35 (v0.5) to carry
/// the list of `@name` attributes preceding the function (e.g. `@test`, and
/// future `@prefer(gpu)` / `@inline`). This is a **migration** (a new field
/// was inserted, not purely additive — but every construction site was
/// updated to pass `attributes: Vec::new()` for non-attributed funcs). The
/// new field sits just before `span` to keep `span` as the trailing anchor
/// (consistent with the other decl structs). The Display impl renders
/// `@test ` (and other attributes) before the `fn` keyword. The codegen
/// pass emits the corresponding Rust attribute (`#[test]`, etc.) when the
/// attribute is recognised; unknown attributes are a codegen error (so we
/// don't silently drop user intent).
#[derive(Debug, Clone, PartialEq)]
pub struct FuncDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Block,
    pub is_async: bool,
    pub is_unsafe: bool,
    pub is_extern: bool,
    /// `@name` attributes preceding the function (T35). Empty for the vast
    /// majority of funcs. The only attribute meaningful in v0.5 is `@test`
    /// (→ `#[test]` at codegen); the design generalises to any future
    /// attribute without another AST migration.
    pub attributes: Vec<Attribute>,
    pub span: Span,
}

/// A `@name` attribute attached to a declaration (T35).
///
/// Buff attributes are the `@`-prefixed form (`@test`, `@prefer(gpu)`,
/// `@inline`, …) analogous to Rust's `#[name]` / `#[name(args)]`. For v0.5
/// only the argument-less `@test` form is meaningful; the `args` field is
/// carried for forward-compatibility with the `@prefer(gpu)` shape the
/// README anticipates, so a second AST migration isn't needed later.
///
/// Attributes are **attached to [`FuncDecl`]s** (collected in
/// [`FuncDecl::attributes`]); in future they may attach to structs/enums
/// too. The parser collects zero-or-more leading `@name` forms before a
/// `func` declaration and stores them in declaration order (leftmost
/// attribute first).
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    /// The attribute name without the leading `@` (e.g. `"test"`).
    pub name: Ident,
    /// Optional parenthesised arguments (e.g. `@prefer(gpu)` carries
    /// `["gpu"]`). Empty for `@test`. Carried for forward-compat so the
    /// v0.5→future `@prefer(gpu)` shape doesn't need another AST migration.
    pub args: Vec<String>,
    pub span: Span,
}

impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.name)?;
        if !self.args.is_empty() {
            f.write_str("(")?;
            for (i, a) in self.args.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{a:?}")?;
            }
            f.write_str(")")?;
        }
        Ok(())
    }
}

impl fmt::Display for FuncDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FuncDecl(")?;
        for a in &self.attributes {
            write!(f, "{a} ")?;
        }
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

/// An import declaration.
///
/// Two syntactic shapes are supported (T29 expanded the original module-path
/// form with the ES6-style `from "..."` form used by the Buff v0.5 module
/// system):
///
/// 1. **ES6 form (T29, v0.5)** — `import { greet, farewell } from "./hello.buff"`
///    or `import * from "./utils.buff"`. Stored as:
///    - `from_path: Some("./hello.buff")`
///    - `imports: ["greet", "farewell"]` (or empty when `wildcard = true`)
///    - `wildcard: false` (or `true` for `import * from`)
///    - `path: []`, `alias: None` (legacy fields unused by this form)
///
/// 2. **Legacy module-path form** — `import a.b.c as alias`. Stored as:
///    - `path: ["a", "b", "c"]`
///    - `alias: Some("alias")`
///    - `from_path: None`, `wildcard: false`
///
/// # Migration notes (additive AST changes)
///
/// ## T29 — `from_path` and `wildcard` fields
///
/// Two new fields — `from_path: Option<String>` and `wildcard: bool` — were
/// added in T29 (v0.5) to support the ES6-style `from "..."` module-import
/// syntax. This is a **migration** (new fields were appended, not purely
/// additive — but every existing construction site was updated to pass
/// `from_path: None` and `wildcard: false`). Both new fields default to
/// "unused" values so legacy-shape ImportDecls continue to behave exactly
/// as before. The Display impl renders whichever form is active (ES6 when
/// `from_path` is `Some`, legacy otherwise).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    /// Legacy dotted module path (`a.b.c`). Empty when using the ES6 form.
    pub path: Vec<Ident>,
    /// Imported symbol names (`{ greet, farewell }`). Empty when `wildcard`.
    pub imports: Vec<Ident>,
    /// Legacy `as alias` rename. `None` for the ES6 form.
    pub alias: Option<Ident>,
    /// ES6 source path string (`from "./hello.buff"`). `None` for the legacy
    /// module-path form. T29 (additive).
    pub from_path: Option<String>,
    /// `import * from "..."` — re-export all public symbols of the target.
    /// T29 (additive). When `true`, `imports` is empty by convention.
    pub wildcard: bool,
    pub span: Span,
}

impl fmt::Display for ImportDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // ES6 form takes precedence when from_path is present.
        if let Some(src) = &self.from_path {
            f.write_str("Import(")?;
            if self.wildcard {
                f.write_str("*")?;
            } else {
                f.write_str("{ ")?;
                for (i, n) in self.imports.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{n}")?;
                }
                f.write_str(" }")?;
            }
            return write!(f, " from {src:?})");
        }
        // Legacy form: `a.b.c [:: imports] [as alias]`.
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

/// A `export <decl>` wrapper declaration (T29).
///
/// Wraps an inner [`Decl`] (currently [`FuncDecl`], [`StructDecl`], or
/// [`EnumDecl`]) and marks it as PUBLIC — visible to importers. The
/// module-graph pass treats any top-level decl NOT wrapped in `ExportDecl`
/// as module-private.
///
/// # Migration notes (additive AST changes)
///
/// ## T29 — new Decl variant
///
/// `Decl::ExportDecl(ExportDecl)` is a **purely additive** new variant (no
/// existing variant changed). All `match` expressions on [`Decl`] across
/// the codebase were updated to add a `Decl::ExportDecl { .. }` arm. The
/// codegen pass unwboxes the inner decl and codegens it as usual (the
/// visibility modifier is preserved on the wrapper, not duplicated on the
/// inner decl — a Rust `pub` keyword will be emitted in a later wave when
/// multi-file codegen lands).
#[derive(Debug, Clone, PartialEq)]
pub struct ExportDecl {
    /// The wrapped, exported declaration. Always one of the public-item
    /// variants (FuncDecl / StructDecl / EnumDecl); the parser rejects
    /// `export import` / `export module` / nested `export export`.
    pub inner: Box<Decl>,
    pub span: Span,
}

impl fmt::Display for ExportDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Export({})", self.inner)
    }
}

/// A re-export declaration: `export * from "./path"` (wildcard) or
/// `export { a, b } from "./path"` (named) (T29).
///
/// The module-graph pass resolves `from` to a target module and exposes
/// the named symbols (or, when `wildcard`, ALL of the target's public
/// symbols) as if they were declared in this module.
///
/// # Migration notes (additive AST changes)
///
/// ## T29 — new Decl variant
///
/// `Decl::ReexportDecl(ReexportDecl)` is **purely additive**. Like
/// [`ExportDecl`], every `match` on [`Decl`] gained a `Decl::ReexportDecl`
/// arm.
#[derive(Debug, Clone, PartialEq)]
pub struct ReexportDecl {
    /// Source path string (`from "./other.buff"`).
    pub from: String,
    /// Specific symbols re-exported (`export { greet } from ...`). Empty
    /// when `wildcard = true`.
    pub names: Vec<Ident>,
    /// `export * from ...` — re-export all public symbols of the target.
    pub wildcard: bool,
    pub span: Span,
}

impl fmt::Display for ReexportDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Reexport(")?;
        if self.wildcard {
            f.write_str("*")?;
        } else {
            f.write_str("{ ")?;
            for (i, n) in self.names.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{n}")?;
            }
            f.write_str(" }")?;
        }
        write!(f, " from {:?})", self.from)
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

/// An `extern crate "name"` declaration (T32 — FFI basics).
///
/// Records a dependency on an external Rust crate (e.g. `extern crate "serde"`
/// records `serde`). Unlike Rust's `extern crate serde;` form, Buff uses a
/// STRING literal for the crate name so it is unambiguous (Buff has no bare
/// `crate` keyword) and so the same shape can later carry a version
/// constraint (`extern crate "serde" "1.0"` — future work). The RustCodegen
/// emits a `use <name>;` item for each and collects the names into a
/// `BTreeSet<String>` exposed via `RustCodegen::extern_crates()` so the
/// pipeline (when it gains Cargo-project assembly) can write
/// `<name> = "*"` lines into the generated `Cargo.toml`.
///
/// # Migration notes (additive AST changes)
///
/// ## T32 — new Decl variant
///
/// `Decl::ExternCrateDecl(ExternCrateDecl)` is **purely additive** (no
/// existing variant changed). All `match` expressions on [`Decl`] across
/// the codebase gained a `Decl::ExternCrateDecl { .. }` arm. The codegen
/// pass records the crate name and emits a `use <name>;` item (single-file
/// codegen); wiring the recorded set into the generated `Cargo.toml` is
/// deferred until the CLI pipeline switches from single-file `rustc`
/// invocation to a Cargo-project model.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternCrateDecl {
    /// Crate name as written in the string literal (`extern crate "serde"`
    /// → `"serde"`). Stored as a `String` (not [`Ident`]) because crate
    /// names may contain `-` (e.g. `rust_decimal`), which is not a valid
    /// Buff identifier character.
    pub name: String,
    pub span: Span,
}

impl fmt::Display for ExternCrateDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ExternCrate({:?})", self.name)
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

/// An `extend TYPE { fn ...; ... }` extension-method block (T75).
///
/// Adds methods to an existing type (primitive or user-defined). The
/// target is the type NAME (`String`, `Int`, `MyStruct`, …) — today
/// always stored as a [`TypeRef::Named`]; generic targets
/// (`extend Vector<T>`) are a future task.
///
/// The methods are full [`FuncDecl`]s (the parser routes each `fn` inside
/// the block through the shared [`parse_func_decl`]). v0.5 single-block
/// per type is the common case; multi-block merging is deferred.
///
/// # Codegen target
///
/// Lowers to a Rust extension trait + blanket-free impl — the standard
/// Rust extension-trait pattern that lets `"x".my_method()` resolve on a
/// type the user didn't define:
///
/// ```ignore
/// // Buff:  extend String { fn shout(self) -> String { ... } }
/// // Rust:
/// trait BuffExtString {
///     fn shout(self) -> String;
/// }
/// impl BuffExtString for String {
///     fn shout(self) -> String { ... }
/// }
/// ```
///
/// The trait name is derived from the target type name as
/// `BuffExt{Type}` (so `extend String` → `BuffExtString`). This is the
/// ONLY `Decl` variant that lowers to TWO `syn::Item`s; see the migration
/// note below.
///
/// # Migration notes (additive AST changes)
///
/// ## T75 — new Decl variant
///
/// `Decl::ExtendBlock(ExtendBlock)` is **purely additive** (no existing
/// variant changed). All `match` expressions on [`Decl`] across the
/// codebase gained a `Decl::ExtendBlock { .. }` arm. The codegen pass
/// emits TWO `syn::Item`s per block — a [`syn::ItemTrait`] (signatures)
/// and a [`syn::ItemImpl`] (bodies) — making this the first decl variant
/// whose lowering produces more than one top-level Rust item. The
/// [`RustCodegen::generate`] item-collection loop was extended to push
/// both items via a dedicated helper (see
/// `RustCodegen::lower_extend_block_items`).
///
/// [`parse_func_decl`]: ../../buff_lang_parser/fn.parse_func_decl.html
/// [`TypeRef::Named`]: crate::ty::TypeRef::Named
#[derive(Debug, Clone, PartialEq)]
pub struct ExtendBlock {
    /// The target type's NAME (e.g. `"String"`, `"Int"`, `"MyStruct"`).
    /// Stored as a [`TypeRef::Named`] so future support for generic
    /// targets (`extend Vector<T>`) needs no AST migration — only the
    /// parser widening + codegen handling the new shapes.
    pub target: TypeRef,
    /// The methods to add to the target type. Each is a full [`FuncDecl`]
    /// (parsed via the shared `parse_func_decl`); bodies are kept (not
    /// abstract signatures) because the extension-trait impl carries them.
    pub methods: Vec<FuncDecl>,
    pub span: Span,
}

impl fmt::Display for ExtendBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ExtendBlock({} {{ ", self.target)?;
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
