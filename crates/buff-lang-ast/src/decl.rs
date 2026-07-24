//! Top-level declaration nodes for the Buff AST.
//!
//! A source file is a sequence of [`Decl`]s (functions, structs, enums,
//! imports, modules, traits). Declarations form the module-level structure.

use std::collections::BTreeMap;
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
    /// An `extern "ABI" [from "crate"] func name(params) -> Ret` declaration
    /// (T119 — minimal extern/bindgen).
    ///
    /// Functionally equivalent to the legacy `extern func name(...)`
    /// (which lowers to [`Decl::FuncDecl`] with `is_extern = true`), but
    /// carries TWO extra pieces of metadata that the legacy form cannot:
    ///
    /// - **`abi`** — the ABI string the user wrote (`"C"`, `"system"`).
    ///   Codegen emits this verbatim in the `extern "ABI" { ... }`
    ///   foreign-mod. Only `"C"` is supported in v1.3; other ABIs are a
    ///   parse error (see `parse_extern_func_decl_with_abi`).
    /// - **`crate_name`** — the optional `from "serde_json"` annotation
    ///   that names the Rust crate providing the symbol. When present
    ///   the codegen records the crate in its `extern_crates` set so the
    ///   pipeline can write `[rust-deps]` entries into `buff.toml`.
    ///
    /// The legacy `extern func name(...) -> Ret` form (no ABI string) is
    /// kept for backward compatibility with v0.5; new code SHOULD use the
    /// richer `extern "C" from "serde_json" func name(...) -> Ret` form.
    ///
    /// Like the legacy form, ExternFuncDecl has NO body — it is a
    /// signature-only foreign-function declaration. The codegen lowers
    /// it to a Rust `extern "ABI" { fn name(...); }` foreign-mod item
    /// and registers the function name in a per-codegen set consulted at
    /// call sites (so a Buff call `name(args)` lowers to a Rust
    /// `unsafe { name(args) }` — Rust requires `unsafe` to call foreign
    /// functions, but Buff hides that from the user).
    ExternFuncDecl(ExternFuncDecl),
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
    /// same target type) and generic targets (`extend Vector<T>`) are deferred.
    ExtendBlock(ExtendBlock),
    /// An `impl Trait for Type { ... }` trait-implementation block
    /// (T75b — associated types in traits).
    ///
    /// Implements a declared [`TraitDecl`] for a target type. The body
    /// supplies:
    ///
    /// - **Associated-type bindings**: `type Item = T;` — one per
    ///   associated type declared by the trait ([`ImplBlock::type_bindings`]).
    /// - **Method implementations**: `func name(...) -> Ret { body }` —
    ///   one per required method; default methods may be overridden
    ///   ([`ImplBlock::methods`]).
    ///
    /// Lowers to a single Rust `syn::ItemImpl` with `trait_` set to
    /// `Some((None, trait_path, For))` — a trait-impl, not an inherent
    /// impl. The associated-type bindings become `syn::ImplItem::AssocType`
    /// items; the methods become `syn::ImplItem::Fn` items.
    ///
    /// This is the SECOND `Decl` variant that lowers to a Rust `ItemImpl`
    /// (the first was [`Decl::ExtendBlock`]); however, unlike
    /// [`Decl::ExtendBlock`], [`Decl::ImplBlock`] lowers to a SINGLE
    /// top-level item, so it does NOT need the multi-item special-case in
    /// [`RustCodegen::generate`] — it goes through the normal `lower_decl`
    /// path.
    ImplBlock(ImplBlock),
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
            Decl::ExternFuncDecl(d) => write!(f, "{d}"),
            Decl::ExtendBlock(d) => write!(f, "{d}"),
            Decl::ImplBlock(d) => write!(f, "{d}"),
        }
    }
}

/// A generic type parameter declaration (T13).
///
/// Represents a single `<T>` or `<T: Bound>` in a generic parameter list.
/// In T13 the `bounds` field is always empty — generic bounds (trait
/// constraints like `<T: Clone>`) are T38. The field exists now so the
/// AST shape is stable when T38 adds bound parsing (no second migration).
///
/// Stored in [`FuncDecl::type_params`], [`StructDecl::type_params`], and
/// [`EnumDecl::type_params`]. The codegen emits each as a Rust type param
/// (`T` or `T: Bound`) on the generated `fn`/`struct`/`enum` item.
/// Monomorphization happens in rustc (zero-cost static dispatch).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    /// The parameter name (e.g. `"T"`, `"U"`, `"Key"`).
    pub name: Ident,
    /// Trait bounds on the parameter (e.g. `[Clone, Debug]` for `<T: Clone + Debug>`).
    /// **Always empty in T13.** Populated by T38 (generic bounds).
    pub bounds: Vec<TypeRef>,
    pub span: Span,
}

