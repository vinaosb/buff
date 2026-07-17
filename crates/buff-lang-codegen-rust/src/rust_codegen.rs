//! The Rust code generator — lowers Buff AST nodes to `syn` types.
//!
//! ## Design
//!
//! - Every Rust construct is built via explicit `syn` struct construction.
//!   We **never** hand-format Rust strings; the only string producers are
//!   `prettyplease` (via [`crate::format`]) and identifier names.
//! - `parse_quote!` is intentionally avoided in non-test code because it
//!   panics on parse failure; we construct `syn` nodes by hand instead.
//! - Unsupported AST nodes return a [`CodegenError`] rather than panicking,
//!   so future tasks (T12/T13/…) can extend coverage incrementally.
//!
//! ## Supported AST → Rust coverage (T11)
//!
//! - `Decl::FuncDecl` → `Item::Fn` (async/unsafe/extern modifiers + params
//!   + return type + body)
//! - `Stmt::LetDecl`, `Stmt::ExprStmt`, `Stmt::Return`, `Stmt::Assignment`,
//!   `Stmt::Break`, `Stmt::Continue`, `Stmt::ForIn`, `Stmt::ForWhile`
//! - `Expr::Literal`, `Expr::Ident`, `Expr::BinaryOp`, `Expr::UnaryOp`,
//!   `Expr::FuncCall`, `Expr::IfExpr`
//! - `Literal::{Int, Float, Double, Bool, String, Byte, Decimal}`
//! - `TypeRef::Named` for the seven v0.1 primitive names (`Int`→`i64`, etc.)
//!   plus `TypeRef::Option` and `TypeRef::Generic` (named base)
//!
//! ## Type-annotated `let` bindings (T12)
//!
//! Every `let` binding emits an explicit Rust type annotation. If the Buff
//! source provides one (`let x: Int = …`), it is used directly; otherwise
//! the integrated [`TypeInferencer`] infers the type from the initializer
//! expression and [`RustCodegen::buff_type_to_syn`] maps it to the
//! corresponding Rust type. [`Type::Decimal`] maps to
//! `rust_decimal::Decimal` (so generated crates must depend on
//! `rust_decimal`/`rust_decimal_macros`).
//!
//! ## Control flow (T13)
//!
//! - `if cond { a } else { b }` → Rust `if` expression (with optional else)
//! - `for x in iter { body }` → Rust `for x in iter { body }`
//! - `for cond { body }` (Buff conditional loop) → Rust `while cond { body }`
//! - `print(arg)` calls map to `println!("{}", arg)` macro invocations.
//!
//! ## Source-map recording (T16)
//!
//! [`CodegenContext::record_mapping`] is available so that each lowered AST
//! node can record its Buff [`Span`] → Rust `(line, col)` mapping. In v0.1
//! the mapping is **not** automatically populated during lowering because:
//!
//! 1. `syn` nodes carry opaque `proc_macro2::Span`s (no source-line info).
//! 2. `prettyplease` reformats the tree after construction, so line numbers
//!    computed pre-format would be wrong.
//!
//! The pipeline (`buff_lang_cli::error_mapper`) therefore uses **filename
//! translation** for v0.1: it replaces the intermediate `.rs` path in
//! `rustc`/panic messages with the original `.buff` path. Exact Buff line
//! translation via the bidirectional [`SourceMap`](buff_lang_error::SourceMap)
//! will land in a later task once a post-prettyplease line scan is available.
//!
//! ## Move semantics (T33a)
//!
//! All bindings are MOVED by default (Rust move semantics). The integrated
//! [`MoveAnalyzer`] pre-classifies each binding as Copy or non-Copy, and
//! `lower_expr` inserts `.clone()` at the use site of any non-Copy variable
//! that has already been moved once. Generated Rust never contains `&`,
//! `&mut`, or lifetime annotations in function signatures.
//!
//! Structs, enums, imports, traits, lambdas, match, method-call and
//! struct-init lowering are deferred to later tasks.

use std::collections::{BTreeSet, HashSet};

use proc_macro2::Span as ProcSpan;
use syn::punctuated::Punctuated;
use syn::{
    Expr as SynExpr, Field as SynField, Fields as SynFields, File, Ident, Item, ItemEnum, ItemFn,
    ItemStruct, Pat, PatIdent, PatType, ReturnType, Signature, Stmt as SynStmt, Type as SynType,
    Visibility,
};

use buff_lang_ast::{
    op::{BinaryOp, UnaryOp},
    Block, Decl, EnumDecl as AstEnumDecl, Expr, FuncDecl, InterpPart, Literal, MatchArm, Pattern,
    Stmt, StructDecl as AstStructDecl, TypeRef,
};
use buff_lang_error::{CodegenError, Diagnostic, Span as BuffSpan};
use buff_lang_types::{prelude::PreludeFn, FloatWidth, IntWidth, Type, TypeInferencer};

use crate::context::CodegenContext;
use crate::move_analysis::MoveAnalyzer;

/// The Rust code generator.
///
/// Owns a [`CodegenContext`] for the lifetime of one generation pass.
/// Construct with [`RustCodegen::new`] (or `Default`).
pub struct RustCodegen {
    ctx: CodegenContext,
    move_analyzer: MoveAnalyzer,
    /// Local type inferencer used to derive Rust type annotations on
    /// `let` bindings that lack an explicit Buff annotation (T12).
    /// Reset between functions via [`TypeInferencer::env`] clear semantics
    /// (we re-bind params + walk let-stmts at the top of each `lower_func`).
    type_inferencer: TypeInferencer,
    /// T26 hook: names of structs that should be emitted with `#[repr(C)]`
    /// between the derive attribute and the `pub struct` line. The full
    /// GPU-dispatch auto-detection that populates this set lands in v1.0;
    /// T26 provides the emission mechanism only. See [`Self::mark_struct_repr_c`].
    repr_c_struct_names: HashSet<String>,
    /// T31: the post-propagation async-function name set. Populated by
    /// [`Self::generate`] via [`buff_lang_types::analyze_async`] BEFORE
    /// per-function lowering starts, so [`Self::lower_func`] can override
    /// each fn's `is_async` flag with the propagated value and
    /// [`Self::lower_expr`] can auto-insert `.await` at async call sites
    /// inside async fns.
    async_fns: BTreeSet<String>,
    /// T31: name of the function currently being lowered (`None` outside
    /// `lower_func`). Used by [`Self::lower_expr`] to decide whether to
    /// emit `.await` at async call sites and by [`Self::lower_method_call`]
    /// to decide whether `.result()` should warn.
    current_fn_name: Option<String>,
    /// T31: depth of `async move { ... }` blocks we're currently inside.
    /// Incremented by [`Self::lower_spawn`] around the task-body lowering
    /// so async calls inside the spawned task still get `.await` (the
    /// `async move` block IS an async context even if the spawning fn is
    /// sync). Combined with [`Self::current_fn_is_async`] via
    /// [`Self::in_async_context`].
    async_block_depth: usize,
    /// T33: depth of `spawn <expr>` bodies we're currently inside.
    /// Incremented by [`Self::lower_spawn`] around the task-body lowering
    /// so ident uses inside a spawn body can be rewritten to
    /// `Arc::clone(&x)` (for Arc-shared bindings). Combined with
    /// [`Self::move_analyzer`]'s `is_arc_var` to decide whether a bare
    /// ident inside a spawn lowers to `Arc::clone(&x)` or to the regular
    /// `.clone()` / move path.
    spawn_depth: usize,
    /// T34: stack of closure "bypass" sets. Each entry is the set of
    /// variable NAMES that should bypass [`MoveAnalyzer::needs_clone`]
    /// while lowering the closure body:
    /// - **captured variables** (free vars of body not bound by params or
    ///   closure-local lets) — computed via
    ///   [`buff_lang_types::closure_captures`], the shared capture analysis
    ///   extracted from T33's spawn free-var walker.
    /// - **closure parameters** — fresh bindings owned by the closure body.
    ///
    /// When lowering an `Expr::Ident` inside a closure body, if the name
    /// is in the top-of-stack bypass set, we emit it plainly WITHOUT
    /// calling [`MoveAnalyzer::needs_clone`] — Rust handles the capture
    /// (by ref or by move) and param ownership automatically, so Buff must
    /// not insert a spurious `.clone()`.
    ///
    /// This is the key interaction between closures (T34) and the move /
    /// clone analysis (T33): without this stack, (a) a non-Copy captured
    /// variable used twice inside a closure would get a spurious
    /// `.clone()` on its second use, and (b) a closure PARAM used
    /// multiple times (e.g. `|x| x * x + x`) would also get spurious
    /// clones — a pre-existing T23 limitation that T34 fixes.
    ///
    /// Nested closures push multiple entries; each closure's bypass set
    /// is computed independently.
    closure_capture_stack: Vec<BTreeSet<String>>,
    /// T31: collected warning-level diagnostics (e.g. `block()` inside an
    /// async fn is a deadlock risk). Publicly accessible via
    /// [`Self::take_warnings`] so callers (CLI, tests) can render them
    /// alongside generated Rust without losing the underlying codegen
    /// result.
    warnings: Vec<Diagnostic>,
    /// T32: crate names recorded by `extern crate "<name>"` declarations.
    /// Populated during [`Self::generate`] as each [`Decl::ExternCrateDecl`]
    /// is lowered; emitted in codegen as a `use <name>;` item. Exposed via
    /// [`Self::extern_crates`] so the pipeline (when it gains Cargo-project
    /// assembly) can write `<name> = "*"` lines into the generated
    /// `Cargo.toml`. A [`BTreeSet`] (not [`HashSet`]) is used so iteration
    /// order is DETERMINISTIC across runs and independent of hash seed
    /// (the T29 flaky-test lesson — never rely on HashSet iteration order
    /// for codegen output).
    extern_crates: BTreeSet<String>,
}

impl RustCodegen {
    /// Create a fresh codegen with an empty context.
    pub fn new() -> Self {
        Self {
            ctx: CodegenContext::new(),
            move_analyzer: MoveAnalyzer::new(),
            type_inferencer: TypeInferencer::new(),
            repr_c_struct_names: HashSet::new(),
            async_fns: BTreeSet::new(),
            current_fn_name: None,
            async_block_depth: 0,
            spawn_depth: 0,
            closure_capture_stack: Vec::new(),
            warnings: Vec::new(),
            extern_crates: BTreeSet::new(),
        }
    }

    /// Borrow the inner context (read-only).
    pub fn context(&self) -> &CodegenContext {
        &self.ctx
    }