impl fmt::Display for TypeParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if !self.bounds.is_empty() {
            f.write_str(": ")?;
            for (i, b) in self.bounds.iter().enumerate() {
                if i > 0 {
                    f.write_str(" + ")?;
                }
                write!(f, "{b}")?;
            }
        }
        Ok(())
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
///
/// ## T13 — `type_params` field
///
/// A `type_params: Vec<TypeParam>` field was **added** in T13 (v1.25) to
/// carry generic type parameters declared on the function (e.g.
/// `func id<T>(x: T) -> T` carries `[T]`). This is a **migration** (a new
/// field was inserted) — every construction site was updated to pass
/// `type_params: Vec::new()` for non-generic funcs. The field sits just
/// after `attributes` and before `span` (keeping `span` as the trailing
/// anchor). The Display impl renders `<T, U>` after the function name.
/// The codegen emits Rust generics on the function signature; rustc
/// performs monomorphization (zero-cost static dispatch).
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
    /// Generic type parameters (T13). Empty for non-generic funcs. When
    /// non-empty, the codegen emits `<T, U, ...>` on the Rust `fn` signature
    /// and rustc monomorphizes each call site. Bounds are always empty in
    /// T13 (populated by T38).
    pub type_params: Vec<TypeParam>,
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
///
/// # T0 — named arguments
///
/// The `named_args` field (T0-G3) carries `key = "value"` pairs for
/// attributes like `@deprecated(since = "2.0", replacement = "new_fn")`.
/// Positional args go in [`Attribute::args`]; keyword args go in
/// [`Attribute::named_args`]. Both can coexist on the same attribute.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    /// The attribute name without the leading `@` (e.g. `"test"`).
    pub name: Ident,
    /// Optional parenthesised positional arguments (e.g. `@prefer(gpu)`
    /// carries `["gpu"]`). Empty for `@test`. Carried for forward-compat
    /// so the v0.5→future `@prefer(gpu)` shape doesn't need another AST
    /// migration.
    pub args: Vec<String>,
    /// Optional `key = "value"` named arguments (T0-G3). Populated by
    /// the parser for forms like `@deprecated(since = "2.0",
    /// replacement = "new_fn")`. Empty for v0.5-era attributes that
    /// use only positional args.
    pub named_args: BTreeMap<String, String>,
    pub span: Span,
}

impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.name)?;
        let has_positional = !self.args.is_empty();
        let has_named = !self.named_args.is_empty();
        if has_positional || has_named {
            f.write_str("(")?;
            for (i, a) in self.args.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{a:?}")?;
            }
            if has_positional && has_named {
                f.write_str(", ")?;
            }
            for (i, (k, v)) in self.named_args.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{k} = {v:?}")?;
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
        // T13: render `<T, U>` when generic params are present.
        if !self.type_params.is_empty() {
            f.write_str("<")?;
            for (i, tp) in self.type_params.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{tp}")?;
            }
            f.write_str(">")?;
        }
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
///
/// # Migration notes (additive AST changes)
///
/// ## T13 — `type_params` field
///
/// A `type_params: Vec<TypeParam>` field was **added** in T13 (v1.25) to
/// carry generic type parameters (e.g. `struct Pair<T, U>` carries `[T, U]`).
/// This is a **migration** — every construction site was updated to pass
/// `type_params: Vec::new()` for non-generic structs. The Display impl
/// renders `<T, U>` after the name. The codegen emits Rust generics on the
/// `struct` item; rustc monomorphizes each instantiation.
#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: Ident,
    pub fields: Vec<(Ident, TypeRef)>,
    pub traits: Vec<Ident>,
    /// Generic type parameters (T13). Empty for non-generic structs.
    pub type_params: Vec<TypeParam>,
    pub span: Span,
}