    /// T31: drain the collected warning diagnostics (e.g. `block()` inside
    /// an async fn is a deadlock risk). Returns them in source order.
    ///
    /// Warnings are accumulated during [`Self::generate`]; calling this
    /// afterwards gives the caller (CLI, tests) a chance to render them
    /// alongside the generated Rust. Calling it twice in a row returns an
    /// empty `Vec` the second time.
    pub fn take_warnings(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.warnings)
    }

    /// T31: borrow the collected warning diagnostics without draining.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    /// T32: borrow the set of `extern crate "<name>"` dependencies recorded
    /// during [`Self::generate`]. The names are stored in a [`BTreeSet`] so
    /// iteration order is deterministic (the T29 flaky-test lesson — never
    /// rely on [`HashSet`] iteration order for codegen output).
    ///
    /// Each name corresponds to a Rust crate that the generated source
    /// depends on. The codegen emits a `use <name>;` item for each; the
    /// pipeline (when it switches from single-file `rustc` invocation to a
    /// full Cargo-project model) should additionally write
    /// `<name> = "*"` (or a pinned version) into the generated
    /// `Cargo.toml`'s `[dependencies]` section. **CLI-Cargo.toml wiring is
    /// deferred** — single-file `rustc` invocation (the current pipeline)
    /// cannot consume external crates without a Cargo manifest, so this
    /// accessor exists for the future Cargo-project pipeline and for
    /// codegen-level tests to assert the recorded dep set.
    pub fn extern_crates(&self) -> &BTreeSet<String> {
        &self.extern_crates
    }

    /// T26 hook: mark a struct name to be emitted with `#[repr(C)]` between
    /// the derive attribute and the `pub struct` line. The full GPU-dispatch
    /// auto-detection that populates this set lands in v1.0; T26 provides the
    /// emission mechanism only (plus the test
    /// `struct_codegen_repr_c_emitted_when_struct_marked`).
    ///
    /// Multiple calls accumulate; the marker set is consumed by
    /// [`Self::lower_struct_decl`] when it walks the declaration list.
    pub fn mark_struct_repr_c(&mut self, name: &str) {
        self.repr_c_struct_names.insert(name.to_string());
    }

    /// Generate a complete [`syn::File`] from a list of Buff declarations.
    ///
    /// Each top-level `Decl` becomes one top-level `syn::Item`. The output
    /// is a fully-formed Rust file ready for [`crate::format`].
    ///
    /// # Builtin `Matrix<T>` injection (T24)
    ///
    /// If the program references the builtin Matrix type — detected by the
    /// presence of a `Matrix.new(...)` constructor call anywhere in the
    /// declaration bodies — a flat-storage `Matrix<T>` struct definition +
    /// `new` impl are **prepended** to the generated items. Emitting
    /// on-demand (vs. always) keeps non-Matrix programs free of the struct.
    /// The struct carries `data: Vec<T>, rows: usize, cols: usize` so its
    /// buffer is contiguous and directly GPU-transferable; the `new(rows,
    /// cols)` impl fills `data` with `T::default()` for `rows * cols`
    /// elements (hence the `T: Default + Clone` bound). This is the
    /// REFACTOR-ready flat-storage pattern shared with the future WGSL
    /// storage-buffer codegen (v1.0).
    pub fn generate(&mut self, decls: &[Decl]) -> Result<File, CodegenError> {
        let mut items = Vec::with_capacity(decls.len());
        // T24: emit the builtin Matrix<T> struct + impl on-demand, before
        // any fn. The two items (struct decl + impl block) are prepended so
        // user functions can refer to `Matrix` and `Matrix::new`.
        if program_uses_matrix(decls) {
            items.extend(matrix_struct_items());
        }
        // T30: emit the builtin `Error` struct + impls on-demand when the
        // program uses the `Error(...)` prelude constructor (which lowers to
        // `Err(Error::new(...))`). Emitting on-demand (vs. always) keeps
        // non-error programs free of the struct — mirroring the Matrix
        // emit-on-demand pattern from T24. The struct implements
        // `std::error::Error` + `Display` + `Debug` + `Clone` so it slots
        // into Rust's `Result<T, E>: Termination` and `?`-propagation
        // machinery directly.
        if program_uses_error(decls) {
            items.extend(error_struct_items());
        }
        // T31: run async call-graph propagation BEFORE per-function
        // lowering so each `lower_func` call can override `is_async` with
        // the propagated value. Buff has no `await` keyword — async-ness
        // propagates up the call graph from declared-async fns to all
        // transitive callers. The result feeds two codegen decisions:
        //   1. `lower_func`: emit `async fn` (and `#[tokio::main]` for
        //      `main`) when the propagated set marks the fn async.
        //   2. `lower_expr` / `lower_method_call`: auto-insert `.await`
        //      at async-call sites inside async fns, and lower
        //      `spawn expr` → `tokio::spawn(async move { expr })` and
        //      `t.result()` → `t.await`.
        self.async_fns = buff_lang_types::analyze_async(decls).names;
        for decl in decls {
            // T29: re-export declarations are a multi-file module-graph
            // concern — they emit no Rust item in single-file codegen.
            // Filter them out so we don't generate inert placeholders.
            if matches!(decl, Decl::ReexportDecl { .. }) {
                continue;
            }
            let item = self.lower_decl(decl)?;
            items.push(item);
        }
        Ok(File {
            shebang: None,
            attrs: Vec::new(),
            items,
        })
    }

    fn lower_decl(&mut self, decl: &Decl) -> Result<Item, CodegenError> {
        match decl {
            // T32: an `extern func ...` declaration is a foreign-function
            // signature with NO body. Lower it to a Rust
            // `extern "C" { fn ...; }` foreign-mod item (the body-having
            // `ItemFn` path cannot represent a bodyless declaration).
            Decl::FuncDecl(f) if f.is_extern => {
                Ok(Item::ForeignMod(self.lower_extern_func_decl(f)?))
            }
            Decl::FuncDecl(f) => Ok(Item::Fn(self.lower_func(f)?)),
            Decl::StructDecl(s) => Ok(Item::Struct(self.lower_struct_decl(s)?)),
            // T27: enum codegen. Builds a Rust `pub enum Name<generics> {
            // Variant, Variant(T), ... }` with `#[derive(Clone, Debug)]`,
            // reusing the T26 derive helper.
            Decl::EnumDecl(e) => Ok(Item::Enum(self.lower_enum_decl(e)?)),
            Decl::ImportDecl { .. } => Err(self.unsupported("import codegen")),
            Decl::ModuleDecl { .. } => Err(self.unsupported("module codegen")),
            Decl::TraitDecl { .. } => Err(self.unsupported("trait codegen")),
            // T29: export wraps an inner decl. In single-file codegen we
            // simply lower the inner decl and stamp `pub` on its visibility
            // (multi-file codegen will route through `mod` blocks in a later
            // wave).
            Decl::ExportDecl(e) => match self.lower_decl(&e.inner)? {
                Item::Fn(mut f) => {
                    f.vis = syn::Visibility::Public(Default::default());
                    Ok(Item::Fn(f))
                }
                other => Ok(other),
            },
            // T29: re-exports never reach lower_decl — generate() filters
            // them out. Keep a defensive arm for direct callers.
            Decl::ReexportDecl { .. } => Err(self.unsupported("reexport codegen")),
            // T32: `extern crate "<name>"` — record the name in the
            // extern-crate dep set (exposed via [`Self::extern_crates`])
            // and emit a `use <name>;` item so generated code can refer
            // to the crate by bare name. Wiring the recorded set into the
            // generated Cargo.toml is deferred until the CLI switches to
            // a Cargo-project pipeline.
            Decl::ExternCrateDecl(d) => {
                self.extern_crates.insert(d.name.clone());
                Ok(Item::Use(self.lower_extern_crate_use(&d.name)))
            }
        }
    }

    /// Lower a Buff [`AstStructDecl`] to a Rust [`syn::ItemStruct`] (T26).
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// #[derive(Clone, Debug)]
    /// pub struct Name {
    ///     pub field: <rust_type>,
    ///     ...
    /// }
    /// ```
    ///
    /// Every field is `pub` so generated code can construct and read struct
    /// values without accessor boilerplate (Buff hides encapsulation from
    /// users; the generated Rust just compiles). Field types go through
    /// [`Self::ast_typeref_to_syn`] so the same primitive mapping that drives
    /// `let`-binding annotations (Int→i64, Float→f32, String→String, …)
    /// applies to struct fields.
    ///
    /// # `#[repr(C)]` hook
    ///
    /// If the struct name has been marked via [`Self::mark_struct_repr_c`],
    /// an additional `#[repr(C)]` attribute is emitted BETWEEN the derive
    /// attribute and the `pub struct` line. The marker set is populated by
    /// future GPU-dispatch analysis (v1.0); T26 provides the emission
    /// mechanism only. See [`derive_and_repr_attrs`] for attribute ordering.
    ///
    /// # `traits` field
    ///
    /// The `traits` field on [`AstStructDecl`] is currently ignored at
    /// codegen time — Buff emits blanket `Clone + Debug` derives; user-specified
    /// trait impls are a later task.
    fn lower_struct_decl(&mut self, s: &AstStructDecl) -> Result<ItemStruct, CodegenError> {
        // Build named fields: each field is `pub name: <rust_type>`. Field
        // visibility is always `pub` so any generated constructor / user
        // code can initialise and read struct values without accessors.
        let mut named_fields: Punctuated<SynField, syn::Token![,]> = Punctuated::new();
        for (fname, ftype) in &s.fields {
            let rust_ty = self.ast_typeref_to_syn(ftype)?;
            named_fields.push(SynField {
                attrs: Vec::new(),
                vis: Visibility::Public(Default::default()),
                ident: Some(ast_ident_to_syn(fname)),
                colon_token: Some(Default::default()),
                ty: rust_ty,
                mutability: syn::FieldMutability::None,
            });
        }

        // T26 repr(C) hook: emit `#[repr(C)]` between the derive attribute
        // and `pub struct` when the struct name was marked via
        // `mark_struct_repr_c`. Full GPU-dispatch auto-detection lands in v1.0.
        let emit_repr_c = self.repr_c_struct_names.contains(&s.name.name);

        Ok(ItemStruct {
            attrs: derive_and_repr_attrs(emit_repr_c),
            vis: Visibility::Public(Default::default()),
            struct_token: Default::default(),
            ident: ast_ident_to_syn(&s.name),
            generics: syn::Generics::default(),
            fields: SynFields::Named(syn::FieldsNamed {
                brace_token: Default::default(),
                named: named_fields,
            }),
            semi_token: Some(Default::default()),
        })
    }

    /// Lower a Buff [`AstEnumDecl`] to a Rust [`syn::ItemEnum`] (T27).
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// #[derive(Clone, Debug)]
    /// pub enum Name<T, E> {
    ///     Variant,
    ///     Tuple(T, E),
    ///     ...
    /// }
    /// ```
    ///
    /// Unit variants become Rust unit variants; data-carrying variants
    /// become Rust tuple variants. Every variant is `pub` (so generated code
    /// can construct and pattern-match on values without accessor boilerplate,
    /// matching the T26 struct-field policy). Variant payload types go through
    /// [`Self::ast_typeref_to_syn`] so the same primitive mapping that drives
    /// struct fields and `let`-binding annotations applies here too.
    ///
    /// Generic parameters declared on the enum (`<T, E>`) are emitted as Rust
    /// generic params on the enum (no bounds — Buff does not have user-specified
    /// trait bounds in v0.5). The variant payloads may reference any of these
    /// type params by name (e.g. `Ok(T)`); they are lowered as ordinary named
    /// type references via [`Self::ast_typeref_to_syn`].
    ///
    /// The `#[derive(Clone, Debug)]` attribute reuses the T26 helper
    /// [`derive_and_repr_attrs`] so derive-attribute construction stays in one
    /// place. We pass `emit_repr_c = false` — enums never get `#[repr(C)]`
    /// in v0.5 (the GPU-dispatch repr-C hook is struct-only; enum repr hints
    /// are deferred to v1.0 when tagged unions land).
    fn lower_enum_decl(&mut self, e: &AstEnumDecl) -> Result<ItemEnum, CodegenError> {
        // Build the variant list. Each variant becomes a `syn::Variant`:
        // - unit variant (no payload) -> `Fields::Unit`
        // - tuple variant (payload) -> `Fields::Unnamed` with one field per
        //   payload type. The field names are anonymous (`_0`, `_1`, ... is
        //   implicit in tuple structs/enums — `syn::Field` carries
        //   `ident: None` for unnamed positions).
        let mut variants: Punctuated<syn::Variant, syn::Token![,]> = Punctuated::new();
        for v in &e.variants {
            let fields = match &v.data {
                None => syn::Fields::Unit,
                Some(tys) => {
                    let mut unnamed: Punctuated<SynField, syn::Token![,]> = Punctuated::new();
                    for ty in tys {
                        let rust_ty = self.ast_typeref_to_syn(ty)?;
                        unnamed.push(SynField {
                            attrs: Vec::new(),
                            vis: Visibility::Inherited,
                            ident: None,
                            colon_token: None,
                            ty: rust_ty,
                            mutability: syn::FieldMutability::None,
                        });
                    }
                    syn::Fields::Unnamed(syn::FieldsUnnamed {
                        paren_token: Default::default(),
                        unnamed,
                    })
                }
            };
            variants.push(syn::Variant {
                attrs: Vec::new(),
                ident: ast_ident_to_syn(&v.name),
                fields,
                discriminant: None,
            });
        }

        // Generic params: build a `syn::Generics` with one type param per
        // declared generic on the enum. No bounds, no defaults (v0.5 minimal).
        let generics = if e.generics.is_empty() {
            syn::Generics::default()
        } else {
            let mut params: Punctuated<syn::GenericParam, syn::Token![,]> = Punctuated::new();
            for g in &e.generics {
                params.push(syn::GenericParam::Type(syn::TypeParam {
                    attrs: Vec::new(),
                    ident: ast_ident_to_syn(g),
                    colon_token: None,
                    bounds: Default::default(),
                    eq_token: None,
                    default: None,
                }));
            }
            syn::Generics {
                lt_token: Some(Default::default()),
                params,
                gt_token: Some(Default::default()),
                where_clause: None,
            }
        };

        Ok(ItemEnum {
            attrs: derive_and_repr_attrs(false),
            vis: Visibility::Public(Default::default()),
            enum_token: Default::default(),
            ident: ast_ident_to_syn(&e.name),
            generics,
            brace_token: Default::default(),
            variants,
        })
    }

    fn lower_func(&mut self, f: &FuncDecl) -> Result<ItemFn, CodegenError> {
        // Reset move-analysis state and pre-classify Copy vars for this fn.
        self.move_analyzer.reset();
        self.move_analyzer.preanalyze_func(f);

        // Reset the type inferencer for this function: re-bind parameters
        // using the same primitive-mapping rules that TypeInferencer uses
        // internally (see `typeref_to_type` in buff_lang_types::infer).
        self.type_inferencer = TypeInferencer::new();
        for p in &f.params {
            if let Some(ty) = typeref_to_type(&p.ty) {
                self.type_inferencer.bind(&p.name.name, ty);
            }
        }

        // T31: override the declared `is_async` flag with the PROPAGATED
        // value from the call-graph fixpoint. Buff has no `await` keyword;
        // a fn becomes async if it (transitively) calls an async fn, even
        // when the user didn't write `async`. The propagated set is
        // computed once at the top of [`Self::generate`].
        let is_async = self.async_fns.contains(&f.name.name);

        // T31: record the current fn name so `lower_expr` and
        // `lower_method_call` can decide whether to emit `.await`.
        // (The propagated `async_fns` set is program-wide — it doesn't
        // change per-function.)
        let prev_fn_name = self.current_fn_name.replace(f.name.name.clone());

        let name = ast_ident_to_syn(&f.name);

        let mut inputs: Punctuated<syn::FnArg, syn::Token![,]> = Punctuated::new();
        for p in &f.params {
            let ident = ast_ident_to_syn(&p.name);
            let ty = self.ast_typeref_to_syn(&p.ty)?;
            inputs.push(syn::FnArg::Typed(PatType {
                attrs: Vec::new(),
                pat: Box::new(Pat::Ident(PatIdent {
                    attrs: Vec::new(),
                    ident,
                    by_ref: None,
                    mutability: None,
                    subpat: None,
                })),
                colon_token: Default::default(),
                ty: Box::new(ty),
            }));
        }

        let output = match &f.return_type {
            Some(ty) => {
                ReturnType::Type(Default::default(), Box::new(self.ast_typeref_to_syn(ty)?))
            }
            None => ReturnType::Default,
        };

        let sig = Signature {
            constness: None,
            asyncness: is_async.then(Default::default),
            unsafety: f.is_unsafe.then(Default::default),
            abi: f.is_extern.then(|| syn::Abi {
                extern_token: Default::default(),
                name: None,
            }),
            fn_token: Default::default(),
            ident: name,
            generics: Default::default(),
            paren_token: Default::default(),
            inputs,
            variadic: None,
            output,
        };

        let block = self.lower_block(&f.body)?;

        // T31: emit `#[tokio::main]` on the entry `main` fn when it's in
        // the propagated async set. Buff has no `await` keyword, so the
        // `main` fn becomes async automatically when it transitively
        // calls any async fn; tokio's runtime attribute is what makes
        // that work.
        //
        // T35: emit `#[test]` (and other recognised attributes) from the
        // FuncDecl's `attributes` list. `@test` → `#[test]`; unknown
        // attributes are a codegen error (so we don't silently drop user
        // intent). The `#[tokio::main]` case is mutually exclusive with
        // `#[test]` (a fn can't be both the entry point AND a test), so
        // we handle them in separate branches.
        let mut attrs: Vec<syn::Attribute> = if is_async && f.name.name == "main" {
            vec![syn::parse_quote!(#[tokio::main])]
        } else {
            Vec::new()
        };
        for attr in &f.attributes {
            match attr.name.name.as_str() {
                "test" => attrs.push(syn::parse_quote!(#[test])),
                // Unknown attribute — surface as a codegen error so the
                // user knows it was not applied (rather than silently
                // dropping it). Future tasks can add recognised attributes
                // (e.g. `@inline` → `#[inline]`) here.
                other => {
                    return Err(self.unsupported(&format!(
                        "unrecognised attribute `@{other}` (only `@test` is supported in v0.5)"
                    )));
                }
            }
        }

        // Restore the previous fn context (in case of nested lowering —
        // currently impossible but defensive).
        self.current_fn_name = prev_fn_name;

        Ok(ItemFn {
            attrs,
            vis: Visibility::Inherited,
            sig,
            block: Box::new(block),
        })
    }

    /// Lower a Buff `extern func name(params) -> Ret` declaration (T32 —
    /// FFI) to a Rust [`syn::ItemForeignMod`] of the form
    /// `extern "C" { fn name(params) -> Ret; }`.
    ///
    /// Foreign functions have NO body (the [`FuncDecl::body`] field holds an
    /// empty placeholder Block that the parser synthesises; it is dropped
    /// here). Each extern func becomes its own `extern "C" { ... }` block —
    /// a per-decl block keeps codegen simple and the generated output easy
    /// to read. The ABI is fixed to `"C"` for v0.5 (Buff exposes no way to
    /// pick another ABI like `"system"`); a future task may add
    /// `extern "system" func ...` syntax.
    ///
    /// Parameter and return-type lowering reuse [`Self::ast_typeref_to_syn`]
    /// so the standard Buff→Rust primitive mapping (Int→i64, String→String,
    /// …) applies uniformly to FFI signatures.
    ///
    /// The functions inside an `extern "C" { ... }` block are implicitly
    /// `unsafe` to CALL (Rust requires an `unsafe { ... }` block at every
    /// call site) — we do NOT stamp `unsafe` on the foreign-mod itself, so
    /// the generated code compiles on every Rust edition/toolchain without
    /// needing the `unsafe_extern_blocks` feature (stabilised in Rust 1.82
    /// but kept off here for maximum compatibility).
    fn lower_extern_func_decl(
        &mut self,
        f: &FuncDecl,
    ) -> Result<syn::ItemForeignMod, CodegenError> {
        // Build the parameter list (same shape as lower_func, but as
        // ForeignItemFn inputs which are bare PatType pairs).
        let mut inputs: Punctuated<syn::FnArg, syn::Token![,]> = Punctuated::new();
        for p in &f.params {
            let ident = ast_ident_to_syn(&p.name);
            let ty = self.ast_typeref_to_syn(&p.ty)?;
            inputs.push(syn::FnArg::Typed(PatType {
                attrs: Vec::new(),
                pat: Box::new(Pat::Ident(PatIdent {
                    attrs: Vec::new(),
                    ident,
                    by_ref: None,
                    mutability: None,
                    subpat: None,
                })),
                colon_token: Default::default(),
                ty: Box::new(ty),
            }));
        }

        let output = match &f.return_type {
            Some(ty) => {
                ReturnType::Type(Default::default(), Box::new(self.ast_typeref_to_syn(ty)?))
            }
            None => ReturnType::Default,
        };

        // The signature inside a foreign-mod is a ForeignItemFn: it carries
        // its own (smaller) Signature and ends with a semicolon — there is
        // no body.
        let foreign_fn = syn::ForeignItemFn {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            sig: Signature {
                constness: None,
                asyncness: None,
                unsafety: None,
                abi: None,
                fn_token: Default::default(),
                ident: ast_ident_to_syn(&f.name),
                generics: Default::default(),
                paren_token: Default::default(),
                inputs,
                variadic: None,
                output,
            },
            semi_token: Default::default(),
        };

        Ok(syn::ItemForeignMod {
            attrs: Vec::new(),
            // `unsafety: None` keeps the block compatible with all Rust
            // editions; functions inside are implicitly unsafe to call.
            unsafety: None,
            abi: syn::Abi {
                extern_token: Default::default(),
                name: Some(syn::LitStr::new("C", ProcSpan::call_site())),
            },
            brace_token: Default::default(),
            items: vec![syn::ForeignItem::Fn(foreign_fn)],
        })
    }

    /// Build a `use <name>;` item for an `extern crate "<name>"`
    /// declaration (T32). The `use` brings the crate's root into scope so
    /// generated code can refer to its items by bare path
    /// (e.g. `serde::Serialize`). We emit `use <name>;` (NOT
    /// `use <name>::*;`) to avoid glob-import lint warnings — callers
    /// qualify paths explicitly.
    fn lower_extern_crate_use(&self, name: &str) -> syn::ItemUse {
        // Build the `use <name>;` item. Crate names may contain `-`
        // (e.g. `rust-decimal`) which is NOT a valid Rust identifier —
        // crates.io normalises `-` to `_` in Rust source. We do the same
        // here so generated `use rust_decimal;` matches what Cargo would
        // expect. (If the user wrote `extern crate "rust-decimal"` the
        // generated `use` becomes `use rust_decimal;`.)
        //
        // In Rust 2018+ `use <crate>;` brings the external crate into scope.
        // In syn this is a `UseTree::Name` (a single-segment path), NOT a
        // `UseTree::Path` with a nested name (which would wrongly emit
        // `use serde::serde;`).
        let rust_ident_name = name.replace('-', "_");
        syn::ItemUse {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            use_token: Default::default(),
            leading_colon: None,
            semi_token: Default::default(),
            tree: syn::UseTree::Name(syn::UseName {
                ident: Ident::new(&rust_ident_name, ProcSpan::call_site()),
            }),
        }
    }

    fn lower_block(&mut self, block: &Block) -> Result<syn::Block, CodegenError> {
        let mut stmts = Vec::with_capacity(block.stmts.len());
        for stmt in &block.stmts {
            stmts.push(self.lower_stmt(stmt)?);
        }
        Ok(syn::Block {
            brace_token: Default::default(),
            stmts,
        })
    }

    fn lower_stmt(&mut self, stmt: &Stmt) -> Result<SynStmt, CodegenError> {
        match stmt {
            Stmt::LetDecl {
                name,
                value,
                mutable,
                ty,
                ..
            } => {
                let ident = ast_ident_to_syn(name);
                let init_expr = self.lower_expr(value)?;

                // T33: Arc-wrap the initializer of a binding captured
                // across a `spawn` boundary. The resulting binding has
                // type `Arc<T>` (rather than `T`); inside spawn bodies
                // uses are lowered to `Arc::clone(&x)` (cheap refcount
                // bump), and any subsequent mutation is lowered to
                // `Arc::make_mut(&mut x)` (copy-on-write). This is how
                // Buff hides the borrow-checker from the user when data
                // is shared between the spawning thread and a spawned
                // task — Rust's `Arc` gives sound shared ownership
                // without exposing `Rc`/`Arc`/`Mutex` syntax in Buff.
                let is_arc_var = self.move_analyzer.is_arc_var(&name.name);
                let init_expr = if is_arc_var {
                    wrap_in_arc_new(init_expr)
                } else {
                    init_expr
                };

                // Run the inferencer on the value so we can emit an
                // explicit Rust type annotation. If the user wrote an
                // explicit Buff annotation (`ty: Some(..)`), prefer it;
                // otherwise fall back to the inferred type (T12).
                //
                // T33: when Arc-wrapping, SKIP the annotation — the
                // binding's actual Rust type is `Arc<T>` (not the `T`
                // the inferencer derives from the pre-wrap initializer),
                // and emitting `let s: String = Arc::new(...)` would be
                // incoherent. Letting Rust infer `Arc<T>` keeps the
                // generated source compiling. (A future task may compute
                // the wrapped type explicitly; for v0.5 inference is
                // simpler and equally correct.)
                let inferred_syn_ty: Option<SynType> = if is_arc_var {
                    None
                } else if let Some(type_ref) = ty {
                    Some(self.ast_typeref_to_syn(type_ref)?)
                } else {
                    // Bind in the inferencer so later statements can see
                    // this name; on error we fall back to no annotation.
                    let inferred = self
                        .type_inferencer
                        .infer_stmt(stmt)
                        .unwrap_or(Type::Unknown);
                    self.buff_type_to_syn(&inferred)
                };

                // Wrap the pattern in `Pat::Type` when an annotation is present
                // so we emit `let x: T = v;` rather than `let x = v;`.
                let pat = match inferred_syn_ty {
                    Some(ty_syn) => Pat::Type(PatType {
                        attrs: Vec::new(),
                        pat: Box::new(Self::make_let_pat(ident, *mutable)),
                        colon_token: Default::default(),
                        ty: Box::new(ty_syn),
                    }),
                    None => Self::make_let_pat(ident, *mutable),
                };
                let local = syn::Local {
                    attrs: Vec::new(),
                    let_token: Default::default(),
                    pat,
                    init: Some(syn::LocalInit {
                        eq_token: Default::default(),
                        expr: Box::new(init_expr),
                        diverge: None,
                    }),
                    semi_token: Default::default(),
                };
                Ok(SynStmt::Local(local))
            }
            Stmt::ExprStmt(expr, _) => {
                let e = self.lower_expr(expr)?;
                Ok(SynStmt::Expr(e, Some(Default::default())))
            }
            Stmt::Return(opt_expr, _) => {
                let return_expr = match opt_expr {
                    Some(expr) => SynExpr::Return(syn::ExprReturn {
                        attrs: Vec::new(),
                        return_token: Default::default(),
                        expr: Some(Box::new(self.lower_expr(expr)?)),
                    }),
                    None => SynExpr::Return(syn::ExprReturn {
                        attrs: Vec::new(),
                        return_token: Default::default(),
                        expr: None,
                    }),
                };
                Ok(SynStmt::Expr(return_expr, Some(Default::default())))
            }
            Stmt::Assignment {
                target, op, value, ..
            } => {
                // The LHS of an assignment is NOT a "use" — it doesn't
                // consume a move. If the target is a bare Ident, lower it
                // directly without consulting the move analyzer.
                //
                // T33: if the target is a bare Ident naming an
                // Arc-shared-and-subsequently-mutated binding (CoW site),
                // wrap it in `Arc::make_mut(&mut x)`. This gives
                // copy-on-write semantics: the inner value is cloned
                // only if the Arc's refcount > 1 (i.e. when the spawned
                // task is actually observing the same Arc); otherwise
                // `make_mut` borrows the value in place with no clone.
                // The resulting LHS is `*Arc::make_mut(&mut x)` so the
                // assignment writes through to the (possibly-cloned)
                // inner value.
                let lhs = if let Expr::Ident(name, _) = &target {
                    if self.move_analyzer.is_arc_mut_var(&name.name) {
                        arc_make_mut_deref(name)
                    } else {
                        SynExpr::Path(syn::ExprPath {
                            attrs: Vec::new(),
                            qself: None,
                            path: syn::Path::from(ast_ident_to_syn(name)),
                        })
                    }
                } else {
                    self.lower_expr(target)?
                };
                let rhs = self.lower_expr(value)?;
                let assign = self.make_binary_op(*op, lhs, rhs)?;
                Ok(SynStmt::Expr(assign, Some(Default::default())))
            }
            Stmt::Break(_) => {
                let brk = SynExpr::Break(syn::ExprBreak {
                    attrs: Vec::new(),
                    break_token: Default::default(),
                    label: None,
                    expr: None,
                });
                Ok(SynStmt::Expr(brk, Some(Default::default())))
            }
            Stmt::Continue(_) => {
                let cont = SynExpr::Continue(syn::ExprContinue {
                    attrs: Vec::new(),
                    continue_token: Default::default(),
                    label: None,
                });
                Ok(SynStmt::Expr(cont, Some(Default::default())))
            }
            Stmt::ForIn {
                var, iter, body, ..
            } => {
                let var_ident = ast_ident_to_syn(var);
                let iter_expr = self.lower_expr(iter)?;
                let body_block = self.lower_block(body)?;
                let pat = Pat::Ident(PatIdent {
                    attrs: Vec::new(),
                    ident: var_ident,
                    by_ref: None,
                    mutability: None,
                    subpat: None,
                });
                let for_loop = SynExpr::ForLoop(syn::ExprForLoop {
                    attrs: Vec::new(),
                    label: None,
                    for_token: Default::default(),
                    pat: Box::new(pat),
                    in_token: Default::default(),
                    expr: Box::new(iter_expr),
                    body: body_block,
                });
                Ok(SynStmt::Expr(for_loop, Some(Default::default())))
            }
            Stmt::ForWhile { cond, body, .. } => {
                // Buff's `for cond { body }` (conditional-loop form) maps
                // directly to Rust's `while cond { body }` (T13).
                let cond_expr = self.lower_expr(cond)?;
                let body_block = self.lower_block(body)?;
                let while_expr = SynExpr::While(syn::ExprWhile {
                    attrs: Vec::new(),
                    label: None,
                    while_token: Default::default(),
                    cond: Box::new(cond_expr),
                    body: body_block,
                });
                Ok(SynStmt::Expr(while_expr, Some(Default::default())))
            }
        }
    }

    fn make_let_pat(ident: Ident, mutable: bool) -> Pat {
        Pat::Ident(PatIdent {
            attrs: Vec::new(),
            ident,
            by_ref: None,
            mutability: mutable.then(Default::default),
            subpat: None,
        })
    }

    fn lower_expr(&mut self, expr: &Expr) -> Result<SynExpr, CodegenError> {
        match expr {
            Expr::Literal(lit, _) => self.lower_literal(lit),
            Expr::Ident(name, _) => {
                let path = syn::ExprPath {
                    attrs: Vec::new(),
                    qself: None,
                    path: syn::Path::from(ast_ident_to_syn(name)),
                };
                // T33: Arc-shared binding captured inside a spawn body —
                // emit `Arc::clone(&x)` instead of moving or deep-cloning.
                // The Arc wrap was inserted at the binding's `let` site
                // (see [`Self::lower_stmt`]`'s LetDecl arm); here we grab
                // a cheap refcount-bumping clone so the spawned task owns
                // its own `Arc<T>` handle to the shared data.
                if self.spawn_depth > 0 && self.move_analyzer.is_arc_var(&name.name) {
                    return Ok(arc_clone_call(name));
                }
                // T34: if this ident is a variable CAPTURED by the closure
                // whose body we're currently lowering, emit it plainly
                // WITHOUT consulting [`MoveAnalyzer::needs_clone`]. Rust
                // closures handle capture (by ref or by move) automatically;
                // Buff must not insert a spurious `.clone()` for uses of a
                // captured variable INSIDE the closure body. Without this
                // guard, a non-Copy captured var used twice inside a
                // closure would get a wrong `.clone()` on its second use.
                if self.is_captured_in_closure(&name.name) {
                    return Ok(SynExpr::Path(path));
                }
                if self.move_analyzer.needs_clone(&name.name) {
                    // Insert `.clone()` so this use is valid after a prior move.
                    Ok(SynExpr::MethodCall(syn::ExprMethodCall {
                        attrs: Vec::new(),
                        receiver: Box::new(SynExpr::Path(path)),
                        dot_token: Default::default(),
                        method: Ident::new("clone", ProcSpan::call_site()),
                        turbofish: None,
                        paren_token: Default::default(),
                        args: Default::default(),
                    }))
                } else {
                    Ok(SynExpr::Path(path))
                }
            }
            Expr::BinaryOp { op, lhs, rhs, .. } => {
                let lhs = self.lower_expr(lhs)?;
                let rhs = self.lower_expr(rhs)?;
                self.make_binary_op(*op, lhs, rhs)
            }
            Expr::UnaryOp { op, operand, .. } => {
                let operand = self.lower_expr(operand)?;
                self.make_unary_op(*op, operand)
            }
            Expr::FuncCall { callee, args, .. } => {
                // T96: standard-library prelude. A bare-ident callee whose
                // name is a recognised prelude function is lowered to the
                // corresponding Rust idiom (math, conversion, I/O) WITHOUT
                // requiring an `import` in Buff source. The mapping table
                // lives in [`RustCodegen::lower_prelude_call`].
                //
                // T13 legacy: `print(x)` was originally special-cased here
                // to `println!("{}", x)`. T96 generalises that to the full
                // prelude AND tightens the string-literal case so
                // `print("hello")` now emits `println!("hello")` (no `{}`).
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if let Some(fn_) = buff_lang_types::prelude::lookup(&name.name) {
                        return self.lower_prelude_call(fn_, args);
                    }
                    // T30: `Error("msg")` is a prelude error constructor
                    // (NOT a reserved keyword and NOT a user function). It
                    // lowers to `Err(Error::new(arg))` so it produces a
                    // `Result<_, Error>` value directly — letting
                    // `return Error("msg")` early-return an Err without the
                    // user writing `Err(...)` themselves. The builtin `Error`
                    // struct is emitted on-demand by [`Self::generate`] when
                    // this constructor appears (mirroring the Matrix
                    // emit-on-demand pattern from T24).
                    if name.name == "Error" && args.len() == 1 {
                        return self.lower_error_constructor(args);
                    }
                    // T31: `block(expr)` is a prelude-style async-blocking
                    // form. It runs an async expression synchronously by
                    // spinning up a one-shot tokio runtime and calling
                    // `.block_on(expr)` on it. Inside an async fn this is a
                    // DEADLOCK RISK (the runtime can't run the future while
                    // the current task holds the worker thread), so we
                    // emit a warning diagnostic AND still lower it (the
                    // user gets the warning + the (broken) Rust; they can
                    // then refactor). `block` is NOT a reserved keyword
                    // — it's a builtin name resolved like a prelude fn.
                    if name.name == "block" && args.len() == 1 {
                        return self.lower_block_call(&args[0]);
                    }
                }

                // A function name (bare Ident callee) is NOT a variable
                // use — it doesn't consume a move. Lower it without
                // consulting the move analyzer; other callee shapes
                // (MethodCall, etc.) go through the normal path.
                let callee_is_async = matches!(
                    callee.as_ref(),
                    Expr::Ident(name, _) if self.async_fns.contains(&name.name)
                );
                let callee = match callee.as_ref() {
                    Expr::Ident(name, _) => SynExpr::Path(syn::ExprPath {
                        attrs: Vec::new(),
                        qself: None,
                        path: syn::Path::from(ast_ident_to_syn(name)),
                    }),
                    _ => self.lower_expr(callee)?,
                };
                let mut lowered: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                for a in args {
                    lowered.push(self.lower_expr(a)?);
                }
                let call = SynExpr::Call(syn::ExprCall {
                    attrs: Vec::new(),
                    func: Box::new(callee),
                    paren_token: Default::default(),
                    args: lowered,
                });

                // T31: AUTO-INSERT `.await` at async call sites. Buff has
                // no `await` keyword; the codegen inserts `.await` when:
                //   - the callee is a bare Ident naming an async fn (per
                //     the propagated async set), AND
                //   - we're currently in an async context (the current fn
                //     is async OR we're inside an `async move { ... }`
                //     block — e.g. inside a `spawn` body).
                // This is the ONLY place `.await` is emitted by the call-
                // site rule; `t.result()` → `.await` is a separate path
                // in `lower_method_call`.
                if callee_is_async && self.in_async_context() {
                    Ok(make_await(call))
                } else {
                    Ok(call)
                }
            }
            Expr::IfExpr {
                cond,
                then_block,
                else_block,
                ..
            } => self.lower_if_expr(cond, then_block, else_block.as_ref()),
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => self.lower_method_call(receiver, method, args),
            Expr::StringInterp { parts, .. } => self.lower_string_interp(parts),
            // T23: `[e1, e2, ...]` -> Rust `vec![e1, e2, ...]` macro.
            Expr::ArrayLit { elements, .. } => self.lower_array_lit(elements),
            // T23/T24: Indexing dispatches on index arity.
            // - 1 index (`v[i]`) → Rust `v[i as usize]` (Vector path).
            // - 2 indices (`m[row, col]`) → Rust
            //   `m.data[(row * m.cols + col) as usize]` (flat-storage Matrix
            //   path). The `.data` / `.cols` fields come from the builtin
            //   `Matrix<T>` struct this same codegen emits when a program uses
            //   `Matrix.new(...)`. The flat index is `row * cols + col`
            //   (row-major), cast to `usize` once at the end so any Buff
            //   integer-typed indices work. The base expression is lowered
            //   ONCE and spliced (via clone) into both field-access positions
            //   so the move analyzer's clone decision is preserved.
            Expr::Index { base, indices, .. } => {
                if indices.len() == 2 {
                    self.lower_matrix_index(base, &indices[0], &indices[1])
                } else if indices.len() == 1 {
                    let base_e = self.lower_expr(base)?;
                    let index_e = cast_to_usize(self.lower_expr(&indices[0])?);
                    Ok(SynExpr::Index(syn::ExprIndex {
                        attrs: Vec::new(),
                        expr: Box::new(base_e),
                        bracket_token: Default::default(),
                        index: Box::new(index_e),
                    }))
                } else {
                    Err(self.unsupported(&format!(
                        "indexing with {} indices (only 1 or 2 supported)",
                        indices.len()
                    )))
                }
            }
            // T23: a minimal closure `{ params => expr }` -> Rust
            // `|p1, p2| body`. Param types are inferred by Rust; we emit no
            // type annotations. The body is a single expression (the parser
            // wraps it in a one-statement block).
            Expr::Lambda { params, body, .. } => self.lower_lambda(params, body),
            // T25: a map literal `{"k": v, ...}` -> Rust
            // `std::collections::HashMap::from([("k", v), ...])`. We use the
            // fully-qualified path so generated programs need no `use`
            // import (avoids import management in v0.5). Each entry becomes
            // a Rust tuple `(key, value)`; the outer `[...]` is a const-eval
            // array literal that `HashMap::from` consumes.
            Expr::MapLit { entries, .. } => self.lower_map_lit(entries),
            // T26: struct init `Type { field: value, ... }` → Rust struct
            // expression of the same shape. Each field is lowered as a
            // `field: value` pair inside `Type { ... }`. This mirrors the
            // source form 1:1 because Buff deliberately matches Rust's
            // struct-init syntax ( braces + named fields + colon ).
            Expr::StructInit {
                type_name, fields, ..
            } => self.lower_struct_init(type_name, fields),
            // T27: `match scrutinee { arms }` → Rust `match scrutinee { arms }`.
            // Each arm lowers `pattern => body` to the same Rust shape so the
            // source form maps 1:1 to Rust (Buff deliberately matches Rust's
            // match syntax). Patterns are lowered via [`Self::lower_pattern`]:
            // wildcard → `_`, ident → ident (resolves as variant or binding),
            // variant tuple → `Variant(subpats)`, literal → literal.
            Expr::MatchExpr {
                scrutinee, arms, ..
            } => self.lower_match_expr(scrutinee, arms),
            // T30: `expr?` → Rust's NATIVE `?` operator (`<expr>?`). This is
            // the cleanest mapping: Buff functions that use `?` already lower
            // to Rust functions returning `Result<T, E>`, which is exactly
            // what Rust's `?` requires. The explicit
            // `match expr { Ok(v) => v, Err(e) => return Err(e) }` desugaring
            // is NOT used; native `?` is simpler and equally correct. See the
            // REFACTOR note on [`Self::lower_try`] for the extracted helper.
            Expr::Try { expr, .. } => self.lower_try(expr),
            // T31: `spawn expr` → Rust's `tokio::spawn(async move { expr })`.
            // The operand becomes the body of an `async move` closure so the
            // task owns all captured variables (Buff hides borrow-checker
            // pain from users; the generated Rust must be move-clean). The result
            // is a `tokio::task::JoinHandle<T>` — Buff's `Task<T>` is a thin
            // alias for this type, and the only `.await` on a Task lands at the
            // `t.result()` site (see [`Self::lower_method_call`]).
            Expr::Spawn { task, .. } => self.lower_spawn(task),
            // T68: `start..end` (exclusive) or `start..=end` (inclusive) → Rust
            // range expression. Built via `quote!` so the `..` / `..=` operator
            // is constructed from real `syn` tokens.
            Expr::Range {
                start,
                end,
                inclusive,
                ..
            } => self.lower_range(start, end, *inclusive),
            _ => Err(self.unsupported(&format!("expr codegen not yet implemented for {:?}", expr))),
        }
    }

    fn lower_if_expr(
        &mut self,
        cond: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Result<SynExpr, CodegenError> {
        let cond_expr = self.lower_expr(cond)?;
        let then_branch = self.lower_block(then_block)?;
        let else_branch = match else_block {
            Some(b) => Some((
                Default::default(),
                Box::new(SynExpr::Block(syn::ExprBlock {
                    attrs: Vec::new(),
                    label: None,
                    block: self.lower_block(b)?,
                })),
            )),
            None => None,
        };
        Ok(SynExpr::If(syn::ExprIf {
            attrs: Vec::new(),
            if_token: Default::default(),
            cond: Box::new(cond_expr),
            then_branch,
            else_branch,
        }))
    }

    /// Lower a Buff **prelude** call to the corresponding Rust idiom (T96).
    ///
    /// The prelude is the implicit standard library — these functions are
    /// available in every Buff program without an `import`. The mappings are
    /// grouped by category (matching [`buff_lang_types::prelude`]):
    ///
    /// # Math
    ///
    /// | Buff            | Rust                              | Notes             |
    /// |-----------------|-----------------------------------|-------------------|
    /// | `abs(x)`        | `(x).abs()`                       | works for any numeric (i64 / f32 / f64) |
    /// | `min(a, b)`     | `(a).min(b)`                      | `Ord::min` for ints, inherent `min` for floats |
    /// | `max(a, b)`     | `(a).max(b)`                      | analogous         |
    /// | `sqrt(x)`       | `((x) as f64).sqrt()`             | always returns `f64`; coerce arg up so int args work |
    /// | `floor(x)`      | `((x) as f64).floor()`            | always returns `f64` |
    /// | `ceil(x)`       | `((x) as f64).ceil()`             | always returns `f64` |
    /// | `round(x)`      | `((x) as f64).round()`            | always returns `f64` |
    /// | `pow(b, e)`     | `(b).powf((e) as f64)` if `b` is float-like; else `(b).pow((e) as u32)` | the inferencer picks the arm |
    ///
    /// # Type conversions
    ///
    /// The arg's inferred type drives the Rust idiom:
    ///
    /// | Buff            | Rust (arg is String)               | Rust (arg is numeric)         |
    /// |-----------------|------------------------------------|-------------------------------|
    /// | `Int(x)`        | `x.parse::<i64>().unwrap_or(0)`    | `(x) as i64`                  |
    /// | `Float(x)`      | `x.parse::<f32>().unwrap_or(0.0)`  | `(x) as f32`                  |
    /// | `Bool(x)`       | `x.parse::<bool>().unwrap_or(false)` | `(x) != 0`                  |
    /// | `String(x)`     | `x.to_string()` (any `Display`)    | `x.to_string()`              |
    ///
    /// **Parse-failure policy (v0.5):** `Int("bad")` returns `0`,
    /// `Float("bad")` returns `0.0`, `Bool("bad")` returns `false`. We use
    /// `unwrap_or` (not `expect`/`unwrap`) so generated code never panics
    /// on malformed runtime input. A proper `Result`-returning conversion
    /// API is deferred — see T96 notes in `decisions.md`.
    ///
    /// # I/O
    ///
    /// | Buff                  | Rust                                                        |
    /// |-----------------------|-------------------------------------------------------------|
    /// | `print("lit")`        | `println!("lit")` (a bare string literal drops the `{}`)    |
    /// | `print(x)` / `println(x)` (non-literal) | `println!("{}", x)`                          |
    /// | `read_line()`         | `{ let mut s = String::new(); std::io::stdin().read_line(&mut s).ok(); s }` |
    ///
    /// `read_line()` swallows the trailing newline (matches Rust's
    /// `read_line` semantics — callers can `.trim_end()` if they want it
    /// gone). The `.ok()` discards the `io::Result` error as `Some(())`/
    /// `None` (we don't panic on I/O failure).
    fn lower_prelude_call(
        &mut self,
        fn_: PreludeFn,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        match fn_ {
            // ----- Math ---------------------------------------------------
            PreludeFn::Abs => {
                self.lower_one_arg_method(args, "abs", /*wrap_parens*/ true)
            }
            PreludeFn::Min => self.lower_min_max(args, "min"),
            PreludeFn::Max => self.lower_min_max(args, "max"),
            PreludeFn::Sqrt => self.lower_float_unary(args, "sqrt"),
            PreludeFn::Floor => self.lower_float_unary(args, "floor"),
            PreludeFn::Ceil => self.lower_float_unary(args, "ceil"),
            PreludeFn::Round => self.lower_float_unary(args, "round"),
            PreludeFn::Pow => self.lower_pow(args),

            // ----- Conversions -------------------------------------------
            PreludeFn::Int => self.lower_convert(args, "i64", ConvKind::Numeric),
            PreludeFn::Float => self.lower_convert(args, "f32", ConvKind::Numeric),
            PreludeFn::Bool => self.lower_convert(args, "bool", ConvKind::Bool),
            PreludeFn::String => self.lower_to_string(args),

            // ----- I/O ---------------------------------------------------
            PreludeFn::Print => self.lower_print(args),
            PreludeFn::Println => self.lower_print(args),
            PreludeFn::ReadLine => Ok(self.lower_read_line()),

            // ----- System / environment (T99) ---------------------------
            // args() → std::env::args().collect::<Vec<String>>()
            PreludeFn::Args => {
                if !args.is_empty() {
                    return Err(self.unsupported("args() takes no arguments"));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::args().collect::<Vec<String>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("args() codegen parse: {e}")))
            }
            // env("NAME") → std::env::var("NAME").ok()
            PreludeFn::Env => {
                if args.len() != 1 {
                    return Err(
                        self.unsupported("env() expects exactly 1 argument (the variable name)")
                    );
                }
                let arg = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::var(#arg).ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("env() codegen parse: {e}")))
            }
            // exit(code) → std::process::exit(code)
            PreludeFn::Exit => {
                if args.len() != 1 {
                    return Err(
                        self.unsupported("exit() expects exactly 1 argument (the exit code)")
                    );
                }
                let arg = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::process::exit(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("exit() codegen parse: {e}")))
            }
            // T35: assert_eq(a, b) → assert_eq!(a, b)
            // The Rust `assert_eq!` macro panics when the two args are not
            // equal, which is exactly Buff's semantics. Used inside `@test`
            // functions (where the test runner catches the panic).
            PreludeFn::AssertEq => {
                if args.len() != 2 {
                    return Err(self.unsupported(
                        "assert_eq() expects exactly 2 arguments (actual, expected)",
                    ));
                }
                let lhs = self.lower_expr(&args[0])?;
                let rhs = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    assert_eq!(#lhs, #rhs)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("assert_eq() codegen parse: {e}")))
            }
        }
    }

    /// Lower `abs(x)` → `(x).abs()`. Wrapping the receiver in parens
    /// ensures integer literals like `5` lower to `(5).abs()` rather than
    /// the ambiguous `5.abs()` (which Rust parses as a field access on a
    /// float literal `5.`).
    fn lower_one_arg_method(
        &mut self,
        args: &[Expr],
        method: &str,
        wrap_parens: bool,
    ) -> Result<SynExpr, CodegenError> {
        let recv = self.lower_one_arg(args)?;
        let recv = if wrap_parens {
            wrap_in_parens(recv)
        } else {
            recv
        };
        Ok(method_call_no_args(recv, method))
    }

    /// Lower `min(a, b)` / `max(a, b)` → `(a).<method>(b)`.
    fn lower_min_max(&mut self, args: &[Expr], method: &str) -> Result<SynExpr, CodegenError> {
        if args.len() != 2 {
            return Err(self.unsupported(&format!(
                "{method} expects exactly 2 args, got {}",
                args.len()
            )));
        }
        let a = wrap_in_parens(self.lower_expr(&args[0])?);
        let b = self.lower_expr(&args[1])?;
        Ok(method_call_one_arg(a, method, b))
    }

    /// Lower a float-returning unary math fn (`sqrt`/`floor`/`ceil`/`round`)
    /// to `((x) as f64).<method>()`. Coercing to `f64` first means int args
    /// compile without requiring the user to write `x as Double` manually.
    fn lower_float_unary(&mut self, args: &[Expr], method: &str) -> Result<SynExpr, CodegenError> {
        let recv = self.lower_one_arg(args)?;
        let as_f64 = cast_to(recv, "f64");
        Ok(method_call_no_args(as_f64, method))
    }

    /// Lower `pow(base, exp)` — picks `.powf` for float bases and `.pow` for
    /// integer bases (Rust's `i64::pow` takes `u32`, hence the `as u32` cast).
    fn lower_pow(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 2 {
            return Err(
                self.unsupported(&format!("pow expects exactly 2 args, got {}", args.len()))
            );
        }
        let base = wrap_in_parens(self.lower_expr(&args[0])?);
        let exp_raw = self.lower_expr(&args[1])?;
        // Infer the base type to choose `.pow` vs `.powf`. Inference errors
        // fall back to the integer form (which works for the common case).
        let base_ty = self
            .type_inferencer
            .infer_expr(&args[0])
            .unwrap_or(Type::Unknown);
        if base_ty.is_float_like() {
            let exp = cast_to(exp_raw, "f64");
            Ok(method_call_one_arg(base, "powf", exp))
        } else {
            let exp = cast_to(exp_raw, "u32");
            Ok(method_call_one_arg(base, "pow", exp))
        }
    }

    /// Lower a type conversion (`Int(x)` / `Float(x)` / `Bool(x)`).
    ///
    /// For String args we emit `.parse::<T>().unwrap_or(default)`; for
    /// numeric args we emit `(x) as T`. The `Bool` arm uses `x != 0` for
    /// numerics (Rust has no `as bool` cast) and `.parse::<bool>()` for
    /// strings.
    fn lower_convert(
        &mut self,
        args: &[Expr],
        target: &str,
        kind: ConvKind,
    ) -> Result<SynExpr, CodegenError> {
        let arg = self.lower_one_arg(args)?;
        // Infer the arg's type to dispatch on the source category.
        let arg_ty = self
            .type_inferencer
            .infer_expr(&args[0])
            .unwrap_or(Type::Unknown);
        if matches!(arg_ty, Type::String) {
            // String → parse
            return Ok(parse_with_default(arg, target, &kind));
        }
        // Non-string → numeric coercion (`as T`) for Int/Float, or `!= 0` for Bool.
        match kind {
            ConvKind::Numeric => Ok(cast_to(arg, target)),
            ConvKind::Bool => {
                // `(x) != 0` — wrap the arg in parens so compound exprs bind right.
                let zero = make_int_lit_expr(0);
                Ok(make_binary_expr(
                    syn::BinOp::Ne(Default::default()),
                    wrap_in_parens(arg),
                    zero,
                ))
            }
        }
    }

    /// Lower `String(x)` → `(x).to_string()`. Works for any Rust `Display` type.
    fn lower_to_string(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        let recv = self.lower_one_arg(args)?;
        Ok(method_call_no_args(recv, "to_string"))
    }

    /// Lower `print(x)` / `println(x)`.
    ///
    /// A bare string-literal arg lowers to `println!("the literal text")`
    /// — no `{}` placeholder (T96 acceptance). Any other arg lowers to
    /// `println!("{}", x)`.
    fn lower_print(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!(
                "print/println expect exactly 1 arg, got {}",
                args.len()
            )));
        }
        // String-literal fast path: print("hello") → println!("hello").
        if let Expr::Literal(Literal::String(text), _) = &args[0] {
            return Ok(make_println_macro_literal(text));
        }
        // General path: print(x) → println!("{}", x).
        let arg = self.lower_expr(&args[0])?;
        Ok(make_println_macro(arg))
    }

    /// Lower `read_line()` → a block expression that reads one line of stdin.
    ///
    /// Emits (conceptually):
    /// ```text
    /// {
    ///     let mut __buff_prelude_line = String::new();
    ///     std::io::stdin().read_line(&mut __buff_prelude_line).ok();
    ///     __buff_prelude_line
    /// }
    /// ```
    ///
    /// The block is built via `quote!` and then re-parsed via
    /// `syn::parse2` (which returns a `Result`, unlike `parse_quote!`'s
    /// panic). The placeholder name `__buff_prelude_line` is intentionally
    /// ugly to avoid colliding with any user binding.
    fn lower_read_line(&self) -> SynExpr {
        let tokens: proc_macro2::TokenStream = quote::quote! {{
            let mut __buff_prelude_line = String::new();
            std::io::stdin().read_line(&mut __buff_prelude_line).ok();
            __buff_prelude_line
        }};
        // `quote!`'s `{{...}}` produces a Rust block-expression token
        // stream; re-parse it as a `syn::Expr` (the top-level enum) so it
        // slots into the surrounding expression context. On the (unreachable)
        // parse failure we fall back to a bare `String::new()` call.
        match syn::parse2::<SynExpr>(tokens) {
            Ok(e) => e,
            Err(_) => {
                // Defensive fallback: never panic in codegen. The quote!
                // above is a compile-time-fixed template so a parse failure
                // is a codegen bug, not a user-facing condition.
                let path = rust_path("String::new");
                SynExpr::Call(syn::ExprCall {
                    attrs: Vec::new(),
                    func: Box::new(SynExpr::Path(syn::ExprPath {
                        attrs: Vec::new(),
                        qself: None,
                        path,
                    })),
                    paren_token: Default::default(),
                    args: Default::default(),
                })
            }
        }
    }

    /// Lower exactly one argument, returning an error if the arg count is wrong.
    fn lower_one_arg(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!("expected exactly 1 arg, got {}", args.len())));
        }
        self.lower_expr(&args[0])
    }

    ///
    /// T21 — string methods. The following Buff method names map to specific
    /// Rust idioms (none of them is a literal `recv.method(args)` because
    /// Rust strings don't expose these names directly):
    ///
    /// | Buff                  | Rust                                              |
    /// |-----------------------|---------------------------------------------------|
    /// | `s.char_count()`      | `s.chars().count()`                               |
    /// | `s.byte_len()`        | `s.len()`                                         |
    /// | `s.chars()`           | `s.chars()`                                       |
    /// | `s.bytes()`           | `s.bytes()`                                       |
    /// | `s.graphemes()`       | `unicode_segmentation::UnicodeSegmentation::graphemes(s, true).collect::<String>()` — see note below |
    /// | `s.first()`           | `s.chars().next()`                                |
    /// | `s.last()`            | `s.chars().last()`                                |
    /// | `s.slice(a, b)`       | char-safe slice via `s.chars().skip(a).take(b - a).collect()` |
    ///
    /// `graphemes()` is special-cased to return a `String` (a flattened
    /// representation) for now; a true iterator-returning API will need a
    /// dedicated AST shape (deferred to a later task — see notes).
    ///
    /// Any unrecognised method falls through to a plain `recv.method(args)`
    /// Rust method call, which is correct for arbitrary user-defined methods
    /// and the methods of future types.
    fn lower_method_call(
        &mut self,
        receiver: &Expr,
        method: &buff_lang_ast::common::Ident,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        // T31: `task.result()` → Rust's `task.await`. This is the ONLY
        // `.await` form that originates from a method-call position; it's
        // the suspension-point API on Buff's `Task<T>` (a thin alias for
        // `tokio::task::JoinHandle<T>`). The check MUST run BEFORE the T26
        // field-access-vs-method-call heuristic below, because `result()`
        // is a zero-arg method call and would otherwise be rewritten as a
        // field access `task.result` (which is meaningless on a JoinHandle).
        // We accept both `task.result()` (with parens) and the postfix-
        // form `task.result` (no parens — same AST shape per the parser's
        // Dot arm) by NOT gating on `args.is_empty()` here.
        if method.name == "result" {
            let recv = self.lower_expr(receiver)?;
            return Ok(make_await(recv));
        }

        // T26 field-access-vs-method-call disambiguation.
        //
        // Buff parses `obj.field` and `obj.method()` through the SAME AST
        // shape (`Expr::MethodCall { receiver, method, args }`) — see the
        // parser's `parse_postfix` Dot arm: a `.` followed by an Ident
        // WITHOUT parens produces a zero-arg MethodCall. So `p.name` (a
        // field access on a user struct) and `v.len()` (a real method call)
        // are indistinguishable at the AST level.
        //
        // Heuristic (T26): when `args` is empty AND `method.name` is NOT in
        // the [`KNOWN_ZERO_ARG_METHODS`] allow-list, emit a Rust field
        // access `recv.field` instead of a method call `recv.field()`. This
        // is the additive-only approach: no AST migration required, no new
        // FieldAccess variant — just a codegen-time rewrite. A dedicated
        // `Expr::FieldAccess` AST node is the cleaner long-term shape (see
        // migration note in `decisions.md`), deferred to keep T26 additive.
        //
        // Trade-off: if a user defines a struct with a field literally named
        // `len` / `push` / etc., `obj.len` will emit `obj.len()` (wrong). The
        // allow-list is conservative (only names this codegen actually
        // handles + the universal `clone`/`to_string`/etc.); new builtins
        // added later must be added to the list to preserve the heuristic.
        if args.is_empty() && !KNOWN_ZERO_ARG_METHODS.contains(&method.name.as_str()) {
            let recv = self.lower_expr(receiver)?;
            return Ok(field_access(recv, &method.name));
        }

        // T24: `Matrix.new(rows, cols)` — the builtin Matrix constructor.
        // Buff's constructor convention is `Type.new()` / `Type.from()` (§7),
        // parsed as a MethodCall whose receiver is a bare Ident naming the
        // type. We special-case `Matrix.new(...)` here to lower it to Rust's
        // `Matrix::new(rows, cols)` associated-function call. The `Matrix<T>`
        // struct definition itself is emitted on-demand by
        // [`Self::generate`] when a program uses `Matrix.new(...)`.
        if method.name == "new" {
            if let Expr::Ident(id, _) = receiver {
                if id.name == "Matrix" {
                    return self.lower_matrix_new(args);
                }
            }
        }

        let recv = self.lower_expr(receiver)?;
        let method_name = method.name.as_str();

        // Helper: lower `args` into a Punctuated list.
        let lower_args =
            |codegen: &mut Self| -> Result<Punctuated<SynExpr, syn::Token![,]>, CodegenError> {
                let mut out: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                for a in args {
                    out.push(codegen.lower_expr(a)?);
                }
                Ok(out)
            };

        // String-method mappings.
        let lowered = match method_name {
            // `s.char_count()` → `s.chars().count()`
            "char_count" if args.is_empty() => {
                self.method_chain(recv, &["chars", "count"], None)?
            }
            // `s.byte_len()` → `s.len()`
            "byte_len" if args.is_empty() => self.method_chain(recv, &["len"], None)?,
            // `s.chars()` → `s.chars()`
            "chars" if args.is_empty() => self.method_chain(recv, &["chars"], None)?,
            // `s.bytes()` → `s.bytes()`
            "bytes" if args.is_empty() => self.method_chain(recv, &["bytes"], None)?,
            // `s.first()` → `s.chars().next()`
            "first" if args.is_empty() => self.method_chain(recv, &["chars", "next"], None)?,
            // `s.last()` → `s.chars().last()`
            "last" if args.is_empty() => self.method_chain(recv, &["chars", "last"], None)?,
            // `s.graphemes()` → grapheme iterator wrapped via unicode-segmentation.
            // For now we return a flattened String (`.collect()`) so callers
            // can treat the result as a `String` without dragging the trait
            // into every scope. A future task will introduce a typed iterator.
            "graphemes" if args.is_empty() => self.lower_graphemes_call(recv)?,
            // `s.slice(a, b)` → char-safe slice.
            // Approach: `s.chars().skip(a).take(b - a).collect::<String>()`.
            // We lower the two integer arguments and emit the chain. If `b`
            // is not provided, we use `s.chars().skip(a).collect::<String>()`.
            "slice" => self.lower_slice_call(recv, args)?,
            // T23: Vector iteration methods. `.map` / `.filter` take a single
            // closure and return a new `Vec`; `.reduce` takes a 2-arg closure
            // and returns `Option<T>`. We use `.into_iter()` so the closure
            // params are owned values (Buff hides references from users).
            // `.push(x)` / `.pop()` / `.len()` need no special mapping —
            // they fall through to the default `recv.method(args)` arm below.
            "map" if args.len() == 1 => {
                let f = self.lower_expr(&args[0])?;
                self.lower_into_iter_collect(recv, "map", f)?
            }
            "filter" if args.len() == 1 => {
                let f = self.lower_expr(&args[0])?;
                self.lower_into_iter_collect(recv, "filter", f)?
            }
            "reduce" if args.len() == 1 => {
                let f = self.lower_expr(&args[0])?;
                self.lower_into_iter_reduce(recv, f)?
            }
            // T25: Map methods. The Buff names map to Rust's standard
            // HashMap methods, except `.contains(k)` → `.contains_key(k)`
            // (Buff hides the `_key` suffix for ergonomics). `.get(k)`
            // returns `Option<&V>` in Rust; we keep it as-is (`Option<&V>`)
            // for v0.5 — a future task may add `.cloned()` to recover an
            // owned `Option<V>` if the move-by-default analysis requires it.
            // `.insert(k, v)`, `.remove(k)`, and `.len()` pass through
            // unchanged because Buff and Rust share those names.
            "contains" if args.len() == 1 => {
                let arg = self.lower_expr(&args[0])?;
                method_call_one_arg(recv, "contains_key", arg)
            }
            // `.get`, `.insert`, `.remove`, `.len` all share Rust's name —
            // they fall through to the default arm below with no special
            // mapping. We keep this comment block to document the T25 design
            // (so a future change doesn't accidentally rewrite these).
            // Default: a plain method call `recv.method(args)`.
            _ => {
                let args_punct = lower_args(self)?;
                SynExpr::MethodCall(syn::ExprMethodCall {
                    attrs: Vec::new(),
                    receiver: Box::new(recv),
                    dot_token: Default::default(),
                    method: Ident::new(method_name, ProcSpan::call_site()),
                    turbofish: None,
                    paren_token: Default::default(),
                    args: args_punct,
                })
            }
        };
        Ok(lowered)
    }

    /// Build a chained method call: `recv.m1().m2()...` (no args at any link).
    /// If `final_method` is given, it's used as the OUTERMOST call name (the
    /// last element of `methods` overrides it; passing `None` is equivalent).
    fn method_chain(
        &self,
        recv: SynExpr,
        methods: &[&str],
        _final_method: Option<&str>,
    ) -> Result<SynExpr, CodegenError> {
        let mut acc = recv;
        for &m in methods {
            acc = SynExpr::MethodCall(syn::ExprMethodCall {
                attrs: Vec::new(),
                receiver: Box::new(acc),
                dot_token: Default::default(),
                method: Ident::new(m, ProcSpan::call_site()),
                turbofish: None,
                paren_token: Default::default(),
                args: Default::default(),
            });
        }
        Ok(acc)
    }

    /// Lower `s.graphemes()` to a grapheme-iteration expression that yields a
    /// `String` of concatenated grapheme clusters.
    ///
    /// Emits (conceptually):
    /// ```text
    /// unicode_segmentation::UnicodeSegmentation::graphemes(&s, true)
    ///     .collect::<String>()
    /// ```
    ///
    /// The call is built as a `quote!`-expanded token stream so we never
    /// hand-format Rust. The trait must be in scope at the use site — see
    /// the generated-crate wiring note in T21 deferral.
    fn lower_graphemes_call(&self, recv: SynExpr) -> Result<SynExpr, CodegenError> {
        // We use quote! to build the macro-shaped expression. The receiver
        // is spliced in via `#recv`. The full path avoids needing a `use`
        // import in the generated crate.
        let tokens: proc_macro2::TokenStream =
            syn::parse_str("unicode_segmentation::UnicodeSegmentation::graphemes(&__recv, true)")
                .map_err(|e| self.unsupported(&format!("graphemes path parse: {e}")))?;
        // Manually build: __trait_path::graphemes(&recv, true).collect::<String>()
        // by constructing an ExprMethodCall for `.collect::<String>()`.
        let graphemes_call = splice_receiver_into_call(tokens, recv)?;
        let collect_call = SynExpr::MethodCall(syn::ExprMethodCall {
            attrs: Vec::new(),
            receiver: Box::new(graphemes_call),
            dot_token: Default::default(),
            method: Ident::new("collect", ProcSpan::call_site()),
            // turbofish: `::<String>`
            turbofish: Some(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: Default::default(),
                args: {
                    let mut p: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
                    p.push(syn::GenericArgument::Type(rust_path_type("String")));
                    p
                },
                gt_token: Default::default(),
            }),
            paren_token: Default::default(),
            args: Default::default(),
        });
        Ok(collect_call)
    }

    /// Lower `s.slice(a, b)` to a char-safe slice expression.
    ///
    /// Emits (conceptually) `s.chars().skip(a).take(b - a).collect::<String>()`.
    /// A single-arg form `s.slice(a)` becomes `s.chars().skip(a).collect::<String>()`.
    fn lower_slice_call(&mut self, recv: SynExpr, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.is_empty() || args.len() > 2 {
            return Err(self.unsupported(&format!(
                "slice expects 1 or 2 integer args, got {}",
                args.len()
            )));
        }
        // Start: `s.chars()`
        let chars_call = self.method_chain(recv, &["chars"], None)?;
        // `.skip(a)`
        let skip_arg = self.lower_expr(&args[0])?;
        let skip_call = method_call_one_arg(chars_call, "skip", skip_arg);
        // `.take(b - a)` if a second arg is present; else just chain collect.
        let after_take = if args.len() == 2 {
            let b_arg = self.lower_expr(&args[1])?;
            // Compute `b - a` as a Rust binary subtraction at runtime so the
            // arguments don't have to be literals.
            let b_minus_a = SynExpr::Binary(syn::ExprBinary {
                attrs: Vec::new(),
                left: Box::new(b_arg),
                op: syn::BinOp::Sub(Default::default()),
                right: Box::new(self.lower_expr(&args[0])?),
            });
            method_call_one_arg(skip_call, "take", b_minus_a)
        } else {
            skip_call
        };
        // `.collect::<String>()`
        let collect_call = SynExpr::MethodCall(syn::ExprMethodCall {
            attrs: Vec::new(),
            receiver: Box::new(after_take),
            dot_token: Default::default(),
            method: Ident::new("collect", ProcSpan::call_site()),
            turbofish: Some(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: Default::default(),
                args: {
                    let mut p: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
                    p.push(syn::GenericArgument::Type(rust_path_type("String")));
                    p
                },
                gt_token: Default::default(),
            }),
            paren_token: Default::default(),
            args: Default::default(),
        });
        Ok(collect_call)
    }

    /// Lower a Vector iteration method that returns a new `Vec` (T23).
    ///
    /// `recv.<method>(closure)` → `recv.into_iter().<method>(closure).collect::<Vec<_>>()`.
    /// Used by `.map` and `.filter`. We use `.into_iter()` so the closure
    /// receives owned values (Buff hides references from users); this
    /// consumes the receiver, matching Buff's move-by-default semantics.
    /// The `.collect::<Vec<_>>()` rebuilds a Vec so the result can be indexed
    /// or chained further.
    fn lower_into_iter_collect(
        &self,
        recv: SynExpr,
        method: &str,
        closure: SynExpr,
    ) -> Result<SynExpr, CodegenError> {
        let method_ident = Ident::new(method, ProcSpan::call_site());
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #recv.into_iter().#method_ident(#closure).collect::<Vec<_>>()
        };
        syn::parse2(tokens).map_err(|e| self.unsupported(&format!("{method} codegen parse: {e}")))
    }

    /// Lower `.reduce(closure)` → `recv.into_iter().reduce(closure)` (T23).
    ///
    /// Returns `Option<T>` (Rust parity). The closure is a 2-arg `|a, b| …`.
    fn lower_into_iter_reduce(
        &self,
        recv: SynExpr,
        closure: SynExpr,
    ) -> Result<SynExpr, CodegenError> {
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #recv.into_iter().reduce(#closure)
        };
        syn::parse2(tokens).map_err(|e| self.unsupported(&format!("reduce codegen parse: {e}")))
    }

    /// Lower a string interpolation `"text {expr} more"` to a Rust
    /// `format!("text {} more", expr)` macro invocation.
    ///
    /// The format string is built by walking the parts:
    /// - `InterpPart::Literal(s)` — the literal text, with each `{`/`}`
    ///   escaped to `{{`/`}}` so `format!` doesn't interpret them as slots.
    /// - `InterpPart::Expr(_)` — a `{}` placeholder in the format string, and
    ///   the lowered expression as a positional argument after the string.
    ///
    /// The final `format!` call is built via `quote!` so the format string
    /// and args are spliced in without any hand-formatted Rust.
    fn lower_string_interp(&mut self, parts: &[InterpPart]) -> Result<SynExpr, CodegenError> {
        // Build the format string with `{}` placeholders for each Expr.
        let mut fmt_string = String::new();
        let mut lowered_args: Vec<SynExpr> = Vec::new();
        for part in parts {
            match part {
                InterpPart::Literal(text) => {
                    // Escape `{` → `{{` and `}` → `}}` so they're literal.
                    for c in text.chars() {
                        match c {
                            '{' => fmt_string.push_str("{{"),
                            '}' => fmt_string.push_str("}}"),
                            _ => fmt_string.push(c),
                        }
                    }
                }
                InterpPart::Expr(e) => {
                    fmt_string.push_str("{}");
                    lowered_args.push(self.lower_expr(e)?);
                }
            }
        }
        // Build the format! macro: tokens are "<fmt>", arg1, arg2, ...
        // We build this with quote! by splicing each argument in turn.
        let format_lit = proc_macro2::Literal::string(&fmt_string);
        let args_tokens: Vec<proc_macro2::TokenStream> = lowered_args
            .iter()
            .map(|a| {
                let a = a.clone();
                quote::quote! { #a }
            })
            .collect();
        let combined: proc_macro2::TokenStream = if args_tokens.is_empty() {
            // Should never happen (interp always has at least one Expr),
            // but guard against malformed AST.
            quote::quote! { #format_lit }
        } else {
            let mut ts: proc_macro2::TokenStream = quote::quote! { #format_lit };
            for a in args_tokens {
                ts.extend(quote::quote! { , #a });
            }
            ts
        };
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: syn::Path::from(Ident::new("format", ProcSpan::call_site())),
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Paren(Default::default()),
                tokens: combined,
            },
        }))
    }

    /// Lower a collection literal `[e1, e2, ...]` to Rust's `vec![...]` macro
    /// (T23).
    ///
    /// The element expressions are lowered and spliced into the macro token
    /// stream via `quote!`, comma-separated. An empty literal lowers to
    /// `vec![]` (Rust infers the element type from context — typically the
    /// `let`-binding's type annotation, which the type inferencer drove).
    fn lower_array_lit(&mut self, elements: &[Expr]) -> Result<SynExpr, CodegenError> {
        // Lower each element, then build `vec![e0, e1, ...]`. The `[` / `]`
        // come from the `Bracket` delimiter; the `tokens` stream holds just
        // the comma-separated element expressions (so `vec![]` for empty).
        let mut lowered: Vec<SynExpr> = Vec::with_capacity(elements.len());
        for e in elements {
            lowered.push(self.lower_expr(e)?);
        }
        let mut tokens: proc_macro2::TokenStream = proc_macro2::TokenStream::new();
        for (i, e) in lowered.iter().enumerate() {
            let e = e.clone();
            if i > 0 {
                tokens.extend(quote::quote! { , });
            }
            tokens.extend(quote::quote! { #e });
        }
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: syn::Path::from(Ident::new("vec", ProcSpan::call_site())),
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Bracket(Default::default()),
                tokens,
            },
        }))
    }

    /// Lower a 2-D Matrix index `m[row, col]` to the flat-storage access
    /// `m.data[(row * m.cols + col) as usize]` (T24).
    ///
    /// The base expression `m` is lowered ONCE and the resulting `SynExpr` is
    /// spliced (via [`SynExpr::clone`]) into two positions:
    /// - `m.data` — the field holding the flat `Vec<T>`.
    /// - `m.cols` — the field carrying the column count.
    ///
    /// The flat index expression `row * m.cols + col` is built as a Rust
    /// binary tree (`Mul(row, Field(m, cols))` then `Add(.., col)`) and the
    /// whole thing is wrapped in a single `as usize` cast via [`cast_to`]
    /// (which parenthesises its operand, yielding exactly
    /// `(row * m.cols + col) as usize`). The outer `m.data[...]` is a Rust
    /// index expression.
    ///
    /// Both `row` and `col` are lowered as-is (no per-operand cast); the
    /// single trailing `as usize` covers the whole flat expression. This
    /// matches the T24 acceptance string `m.data[(1 * m.cols + 2) as usize]`.
    ///
    /// **GPU-readiness note**: because storage is one contiguous `Vec<T>`,
    /// the same flat-index expression is what a WGSL shader would compute to
    /// address a storage buffer — the REFACTOR goal of "share flat-storage
    /// pattern with GPU buffer codegen" lands naturally here.
    fn lower_matrix_index(
        &mut self,
        base: &Expr,
        row: &Expr,
        col: &Expr,
    ) -> Result<SynExpr, CodegenError> {
        let base_e = self.lower_expr(base)?;
        let row_e = self.lower_expr(row)?;
        let col_e = self.lower_expr(col)?;
        // `m.data` — field access on the lowered base.
        let data_field = field_access(base_e.clone(), "data");
        // `m.cols` — field access on the lowered base (clone preserves the
        // move analyzer's clone decision, if any, that was baked into base_e).
        let cols_field = field_access(base_e, "cols");
        // `row * m.cols`
        let row_times_cols = SynExpr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(row_e),
            op: syn::BinOp::Mul(Default::default()),
            right: Box::new(cols_field),
        });
        // `(row * m.cols) + col`
        let flat_expr = SynExpr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(row_times_cols),
            op: syn::BinOp::Add(Default::default()),
            right: Box::new(col_e),
        });
        // `((row * m.cols) + col) as usize` — cast_to wraps in parens.
        let flat_index = cast_to(flat_expr, "usize");
        // `m.data[flat_index]`
        Ok(SynExpr::Index(syn::ExprIndex {
            attrs: Vec::new(),
            expr: Box::new(data_field),
            bracket_token: Default::default(),
            index: Box::new(flat_index),
        }))
    }

    /// Lower `Matrix.new(rows, cols)` to Rust's `Matrix::new(rows, cols)`
    /// associated-function call (T24).
    ///
    /// The receiver `Matrix` is NOT lowered as a value (it names a type, not
    /// a variable) — we build the path `Matrix::new` directly and splice the
    /// lowered arguments. The arity is checked: exactly 2 args (rows, cols)
    /// are required. The `Matrix<T>` struct + `new` impl are emitted by
    /// [`Self::generate`] when this constructor appears in the program.
    fn lower_matrix_new(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 2 {
            return Err(self.unsupported(&format!(
                "Matrix.new expects exactly 2 args (rows, cols), got {}",
                args.len()
            )));
        }
        let mut lowered: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
        lowered.push(self.lower_expr(&args[0])?);
        lowered.push(self.lower_expr(&args[1])?);
        Ok(SynExpr::Call(syn::ExprCall {
            attrs: Vec::new(),
            func: Box::new(SynExpr::Path(syn::ExprPath {
                attrs: Vec::new(),
                qself: None,
                path: rust_path("Matrix::new"),
            })),
            paren_token: Default::default(),
            args: lowered,
        }))
    }

    /// Lower a minimal closure `{ params => expr }` to a Rust closure
    /// `|p1, p2| body` (T23 + T34 capture analysis).
    ///
    /// Param types are inferred by Rust — we emit no annotations (matching
    /// Buff's "hide the types" philosophy). The body is a single expression
    /// in T23's minimal shape; if the parser produced a multi-statement
    /// block, it is lowered as a block expression.
    ///
    /// # T34: variable capture
    ///
    /// Before lowering the body, we compute the set of variables CAPTURED
    /// by this closure (free vars of body minus params minus closure-local
    /// lets) via [`buff_lang_types::closure_captures`] — the shared
    /// capture analysis extracted from T33's spawn free-var walker. The
    /// capture set is pushed onto [`Self::closure_capture_stack`] so that
    /// [`Self::lower_expr`]'s `Expr::Ident` arm can emit captured
    /// variables plainly WITHOUT calling [`MoveAnalyzer::needs_clone`].
    ///
    /// Rust closures handle capture automatically (by ref or by move based
    /// on how the body uses the variable). Buff's job is only to AVOID
    /// inserting spurious `.clone()` calls for captured-variable uses
    /// INSIDE the closure body — without the capture stack, a non-Copy
    /// captured var used twice in a closure would get a wrong `.clone()`
    /// on its second use (MoveAnalyzer would see it as "use after move").
    fn lower_lambda(
        &mut self,
        params: &[buff_lang_ast::common::Param],
        body: &Block,
    ) -> Result<SynExpr, CodegenError> {
        // Build the closure parameter patterns: `|p1, p2, ...|`.
        let mut pats: Punctuated<Pat, syn::Token![,]> = Punctuated::new();
        for p in params {
            pats.push(Pat::Ident(PatIdent {
                attrs: Vec::new(),
                ident: ast_ident_to_syn(&p.name),
                by_ref: None,
                mutability: None,
                subpat: None,
            }));
        }
        // T34: compute captures and push onto the stack so the body-
        // lowering path knows which idents are captured (and should
        // bypass needs_clone). Popped after the body is lowered so the
        // stack correctly reflects the enclosing scope on exit.
        //
        // We ALSO insert the closure's own PARAM names into the pushed
        // set: closure params are fresh bindings owned by the closure
        // body, and Rust handles their ownership within the body (Copy
        // params are copied, non-Copy by-value uses are Rust's concern).
        // Without this, a param used multiple times in the body (e.g.
        // `|x| x * x + x`) would get a spurious `.clone()` from the
        // MoveAnalyzer on its second+ use — a pre-existing T23 limitation
        // that T34's capture-aware codegen naturally fixes by treating
        // params the same as captures (bypass needs_clone inside the body).
        let mut bypass_set = buff_lang_types::closure_captures(params, body);
        for p in params {
            bypass_set.insert(p.name.name.clone());
        }
        self.closure_capture_stack.push(bypass_set);
        // Body: a single ExprStmt lowers to a bare expression; otherwise a
        // block expression.
        let body_expr = self.lower_lambda_body(body);
        // Always pop, even if body lowering errored, so the stack stays
        // balanced across error recovery paths.
        self.closure_capture_stack.pop();
        let body_expr = body_expr?;
        Ok(SynExpr::Closure(syn::ExprClosure {
            attrs: Vec::new(),
            lifetimes: Default::default(),
            constness: None,
            movability: None,
            asyncness: None,
            capture: None,
            or1_token: Default::default(),
            or2_token: Default::default(),
            inputs: pats,
            output: ReturnType::Default,
            body: Box::new(body_expr),
        }))
    }

    /// Lower a lambda body. If the block is a single `ExprStmt`, lower that
    /// expression directly (so `|x| x * 2` not `|x| { x * 2 }`); otherwise
    /// lower the block as a `syn::Expr::Block`.
    fn lower_lambda_body(&mut self, body: &Block) -> Result<SynExpr, CodegenError> {
        if body.stmts.len() == 1 {
            if let Stmt::ExprStmt(e, _) = &body.stmts[0] {
                return self.lower_expr(e);
            }
        }
        let block = self.lower_block(body)?;
        Ok(SynExpr::Block(syn::ExprBlock {
            attrs: Vec::new(),
            label: None,
            block,
        }))
    }

    /// Lower a map literal `{"k": v, ...}` (or empty `{:}`) to Rust's
    /// `std::collections::HashMap::from([("k", v), ...])` (T25).
    ///
    /// Each entry's key and value are lowered independently and spliced into
    /// the outer array as Rust tuples. The fully-qualified path
    /// `std::collections::HashMap::from` is used (not a bare `HashMap::from`
    /// with a `use` import) so generated programs need NO import wiring.
    ///
    /// For an empty literal `{:}` we emit `HashMap::from([])` (Rust infers the
    /// key/value types from the `let`-binding annotation, which the codegen's
    /// type inferencer drives).
    ///
    /// The output is built via `quote!` so the outer `::from([...])` shell and
    /// the comma-separated tuple entries are constructed without any
    /// hand-formatted Rust strings.
    fn lower_map_lit(&mut self, entries: &[(Expr, Expr)]) -> Result<SynExpr, CodegenError> {
        // Lower each (key, value) pair into a Rust tuple expression. We
        // build the tuple via `syn::ExprTuple` so it's a real AST node
        // (not a token stream).
        let mut lowered_entries: Vec<SynExpr> = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let k_e = self.lower_expr(k)?;
            let v_e = self.lower_expr(v)?;
            let mut tuple_elems: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
            tuple_elems.push(k_e);
            tuple_elems.push(v_e);
            // A trailing comma is required by Rust for single-element tuples;
            // for 2-element tuples it's optional but harmless. We always add
            // one for uniformity.
            let tuple = SynExpr::Tuple(syn::ExprTuple {
                attrs: Vec::new(),
                paren_token: Default::default(),
                elems: tuple_elems,
            });
            lowered_entries.push(tuple);
        }
        // Build the outer `[<entries>]` array literal as a Rust expression
        // via `quote!`. Each lowered tuple is spliced in comma-separated.
        let mut entries_tokens: proc_macro2::TokenStream = proc_macro2::TokenStream::new();
        for (i, e) in lowered_entries.iter().enumerate() {
            if i > 0 {
                entries_tokens.extend(quote::quote! { , });
            }
            let e = e.clone();
            entries_tokens.extend(quote::quote! { #e });
        }
        // `std::collections::HashMap::from([<entries_tokens>])`
        let tokens: proc_macro2::TokenStream = quote::quote! {
            std::collections::HashMap::from([#entries_tokens])
        };
        syn::parse2(tokens)
            .map_err(|e| self.unsupported(&format!("map literal codegen parse: {e}")))
    }

    /// Lower a Buff struct-init expression `Type { field: value, ... }` to a
    /// Rust [`syn::ExprStruct`] of the same shape (T26).
    ///
    /// Each field is a `field: <lowered_value>` pair; the type path uses the
    /// struct name verbatim (Buff's struct names ARE Rust struct names — no
    /// renaming). The output is built via `quote!` so the brace-delimited
    /// body and comma-separated fields are constructed without any
    /// hand-formatted Rust strings.
    ///
    /// This mirrors the source form 1:1 because Buff deliberately matches
    /// Rust's struct-init syntax ( braces + named fields + colon ).
    fn lower_struct_init(
        &mut self,
        type_name: &buff_lang_ast::common::Ident,
        fields: &[(buff_lang_ast::common::Ident, Expr)],
    ) -> Result<SynExpr, CodegenError> {
        // Lower each field value first.
        let mut lowered_fields: Vec<(Ident, SynExpr)> = Vec::with_capacity(fields.len());
        for (fname, fval) in fields {
            let v = self.lower_expr(fval)?;
            lowered_fields.push((ast_ident_to_syn(fname), v));
        }
        // Build `Type { f1: v1, f2: v2, ... }` via `quote!`. Splice each
        // field as `#fname: #fval` (both are syn expressions/idents that
        // `quote!` can interpolate).
        let type_path = rust_path(&type_name.name);
        let mut fields_tokens: proc_macro2::TokenStream = proc_macro2::TokenStream::new();
        for (i, (fname, fval)) in lowered_fields.iter().enumerate() {
            if i > 0 {
                fields_tokens.extend(quote::quote! { , });
            }
            fields_tokens.extend(quote::quote! { #fname: #fval });
        }
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #type_path { #fields_tokens }
        };
        syn::parse2(tokens)
            .map_err(|e| self.unsupported(&format!("struct init codegen parse: {e}")))
    }

    /// Lower a Buff `match scrutinee { arms }` to a Rust `syn::ExprMatch`
    /// (T27).
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// match <scrutinee> {
    ///     <pattern> => <body>,
    ///     <pattern> => <body>,
    ///     ...
    /// }
    /// ```
    ///
    /// Each arm's body is a `Block` (the parser wraps the single body
    /// expression in a one-statement block). The arm pattern goes through
    /// [`Self::lower_pattern`]; the body goes through [`Self::lower_block`].
    ///
    /// This mirrors the source form 1:1 because Buff deliberately matches
    /// Rust's `match` syntax. Exhaustiveness is checked separately by the
    /// `buff-lang-types` analysis pass; if a match is non-exhaustive the
    /// type-checker flags it BEFORE codegen runs (codegen assumes the match
    /// is well-formed).
    fn lower_match_expr(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
    ) -> Result<SynExpr, CodegenError> {
        let scrut = self.lower_expr(scrutinee)?;
        let mut arms_syn: Vec<syn::Arm> = Vec::with_capacity(arms.len());
        for arm in arms {
            let pat = self.lower_pattern(&arm.pattern)?;
            // The parser wraps the body expression in a one-statement
            // `ExprStmt` block. We lower the block and use it as the arm
            // body — Rust accepts a block as an arm body. If the block has
            // a single trailing expression, prettyplease will format it
            // back as `pat => expr,`; if it's multiple statements, the
            // block form `pat => { ... },` is emitted (also valid Rust).
            let body_block = self.lower_block(&arm.body)?;
            let body_expr = SynExpr::Block(syn::ExprBlock {
                attrs: Vec::new(),
                label: None,
                block: body_block,
            });
            arms_syn.push(syn::Arm {
                attrs: Vec::new(),
                pat,
                guard: None,
                fat_arrow_token: Default::default(),
                body: Box::new(body_expr),
                comma: Some(Default::default()),
            });
        }
        Ok(SynExpr::Match(syn::ExprMatch {
            attrs: Vec::new(),
            match_token: Default::default(),
            expr: Box::new(scrut),
            brace_token: Default::default(),
            arms: arms_syn,
        }))
    }

    /// Lower `expr?` to Rust's native `?` operator (T30 REFACTOR step).
    ///
    /// This is the extracted error-propagation codegen helper. It builds a
    /// `syn::ExprTry` wrapping the lowered operand, which `prettyplease`
    /// prints as `<expr>?`. Rust's `?` performs exactly the early-return
    /// propagation the task requires (`match expr { Ok(v) => v, Err(e) =>
    /// return Err(e.into()) }`), so we delegate to it rather than emitting
    /// the explicit match. The enclosing Buff function must lower to a Rust
    /// function returning `Result<T, E>` — which it does whenever the user
    /// writes a `Result<T, E>` return-type annotation, the only context
    /// where `?` is meaningful.
    ///
    /// Design choice (documented in the task): option (a) — Rust-native `?` —
    /// over option (b) — the explicit match. (a) is simpler, equally correct,
    /// and produces cleaner Rust that rustc optimises identically.
    fn lower_try(&mut self, expr: &Expr) -> Result<SynExpr, CodegenError> {
        let inner = self.lower_expr(expr)?;
        Ok(SynExpr::Try(syn::ExprTry {
            attrs: Vec::new(),
            expr: Box::new(inner),
            question_token: Default::default(),
        }))
    }

    /// Lower the prelude error constructor `Error(arg)` to
    /// `Err(Error::new(arg))` (T30).
    ///
    /// `Error("msg")` in Buff is sugar for an `Err` value carrying a
    /// freshly-constructed `Error` (the builtin error type emitted on-demand
    /// by [`Self::generate`]). It maps to `Err(Error::new(arg))` so a
    /// `return Error("msg")` produces an early `Err` return without the user
    /// writing `Err(...)` themselves.
    ///
    /// The single argument is lowered as a normal expression and spliced
    /// into the `Error::new(...)` call via `quote!` (so no hand-formatted
    /// Rust). The outer `Err(...)` is a path call built the same way.
    fn lower_error_constructor(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!(
                "Error() expects exactly 1 arg, got {}",
                args.len()
            )));
        }
        let arg = self.lower_expr(&args[0])?;
        // `Error::new(#arg)` — built via quote! so the path + arg splice
        // without hand-formatted Rust. The explicit type annotation pins the
        // `parse2` target so type inference doesn't fall back to `()`.
        let inner_call_tokens: proc_macro2::TokenStream = quote::quote! {
            Error::new(#arg)
        };
        let inner_call: SynExpr = syn::parse2(inner_call_tokens)
            .map_err(|e| self.unsupported(&format!("Error() codegen parse: {e}")))?;
        // Wrap in `Err(...)`.
        let tokens: proc_macro2::TokenStream = quote::quote! {
            Err(#inner_call)
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("Err() codegen parse: {e}")))
    }

    /// T31: lower `spawn <expr>` to Rust's `tokio::spawn(async move { <expr> })`.
    ///
    /// The task body becomes the body of an `async move` closure so the
    /// spawned task owns its captured variables (Buff hides borrow-checker
    /// pain from users; the generated Rust must be move-clean). The result
    /// is a `tokio::task::JoinHandle<T>` — Buff's `Task<T>` is a thin alias
    /// for this type, and the only `.await` on a Task lands at the
    /// `t.result()` site (see [`Self::lower_method_call`]).
    ///
    /// Built via `quote!` so the `tokio::spawn(async move { ... })` shape
    /// is constructed from real `syn` tokens rather than hand-formatted
    /// Rust. The single string producer remains `prettyplease::unparse`.
    fn lower_spawn(&mut self, task: &Expr) -> Result<SynExpr, CodegenError> {
        // T31: bump async-block depth so async calls inside the task body
        // still get `.await` (the `async move { ... }` block IS an async
        // context, even if the spawning fn is sync).
        self.async_block_depth += 1;
        // T33: bump spawn depth so ident uses inside the task body get
        // rewritten to `Arc::clone(&x)` (for Arc-shared bindings) instead
        // of moving or deep-cloning. Reset on exit so idents outside the
        // spawn go back to the regular move/clone path.
        self.spawn_depth += 1;
        let task_expr = self.lower_expr(task)?;
        self.spawn_depth -= 1;
        self.async_block_depth -= 1;
        let tokens: proc_macro2::TokenStream = quote::quote! {
            tokio::spawn(async move { #task_expr })
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("spawn codegen parse: {e}")))
    }

    /// T68: lower `start..end` (exclusive) or `start..=end` (inclusive) to a
    /// Rust range expression.
    ///
    /// Exclusive range `0..10` → Rust `0..10` via `syn::ExprRange`.
    /// Inclusive range `0..=10` → Rust `0..=10` via `syn::ExprRange`.
    ///
    /// Built via `quote!` so the `..` / `..=` operator is constructed from
    /// real `syn` tokens rather than hand-formatted Rust.
    fn lower_range(
        &mut self,
        start: &Expr,
        end: &Expr,
        inclusive: bool,
    ) -> Result<SynExpr, CodegenError> {
        let start_e = self.lower_expr(start)?;
        let end_e = self.lower_expr(end)?;
        let tokens: proc_macro2::TokenStream = if inclusive {
            quote::quote! { #start_e ..= #end_e }
        } else {
            quote::quote! { #start_e .. #end_e }
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("range codegen parse: {e}")))
    }

    /// T31: lower `block(<expr>)` to a one-shot tokio runtime block.
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// {
    ///     let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    ///     rt.block_on(<expr>)
    /// }
    /// ```
    ///
    /// This is the SYNC context form — it spins up a fresh current-thread
    /// runtime and blocks the calling thread on the async expression. It's
    /// the bridge between sync code and an async future when no runtime is
    /// already running.
    ///
    /// # `block()` inside an async fn — DEADLOCK RISK warning
    ///
    /// If `block()` is called from inside an async fn (the current fn is in
    /// the propagated async set), we emit a [`Diagnostic::warning`]
    /// explaining the deadlock risk: the runtime worker thread is blocked
    /// on `block_on`, so any future scheduled on the same worker can never
    /// run, deadlocking the program. The warning is appended to
    /// [`Self::warnings`]; the codegen still emits the (broken) Rust so the
    /// user can see what they wrote and decide how to refactor (usually:
    /// remove `block()` and let the async fn `return` the future directly).
    fn lower_block_call(&mut self, expr: &Expr) -> Result<SynExpr, CodegenError> {
        // Warn if we're inside an async fn — block_on in async is a deadlock.
        if self.current_fn_is_async() {
            let span = expr.span();
            self.warnings.push(
                Diagnostic::warning(
                    "`block()` inside an async function can deadlock the runtime",
                    span,
                )
                .with_note(
                    "block_on parks the current worker thread, preventing any future \
                     scheduled on the same worker from running. Consider returning the \
                     future directly instead of blocking on it.",
                ),
            );
        }
        let arg = self.lower_expr(expr)?;
        // Build the one-shot runtime block via quote! — no hand-formatted
        // Rust string. The expect() message is intentionally lowercase +
        // no trailing period per the conventions doc.
        let tokens: proc_macro2::TokenStream = quote::quote! {
            {
                let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
                rt.block_on(#arg)
            }
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("block() codegen parse: {e}")))
    }

    /// T31: is the function we're currently lowering in the propagated
    /// async set? Used by [`Self::lower_block_call`] to decide whether to
    /// emit the deadlock warning.
    ///
    /// Returns `false` when called outside `lower_func` (e.g. when lowering
    /// a free-floating expression in tests).
    fn current_fn_is_async(&self) -> bool {
        match &self.current_fn_name {
            Some(name) => self.async_fns.contains(name),
            None => false,
        }
    }

    /// T31: are we currently inside an async context? True when either:
    ///   - the current fn is async (per [`Self::current_fn_is_async`]), OR
    ///   - we're inside one or more `async move { ... }` blocks (e.g.
    ///     inside a `spawn` body — the spawned task is itself async even
    ///     when the spawner is sync).
    ///
    /// Drives the `.await` insertion decision in [`Self::lower_expr`].
    fn in_async_context(&self) -> bool {
        self.current_fn_is_async() || self.async_block_depth > 0
    }

    /// T34: should `name` bypass [`MoveAnalyzer::needs_clone`] because we're
    /// inside a closure body and `name` is either a **captured variable**
    /// or a **closure parameter**?
    ///
    /// Checks the top-of-stack entry in [`Self::closure_capture_stack`].
    /// Returns `false` when not inside any closure body (empty stack).
    fn is_captured_in_closure(&self, name: &str) -> bool {
        match self.closure_capture_stack.last() {
            Some(bypass) => bypass.contains(name),
            None => false,
        }
    }

    /// Lower a Buff [`Pattern`] to a Rust [`syn::Pat`] (T27).
    ///
    /// Mapping:
    /// - [`Pattern::Wildcard`] → `syn::Pat::Wild` (`_`)
    /// - [`Pattern::Ident(name, _)`] → `syn::Pat::Ident(name)`. Rust resolves
    ///   whether the name is a unit variant or a fresh binding using type
    ///   information (matching Buff's deferred-resolve approach).
    /// - [`Pattern::Literal(lit, _)`] → `syn::Pat::Lit` (literal pattern).
    /// - [`Pattern::Variant { variant, subpatterns, .. }`] →
    ///   - if `subpatterns` is empty: `syn::Pat::Path` (`Variant` alone — a
    ///     unit variant reference; we never reach this from the parser since
    ///     unit variants come through `Pattern::Ident`, but the arm covers
    ///     hand-constructed ASTs from tests).
    ///   - else: `syn::Pat::TupleStruct` with one sub-pattern per slot. The
    ///     path is just `Variant` (no enum prefix) — Rust resolves it when
    ///     the enum is in scope. The `enum_name` field of the AST node is
    ///     ignored at codegen (the parser fills it with `""`).
    fn lower_pattern(&mut self, pat: &Pattern) -> Result<Pat, CodegenError> {
        let syn_pat: Pat = match pat {
            Pattern::Wildcard(_) => Pat::Wild(syn::PatWild {
                attrs: Vec::new(),
                underscore_token: Default::default(),
            }),
            Pattern::Ident(name, _) => Pat::Ident(PatIdent {
                attrs: Vec::new(),
                ident: ast_ident_to_syn(name),
                by_ref: None,
                mutability: None,
                subpat: None,
            }),
            Pattern::Literal(lit, _) => {
                let lit_expr = self.lower_literal(lit)?;
                // `syn::Pat::Lit` is an alias for `syn::ExprLit` in syn 2.0
                // (see `syn::pat.rs`: `ExprLit as PatLit`). So a literal
                // pattern is constructed exactly like a literal expression:
                // wrap the `syn::Lit` in an `ExprLit` and hand it to
                // `Pat::Lit(...)`.
                let expr_lit = match lit_expr {
                    SynExpr::Lit(el) => el,
                    other => {
                        return Err(self.unsupported(&format!(
                            "literal pattern codegen expected Lit, got {other:?}"
                        )))
                    }
                };
                Pat::Lit(expr_lit)
            }
            Pattern::Variant {
                variant,
                subpatterns,
                ..
            } => {
                if subpatterns.is_empty() {
                    // Unit variant via path. Build `syn::Pat::Path` with a
                    // single-segment path equal to the variant name.
                    Pat::Path(syn::PatPath {
                        attrs: Vec::new(),
                        qself: None,
                        path: syn::Path::from(ast_ident_to_syn(variant)),
                    })
                } else {
                    // Tuple-struct variant: `Variant(subpat1, subpat2, ...)`.
                    let mut elems: Punctuated<Pat, syn::Token![,]> = Punctuated::new();
                    for sub in subpatterns {
                        elems.push(self.lower_pattern(sub)?);
                    }
                    Pat::TupleStruct(syn::PatTupleStruct {
                        attrs: Vec::new(),
                        qself: None,
                        path: syn::Path::from(ast_ident_to_syn(variant)),
                        paren_token: Default::default(),
                        elems,
                    })
                }
            }
        };
        Ok(syn_pat)
    }

    fn lower_literal(&mut self, lit: &Literal) -> Result<SynExpr, CodegenError> {
        // T20: Decimal literal → `rust_decimal_macros::dec!(<raw>)`. The raw
        // digit text is parsed into a `proc_macro2::TokenStream` so the
        // *exact* digits survive (no rounding through f64) — this matches
        // what `dec!` expects (a numeric literal token) and preserves
        // trailing zeros like the `0` in `99.90`.
        if let Literal::Decimal(raw) = lit {
            return self.lower_decimal_literal(raw);
        }
        let syn_lit = match lit {
            Literal::Int(n) => {
                syn::Lit::Int(syn::LitInt::new(&n.to_string(), ProcSpan::call_site()))
            }
            Literal::Float(f) => {
                // f32 suffix — prettyplease prints it as `2.5f32`.
                let s = format!("{}f32", float_repr(*f as f64));
                syn::Lit::Float(syn::LitFloat::new(&s, ProcSpan::call_site()))
            }
            Literal::Double(d) => {
                let s = format!("{}f64", float_repr(*d));
                syn::Lit::Float(syn::LitFloat::new(&s, ProcSpan::call_site()))
            }
            Literal::Bool(b) => syn::Lit::Bool(syn::LitBool::new(*b, ProcSpan::call_site())),
            Literal::String(s) => syn::Lit::Str(syn::LitStr::new(s, ProcSpan::call_site())),
            Literal::Byte(b) => {
                syn::Lit::Int(syn::LitInt::new(&b.to_string(), ProcSpan::call_site()))
            }
            // T21: `'A'` → `syn::Lit::Char`. prettyplease prints Rust `char`
            // literals with the correct quoting (including for escapes and
            // non-ASCII scalars).
            Literal::Char(c) => syn::Lit::Char(syn::LitChar::new(*c, ProcSpan::call_site())),
            // Handled by the early return above; this arm exists only so the
            // match is exhaustive (it is never reached).
            Literal::Decimal(_) => {
                return Err(self.unsupported("decimal literal (unreachable arm)"))
            }
        };
        Ok(SynExpr::Lit(syn::ExprLit {
            attrs: Vec::new(),
            lit: syn_lit,
        }))
    }

    /// Lower a Buff `Decimal` literal to the `rust_decimal_macros::dec!(...)`
    /// macro invocation (T20).
    ///
    /// The raw source text is parsed via `syn::parse_str` into a
    /// `proc_macro2::TokenStream` so the exact digits (including trailing
    /// zeros) are preserved verbatim — the value never transits through an
    /// `f64`, guaranteeing exactness end-to-end.
    fn lower_decimal_literal(&self, raw: &str) -> Result<SynExpr, CodegenError> {
        let num_tokens: proc_macro2::TokenStream = syn::parse_str(raw)
            .map_err(|e| self.unsupported(&format!("decimal literal `{raw}`: {e}")))?;
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: rust_path("rust_decimal_macros::dec"),
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Paren(Default::default()),
                tokens: num_tokens,
            },
        }))
    }

    fn make_binary_op(
        &mut self,
        op: BinaryOp,
        lhs: SynExpr,
        rhs: SynExpr,
    ) -> Result<SynExpr, CodegenError> {
        use syn::BinOp;
        let result = match op {
            BinaryOp::And => self.bin_arith(BinOp::And(Default::default()), lhs, rhs),
            BinaryOp::Or => self.bin_arith(BinOp::Or(Default::default()), lhs, rhs),
            BinaryOp::Add => self.bin_arith(BinOp::Add(Default::default()), lhs, rhs),
            BinaryOp::Sub => self.bin_arith(BinOp::Sub(Default::default()), lhs, rhs),
            BinaryOp::Mul => self.bin_arith(BinOp::Mul(Default::default()), lhs, rhs),
            BinaryOp::Div => self.bin_arith(BinOp::Div(Default::default()), lhs, rhs),
            BinaryOp::Mod => self.bin_arith(BinOp::Rem(Default::default()), lhs, rhs),
            BinaryOp::Eq => self.bin_arith(BinOp::Eq(Default::default()), lhs, rhs),
            BinaryOp::Neq => self.bin_arith(BinOp::Ne(Default::default()), lhs, rhs),
            BinaryOp::Lt => self.bin_arith(BinOp::Lt(Default::default()), lhs, rhs),
            BinaryOp::Gt => self.bin_arith(BinOp::Gt(Default::default()), lhs, rhs),
            BinaryOp::Lte => self.bin_arith(BinOp::Le(Default::default()), lhs, rhs),
            BinaryOp::Gte => self.bin_arith(BinOp::Ge(Default::default()), lhs, rhs),
            BinaryOp::BitAnd => self.bin_arith(BinOp::BitAnd(Default::default()), lhs, rhs),
            BinaryOp::BitOr => self.bin_arith(BinOp::BitOr(Default::default()), lhs, rhs),
            BinaryOp::BitXor => self.bin_arith(BinOp::BitXor(Default::default()), lhs, rhs),
            BinaryOp::Shl => self.bin_arith(BinOp::Shl(Default::default()), lhs, rhs),
            BinaryOp::Shr => self.bin_arith(BinOp::Shr(Default::default()), lhs, rhs),
            BinaryOp::Assign => SynExpr::Assign(syn::ExprAssign {
                attrs: Vec::new(),
                left: Box::new(lhs),
                eq_token: Default::default(),
                right: Box::new(rhs),
            }),
            BinaryOp::AddAssign
            | BinaryOp::SubAssign
            | BinaryOp::MulAssign
            | BinaryOp::DivAssign
            | BinaryOp::ModAssign => {
                let binop = match op {
                    BinaryOp::AddAssign => BinOp::AddAssign(Default::default()),
                    BinaryOp::SubAssign => BinOp::SubAssign(Default::default()),
                    BinaryOp::MulAssign => BinOp::MulAssign(Default::default()),
                    BinaryOp::DivAssign => BinOp::DivAssign(Default::default()),
                    BinaryOp::ModAssign => BinOp::RemAssign(Default::default()),
                    _ => unreachable!(),
                };
                SynExpr::Binary(syn::ExprBinary {
                    attrs: Vec::new(),
                    left: Box::new(lhs),
                    op: binop,
                    right: Box::new(rhs),
                })
            }
        };
        Ok(result)
    }

    fn bin_arith(&self, op: syn::BinOp, lhs: SynExpr, rhs: SynExpr) -> SynExpr {
        SynExpr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(lhs),
            op,
            right: Box::new(rhs),
        })
    }

    fn make_unary_op(&mut self, op: UnaryOp, operand: SynExpr) -> Result<SynExpr, CodegenError> {
        // Buff's `~` (bitwise NOT on integers) maps to Rust's `!` on integers.
        let unop = match op {
            UnaryOp::Neg => syn::UnOp::Neg(Default::default()),
            UnaryOp::Not => syn::UnOp::Not(Default::default()),
            UnaryOp::BitNot => syn::UnOp::Not(Default::default()),
        };
        Ok(SynExpr::Unary(syn::ExprUnary {
            attrs: Vec::new(),
            op: unop,
            expr: Box::new(operand),
        }))
    }

    /// Convert a Buff [`TypeRef`] to a Rust [`syn::Type`].
    ///
    /// Returns an error for unsupported forms (function types); these will
    /// land in T12/T13.
    fn ast_typeref_to_syn(&self, ty: &TypeRef) -> Result<SynType, CodegenError> {
        match ty {
            TypeRef::Named { name, .. } => {
                // T32: the Buff→Rust primitive-name mapping is now a
                // single named, configurable table at
                // [`buff_primitive_to_rust_name`] (covers all 9 primitive
                // names: Int, Byte, Bits, Float, Double, Bool, String,
                // Char, Decimal). Unknown names pass through unchanged so
                // user-defined types (struct/enum names) keep their spelling.
                let rust_name = buff_primitive_to_rust_name(&name.name);
                Ok(rust_path_type(rust_name))
            }
            TypeRef::Option(inner, _) => {
                let inner_ty = self.ast_typeref_to_syn(inner)?;
                Ok(make_generic_path_type("Option", vec![inner_ty]))
            }
            TypeRef::Generic { base, args, .. } => {
                // Lower the base type to a path string (we only support Named base for now).
                let base_name = match base.as_ref() {
                    TypeRef::Named { name, .. } => name.name.clone(),
                    _ => return Err(self.unsupported("generic with non-named base type")),
                };
                let lowered_args: Result<Vec<SynType>, CodegenError> =
                    args.iter().map(|a| self.ast_typeref_to_syn(a)).collect();
                let lowered_args = lowered_args?;
                Ok(make_generic_path_type(&base_name, lowered_args))
            }
            TypeRef::Function { .. } => Err(self.unsupported("function-type codegen (T12/T13)")),
        }
    }

    /// Map a resolved Buff [`Type`] (post-inference) to a Rust [`syn::Type`].
    ///
    /// Returns `None` for [`Type::Unknown`] and [`Type::Void`] — callers
    /// (notably `let` lowering) treat `None` as "no annotation emitted".
    /// [`Type::Decimal`] maps to `rust_decimal::Decimal` (the crate is a
    /// dependency of `buff-lang-codegen-rust` so generated crates must depend
    /// on it as well — the runtime/driver is responsible for that).
    fn buff_type_to_syn(&self, ty: &Type) -> Option<SynType> {
        // Handle generic types (Vector, Matrix, Option) first.
        match ty {
            Type::Vector(elem) => {
                let inner = self.buff_type_to_syn(elem)?;
                return Some(make_generic_path_type("Vec", vec![inner]));
            }
            Type::Matrix(elem) => {
                // T24: Matrix<T> maps to the builtin `Matrix<T>` struct that
                // this codegen emits on-demand. The inner element type uses
                // the standard mapping; an Unknown element falls back to
                // i64 (Buff's default Int) so the annotation still compiles.
                let inner = self
                    .buff_type_to_syn(elem)
                    .unwrap_or_else(|| rust_path_type("i64"));
                return Some(make_generic_path_type("Matrix", vec![inner]));
            }
            Type::Option(inner) => {
                let inner = self.buff_type_to_syn(inner)?;
                return Some(make_generic_path_type("Option", vec![inner]));
            }
            // T25: Map<K, V> → Rust `std::collections::HashMap<K, V>`. We use
            // the fully-qualified path so generated programs do NOT need a
            // `use std::collections::HashMap;` import (the literal codegen
            // also uses the fully-qualified path, keeping import-management
            // out of the picture for v0.5).
            Type::Map(key, value) => {
                let k = self
                    .buff_type_to_syn(key)
                    .unwrap_or_else(|| rust_path_type("i64"));
                let v = self
                    .buff_type_to_syn(value)
                    .unwrap_or_else(|| rust_path_type("i64"));
                return Some(make_qualified_generic_path_type(
                    "std::collections::HashMap",
                    vec![k, v],
                ));
            }
            // T30: Result<T, E> → Rust `Result<T, E>` (the std Result is in
            // scope by default, so no fully-qualified path needed — mirroring
            // Option<T>'s 1:1 mapping from T28). Both inners must resolve to
            // a concrete Rust type; an Unknown inner (e.g. `Ok(42)` infers
            // `Result<Int<64>, Unknown>`) makes the whole annotation
            // indeterminate, so we return None and let Rust infer from
            // context (function return type, etc.).
            Type::Result(ok, err) => {
                let ok_ty = self.buff_type_to_syn(ok)?;
                let err_ty = self.buff_type_to_syn(err)?;
                return Some(make_generic_path_type("Result", vec![ok_ty, err_ty]));
            }
            _ => {}
        }
        let rust_name: &str = match ty {
            Type::Int {
                width: IntWidth::W8,
            } => "i8",
            Type::Int {
                width: IntWidth::W16,
            } => "i16",
            Type::Int {
                width: IntWidth::W32,
            } => "i32",
            Type::Int {
                width: IntWidth::W64,
            } => "i64",
            Type::Int {
                width: IntWidth::W128,
            } => "i128",
            Type::Bits {
                width: IntWidth::W8,
            } => "u8",
            Type::Bits {
                width: IntWidth::W16,
            } => "u16",
            Type::Bits {
                width: IntWidth::W32,
            } => "u32",
            Type::Bits {
                width: IntWidth::W64,
            } => "u64",
            Type::Bits {
                width: IntWidth::W128,
            } => "u128",
            // f16 is unstable in std; we map to f32 as a safe approximation.
            Type::Float {
                width: FloatWidth::W16,
            } => "f32",
            Type::Float {
                width: FloatWidth::W32,
            } => "f32",
            Type::Float {
                width: FloatWidth::W64,
            } => "f64",
            Type::Double => "f64",
            Type::Bool => "bool",
            Type::String => "String",
            // T21: Char → Rust's `char` (a 4-byte Unicode scalar value).
            Type::Char => "char",
            Type::Decimal => "rust_decimal::Decimal",
            Type::Unknown | Type::Void => return None,
            // Vector, Matrix, Map, Option, and Result are handled by the
            // early-return match above; this arm is unreachable but required
            // for exhaustiveness.
            Type::Vector(_)
            | Type::Matrix(_)
            | Type::Option(_)
            | Type::Map(_, _)
            | Type::Result(_, _) => return None,
        };
        Some(rust_path_type(rust_name))
    }

    fn unsupported(&self, what: &str) -> CodegenError {
        CodegenError::new(Diagnostic::error(
            format!("unsupported: {what}"),
            BuffSpan::dummy(),
        ))
    }
}

impl Default for RustCodegen {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Convert a Buff [`buff_lang_ast::Ident`] into a `syn::Ident`. The byte offsets
/// in the Buff span don't carry over (proc-macro2 spans are opaque), so we
/// just use `call_site` here. The source-map mapping (Buff span → Rust
/// line/col) is recorded separately in [`CodegenContext`].
fn ast_ident_to_syn(ident: &buff_lang_ast::common::Ident) -> Ident {
    Ident::new(&ident.name, ProcSpan::call_site())
}

/// T32: the single named, configurable Buff→Rust primitive-type mapping
/// table.
///
/// Maps each of Buff's 9 primitive type NAMES to the corresponding Rust
/// type name (as written in source — the caller wraps it in a
/// [`rust_path_type`] to form a [`SynType`]). This is the ONE place that
/// knows how Buff primitive names spell in Rust; both
/// [`RustCodegen::ast_typeref_to_syn`] (unresolved [`TypeRef`]s from
/// user-written annotations) and any future "reverse" mapping (Rust→Buff
/// for diagnostics) should consult this table.
///
/// The 9 primitive names covered (the task's "13 types" counts 9
/// primitives + 4 generic containers — Vector, Option, Matrix, Map, Result
/// — which are handled structurally in [`RustCodegen::ast_typeref_to_syn`]
/// / [`RustCodegen::buff_type_to_syn`] because they carry type arguments):
///
/// | Buff name | Rust name            |
/// |-----------|----------------------|
/// | `Int`     | `i64`                |
/// | `Byte`    | `u8`                 |
/// | `Bits`    | `u64`                |
/// | `Float`   | `f32`                |
/// | `Double`  | `f64`                |
/// | `Bool`    | `bool`               |
/// | `String`  | `String`             |
/// | `Char`    | `char`               |
/// | `Decimal` | `rust_decimal::Decimal` |
///
/// Unknown names (anything not in the table) are returned unchanged so
/// user-defined types (struct/enum names, generic type parameters like
/// `T`) keep their spelling — they become Rust path types verbatim.
pub fn buff_primitive_to_rust_name(buff_name: &str) -> &str {
    match buff_name {
        "Int" => "i64",
        "Byte" => "u8",
        "Bits" => "u64",
        "Float" => "f32",
        "Double" => "f64",
        "Bool" => "bool",
        "String" => "String",
        "Char" => "char",
        "Decimal" => "rust_decimal::Decimal",
        other => other,
    }
}

/// Build a `syn::Type::Path` from a `::`-separated Rust type name string
/// (e.g. `"i64"`, `"bool"`, `"rust_decimal::Decimal"`). Each `::`-separated
/// segment becomes a [`syn::PathSegment`]. The result is always a plain path
/// with no generic arguments.
fn rust_path_type(name: &str) -> SynType {
    SynType::Path(syn::TypePath {
        qself: None,
        path: rust_path(name),
    })
}

/// Build a `syn::Path` from a `::`-separated name string
/// (e.g. `"rust_decimal_macros::dec"`). Used for macro paths like the
/// `dec!(...)` codegen in T20.
fn rust_path(name: &str) -> syn::Path {
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    for seg in name.split("::") {
        segments.push(syn::PathSegment {
            ident: Ident::new(seg, ProcSpan::call_site()),
            arguments: syn::PathArguments::None,
        });
    }
    syn::Path {
        leading_colon: None,
        segments,
    }
}

/// Build a `println!("{}", arg)` macro invocation as a `syn::Expr::Macro`.
///
/// Used by the `print(x)` → `println!("{}", x)` mapping (T13/T96). The macro
/// token stream is built via `quote!` so it round-trips through `syn`'s
/// printer without any hand-rolled string formatting.
fn make_println_macro(arg: SynExpr) -> SynExpr {
    SynExpr::Macro(syn::ExprMacro {
        attrs: Vec::new(),
        mac: syn::Macro {
            path: syn::Path::from(Ident::new("println", ProcSpan::call_site())),
            bang_token: Default::default(),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: quote::quote! { "{}", #arg },
        },
    })
}

/// Build a `println!("literal_text")` macro invocation — the T96 string-
/// literal fast path for `print("hello")` → `println!("hello")` (no `{}`
/// placeholder, the literal text becomes the format string itself).
fn make_println_macro_literal(text: &str) -> SynExpr {
    // Build the format-string literal via `proc_macro2::Literal::string` so
    // Rust-level escapes in `text` survive correctly (e.g. embedded quotes,
    // backslashes, newlines).
    let format_lit = proc_macro2::Literal::string(text);
    SynExpr::Macro(syn::ExprMacro {
        attrs: Vec::new(),
        mac: syn::Macro {
            path: syn::Path::from(Ident::new("println", ProcSpan::call_site())),
            bang_token: Default::default(),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: quote::quote! { #format_lit },
        },
    })
}

/// Build a `recv.method()` (zero-arg) method call.
fn method_call_no_args(recv: SynExpr, method: &str) -> SynExpr {
    SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(recv),
        dot_token: Default::default(),
        method: Ident::new(method, ProcSpan::call_site()),
        turbofish: None,
        paren_token: Default::default(),
        args: Default::default(),
    })
}

/// Wrap an expression in parentheses: `(e)`. Used to disambiguate method-
/// call receivers so integer literals like `5` lower to `(5).abs()` rather
/// than the ambiguous `5.abs()` (which Rust parses as a field access on the
/// float literal `5.`).
fn wrap_in_parens(e: SynExpr) -> SynExpr {
    SynExpr::Paren(syn::ExprParen {
        attrs: Vec::new(),
        paren_token: Default::default(),
        expr: Box::new(e),
    })
}

/// Build a named field access `base.field` (T24).
///
/// Used by the Matrix 2-D index codegen to build `m.data` and `m.cols`. The
/// base expression is taken by value (the caller clones when re-use is
/// needed, as in [`RustCodegen::lower_matrix_index`]).
fn field_access(base: SynExpr, field: &str) -> SynExpr {
    SynExpr::Field(syn::ExprField {
        attrs: Vec::new(),
        base: Box::new(base),
        dot_token: Default::default(),
        member: syn::Member::Named(Ident::new(field, ProcSpan::call_site())),
    })
}

/// T31: build a Rust `.await` expression `<base>.await` (T31).
///
/// This is the ONLY place in the codegen that produces a Rust `.await`.
/// Buff has no `await` keyword — the codegen auto-inserts `.await` at two
/// sites:
///
/// 1. **Async call sites inside async fns** — when the callee is a known
///    async fn and the current fn is async, the call is wrapped:
///    `callee(args)` → `callee(args).await`.
/// 2. **`Task<T>.result()`** — `t.result()` → `t.await`.
///
/// The `ExprAwait` syn node is constructed by hand (NOT via `quote!`) so
/// the base expression is spliced directly into the `base` slot — keeping
/// the resulting syn tree as direct as possible.
fn make_await(base: SynExpr) -> SynExpr {
    SynExpr::Await(syn::ExprAwait {
        attrs: Vec::new(),
        base: Box::new(base),
        dot_token: Default::default(),
        await_token: Default::default(),
    })
}

/// T33: wrap an initializer in `Arc::new(...)` (used for Arc-shared
/// bindings — those captured across a `spawn` boundary).
///
/// Builds `std::sync::Arc::new(<inner>)` as a `syn::ExprCall` on the
/// fully-qualified `std::sync::Arc::new` path. The fully-qualified form
/// is used so generated code never needs a `use std::sync::Arc;` import
/// (mirrors the T25 HashMap pattern and the T24 Matrix pattern —
/// emit-on-demand codegen keeps the generated source self-contained).
fn wrap_in_arc_new(inner: SynExpr) -> SynExpr {
    let arc_new_path = rust_path("std::sync::Arc::new");
    let callee = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: arc_new_path,
    });
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(inner);
    SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(callee),
        paren_token: Default::default(),
        args,
    })
}