impl fmt::Display for StructDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StructDecl({}", self.name)?;
        // T13: render `<T, U>` when generic params are present.
        if !self.type_params.is_empty() {
            f.write_str("<")?;
            for (i, tp) in self.type_params.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{tp}")?;
            }
            f.write_str(">")?;
        }
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
/// ## T27 — `generics` field (superseded by T13)
///
/// A `generics: Vec<Ident>` field was **added** in T27 (v0.5) to carry the
/// list of type parameter names on the enum (e.g. `Result<T, E>` carried
/// `["T", "E"]`). This was a bare-name representation with no bounds.
///
/// ## T13 — `type_params` field (replaces `generics`)
///
/// The T27 `generics: Vec<Ident>` field was **replaced** in T13 (v1.25) by
/// `type_params: Vec<TypeParam>`, unifying the generic-param representation
/// across [`FuncDecl`], [`StructDecl`], and [`EnumDecl`]. This is a
/// **migration** (the field name and type changed) — every construction site
/// and every access site was updated: the parser, the codegen (which now
/// emits bounds via the richer [`TypeParam`] struct), the naming linter, the
/// formatter, and the rename refactoring. The Display impl renders `<T, E>`
/// after the name (unchanged from T27 for the bounds-empty case). The
/// `bounds` field inside each [`TypeParam`] is always empty in T13
/// (populated by T38).
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: Ident,
    /// Generic type parameters (T13, replacing T27's `generics: Vec<Ident>`).
    /// Empty for non-generic enums.
    pub type_params: Vec<TypeParam>,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

impl fmt::Display for EnumDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EnumDecl({}", self.name)?;
        // T13: render `<T, E>` when generic params are present (was `generics` in T27).
        if !self.type_params.is_empty() {
            f.write_str("<")?;
            for (i, tp) in self.type_params.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{tp}")?;
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

/// An `extern "ABI" [from "crate"] func name(params) -> Ret` declaration
/// (T119 — minimal extern/bindgen).
///
/// Carries the same foreign-function-declaration semantics as the legacy
/// `extern func name(...) -> Ret` form (which lowers to [`FuncDecl`] with
/// `is_extern = true`), but additionally records the explicit ABI string
/// the user wrote and an optional source-crate annotation.
///
/// # Field-by-field
///
/// - **`abi`** — the literal ABI string the user wrote (`"C"`, `"system"`).
///   The parser only accepts `"C"` in v1.3 (per the T119 spec: "use `"C"`
///   ABI for stability/cross-language compatibility"). Other ABIs are a
///   parse error. The string is emitted verbatim inside the
///   `extern "ABI" { ... }` foreign-mod at codegen time.
/// - **`crate_name`** — the optional source crate (`from "serde_json"`).
///   When present the codegen records the crate name in its
///   `extern_crates` set so the pipeline can write `[rust-deps]`
///   entries into `buff.toml`. When `None` the user did not write a
///   `from "..."` annotation and no `[rust-deps]` entry is generated
///   for this declaration (the symbol is assumed to be provided by the
///   default link path).
/// - **`name`** — the Buff-side function name. The user calls this in
///   Buff source (`name(args)`); the codegen emits a Rust foreign-fn
///   signature with the same name inside `extern "ABI" { ... }`.
/// - **`params`** / **`return_type`** — the signature. Types go through
///   the standard Buff→Rust primitive mapping (Int→i64, String→String,
///   …) via `ast_typeref_to_syn` at codegen time.
///
/// # Generics
///
/// `ExternFuncDecl` does NOT carry a `generics` field. Generic or
/// trait-bounded extern declarations are REJECTED at parse time with a
/// clear error (the T119 spec: "A generic/trait-heavy Rust API is
/// REJECTED with a clear error (generics unsupported in v1.3)").
///
/// # Body
///
/// Foreign functions have NO body — they are signature-only declarations.
/// The actual implementation is provided either by an external library
/// (linked via the platform's normal FFI mechanism) or by a sibling Rust
/// source file in the user's project (`externs.rs`) that defines
/// `pub extern "C" fn name(...) -> ... { ... }` with a real body. The
/// Buff compiler emits the DECLARATION only; the user supplies the body.
///
/// # Migration notes (additive AST changes)
///
/// ## T119 — new Decl variant
///
/// `Decl::ExternFuncDecl(ExternFuncDecl)` is **purely additive** (no
/// existing variant changed). All `match` expressions on [`Decl`] across
/// the codebase gained a `Decl::ExternFuncDecl { .. }` arm. The codegen
/// pass emits a `syn::ItemForeignMod` (the same shape as the legacy
/// `extern func` lowering) and additionally records the crate name (when
/// present) in `RustCodegen::extern_crates` so the CLI pipeline can
/// populate `[rust-deps]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternFuncDecl {
    /// The ABI string the user wrote (`"C"`, `"system"`). The parser
    /// only accepts `"C"` in v1.3; other ABIs are a parse error.
    pub abi: String,
    /// The optional source crate (`from "serde_json"`). `None` when the
    /// user did not write a `from "..."` annotation.
    pub crate_name: Option<String>,
    /// The Buff-side function name. The user calls this in Buff source.
    pub name: Ident,
    /// The function parameters (same shape as [`FuncDecl::params`]).
    pub params: Vec<Param>,
    /// The optional return type. `None` means the function returns unit.
    pub return_type: Option<TypeRef>,
    pub span: Span,
}

impl fmt::Display for ExternFuncDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Render as `Extern("C" from "serde_json" fn name(params) -> Ret)`
        // — the same surface syntax the user wrote, modulo whitespace.
        write!(f, "Extern({:?} ", self.abi)?;
        if let Some(c) = &self.crate_name {
            write!(f, "from {:?} ", c)?;
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
        f.write_str(")")
    }
}