/// T33: build `Arc::clone(&name)` (used at use sites of Arc-shared
/// bindings INSIDE a spawn body).
///
/// The argument is a borrowed reference (`&name`) so `Arc::clone` bumps
/// the refcount without cloning the underlying data. The fully-qualified
/// path keeps generated source free of any `use std::sync::Arc;`.
fn arc_clone_call(name: &buff_lang_ast::common::Ident) -> SynExpr {
    let arc_clone_path = rust_path("std::sync::Arc::clone");
    let callee = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: arc_clone_path,
    });
    // `&name` — single-segment borrow of the binding.
    let borrowed_name = SynExpr::Reference(syn::ExprReference {
        attrs: Vec::new(),
        and_token: Default::default(),
        mutability: None,
        expr: Box::new(SynExpr::Path(syn::ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: syn::Path::from(ast_ident_to_syn(name)),
        })),
    });
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(borrowed_name);
    SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(callee),
        paren_token: Default::default(),
        args,
    })
}

/// T33: build `*Arc::make_mut(&mut name)` — the LHS of an assignment to
/// an Arc-shared-and-subsequently-mutated binding (CoW site).
///
/// `Arc::make_mut(&mut x)` returns `&mut T`, cloning the inner value
/// only if the Arc's refcount > 1 (i.e. when the spawned task is
/// actually observing the same Arc). The leading `*` dereferences so
/// the assignment writes through to the (possibly-cloned) inner value:
/// `*Arc::make_mut(&mut v) = vec![3, 4]`. The fully-qualified path
/// keeps generated source free of any `use std::sync::Arc;`.
fn arc_make_mut_deref(name: &buff_lang_ast::common::Ident) -> SynExpr {
    let arc_make_mut_path = rust_path("std::sync::Arc::make_mut");
    let callee = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: arc_make_mut_path,
    });
    // `&mut name` — mutable borrow of the binding.
    let mut_borrowed_name = SynExpr::Reference(syn::ExprReference {
        attrs: Vec::new(),
        and_token: Default::default(),
        mutability: Some(Default::default()),
        expr: Box::new(SynExpr::Path(syn::ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: syn::Path::from(ast_ident_to_syn(name)),
        })),
    });
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(mut_borrowed_name);
    let make_mut_call = SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(callee),
        paren_token: Default::default(),
        args,
    });
    // `*<call>` — prefix dereference so the surrounding assignment writes
    // through to the inner value.
    SynExpr::Unary(syn::ExprUnary {
        attrs: Vec::new(),
        op: syn::UnOp::Deref(Default::default()),
        expr: Box::new(make_mut_call),
    })
}

/// Allow-list of method names that ALWAYS lower to a Rust method call even
/// when called with zero arguments (T26 field-access heuristic).
///
/// Any zero-arg `obj.name` whose `name` is NOT in this list lowers to a Rust
/// field access `obj.name`. Anything in the list stays a method call
/// `obj.name()`. The list contains:
///
/// - The string-method family this codegen explicitly handles
///   (`char_count`/`byte_len`/`chars`/`bytes`/`first`/`last`/`graphemes`).
/// - Universal `clone`/`to_string`/`to_owned`/`into`/etc. that show up via
///   move analysis and the standard library.
/// - Common collection zero-arg methods (`len`/`is_empty`/`iter`/...).
/// - Numeric methods that don't take args (`abs`/`sqrt`/`floor`/...).
///
/// Adding a new zero-arg builtin in a later task MUST extend this list,
/// otherwise users calling `obj.<new_builtin>()` will see broken field
/// access codegen. The unit test `t26_known_zero_arg_methods_table_is_load_bearing`
/// pins the table so a careless rename is caught.
const KNOWN_ZERO_ARG_METHODS: &[&str] = &[
    // String methods (this codegen explicitly lowers these).
    "char_count",
    "byte_len",
    "chars",
    "bytes",
    "first",
    "last",
    "graphemes",
    // Universal / standard-library methods.
    "clone",
    "to_string",
    "to_owned",
    "into",
    "as_ref",
    "as_mut",
    "default",
    "to_lowercase",
    "to_uppercase",
    "trim",
    "trim_start",
    "trim_end",
    // Collection zero-arg methods.
    "len",
    "is_empty",
    "iter",
    "iter_mut",
    "into_iter",
    "keys",
    "values",
    "pop",
    "clear",
    // Iterator adaptors (zero-arg form).
    "rev",
    "count",
    "sum",
    "product",
    "next",
    "enumerate",
    "flatten",
    "step_by",
    // Numeric zero-arg methods.
    "abs",
    "sqrt",
    "floor",
    "ceil",
    "round",
    "signum",
    "trunc",
    "fract",
    "recip",
    "is_nan",
    "is_infinite",
    "is_finite",
    "is_sign_positive",
    "is_sign_negative",
    "to_degrees",
    "to_radians",
    "exp",
    "ln",
    "log2",
    "log10",
    "tan",
    "sin",
    "cos",
    "atan",
    "asin",
    "acos",
    "tanh",
    "sinh",
    "cosh",
    "powi",
    "powf",
];