/// A trait declaration with default methods and inheritance (T93).
///
/// Shape:
/// - `trait Greetable { fn name() -> String; fn greet() { print(name()) } }`
///   — `name` is a REQUIRED method (bodyless, signature only); `greet` is a
///   DEFAULT method (signature + body, may call required methods).
/// - `trait Pet : Animal { fn pet() { ... } }` — inheritance via `: Supertrait`
///   (comma-separated for multiple supertraits: `trait A : B, C { ... }`).
///
/// # Required vs default distinction
///
/// Inside the trait body, each `fn` member is classified by its trailing
/// punctuation:
/// - `fn name(params) -> Ret;` (semicolon) → REQUIRED method, stored as a
///   [`MethodSig`] in [`TraitDecl::required`]. Implementors MUST provide a
///   body.
/// - `fn name(params) -> Ret { body }` (brace block) → DEFAULT method, stored
///   as a full [`FuncDecl`] in [`TraitDecl::defaults`]. Implementors inherit
///   the body unless they override it.
///
/// # Codegen target
///
/// Lowers to a Rust [`syn::ItemTrait`]: required methods become bodyless
/// trait method signatures; default methods become trait methods WITH a
/// default body (Rust default-method syntax); supertraits populate the
/// trait's `supertraits` Punctuated list.
///
/// # Migration notes (additive AST changes)
///
/// ## T93 — redesign of existing TraitDecl
///
/// The pre-T93 `TraitDecl` had a single `methods: Vec<FuncDecl>` field (every
/// method carried a body, with no way to express bodyless required methods
/// or trait inheritance). T93 replaces `methods` with THREE fields:
/// `supertraits: Vec<TypeRef>`, `required: Vec<MethodSig>`, and
/// `defaults: Vec<FuncDecl>`. This is a **migration** (the `methods` field
/// was removed and three new fields inserted), but since NO construction
/// site existed pre-T93 (the parser never produced a `Decl::TraitDecl`, the
/// codegen returned `unsupported`, and no test built one), the migration is
/// zero-impact — every existing `match` arm using `{ .. }` or accessing
/// `.name` continues to compile unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    /// The trait name (`trait Greetable` → `"Greetable"`).
    pub name: Ident,
    /// Supertraits declared via `: A, B` after the trait name
    /// (`trait Pet : Animal` → `[Animal]`). Empty when the trait has no
    /// supertraits. Stored as [`TypeRef::Named`] (today always a bare name;
    /// generic supertraits like `trait Foo : Bar<Int>` are deferred).
    pub supertraits: Vec<TypeRef>,
    /// ASSOCIATED TYPES declared inside the trait body via `type Item;`
    /// (T75b — associated types in traits). Each is a placeholder name
    /// that implementors of the trait MUST bind via
    /// `type Item = ConcreteType;` in their [`ImplBlock`]. Methods of the
    /// trait (both required and default) may reference the associated-type
    /// name as a [`TypeRef::Named`] in their signatures/bodies; the
    /// codegen-lowered Rust trait declares them as `type Item;` items
    /// (`syn::TraitItemType`), and the lowered trait-impl rewrites each
    /// reference to the bound concrete type at codegen time.
    ///
    /// Stored BEFORE [`required`](Self::required) / [`defaults`](Self::defaults)
    /// in the trait body — the canonical Rust idiom is to list associated
    /// types first. The parser accepts them in any order (a `type Item;`
    /// may appear between two `fn` members).
    pub associated_types: Vec<AssociatedType>,
    /// REQUIRED (bodyless) method signatures — `fn name(params) -> Ret;`.
    /// Implementors of the trait MUST provide a body for each. Stored as
    /// [`MethodSig`] (signature only, no body).
    pub required: Vec<MethodSig>,
    /// DEFAULT methods — `fn name(params) -> Ret { body }`. Implementors
    /// inherit the body unless they override it. Stored as full
    /// [`FuncDecl`]s so the body, params, return type, and span are all
    /// preserved (reuses the same shape as regular funcs + extend-block
    /// methods).
    pub defaults: Vec<FuncDecl>,
    pub span: Span,
}