/// Build the attribute list for a generated struct: always
/// `#[derive(Clone, Debug)]`, plus an optional `#[repr(C)]` when
/// `emit_repr_c` is true (T26 hook).
///
/// Ordering: derive attribute first, then `#[repr(C)]` (matching the layout
/// the T26 snapshot asserts). Both attributes use `syn::Attribute::parse_attr`
/// style construction (meta list / path) so they survive `prettyplease`
/// formatting without round-tripping through strings.
///
/// This is the shared derive-attribute helper extracted during the T26
/// REFACTOR step: future struct-like decls (enum repr hints, future trait
/// derives) reuse this function. The signature is `(emit_repr_c: bool) -> Vec<Attribute>`
/// so callers stay in control of whether repr(C) applies.
fn derive_and_repr_attrs(emit_repr_c: bool) -> Vec<syn::Attribute> {
    // Build `#[derive(Clone, Debug)]` via `syn::Attribute` construction.
    let derive_attr = syn::Attribute {
        pound_token: Default::default(),
        style: syn::AttrStyle::Outer,
        bracket_token: Default::default(),
        meta: syn::Meta::List(syn::MetaList {
            path: rust_path("derive"),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: {
                // Build `Clone, Debug` as a token stream — this is a
                // fixed template parsed once at codegen time (not a runtime
                // Rust-string assembler; the single string producer remains
                // `prettyplease::unparse`).
                let mut t = proc_macro2::TokenStream::new();
                t.extend(quote::quote! { Clone });
                t.extend(quote::quote! { , });
                t.extend(quote::quote! { Debug });
                t
            },
        }),
    };
    let mut attrs = vec![derive_attr];
    if emit_repr_c {
        // `#[repr(C)]` — a path with one generic-style argument `(C)`.
        let repr_attr = syn::Attribute {
            pound_token: Default::default(),
            style: syn::AttrStyle::Outer,
            bracket_token: Default::default(),
            meta: syn::Meta::List(syn::MetaList {
                path: rust_path("repr"),
                delimiter: syn::MacroDelimiter::Paren(Default::default()),
                tokens: quote::quote! { C },
            }),
        };
        attrs.push(repr_attr);
    }
    attrs
}

/// Walk the declaration list looking for any `Matrix.new(...)` constructor
/// call (T24). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to prepend the builtin `Matrix<T>` struct.
///
/// The detection is conservative: only the canonical constructor pattern
/// (`Matrix` Ident receiver, `new` method) triggers injection. 2-D indexing
/// on a Matrix-typed value WITHOUT a prior `Matrix.new(...)` would not
/// trigger injection by itself — but every well-formed Matrix program must
/// construct one first, so this signal is sufficient in practice. A
/// type-annotation-only `Matrix<T>` (with no constructor) is a rare edge
/// case deferred to a later task.
fn program_uses_matrix(decls: &[Decl]) -> bool {
    for decl in decls {
        let Decl::FuncDecl(f) = decl else {
            continue;
        };
        if block_uses_matrix(&f.body) {
            return true;
        }
    }
    false
}

/// Recursive helper for [`program_uses_matrix`]: scan a block's statements
/// and their nested expressions for a `Matrix.new(...)` call.
fn block_uses_matrix(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_matrix)
}

/// Check a single statement (and its nested expressions) for Matrix.new.
fn stmt_uses_matrix(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. } | Stmt::ExprStmt(value, _) | Stmt::Return(Some(value), _) => {
            expr_uses_matrix(value)
        }
        Stmt::Assignment { target, value, .. } => {
            expr_uses_matrix(target) || expr_uses_matrix(value)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_matrix(iter) || block_uses_matrix(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_matrix(cond) || block_uses_matrix(body),
    }
}