impl fmt::Display for TraitDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TraitDecl({}", self.name)?;
        if !self.supertraits.is_empty() {
            f.write_str(": ")?;
            for (i, st) in self.supertraits.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{st}")?;
            }
        }
        f.write_str(" { ")?;
        let mut first = true;
        for at in &self.associated_types {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            write!(f, "{at}")?;
        }
        for req in &self.required {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            write!(f, "{req}")?;
        }
        for def in &self.defaults {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            write!(f, "{def}")?;
        }
        f.write_str(" })")
    }
}

/// A REQUIRED (bodyless) trait-method signature (T93).
///
/// Represents the `fn name(params) -> Ret;` form inside a trait body — a
/// method that implementors MUST provide a body for. Unlike a full
/// [`FuncDecl`], a [`MethodSig`] carries NO body (the semicolon-terminated
/// form is bodyless by definition).
///
/// Stored in [`TraitDecl::required`]; lowered to a bodyless
/// `syn::TraitItemFn` at codegen time.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodSig {
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub span: Span,
}

impl fmt::Display for MethodSig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
        f.write_str(";")
    }
}

/// An ASSOCIATED TYPE declaration inside a trait body (T75b — associated
/// types in traits).
///
/// Represents the `type Item;` (or `type Item: Bound;`) form inside a
/// trait body. Implementors of the trait MUST supply a concrete binding
/// (`type Item = T;`) in their [`ImplBlock`]. Methods of the trait may
/// reference the associated-type name as a [`TypeRef::Named`] in their
/// signatures and bodies.
///
/// Stored in [`TraitDecl::associated_types`]; lowered to a
/// `syn::TraitItemType` (`type Item;`) at codegen time.
///
/// # Bounds
///
/// The `bounds` field carries optional trait bounds (`type Item: Clone + Debug`
/// → `bounds = [Clone, Debug]`). The parser accepts but currently does
/// NOT enforce bounds at type-check time — they are passed through to the
/// lowered Rust trait item unchanged (rustc enforces them). An empty
/// `bounds` vec means "any type" (the common case).
#[derive(Debug, Clone, PartialEq)]
pub struct AssociatedType {
    /// The associated-type name (`type Item` → `"Item"`).
    pub name: Ident,
    /// Optional trait bounds declared after `:` (`type Item: Clone` →
    /// `[Clone]`). Each bound is a [`TypeRef::Named`] today (the same
    /// shape used for supertraits). Empty when no bounds are declared.
    pub bounds: Vec<TypeRef>,
    pub span: Span,
}

impl fmt::Display for AssociatedType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "type {}", self.name)?;
        if !self.bounds.is_empty() {
            f.write_str(": ")?;
            for (i, b) in self.bounds.iter().enumerate() {
                if i > 0 {
                    f.write_str(" + ")?;
                }
                write!(f, "{b}")?;
            }
        }
        f.write_str(";")
    }
}