/// Recursively scan an expression tree for a `Matrix.new(...)` MethodCall.
fn expr_uses_matrix(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            if method.name == "new" {
                if let Expr::Ident(id, _) = receiver.as_ref() {
                    if id.name == "Matrix" {
                        return true;
                    }
                }
            }
            expr_uses_matrix(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_matrix(lhs) || expr_uses_matrix(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_matrix(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_matrix(callee) || args.iter().any(expr_uses_matrix)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_matrix(cond)
                || block_uses_matrix(then_block)
                || else_block.as_ref().is_some_and(block_uses_matrix)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e) => expr_uses_matrix(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_matrix),
        Expr::Index { base, indices, .. } => {
            expr_uses_matrix(base) || indices.iter().any(expr_uses_matrix)
        }
        // T25: a map literal may contain a Matrix expression as a key/value;
        // recurse conservatively.
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_matrix(k) || expr_uses_matrix(v)),
        Expr::Lambda { body, .. } => block_uses_matrix(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_matrix(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_matrix(scrutinee) || arms.iter().any(|arm| block_uses_matrix(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_matrix(inner),
        // T30: recurse into the `?` operand so a Matrix constructor inside a
        // propagated expression is still detected.
        Expr::Try { expr, .. } => expr_uses_matrix(expr),
        // T31: `spawn expr` — does NOT use Matrix (the task body is opaque
        // to the Matrix emit-on-demand detector for v0.5).
        Expr::Spawn { task, .. } => expr_uses_matrix(task),
        // T68: `start..end` — recurse into both bounds.
        Expr::Range { start, end, .. } => expr_uses_matrix(start) || expr_uses_matrix(end),
    }
}

/// Build the builtin `Matrix<T>` struct AND its `new` impl as a
/// `Vec<Item>` (T24).
///
/// Emits (conceptually):
///
/// ```rust,ignore
/// #[derive(Clone, Debug)]
/// pub struct Matrix<T> {
///     pub data: Vec<T>,
///     pub rows: usize,
///     pub cols: usize,
/// }
///
/// impl<T: Default + Clone> Matrix<T> {
///     pub fn new(rows: usize, cols: usize) -> Self {
///         Self {
///             data: vec![T::default(); rows * cols],
///             rows,
///             cols,
///         }
///     }
/// }
/// ```
///
/// Built via `quote!`-equivalent (`syn::parse_str` on a string literal that
/// is itself valid Rust source) and re-parsed via
/// `syn::parse_str::<syn::File>` (returns `Result`, no panic — unlike
/// `parse_quote!`). On the (unreachable) parse failure we return an empty
/// vec; the generated program would then reference an undefined `Matrix`
/// type and fail later at rustc, which is the correct degradation (a
/// codegen bug, not a user-facing panic).
///
/// **Storage note**: `data` is a flat `Vec<T>` (NOT `Vec<Vec<T>>`) so the
/// buffer is contiguous and GPU-transferable — a `Matrix<Float<32>>` of
/// `rows * cols` f32 values can be uploaded to a WGSL storage buffer
/// verbatim. This is the flat-storage pattern the REFACTOR goal targets
/// for sharing with the GPU buffer codegen (v1.0).
///
/// **Note on the string source**: this is NOT raw-string Rust codegen —
/// the string is a *fixed template* parsed once at codegen time into
/// `syn::Item`s, after which all transformation goes through the syn tree
/// and `prettyplease`. It plays the same role as the `quote!` token-stream
/// templates used elsewhere in this file (e.g. `lower_read_line`,
/// `lower_into_iter_collect`) — a compile-time-fixed scaffold that is
/// re-parsed, not a runtime Rust-string assembler. The single string
/// producer remains `prettyplease::unparse`.
fn matrix_struct_items() -> Vec<Item> {
    let src = r#"
        #[derive(Clone, Debug)]
        pub struct Matrix<T> {
            pub data: Vec<T>,
            pub rows: usize,
            pub cols: usize,
        }

        impl<T: Default + Clone> Matrix<T> {
            pub fn new(rows: usize, cols: usize) -> Self {
                Self {
                    data: vec![T::default(); rows * cols],
                    rows,
                    cols,
                }
            }
        }
    "#;
    match syn::parse_str::<File>(src) {
        Ok(file) => file.items,
        Err(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// T30 — builtin `Error` struct (emit-on-demand, mirrors Matrix pattern).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any `Error(...)` constructor call
/// (T30). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to prepend the builtin `Error` struct.
///
/// Detection is conservative: only the canonical constructor shape
/// (`FuncCall { callee: Ident("Error"), args.len() == 1 }`) triggers
/// emission. A program that mentions `Error` only in a type annotation
/// (`Result<_, Error>`) WITHOUT a constructor would not trigger emission by
/// itself — but every well-formed error-producing program must call
/// `Error(...)` to create one, so this signal is sufficient in practice.
/// (The v0.5 limitation is documented in `decisions.md`.)
fn program_uses_error(decls: &[Decl]) -> bool {
    for decl in decls {
        let Decl::FuncDecl(f) = decl else {
            continue;
        };
        if block_uses_error(&f.body) {
            return true;
        }
    }
    false
}

/// Recursive helper for [`program_uses_error`]: scan a block's statements.
fn block_uses_error(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_error)
}

/// Check a single statement (and its nested expressions) for `Error(...)`.
fn stmt_uses_error(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. } | Stmt::ExprStmt(value, _) | Stmt::Return(Some(value), _) => {
            expr_uses_error(value)
        }
        Stmt::Assignment { target, value, .. } => expr_uses_error(target) || expr_uses_error(value),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_error(iter) || block_uses_error(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_error(cond) || block_uses_error(body),
    }
}

/// Recursively scan an expression tree for an `Error(...)` constructor call.
fn expr_uses_error(expr: &Expr) -> bool {
    match expr {
        Expr::FuncCall { callee, args, .. } => {
            if let Expr::Ident(name, _) = callee.as_ref() {
                if name.name == "Error" && args.len() == 1 {
                    return true;
                }
            }
            expr_uses_error(callee) || args.iter().any(expr_uses_error)
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_uses_error(receiver) || args.iter().any(expr_uses_error)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_error(lhs) || expr_uses_error(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_error(operand),
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_error(cond)
                || block_uses_error(then_block)
                || else_block.as_ref().is_some_and(block_uses_error)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e) => expr_uses_error(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_error),
        Expr::Index { base, indices, .. } => {
            expr_uses_error(base) || indices.iter().any(expr_uses_error)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_error(k) || expr_uses_error(v)),
        Expr::Lambda { body, .. } => block_uses_error(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_error(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_error(scrutinee) || arms.iter().any(|arm| block_uses_error(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_error(inner),
        // T30: recurse into the `?` operand.
        Expr::Try { expr, .. } => expr_uses_error(expr),
        // T31: recurse into the spawn task body.
        Expr::Spawn { task, .. } => expr_uses_error(task),
        // T68: `start..end` — recurse into both bounds.
        Expr::Range { start, end, .. } => expr_uses_error(start) || expr_uses_error(end),
    }
}

/// Build the builtin `Error` struct + its `new` impl + `Display` + Error trait
/// impls as a `Vec<Item>` (T30).
///
/// Emits (conceptually):
///
/// ```rust,ignore
/// #[derive(Clone, Debug)]
/// pub struct Error {
///     pub message: String,
/// }
///
/// impl Error {
///     pub fn new(message: impl Into<String>) -> Self {
///         Self { message: message.into() }
///     }
/// }
///
/// impl std::fmt::Display for Error {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         write!(f, "{}", self.message)
///     }
/// }
///
/// impl std::error::Error for Error {}
/// ```
///
/// This makes `Error` a proper Rust error type: it implements
/// `std::error::Error` (so `?` propagation's `From` bound is satisfiable
/// when the enclosing fn returns `Result<T, Error>`), `Display` (required by
/// `std::error::Error`), and `Debug` + `Clone` (consistent with every other
/// generated type via [`derive_and_repr_attrs`]).
///
/// Built via the same fixed-template-then-`syn::parse_str` approach as
/// [`matrix_struct_items`] (T24). See that function's docstring for the
/// "this is NOT raw-string codegen" rationale — the string is a
/// compile-time-fixed scaffold re-parsed into `syn::Item`s.
fn error_struct_items() -> Vec<Item> {
    let src = r#"
        #[derive(Clone, Debug)]
        pub struct Error {
            pub message: String,
        }

        impl Error {
            pub fn new(message: impl Into<String>) -> Self {
                Self {
                    message: message.into(),
                }
            }
        }

        impl std::fmt::Display for Error {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.message)
            }
        }

        impl std::error::Error for Error {}
    "#;
    match syn::parse_str::<File>(src) {
        Ok(file) => file.items,
        Err(_) => Vec::new(),
    }
}

/// Build a Rust `as usize` cast for a vector index (T23).
///
/// Unlike [`cast_to`], this only wraps the operand in parens when it is a
/// non-atomic expression (binary/unary/cast), so the common cases stay clean:
/// `0 as usize`, `i as usize`. Compound indices like `a + b` become
/// `(a + b) as usize` so the cast doesn't bind tighter than the `+`.
fn cast_to_usize(e: SynExpr) -> SynExpr {
    let needs_parens = matches!(
        e,
        SynExpr::Binary(_) | SynExpr::Unary(_) | SynExpr::Cast(_) | SynExpr::Range(_)
    );
    let operand = if needs_parens { wrap_in_parens(e) } else { e };
    SynExpr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(operand),
        as_token: Default::default(),
        ty: Box::new(rust_path_type("usize")),
    })
}

/// Build a Rust `as` cast: `(e) as T`. The receiver is parenthesised so
/// compound expressions bind correctly (e.g. `(a + b) as f64` not `a + b as f64`).
fn cast_to(e: SynExpr, target: &str) -> SynExpr {
    SynExpr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(wrap_in_parens(e)),
        as_token: Default::default(),
        ty: Box::new(rust_path_type(target)),
    })
}

/// Build a Rust integer-literal expression (`0`, `1`, etc.).
fn make_int_lit_expr(n: i64) -> SynExpr {
    SynExpr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Int(syn::LitInt::new(&n.to_string(), ProcSpan::call_site())),
    })
}

/// Build a binary expression `lhs <op> rhs`.
fn make_binary_expr(op: syn::BinOp, lhs: SynExpr, rhs: SynExpr) -> SynExpr {
    SynExpr::Binary(syn::ExprBinary {
        attrs: Vec::new(),
        left: Box::new(lhs),
        op,
        right: Box::new(rhs),
    })
}

/// Discriminates the two Rust idioms for type-constructor prelude functions.
///
/// - `Numeric` covers `Int(x)` / `Float(x)` — Rust emits `(x) as T`.
/// - `Bool` is separate because Rust has no `as bool` cast; the numeric→bool
///   mapping is `x != 0`.
#[derive(Clone, Copy)]
enum ConvKind {
    Numeric,
    Bool,
}

/// Build a `x.parse::<T>().unwrap_or(default)` expression for the string-arg
/// branch of `Int(x)`/`Float(x)`/`Bool(x)`. Built via `quote!` so the
/// turbofish `::<T>` and method chain are constructed without hand-formatted
/// Rust.
fn parse_with_default(arg: SynExpr, target: &str, kind: &ConvKind) -> SynExpr {
    // Build `arg.parse::<target>()` as a method call with turbofish.
    let parse_call = SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(arg),
        dot_token: Default::default(),
        method: Ident::new("parse", ProcSpan::call_site()),
        turbofish: Some(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: {
                let mut p: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
                p.push(syn::GenericArgument::Type(rust_path_type(target)));
                p
            },
            gt_token: Default::default(),
        }),
        paren_token: Default::default(),
        args: Default::default(),
    });
    // `.unwrap_or(<default-lit>)` — for numerics the default is the unsuffixed
    // integer `0`; for bool it's `false`. Both are valid Rust literal tokens.
    let default_tokens: proc_macro2::TokenStream = match kind {
        ConvKind::Numeric => {
            let lit = proc_macro2::Literal::i64_unsuffixed(0);
            quote::quote! { #lit }
        }
        ConvKind::Bool => {
            quote::quote! { false }
        }
    };
    let default_expr =
        syn::parse2::<SynExpr>(default_tokens).unwrap_or_else(|_| make_int_lit_expr(0));
    // `.unwrap_or(default_expr)` — single-arg method call on the parse result.
    method_call_one_arg(parse_call, "unwrap_or", default_expr)
}

/// Mirror of the private `typeref_to_type` in `buff_lang_types::infer`.
///
/// Used by [`RustCodegen::lower_func`] to seed the [`TypeInferencer`]
/// environment with function-parameter types so subsequent `let`
/// bindings can refer to params and still get a useful inferred type.
fn typeref_to_type(ty: &TypeRef) -> Option<Type> {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "Int" => Some(Type::int_default()),
            "Float" => Some(Type::float_default()),
            "Double" => Some(Type::double()),
            "Bool" => Some(Type::bool()),
            "String" => Some(Type::string()),
            // T21: Char annotation maps to the resolved Char type.
            "Char" => Some(Type::char()),
            "Byte" => Some(Type::byte()),
            "Decimal" => Some(Type::Decimal),
            "Void" => Some(Type::Void),
            _ => None,
        },
        _ => None,
    }
}

/// Build a `Type::Path` with generic type arguments, e.g.
/// `Option<T>`, `Vec<T>`.
fn make_generic_path_type(name: &str, args: Vec<SynType>) -> SynType {
    let mut path_args: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
    for a in args {
        path_args.push(syn::GenericArgument::Type(a));
    }
    let segment = syn::PathSegment {
        ident: Ident::new(name, ProcSpan::call_site()),
        arguments: syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: path_args,
            gt_token: Default::default(),
        }),
    };
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    segments.push(segment);
    SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments,
        },
    })
}

/// Like [`make_generic_path_type`] but accepts a `::`-separated qualified
/// path (e.g. `"std::collections::HashMap"`) and attaches the generic
/// arguments to the LAST segment. Used by the T25 Map codegen so generated
/// programs can reference `std::collections::HashMap<K, V>` without a `use`
/// import (avoiding import management in v0.5).
fn make_qualified_generic_path_type(qualified_name: &str, args: Vec<SynType>) -> SynType {
    let mut path_args: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
    for a in args {
        path_args.push(syn::GenericArgument::Type(a));
    }
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    let mut parts = qualified_name.split("::").collect::<Vec<_>>();
    let last_idx = parts.len().saturating_sub(1);
    for (i, seg) in parts.drain(..).enumerate() {
        let arguments = if i == last_idx {
            syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: Default::default(),
                args: path_args.clone(),
                gt_token: Default::default(),
            })
        } else {
            syn::PathArguments::None
        };
        segments.push(syn::PathSegment {
            ident: Ident::new(seg, ProcSpan::call_site()),
            arguments,
        });
    }
    SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments,
        },
    })
}

/// Format a float so that it always has a decimal point or exponent (so the
/// `f32`/`f64` suffix binds to a float literal, not an integer).
fn float_repr(d: f64) -> String {
    let s = format!("{d}");
    if s.contains('.')
        || s.contains('e')
        || s.contains('E')
        || s == "inf"
        || s == "-inf"
        || s == "NaN"
    {
        s
    } else {
        format!("{s}.0")
    }
}

/// Build a `recv.method(arg)` single-argument method call.
///
/// Used by the string-method codegen helpers (e.g. `s.chars().skip(n)`).
fn method_call_one_arg(recv: SynExpr, method: &str, arg: SynExpr) -> SynExpr {
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(arg);
    SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(recv),
        dot_token: Default::default(),
        method: Ident::new(method, ProcSpan::call_site()),
        turbofish: None,
        paren_token: Default::default(),
        args,
    })
}

/// Take a token stream that calls a fully-qualified function with a single
/// placeholder argument `__recv` and replace that placeholder with an
/// actual lowered receiver expression.
///
/// The `tokens` argument is expected to parse as a Rust function-call
/// expression (e.g. `path::func(&__recv, true)`). We use `quote!` to splice
/// the receiver in: we re-parse a small template that names `__recv` and
/// then walk the resulting `ExprCall` to substitute the real receiver.
///
/// This indirection is needed because `quote!` cannot easily splice into
/// an arbitrary position inside a string-built token stream — we instead
/// parse the template to a real `ExprCall`, then swap the first argument.
fn splice_receiver_into_call(
    tokens: proc_macro2::TokenStream,
    recv: SynExpr,
) -> Result<SynExpr, CodegenError> {
    // Rebuild via quote! so we never hand-format. The placeholder name
    // `__recv` is referenced as a Rust identifier in the template; we then
    // construct the call by hand using the lowered receiver.
    //
    // Simpler approach: construct the call directly via syn::ExprCall with
    // the lowered recv as the first arg and `true` as the second.
    let _ = tokens; // discarded; we rebuild from scratch to stay quote!-based.
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    // `&recv` — syn doesn't have a one-liner for `&expr`, so we build it.
    let borrow_recv = SynExpr::Reference(syn::ExprReference {
        attrs: Vec::new(),
        and_token: Default::default(),
        mutability: None,
        expr: Box::new(recv),
    });
    args.push(borrow_recv);
    args.push(SynExpr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Bool(syn::LitBool::new(true, ProcSpan::call_site())),
    }));
    Ok(SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(SynExpr::Path(syn::ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: rust_path("unicode_segmentation::UnicodeSegmentation::graphemes"),
        })),
        paren_token: Default::default(),
        args,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::common::{Block, Ident as AstIdent, Param};
    use buff_lang_ast::{op::BinaryOp, op::UnaryOp, Literal};
    use buff_lang_error::Span;

    fn dummy_span() -> Span {
        Span::dummy()
    }

    fn int_lit(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n), dummy_span())
    }

    fn ident_expr(s: &str) -> Expr {
        Expr::Ident(AstIdent::new(s, dummy_span()), dummy_span())
    }

    #[test]
    fn empty_func_generates_syn_file() {
        let func = FuncDecl {
            name: AstIdent::new("empty", dummy_span()),
            params: Vec::new(),
            return_type: None,
            body: Block::empty(dummy_span()),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy_span(),
        };
        let mut codegen = RustCodegen::new();
        let file = codegen
            .generate(&[Decl::FuncDecl(func)])
            .expect("empty func must codegen");
        assert_eq!(file.items.len(), 1);
        assert!(matches!(file.items[0], Item::Fn(_)));
    }

    #[test]
    fn binary_op_lowers_to_expr_binary() {
        let mut codegen = RustCodegen::new();
        let lhs = int_lit(1);
        let rhs = int_lit(2);
        let expr = Expr::BinaryOp {
            op: BinaryOp::Add,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span: dummy_span(),
        };
        let syn_expr = codegen.lower_expr(&expr).unwrap();
        assert!(matches!(syn_expr, SynExpr::Binary(_)));
    }

    #[test]
    fn unary_neg_lowers_correctly() {
        let mut codegen = RustCodegen::new();
        let operand = int_lit(5);
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(operand),
            span: dummy_span(),
        };
        let syn_expr = codegen.lower_expr(&expr).unwrap();
        match syn_expr {
            SynExpr::Unary(u) => assert!(matches!(u.op, syn::UnOp::Neg(_))),
            other => panic!("expected Unary, got {other:?}"),
        }
    }

    #[test]
    fn type_int_maps_to_i64() {
        let codegen = RustCodegen::new();
        let tr = TypeRef::Named {
            name: AstIdent::new("Int", dummy_span()),
            span: dummy_span(),
        };
        let ty = codegen.ast_typeref_to_syn(&tr).unwrap();
        match ty {
            SynType::Path(p) => {
                let seg = p.path.segments.first().unwrap();
                assert_eq!(seg.ident.to_string(), "i64");
            }
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn type_option_maps_to_rust_option() {
        let codegen = RustCodegen::new();
        let tr = TypeRef::Option(
            Box::new(TypeRef::Named {
                name: AstIdent::new("Int", dummy_span()),
                span: dummy_span(),
            }),
            dummy_span(),
        );
        let ty = codegen.ast_typeref_to_syn(&tr).unwrap();
        match ty {
            SynType::Path(p) => {
                let seg = p.path.segments.first().unwrap();
                assert_eq!(seg.ident.to_string(), "Option");
                match &seg.arguments {
                    syn::PathArguments::AngleBracketed(ab) => assert_eq!(ab.args.len(), 1),
                    _ => panic!("expected angle-bracketed args"),
                }
            }
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn func_call_with_two_args_lowers() {
        let mut codegen = RustCodegen::new();
        let callee = ident_expr("foo");
        let args = vec![int_lit(1), int_lit(2)];
        let expr = Expr::FuncCall {
            callee: Box::new(callee),
            args,
            span: dummy_span(),
        };
        let syn_expr = codegen.lower_expr(&expr).unwrap();
        match syn_expr {
            SynExpr::Call(c) => assert_eq!(c.args.len(), 2),
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn struct_codegen_lowers_empty_struct_to_pub_struct_with_derives() {
        // T26: the pre-T26 behaviour returned CodegenError. T26 actually
        // implements struct codegen; we now verify the new GREEN path.
        let sd = buff_lang_ast::decl::StructDecl {
            name: AstIdent::new("Foo", dummy_span()),
            fields: Vec::new(),
            traits: Vec::new(),
            span: dummy_span(),
        };
        let mut codegen = RustCodegen::new();
        let result = codegen.generate(&[Decl::StructDecl(sd)]);
        let file = result.expect("struct codegen must succeed post-T26");
        assert_eq!(file.items.len(), 1);
        assert!(matches!(file.items[0], Item::Struct(_)));
    }

    #[test]
    fn float_repr_handles_integer_floats() {
        assert_eq!(float_repr(2.0), "2.0");
        assert_eq!(float_repr(2.5), "2.5");
    }

    #[test]
    fn make_let_pat_respects_mutability() {
        let pat = RustCodegen::make_let_pat(Ident::new("x", ProcSpan::call_site()), true);
        match pat {
            Pat::Ident(p) => assert!(p.mutability.is_some()),
            _ => panic!("expected Ident pat"),
        }
    }

    // Touch a few param/stmt shapes so unused-import warnings don't fire.
    #[test]
    fn _param_and_stmt_construction_smoke() {
        let _param = Param {
            name: AstIdent::new("p", dummy_span()),
            ty: TypeRef::Named {
                name: AstIdent::new("Int", dummy_span()),
                span: dummy_span(),
            },
            span: dummy_span(),
        };
        let _stmt = Stmt::Break(dummy_span());
    }

    // -----------------------------------------------------------------------
    // T22 — Fixed-width `Int<W>` codegen mapping contract.
    //
    // The T22 spec says fixed-mode overflow must "panic in debug, wrap in
    // release". Buff inherits this behaviour FOR FREE from Rust: codegen
    // maps each fixed `Int<W>` to the corresponding native Rust integer
    // (`i8`/`i16`/`i32`/`i64`/`i128`), and Rust's native arithmetic already
    // has the debug-panic/release-wrap overflow contract. No explicit
    // `checked_*` calls are emitted.
    //
    // These tests mechanically pin the mapping so a regression in
    // `buff_type_to_syn` cannot silently widen every fixed-width integer
    // (which would change the overflow boundary). See T22 evidence file
    // `task-22-overflow-modes.txt`.
    // -----------------------------------------------------------------------

    /// Helper: extract the leading path-segment ident from a `syn::Type` (or
    /// panic). Used by the T22 fixed-width mapping tests below.
    fn first_path_segment_str(ty: &SynType) -> String {
        match ty {
            SynType::Path(p) => p
                .path
                .segments
                .first()
                .map(|s| s.ident.to_string())
                .unwrap_or_else(|| panic!("path has no segments")),
            _ => panic!("expected Path, got {ty:?}"),
        }
    }

    #[test]
    fn t22_fixed_int_widths_map_to_native_rust_widths() {
        let codegen = RustCodegen::new();
        // Every fixed Int<W> must map to the SAME-width native Rust integer.
        for (w, expected) in [
            (IntWidth::W8, "i8"),
            (IntWidth::W16, "i16"),
            (IntWidth::W32, "i32"),
            (IntWidth::W64, "i64"),
            (IntWidth::W128, "i128"),
        ] {
            let ty = Type::Int { width: w };
            let syn_ty = codegen
                .buff_type_to_syn(&ty)
                .expect("Int<W> must map to a Rust type");
            assert_eq!(
                first_path_segment_str(&syn_ty),
                expected,
                "Int<{:?}] -> wrong Rust width",
                w
            );
        }
    }

    #[test]
    fn t22_fixed_int8_preserves_width_through_arithmetic() {
        // The full T22 "fixed mode preserves type" contract: an i8 value
        // stays i8 after arithmetic because (a) the TypeInferencer preserves
        // width via promote_binary and (b) the codegen maps the resulting
        // Int<8> back to i8.  We verify the codegen end of that chain here.
        // (The inferencer end is covered by `numeric_coercion::fixed_int8_*`.)
        let codegen = RustCodegen::new();
        let syn_ty = codegen
            .buff_type_to_syn(&Type::Int {
                width: IntWidth::W8,
            })
            .expect("Int<8> maps to i8");
        assert_eq!(first_path_segment_str(&syn_ty), "i8");
        // And Int<32> + Int<32> = Int<32> maps back to i32 (not widened to i64).
        let syn_ty = codegen
            .buff_type_to_syn(&Type::Int {
                width: IntWidth::W32,
            })
            .expect("Int<32> maps to i32");
        assert_eq!(first_path_segment_str(&syn_ty), "i32");
    }
}