/// An ASSOCIATED-TYPE BINDING inside an [`ImplBlock`] (T75b — associated
/// types in traits).
///
/// Represents the `type Item = ConcreteType;` form inside an impl block.
/// Each binding MUST match (by name) an [`AssociatedType`] declared in the
/// trait being implemented. The `target` is the concrete [`TypeRef`] the
/// implementor chose for that associated type.
///
/// Lowered to a `syn::ImplItem::AssocType` (`type Item = T;`) at codegen
/// time.
#[derive(Debug, Clone, PartialEq)]
pub struct AssociatedTypeBinding {
    /// The associated-type name being bound (`type Item = T` → `"Item"`).
    /// MUST match (by name) an [`AssociatedType::name`] in the implemented
    /// trait.
    pub name: Ident,
    /// The concrete type assigned to this associated type
    /// (`type Item = Int` → `Int`).
    pub target: TypeRef,
    pub span: Span,
}

impl fmt::Display for AssociatedTypeBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "type {} = {};", self.name, self.target)
    }
}

/// An `impl Trait for Type { ... }` trait-implementation block (T75b —
/// associated types in traits).
///
/// Implements a declared [`TraitDecl`] for a target type. The body carries
/// the concrete associated-type bindings (`type Item = T;`) and method
/// implementations (`fn ... { body }`) that satisfy the trait's required
/// surface.
///
/// # Codegen target
///
/// Lowers to a Rust `syn::ItemImpl` with `trait_` set to
/// `Some((None, trait_path, For))` so it is a trait-impl (not an inherent
/// impl). Associated-type bindings become `syn::ImplItem::AssocType` items;
/// method impls become `syn::ImplItem::Fn` items.
///
/// # Example
///
/// ```text
/// // Buff:
/// trait Container {
///     type Item;
///     func get(index: Int) -> Item;
/// }
/// struct Box {
///     value: Int,
/// }
/// impl Container for Box {
///     type Item = Int;
///     func get(index: Int) -> Int {
///         return self.value
///     }
/// }
/// // Rust:
/// trait Container {
///     type Item;
///     fn get(&self, index: i64) -> Self::Item;
/// }
/// impl Container for Box {
///     type Item = i64;
///     fn get(&self, index: i64) -> i64 {
///         self.value
///     }
/// }
/// ```
///
/// # Migration notes (additive AST changes)
///
/// ## T75b — new Decl variant
///
/// `Decl::ImplBlock(ImplBlock)` is **purely additive** (no existing
/// variant changed). All `match` expressions on [`Decl`] across the
/// codebase gained a `Decl::ImplBlock { .. }` arm (or fell through an
/// existing `_ =>` catch-all). The codegen pass emits a SINGLE
/// `syn::ItemImpl` per block (unlike [`Decl::ExtendBlock`] which emits
/// two items), so no special multi-item handling is needed in
/// [`RustCodegen::generate`].
#[derive(Debug, Clone, PartialEq)]
pub struct ImplBlock {
    /// The trait being implemented. Stored as a [`TypeRef::Named`] today
    /// (always a bare trait name like `Container`; generic trait impls
    /// like `impl Iterable<Int> for Vec<Int>` are deferred).
    pub trait_name: TypeRef,
    /// The target type the trait is being implemented FOR. Stored as a
    /// [`TypeRef::Named`] today (always a bare type name; generic targets
    /// like `impl Foo for Vec<Int>` are deferred).
    pub target: TypeRef,
    /// Associated-type bindings (`type Item = T;`). Each binding MUST
    /// match (by name) an [`AssociatedType`] declared in the implemented
    /// trait. May be empty if the trait declares no associated types.
    pub type_bindings: Vec<AssociatedTypeBinding>,
    /// Method implementations (`fn name(...) -> Ret { body }`). Each is
    /// a full [`FuncDecl`] (parsed via the shared `parse_func_decl`).
    /// Bodies are required — there is no bodyless form in an impl block
    /// (unlike trait bodies which support `fn ...;`).
    pub methods: Vec<FuncDecl>,
    pub span: Span,
}

impl fmt::Display for ImplBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ImplBlock({} for {} {{ ", self.trait_name, self.target)?;
        let mut first = true;
        for b in &self.type_bindings {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            write!(f, "{b}")?;
        }
        for m in &self.methods {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
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
            type_params: Vec::new(),
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
