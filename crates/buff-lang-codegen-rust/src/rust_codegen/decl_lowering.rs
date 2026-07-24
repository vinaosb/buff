//! T105a - decl lowering: type_params/decl/struct/enum/func/extern/trait/extend (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move of `impl RustCodegen` methods into this child module so the
//! parent file shrinks. Methods are pub(super); the parent declares only
//! `mod <name>;` (inherent methods resolve by type, no `use` needed). Child
//! inherits parent imports via use super::* and may call the parent private
//! methods (descendant privacy) and the extracted helper modules.

use super::*;

impl RustCodegen {

    /// Build a `syn::Generics` from a slice of Buff [`TypeParam`]s (T13 +
    /// T38 trait bounds).
    ///
    /// Each TypeParam becomes a `syn::GenericParam::Type` with the param's
    /// name as the ident. **T38**: when `tp.bounds` is non-empty, each bound
    /// [`TypeRef`] is lowered to a `syn::TypeParamBound::Trait` and the
    /// `colon_token` + `bounds` Punctuated list are populated, producing
    /// Rust `<T: Clone + Debug>` syntax. rustc enforces the bounds at every
    /// monomorphization call site (the actual "does type X implement trait
    /// Y" check is delegated to rustc — Buff's type checker records the
    /// bounds and emits them; it does not maintain a trait-impl registry).
    ///
    /// Bound lowering supports:
    /// - `Named` bounds (`Clone`, `Debug`) → single-segment trait path.
    /// - `Generic` bounds (`Iterator<Item=T>`) → single-segment path with
    ///   angle-bracketed generic arguments (each arg lowered via
    ///   [`Self::ast_typeref_to_syn`]).
    ///
    /// Returns `syn::Generics::default()` when the slice is empty (the common
    /// case — non-generic decls). The lt_token/gt_token are `Some` only when
    /// the param list is non-empty (matching Rust's prettyplease formatting).
    pub(super) fn type_params_to_generics(
        &mut self,
        type_params: &[TypeParam],
    ) -> syn::Generics {
        if type_params.is_empty() {
            return syn::Generics::default();
        }
        let mut params: Punctuated<syn::GenericParam, syn::Token![,]> = Punctuated::new();
        for tp in type_params {
            // T38: lower each bound TypeRef to a syn::TypeParamBound::Trait.
            let mut bound_list: Punctuated<syn::TypeParamBound, syn::Token![+]> =
                Punctuated::new();
            let has_bounds = !tp.bounds.is_empty();
            for bound in &tp.bounds {
                if let Some(path) = self.typeref_to_trait_path(bound) {
                    bound_list.push(syn::TypeParamBound::Trait(syn::TraitBound {
                        paren_token: None,
                        modifier: syn::TraitBoundModifier::None,
                        lifetimes: None,
                        path,
                    }));
                }
            }
            params.push(syn::GenericParam::Type(syn::TypeParam {
                attrs: Vec::new(),
                ident: ast_ident_to_syn(&tp.name),
                colon_token: has_bounds.then(Default::default),
                bounds: bound_list,
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
    }

    /// T38: convert a bound [`TypeRef`] to a `syn::Path` suitable for a
    /// `syn::TypeParamBound::Trait`. Returns `None` for shapes that cannot
    /// form a trait path (function/union/tuple bounds — a parse error in
    /// practice, since the parser only produces Named/Generic bounds here).
    ///
    /// - `Named{name}` → single-segment path `Clone`.
    /// - `Generic{base: Named{name}, args}` → single-segment path `Iterator`
    ///   with angle-bracketed generic args (each arg lowered via
    ///   [`Self::ast_typeref_to_syn`]; an unlowerable arg drops the whole
    ///   bound via the `None` return so rustc — not prettyplease — reports
    ///   the malformed bound).
    fn typeref_to_trait_path(&mut self, bound: &TypeRef) -> Option<syn::Path> {
        match bound {
            TypeRef::Named { name, .. } => {
                let segment = syn::PathSegment {
                    ident: Ident::new(&name.name, ProcSpan::call_site()),
                    arguments: syn::PathArguments::None,
                };
                let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
                segments.push(segment);
                Some(syn::Path {
                    leading_colon: None,
                    segments,
                })
            }
            TypeRef::Generic { base, args, .. } => {
                let name = match base.as_ref() {
                    TypeRef::Named { name, .. } => &name.name,
                    _ => return None,
                };
                // Lower each generic arg to a syn::Type; if any fails, drop
                // the whole bound (return None) so rustc reports the issue
                // rather than prettyplease panicking on a malformed path.
                let lowered_args: Vec<SynType> = args
                    .iter()
                    .map(|a| self.ast_typeref_to_syn(a).ok())
                    .collect::<Option<Vec<_>>>()?;
                let mut path_args: Punctuated<syn::GenericArgument, syn::Token![,]> =
                    Punctuated::new();
                for a in lowered_args {
                    path_args.push(syn::GenericArgument::Type(a));
                }
                let segment = syn::PathSegment {
                    ident: Ident::new(name, ProcSpan::call_site()),
                    arguments: syn::PathArguments::AngleBracketed(
                        syn::AngleBracketedGenericArguments {
                            colon2_token: None,
                            lt_token: Default::default(),
                            args: path_args,
                            gt_token: Default::default(),
                        },
                    ),
                };
                let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
                segments.push(segment);
                Some(syn::Path {
                    leading_colon: None,
                    segments,
                })
            }
            // Function/Option/Union/Tuple bounds are not valid trait paths.
            _ => None,
        }
    }

    pub(super) fn lower_decl(&mut self, decl: &Decl) -> Result<Item, CodegenError> {
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
            // T93: trait declarations lower to a Rust `syn::ItemTrait`.
            // Required methods (MethodSig) become bodyless trait method
            // signatures; default methods (FuncDecl with body) become
            // trait methods WITH a default body; supertraits populate the
            // trait's `supertraits` Punctuated list.
            Decl::TraitDecl(t) => Ok(Item::Trait(self.lower_trait_decl(t)?)),
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
            // T119: `extern "ABI" [from "crate"] func name(...) -> Ret`.
            // Lowers to a `syn::ItemForeignMod` (same shape as the legacy
            // `extern func` lowering), AND records the optional source
            // crate in `extern_crates` so the CLI pipeline can populate
            // `[rust-deps]` in `buff.toml`.
            Decl::ExternFuncDecl(d) => {
                if let Some(c) = &d.crate_name {
                    self.extern_crates.insert(c.clone());
                }
                Ok(Item::ForeignMod(self.lower_extern_func_decl_with_abi(d)?))
            }
            // T75: extend blocks lower to TWO `syn::Item`s (trait + impl),
            // so they cannot be expressed via the single-`Item` return of
            // `lower_decl`. The proper emission path is via
            // [`Self::generate`], which special-cases `Decl::ExtendBlock`
            // and pushes both items. A direct call here is a caller misuse
            // — surface a clear error rather than dropping half the items.
            Decl::ExtendBlock(_) => Err(self.unsupported(
                "extend block codegen — use RustCodegen::generate (which emits trait + impl)",
            )),
            // T75b: `impl Trait for Type { type X = T; fn ... }` lowers to
            // a SINGLE Rust `syn::ItemImpl` with `trait_` set (a trait
            // impl). Unlike `extend`, this variant emits ONE item, so it
            // goes through the normal `lower_decl` path (no special-case
            // in `generate()`).
            Decl::ImplBlock(i) => Ok(Item::Impl(self.lower_impl_block(i)?)),
        }
    }

    /// Lower a Buff [`AstStructDecl`] to a Rust [`syn::ItemStruct`] (T26).
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// #[derive(Clone, PartialEq, Debug)]                 // or with `Hash` too
    /// pub struct Name {
    ///     pub field: <rust_type>,
    ///     ...
    /// }
    /// ```
    ///
    /// T107 extends T26's `#[derive(Clone, Debug)]` to also derive
    /// `PartialEq` (always — every Rust primitive Buff uses impls
    /// `PartialEq`, including floats) and `Hash` (CONDITIONALLY — only when
    /// every field's Rust type impls `Hash`; see [`struct_derive_attrs`]
    /// and [`Self::compute_hash_safe_structs`] for the conditional logic).
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
    /// mechanism only. See [`struct_derive_attrs`] for attribute ordering.
    ///
    /// # `traits` field
    ///
    /// The `traits` field on [`AstStructDecl`] is currently ignored at
    /// codegen time — Buff emits blanket `Clone + PartialEq + Debug`
    /// derives (plus `Hash` when safe); user-specified trait impls are a
    /// later task.
    pub(super) fn lower_struct_decl(&mut self, s: &AstStructDecl) -> Result<ItemStruct, CodegenError> {
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

        // T107: build the derive attribute set. ALWAYS include Clone,
        // PartialEq, Debug. Include Hash ONLY when EVERY field's type is
        // Hash-safe (recursively across user struct references — the
        // transitive fixpoint is precomputed in `self.hash_safe_structs`).
        // Rust's derived `Hash` impl requires ALL field types to impl
        // `Hash`, so a struct with a Float/Double/Vector/Map field (or a
        // field whose user-struct type itself can't derive Hash) must NOT
        // carry the Hash derive — rustc would reject the impl.
        let all_fields_hash_safe = s
            .fields
            .iter()
            .all(|(_, ty)| type_is_hash_safe(ty, &self.hash_safe_structs));

        // T50: GPU-bound structs (those that flow through a parallel
        // combinator) get a STRICT SUPERSET of the regular derive list
        // PLUS `#[repr(C)]` for stable C layout. The GPU path replaces
        // the regular derive path entirely: it always emits
        // `Clone, Copy, PartialEq, Debug, bytemuck::Pod,
        // bytemuck::Zeroable` + `#[repr(C)]`, and NEVER includes `Hash`
        // (Pod + floats are incompatible with Hash; GPU-bound structs
        // typically have Float fields anyway). The original T26 hook
        // (`emit_repr_c` via `mark_struct_repr_c`) is preserved as a
        // manual override — a struct is emitted with repr(C) if EITHER
        // the gpu-bound analysis marks it OR the manual hook does.
        let is_gpu_bound = self.gpu_bound_structs.contains(&s.name.name);
        let attrs = if is_gpu_bound {
            gpu_struct_derive_attrs()
        } else {
            struct_derive_attrs(emit_repr_c, all_fields_hash_safe)
        };

        Ok(ItemStruct {
            attrs,
            vis: Visibility::Public(Default::default()),
            struct_token: Default::default(),
            ident: ast_ident_to_syn(&s.name),
            generics: self.type_params_to_generics(&s.type_params),
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
    pub(super) fn lower_enum_decl(&mut self, e: &AstEnumDecl) -> Result<ItemEnum, CodegenError> {
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

        // T13: Generic params are now built via the shared `type_params_to_generics`
        // helper (same as struct + func). Bounds always empty in T13 (T38 populates).
        let generics = self.type_params_to_generics(&e.type_params);

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

    pub(super) fn lower_func(&mut self, f: &FuncDecl) -> Result<ItemFn, CodegenError> {
        // Reset move-analysis state and pre-classify Copy vars for this fn.
        self.move_analyzer.reset();
        self.move_analyzer.preanalyze_func(f);

        // T42: install this function's atomic-promotable captures.
        // The set is consulted by the `LetDecl`, `Assignment`, and
        // `Expr::Ident` lowering arms to emit `AtomicI64::new(...)` /
        // `fetch_add(...)` / `load(...)` instead of plain mutable
        // integer codegen. Reset per function (a different function's
        // `t` is independent even if it shares the name).
        self.current_atomic_set = self.atomic_promotions.for_func(&f.name.name);

        // T100: reset the per-function deferred-expression accumulator.
        // `Stmt::Defer` arms inside the body (collected by lower_block)
        // push here; we drain them in reverse (LIFO) at every return and
        // at the body's fall-through tail (below).
        self.deferred_exprs.clear();

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
            generics: self.type_params_to_generics(&f.type_params),
            paren_token: Default::default(),
            inputs,
            variadic: None,
            output,
        };

        let mut block = self.lower_block(&f.body)?;

        // T124c: emit the tracing-subscriber init at the top of `main`
        // when the program uses the prelude `Log` module. The init MUST
        // be the FIRST statement so any subsequent `Log.info(...)` call
        // is captured by the subscriber (an uninstalled subscriber
        // silently drops events — `tracing` is a no-op without one).
        // Mirrors the `#[tokio::main]` attribute injection pattern
        // (T31): a single program-wide decision made in `generate()`
        // (recorded in `extern_crates`) drives a per-`main` emission
        // here. The init is wrapped in a `{ ... }` block so its helper
        // binding (`__buff_log_filter`) doesn't leak into the user's
        // `main` body — see [`tracing_subscriber_init_stmt`] for the
        // design rationale.
        //
        // Only emitted for `main` (NOT for arbitrary fns) so a library
        // function using `Log` doesn't pay the init cost — the calling
        // binary's `main` is the canonical install site for the global
        // subscriber.
        if f.name.name == "main" && self.extern_crates.contains("tracing") {
            if let Some(init_stmt) = tracing_subscriber_init_stmt() {
                block.stmts.insert(0, init_stmt);
            }
        }

        // T114: auto-load `.env` at the top of `main` (like Bun/Deno/Node.js
        // dotenv). Emitted unconditionally for `main` — reads KEY=VALUE pairs
        // from `.env` into the process environment. Does NOT override existing
        // env vars (only sets if absent). Simple parsing: one KEY=VALUE per
        // line, skip `#` comments, skip blank lines. No complex .env syntax.
        //
        // Emitted AFTER the tracing subscriber init (if any) so the subscriber
        // can read env vars from `.env` for its own configuration (e.g.
        // `BUFF_LOG` filter).
        if f.name.name == "main" {
            if let Some(init_stmt) = dotenv_auto_load_stmt() {
                block.stmts.push(init_stmt);
            }
        }

        // T100: fall-through tail. Any defers still in the accumulator at
        // this point were NOT drained by an explicit `return` inside the
        // body — they fire at the implicit function exit (the body's last
        // statement was something other than a return, OR the body ends
        // with a trailing expr/let). Emit them in REVERSE order (LIFO:
        // last-registered defer runs first) as tail sibling statements.
        // `drain(..).rev()` consumes the accumulator so a second drain at
        // a later exit point would be a no-op (defensive — there is no
        // later exit point after the body, but the shape is symmetric with
        // the return-site drain in lower_block).
        for deferred in self.deferred_exprs.drain(..).rev() {
            block
                .stmts
                .push(SynStmt::Expr(deferred, Some(Default::default())));
        }

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
                // T0-B4: `@feature(name)` is consumed by the pre-filter
                // pass ([`filter_by_features`]) BEFORE lowering. By the
                // time we reach here, the fn is enabled — strip the
                // attribute (no Rust equivalent needed; Cargo handles
                // feature gating at the dep-resolution layer, not via
                // per-fn attributes).
                "feature" => continue,
                // T0-B2: `@internal` is convention-only for v1.13-v1.17.
                // The LSP / docs surface a warning when the fn is used
                // outside its declaring crate; no Rust lowering needed.
                "internal" => continue,
                // T0-F2: test-related attributes (alongside @test). Each
                // lowers 1:1 to the corresponding Rust test attribute.
                "should_panic" => attrs.push(syn::parse_quote!(#[should_panic])),
                "ignore" => attrs.push(syn::parse_quote!(#[ignore])),
                "bench" => attrs.push(syn::parse_quote!(#[bench])),
                // T0-F2: `@property` requires proptest as an extern dep
                // (arrives with future buff-test crate). For v1.13 we
                // strip it — the attribute is accepted by the parser so
                // users can write the source today; lowering is deferred.
                "property" => continue,
                // T0-G3: `@deprecated(since = "X", replacement = "Y")`
                // lowers to Rust's `#[deprecated(since = "X", note =
                // "use 'Y'")]`. Both keyword args are optional — when
                // absent, the corresponding Rust field is omitted.
                "deprecated" => {
                    let since = attr.named_args.get("since");
                    let note: Option<String> = attr
                        .named_args
                        .get("replacement")
                        .map(|r| format!("use '{r}'"))
                        .or_else(|| attr.named_args.get("note").cloned());
                    let parsed = match (since, note) {
                        (Some(s), Some(n)) => {
                            syn::parse_quote!(#[deprecated(since = #s, note = #n)])
                        }
                        (Some(s), None) => syn::parse_quote!(#[deprecated(since = #s)]),
                        (None, Some(n)) => syn::parse_quote!(#[deprecated(note = #n)]),
                        (None, None) => syn::parse_quote!(#[deprecated]),
                    };
                    attrs.push(parsed);
                }
                // T65: `@blocking` marks a function as performing blocking
                // I/O (for the async runtime). Buff auto-propagates `async`;
                // `@blocking` signals that the function's body blocks the
                // thread and should be dispatched via `spawn_blocking` when
                // called from an async context. We emit a `#[doc]` marker so
                // the attribute survives into the generated Rust source as
                // machine-readable metadata for the runtime dispatch layer,
                // without requiring a custom Rust attribute (which would be
                // rejected by rustc). This is a pure marker — the function's
                // async propagation is unchanged (a `@blocking` fn is still
                // emitted `async` if it transitively calls async fns, so the
                // generated Rust always compiles).
                "blocking" => attrs.push(syn::parse_quote!(#[doc = "@blocking"])),
                // T66: `@workgroup(N)` sets the GPU workgroup size for a
                // GPU-dispatched function. The codegen emits a `#[doc]`
                // marker recording N so the runtime/dispatch layer can read
                // the workgroup size from the generated source metadata.
                // When N is omitted (argument-less `@workgroup`), the default
                // of 64 is used (matching the runtime's default workgroup
                // size). This is a pure metadata marker — the actual WGSL
                // shader + wgpu dispatch config is emitted by
                // `buff-lang-codegen-wgsl` and `buff-lang-runtime`.
                "workgroup" => {
                    let n = attr.args.first().map(|s| s.as_str()).unwrap_or("64");
                    let doc = format!("@workgroup({n})");
                    attrs.push(syn::parse_quote!(#[doc = #doc]));
                }
                // T76: `@inline` hints to rustc/LLVM to inline this function
                // at call sites. Rust's `#[inline]` is the direct equivalent.
                "inline" => attrs.push(syn::parse_quote!(#[inline])),
                // T76: `@no_inline` hints to rustc/LLVM to NEVER inline this
                // function (useful for cold paths or to reduce code bloat).
                // Rust's `#[inline(never)]` is the direct equivalent.
                "no_inline" => attrs.push(syn::parse_quote!(#[inline(never)])),
                // T69: `@no-alloc` marks the function as allocation-free. The
                // attribute is consumed here (stripped — no Rust lowering); the
                // verification is a post-lowering scan of the generated body
                // (see the `is_no_alloc` block below), which emits a warning
                // per heap-allocating construct found. Both the hyphenated
                // (`@no-alloc`) and underscored (`@no_alloc`) spellings are
                // accepted (defensive — matches the @no_inline convention).
                "no-alloc" | "no_alloc" => continue,
                // T64: `@prefer(cpu)` / `@prefer(gpu)` / `@prefer(npu)` —
                // dispatch hint overrides. The arg selects the target
                // backend the user wants the runtime to favour. Emitted
                // as a `#[doc]` marker (mirrors the T65 `@blocking` and
                // T66 `@workgroup` pattern) so the runtime dispatch layer
                // (`buff_lang_runtime::hints`) can read the user's
                // override from the generated source metadata, without
                // requiring a custom Rust attribute (which rustc would
                // reject). `@prefer(cpu)` is new in T64: it pins the
                // function to the CPU path, defeating the arithmetic-
                // intensity GPU promotion that the automatic dispatcher
                // would otherwise apply. `@prefer(gpu)`/`@prefer(npu)`
                // are the v1.0 accelerator hints (honored subject to the
                // cost-model override in `decide_with_prefer`). Pure
                // metadata — the generated Rust is semantically unchanged
                // and always compiles; the marker only influences runtime
                // dispatch at call sites.
                "prefer" => {
                    let target = attr.args.first().map(|s| s.as_str()).unwrap_or("cpu");
                    let doc = format!("@prefer({target})");
                    attrs.push(syn::parse_quote!(#[doc = #doc]));
                }
                // T64: `@force(gpu)` — unconditional dispatch override.
                // Unlike `@prefer(gpu)`, `@force(gpu)` bypasses the cost
                // model entirely: the runtime routes to `GpuCompute`
                // whenever a GPU adapter is available, regardless of
                // element count (no `PREFER_GPU_MIN_ELEMENTS` gate) or
                // arithmetic intensity. Emitted as a `#[doc]` marker for
                // the runtime dispatch layer (same shape as `@prefer`).
                // Falls back to CPU only when no GPU is present — graceful
                // degradation, never panics on GPU-less hosts. The arg is
                // carried verbatim so future targets (`@force(npu)`) need
                // no codegen change.
                "force" => {
                    let target = attr.args.first().map(|s| s.as_str()).unwrap_or("gpu");
                    let doc = format!("@force({target})");
                    attrs.push(syn::parse_quote!(#[doc = #doc]));
                }
                // Unknown attribute — surface as a codegen error so the
                // user knows it was not applied (rather than silently
                // dropping it). Future tasks can add recognised attributes
                // (e.g. `@inline` → `#[inline]`) here.
                other => {
                    return Err(self.unsupported(&format!(
                        "unrecognised attribute `@{other}` \
                         (supported: @test, @feature, @internal, @deprecated, \
                         @should_panic, @ignore, @bench, @property, @blocking, \
                         @workgroup, @inline, @no_inline, @prefer, @force, \
                         @no-alloc)"
                    )));
                }
            }
        }

        // T69: `@no-alloc` lint. When the function is marked allocation-
        // free, scan the GENERATED Rust body for heap-allocating
        // constructs (`vec!`, `Box::new`, `String::from`, `Vec::new`,
        // `String::new`, `format!`, `.to_string()`, `.to_owned()`). Each
        // hit emits a WARNING (via the shared `warnings` channel, surfaced
        // to the CLI / tests via [`Self::take_warnings`]) — this is a lint,
        // NOT a hard error: the code still compiles and runs; the user is
        // informed that their allocation-free promise was violated so they
        // can fix the body or drop the attribute. The scan walks the
        // lowered [`syn::Block`] with a `syn::visit::Visit` walker, catching
        // allocations however deeply nested. Both `@no-alloc` (hyphenated)
        // and `@no_alloc` (underscored) spellings are honored.
        let is_no_alloc = f
            .attributes
            .iter()
            .any(|a| matches!(a.name.name.as_str(), "no-alloc" | "no_alloc"));
        if is_no_alloc {
            for violation in check_no_alloc_violations(&block) {
                self.warnings.push(Diagnostic::warning(
                    format!("@no-alloc violation in `{}`: {violation}", f.name.name),
                    f.span,
                ));
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
    pub(super) fn lower_extern_func_decl(
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

    /// Lower a Buff [`ExternFuncDecl`] (T119) to a Rust
    /// [`syn::ItemForeignMod`] of the form
    /// `extern "ABI" { fn name(params) -> Ret; }`.
    ///
    /// This is the rich-ABI sibling of [`Self::lower_extern_func_decl`] —
    /// the difference is that the ABI string is taken from the user's
    /// declaration (`extern "C"`, `extern "system"`, …) rather than
    /// hardcoded to `"C"`. In v1.3 only `"C"` is accepted by the parser,
    /// so in practice the two methods produce byte-identical output today
    /// — but this method preserves the user's spelling so future ABIs
    /// (`"system"`, `"stdcall"`, …) can be added by widening the parser
    /// accept-list without touching codegen.
    ///
    /// The optional `crate_name` annotation is recorded by the caller
    /// ([`Self::lower_decl`]) BEFORE this method runs — by the time we
    /// get here the crate (if any) is already in [`Self::extern_crates`]
    /// so this method's only job is the foreign-mod emission.
    pub(super) fn lower_extern_func_decl_with_abi(
        &mut self,
        d: &buff_lang_ast::ExternFuncDecl,
    ) -> Result<syn::ItemForeignMod, CodegenError> {
        // Build the parameter list (same logic as lower_extern_func_decl).
        let mut inputs: Punctuated<syn::FnArg, syn::Token![,]> = Punctuated::new();
        for p in &d.params {
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
        let output = match &d.return_type {
            Some(ty) => {
                ReturnType::Type(Default::default(), Box::new(self.ast_typeref_to_syn(ty)?))
            }
            None => ReturnType::Default,
        };
        let foreign_fn = syn::ForeignItemFn {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            sig: Signature {
                constness: None,
                asyncness: None,
                unsafety: None,
                abi: None,
                fn_token: Default::default(),
                ident: ast_ident_to_syn(&d.name),
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
            unsafety: None,
            abi: syn::Abi {
                extern_token: Default::default(),
                // Use the user-written ABI verbatim ("C", "system", …).
                // The parser has already validated it's in the v1.3
                // accept-list (only "C").
                name: Some(syn::LitStr::new(&d.abi, ProcSpan::call_site())),
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
    pub(super) fn lower_extern_crate_use(&self, name: &str) -> syn::ItemUse {
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

    /// T75: lower an `extend TYPE { fn ...; ... }` block to TWO top-level
    /// Rust items — an extension-trait declaration (signatures only) and a
    /// blanket-free `impl Trait for Type { ... }` (the bodies).
    ///
    /// This is the ONLY decl variant whose lowering produces more than one
    /// `syn::Item`; [`Self::generate`] special-cases `Decl::ExtendBlock` to
    /// extend the items Vec with both items.
    ///
    /// # Trait-name scheme
    ///
    /// The trait name is derived from the target type name as
    /// `BuffExt{Type}` (e.g. `extend String` → `BuffExtString`,
    /// `extend Int` → `BuffExtInt`). v0.5 single extend-block per target
    /// type is the common case; multi-block merging (`extend String { ... }`
    /// twice) is deferred (would collide here today — a future task could
    /// suffix `_2`, `_3`, … or merge the methods into one trait).
    ///
    /// # Per-method lowering
    ///
    /// Each [`FuncDecl`] inside the block is lowered TWICE:
    /// - **trait side** — only the SIGNATURE (`fn name(params) -> Ret;`),
    ///   built by reusing the same signature-construction logic as
    ///   [`Self::lower_func`] (params, return type, asyncness) but WITHOUT
    ///   a body. The signature is wrapped in a [`syn::TraitItemFn`] and
    ///   pushed onto the trait's `items` list.
    /// - **impl side** — the FULL [`syn::ItemFn`] produced by
    ///   [`Self::lower_func`], rewrapped as a [`syn::ImplItemFn`] (we
    ///   strip the ItemFn wrapper and reuse the inner `Signature` + body
    ///   Block). The vis is `Inherited` (Rust impl items are always
    ///   implicitly public via the trait).
    ///
    /// Async-extension methods (`fn async_fetch(self) -> String` inside an
    /// `extend`) propagate the `async` flag the same way regular funcs do.
    ///
    /// # Errors
    ///
    /// Returns [`CodegenError`] if any method body fails to lower (the
    /// underlying `lower_func`/signature-builder propagates the error).
    pub(super) fn lower_extend_block_items(
        &mut self,
        e: &buff_lang_ast::ExtendBlock,
    ) -> Result<Vec<Item>, CodegenError> {
        // Derive the trait name: `BuffExt{TargetTypeName}`.
        let target_name = match &e.target {
            TypeRef::Named { name, .. } => &name.name,
            // Generic targets (`extend Vector<T>`) are deferred — they'd
            // need the trait + impl to carry matching generic params.
            TypeRef::Generic { base, .. } => {
                if let TypeRef::Named { name, .. } = base.as_ref() {
                    &name.name
                } else {
                    return Err(self.unsupported(
                        "extend block with nested generic target type (only named types supported in v0.5)",
                    ));
                }
            }
            _ => {
                return Err(self.unsupported(
                    "extend block with non-named target type (only TypeRef::Named supported in v0.5)",
                ));
            }
        };
        let trait_name_str = format!("BuffExt{target_name}");
        let trait_ident = Ident::new(&trait_name_str, ProcSpan::call_site());
        let target_type = self.ast_typeref_to_syn(&e.target)?;

        // Build the trait item list (signatures only) and the impl item
        // list (full fns) in one pass over the methods.
        let mut trait_items: Vec<syn::TraitItem> = Vec::with_capacity(e.methods.len());
        let mut impl_items: Vec<syn::ImplItem> = Vec::with_capacity(e.methods.len());
        for method in &e.methods {
            // Trait signature (no body). Reuse the signature-builder from
            // lower_func by building the full ItemFn first, then peeling
            // off the Signature and synthesising a TraitItemFn from it.
            let item_fn = self.lower_func(method)?;
            // T75: extension methods almost always take `self` as the
            // first parameter. Rust's idiomatic spelling is a bare
            // receiver `self` (NOT `self: Type`). When the Buff method's
            // first param is named `self`, rewrite the FIRST signature
            // input from a `FnArg::Typed(PatType { ident: self, ty })`
            // into a `FnArg::Receiver { self_token }` so the generated
            // Rust reads `fn shout(self) -> ...` instead of
            // `fn shout(self: String) -> ...`. Both forms compile, but
            // the bare form is the spec QA shape and matches what every
            // other extension-trait library emits.
            let trait_sig = rewrite_self_receiver(item_fn.sig.clone());
            let impl_sig = rewrite_self_receiver(item_fn.sig.clone());
            // The trait item carries the signature with NO default body.
            // `default: None` means "no default implementation — impls
            // must provide one" (exactly what the impl side below does).
            trait_items.push(syn::TraitItem::Fn(syn::TraitItemFn {
                attrs: Vec::new(),
                sig: trait_sig,
                default: None,
                semi_token: Some(Default::default()),
            }));
            // The impl item carries the FULL fn (body + signature). We
            // re-wrap the ItemFn's signature + block into an ImplItemFn.
            // Visibility is Inherited — Rust impl items inherit from the
            // trait, so emitting `pub` here would be a lint warning.
            impl_items.push(syn::ImplItem::Fn(syn::ImplItemFn {
                attrs: item_fn.attrs,
                vis: Visibility::Inherited,
                defaultness: None,
                sig: impl_sig,
                block: *item_fn.block,
            }));
        }

        // Assemble the trait item: `trait BuffExtString { fn ...; ... }`.
        let trait_item = Item::Trait(syn::ItemTrait {
            attrs: Vec::new(),
            vis: Visibility::Public(Default::default()),
            unsafety: None,
            auto_token: None,
            restriction: None,
            trait_token: Default::default(),
            ident: trait_ident,
            generics: Default::default(),
            colon_token: None,
            supertraits: Punctuated::new(),
            brace_token: Default::default(),
            items: trait_items,
        });

        // Assemble the impl item:
        // `impl BuffExtString for String { fn ... { ... } ... }`.
        // The `trait_` tuple is `(Option<Not>, Path, for_token)` —
        // `None` for the bang means "implementing" (vs `!` for
        // "auto-trait negative impl"), and `for_token` is the literal
        // `for` keyword between the trait path and the self type.
        let impl_item = Item::Impl(syn::ItemImpl {
            attrs: Vec::new(),
            defaultness: None,
            unsafety: None,
            generics: Default::default(),
            impl_token: Default::default(),
            trait_: Some((
                None,
                syn::Path::from(Ident::new(&trait_name_str, ProcSpan::call_site())),
                Default::default(),
            )),
            self_ty: Box::new(target_type),
            brace_token: Default::default(),
            items: impl_items,
        });

        Ok(vec![trait_item, impl_item])
    }

    /// T93: lower a Buff [`buff_lang_ast::TraitDecl`] to a Rust
    /// [`syn::ItemTrait`] with default methods and supertrait inheritance.
    ///
    /// Emits (conceptually):
    ///
    /// ```ignore
    /// // Buff:
    /// //   trait Greetable {
    /// //       fn name() -> String;
    /// //       fn greet() { print(name()) }
    /// //   }
    /// //   trait Pet : Animal { fn pet() { ... } }
    /// // Rust:
    /// pub trait Greetable {
    ///     fn name() -> String;
    ///     fn greet() { /* default body */ }
    /// }
    /// pub trait Pet: Animal {
    ///     fn pet() { /* default body */ }
    /// }
    /// ```
    ///
    /// # Required vs default methods
    ///
    /// - REQUIRED methods ([`buff_lang_ast::MethodSig`], bodyless) lower to
    ///   `syn::TraitItemFn` with `default: None` — a bodyless trait method
    ///   signature that implementors MUST provide a body for.
    /// - DEFAULT methods ([`buff_lang_ast::FuncDecl`] with body) lower to
    ///   `syn::TraitItemFn` with `default: Some(block)` — a trait method
    ///   WITH a default body that implementors inherit unless they override.
    ///
    /// The member order is PRESERVED (required methods first, then
    /// defaults), matching the source declaration order within each
    /// category. (Buff syntax interleaves them; the codegen groups them
    /// because the AST stores them in separate Vecs. This is acceptable
    /// for Rust — trait method order is not semantically significant.)
    ///
    /// # Supertraits
    ///
    /// Each supertrait [`TypeRef`] (today always [`TypeRef::Named`]) lowers
    /// to a `syn::TypeParamBound::Trait` with a single-segment path. The
    /// supertraits populate the trait's `supertraits` Punctuated list,
    /// producing Rust `trait Pet: Animal { ... }` syntax. Multiple
    /// supertraits are `+`-separated in Rust (e.g. `trait A: B + C`).
    ///
    /// # `&self` receiver
    ///
    /// Trait methods that declare a `self` parameter (the first param named
    /// `self`) are rewritten to a bare `syn::FnArg::Receiver` via the T75
    /// [`rewrite_self_receiver`] helper, matching the extend-block
    /// convention. Methods WITHOUT a `self` param (associated functions)
    /// are emitted as-is — valid Rust trait syntax.
    pub(super) fn lower_trait_decl(
        &mut self,
        t: &buff_lang_ast::TraitDecl,
    ) -> Result<syn::ItemTrait, CodegenError> {
        let trait_ident = ast_ident_to_syn(&t.name);

        // Build the supertraits Punctuated list. Each supertrait is a
        // TypeParamBound::Trait with a single-segment path. We only
        // support named supertraits today (generic supertraits like
        // `trait Foo : Bar<Int>` are deferred).
        let mut supertraits: Punctuated<syn::TypeParamBound, syn::Token![+]> = Punctuated::new();
        for st in &t.supertraits {
            let st_name = match st {
                TypeRef::Named { name, .. } => &name.name,
                _ => {
                    return Err(self.unsupported(
                        "trait supertrait that is not a simple named type (generic supertraits are deferred)",
                    ));
                }
            };
            let path = syn::Path::from(Ident::new(st_name, ProcSpan::call_site()));
            supertraits.push(syn::TypeParamBound::Trait(syn::TraitBound {
                paren_token: None,
                modifier: syn::TraitBoundModifier::None,
                lifetimes: None,
                path,
            }));
        }

        // Build the trait item list: associated types first (T75b), then
        // required methods, then defaults. The capacity hint covers all
        // three categories.
        let mut trait_items: Vec<syn::TraitItem> = Vec::with_capacity(
            t.associated_types.len() + t.required.len() + t.defaults.len(),
        );

        // T75b: ASSOCIATED TYPES — `type Item;` (or `type Item: Bound;`).
        // Each lowers to a `syn::TraitItem::AssocType` (`type Item;`).
        // Bounds are translated into a `Punctuated<TypeParamBound, +>`
        // mirroring the supertrait encoding above. An associated type
        // without bounds (the common case) emits a bare `type Item;`.
        for at in &t.associated_types {
            let mut bounds: Punctuated<syn::TypeParamBound, syn::Token![+]> = Punctuated::new();
            for b in &at.bounds {
                let b_name = match b {
                    TypeRef::Named { name, .. } => &name.name,
                    _ => {
                        return Err(self.unsupported(
                            "associated-type bound that is not a simple named type (generic bounds are deferred)",
                        ));
                    }
                };
                let path = syn::Path::from(Ident::new(b_name, ProcSpan::call_site()));
                bounds.push(syn::TypeParamBound::Trait(syn::TraitBound {
                    paren_token: None,
                    modifier: syn::TraitBoundModifier::None,
                    lifetimes: None,
                    path,
                }));
            }
            trait_items.push(syn::TraitItem::Type(syn::TraitItemType {
                attrs: Vec::new(),
                type_token: Default::default(),
                ident: ast_ident_to_syn(&at.name),
                generics: Default::default(),
                colon_token: (!bounds.is_empty()).then(Default::default),
                bounds,
                // `default: None` → no default type (implementors MUST bind).
                default: None,
                semi_token: Default::default(),
            }));
        }

        // REQUIRED methods — bodyless signatures.
        for req in &t.required {
            let sig =
                self.build_method_signature(req.name.clone(), &req.params, &req.return_type)?;
            let sig = rewrite_self_receiver(sig);
            trait_items.push(syn::TraitItem::Fn(syn::TraitItemFn {
                attrs: Vec::new(),
                sig,
                // `default: None` → required method (no default body).
                default: None,
                semi_token: Some(Default::default()),
            }));
        }

        // DEFAULT methods — signature + body. We reuse lower_func to get
        // the full ItemFn (body + signature + move-analysis), then extract
        // the signature and block for the TraitItemFn default body.
        for def in &t.defaults {
            let item_fn = self.lower_func(def)?;
            let sig = rewrite_self_receiver(item_fn.sig);
            trait_items.push(syn::TraitItem::Fn(syn::TraitItemFn {
                attrs: item_fn.attrs,
                sig,
                // `default: Some(block)` → trait method WITH a default body.
                default: Some(*item_fn.block),
                // No semi_token when a default body is present.
                semi_token: None,
            }));
        }

        // Assemble the trait item.
        Ok(syn::ItemTrait {
            attrs: Vec::new(),
            vis: Visibility::Public(Default::default()),
            unsafety: None,
            auto_token: None,
            restriction: None,
            trait_token: Default::default(),
            ident: trait_ident,
            generics: Default::default(),
            // colon_token is Some only when supertraits is non-empty.
            colon_token: (!t.supertraits.is_empty()).then(Default::default),
            supertraits,
            brace_token: Default::default(),
            items: trait_items,
        })
    }

    /// T75b: lower a Buff [`buff_lang_ast::ImplBlock`] to a Rust
    /// [`syn::ItemImpl`] with `trait_` set — a trait-impl (not an
    /// inherent impl).
    ///
    /// Emits (conceptually):
    ///
    /// ```ignore
    /// // Buff:
    /// //   trait Container { type Item; fn get(i: Int) -> Item; }
    /// //   struct Box { value: Int }
    /// //   impl Container for Box {
    /// //       type Item = Int;
    /// //       fn get(i: Int) -> Int { return self.value }
    /// //   }
    /// // Rust:
    /// impl Container for Box {
    ///     type Item = i64;
    ///     fn get(&self, i: i64) -> i64 { self.value }
    /// }
    /// ```
    ///
    /// # Member lowering
    ///
    /// - **Associated-type bindings** (`type Item = T;`) become
    ///   `syn::ImplItem::AssocType` items — one per binding. The
    ///   `eq_token` and `ty` fields carry the concrete type choice.
    /// - **Method implementations** (`fn ... { body }`) become
    ///   `syn::ImplItem::Fn` items via [`Self::lower_func`] (same path as
    ///   extend-block methods — full move-analysis, signature, body).
    ///
    /// # `&self` receiver
    ///
    /// Same rewrite as [`Self::lower_extend_block_items`]: a first param
    /// named `self` is converted to a bare `FnArg::Receiver` so the
    /// generated Rust reads `fn get(&self, ...)` instead of
    /// `fn get(self: Box, ...)`. The receiver is borrowed (`&self`) by
    /// default — same convention as the extend-block path.
    ///
    /// # Errors
    ///
    /// Returns [`CodegenError`] (via [`Self::unsupported`]) when:
    /// - the trait name is not a simple [`TypeRef::Named`] (generic trait
    ///   impls are deferred),
    /// - the target type is not a simple [`TypeRef::Named`] (generic
    ///   targets are deferred),
    /// - any method body fails to lower.
    pub(super) fn lower_impl_block(
        &mut self,
        i: &buff_lang_ast::ImplBlock,
    ) -> Result<syn::ItemImpl, CodegenError> {
        // Trait name — must be a simple named type today. Generic trait
        // impls (`impl Iterable<Int> for Vec<Int>`) are deferred.
        let trait_path = match &i.trait_name {
            TypeRef::Named { name, .. } => {
                syn::Path::from(Ident::new(&name.name, ProcSpan::call_site()))
            }
            _ => {
                return Err(self.unsupported(
                    "impl block with non-named trait name (generic trait impls are deferred)",
                ));
            }
        };
        // Target type — must be a simple named type today.
        let target_type = self.ast_typeref_to_syn(&i.target)?;

        // Build the impl-items list: associated-type bindings first (T75b),
        // then method implementations. Capacity hint covers both.
        let mut impl_items: Vec<syn::ImplItem> =
            Vec::with_capacity(i.type_bindings.len() + i.methods.len());

        // T75b: ASSOCIATED-TYPE BINDINGS — `type Item = T;`. Each becomes
        // a `syn::ImplItem::Type` carrying the concrete type choice.
        for b in &i.type_bindings {
            let ty = self.ast_typeref_to_syn(&b.target)?;
            impl_items.push(syn::ImplItem::Type(syn::ImplItemType {
                attrs: Vec::new(),
                vis: Visibility::Inherited,
                defaultness: None,
                type_token: Default::default(),
                ident: ast_ident_to_syn(&b.name),
                generics: Default::default(),
                eq_token: Default::default(),
                ty,
                semi_token: Default::default(),
            }));
        }

        // METHOD IMPLEMENTATIONS — `fn ... { body }`. Reuse lower_func
        // (the same path used by extend-block methods) so move-analysis,
        // signature rewriting, and async handling all apply uniformly.
        for method in &i.methods {
            let item_fn = self.lower_func(method)?;
            let sig = rewrite_self_receiver(item_fn.sig);
            impl_items.push(syn::ImplItem::Fn(syn::ImplItemFn {
                attrs: item_fn.attrs,
                vis: Visibility::Inherited,
                defaultness: None,
                sig,
                block: *item_fn.block,
            }));
        }

        // Assemble the trait-impl. `trait_` set to
        // `Some((None, path, For))` makes this a trait-impl (vs an
        // inherent impl when `trait_` is `None`). The `None` for the bang
        // means "implementing" (vs `!` for negative impls).
        Ok(syn::ItemImpl {
            attrs: Vec::new(),
            defaultness: None,
            unsafety: None,
            generics: Default::default(),
            impl_token: Default::default(),
            trait_: Some((None, trait_path, Default::default())),
            self_ty: Box::new(target_type),
            brace_token: Default::default(),
            items: impl_items,
        })
    }

    /// T93: build a [`syn::Signature`] from a method's name, params, and
    /// optional return type. Shared by required-method lowering (no body)
    /// and could be reused for default-method signatures (though those go
    /// through [`Self::lower_func`] for move-analysis). The signature is
    /// NOT async/unsafe/extern — trait methods in v0.5 are plain `fn`.
    pub(super) fn build_method_signature(
        &mut self,
        name: buff_lang_ast::Ident,
        params: &[buff_lang_ast::Param],
        return_type: &Option<TypeRef>,
    ) -> Result<Signature, CodegenError> {
        let mut inputs: Punctuated<syn::FnArg, syn::Token![,]> = Punctuated::new();
        for p in params {
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
        let output = match return_type {
            Some(ty) => {
                ReturnType::Type(Default::default(), Box::new(self.ast_typeref_to_syn(ty)?))
            }
            None => ReturnType::Default,
        };
        Ok(Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: Default::default(),
            ident: ast_ident_to_syn(&name),
            generics: Default::default(),
            paren_token: Default::default(),
            inputs,
            variadic: None,
            output,
        })
    }

}

// ===========================================================================
// T69: @no-alloc lint — allocation-free function verification
// ===========================================================================

/// T69: scan a lowered Rust [`syn::Block`] for heap-allocating constructs and
/// return one human-readable description per violation.
///
/// Used by [`RustCodegen`](super::RustCodegen) when a function carries the
/// `@no-alloc` attribute. Each returned string names the offending construct
/// (e.g. `` `vec!` macro allocates on the heap ``); the caller wraps it in a
/// [`Diagnostic::warning`] tagged with the function's span.
///
/// # What counts as an allocation
///
/// The lint flags the unambiguous heap-producers in generated Rust:
///
/// - **Macros**: `vec!`, `format!` (and the printing macros `println!` /
///   `print!` / `eprintln!` / `eprint!` / `write!` / `writeln!`, which format
///   a `String` internally).
/// - **Path calls**: `String::from`, `String::new`, `Vec::new`,
///   `Vec::with_capacity`, `Box::new`.
/// - **Method calls**: `.to_string()`, `.to_owned()` (each clones into a fresh
///   heap `String`).
///
/// Iterator `.collect()` is intentionally NOT flagged: it MAY allocate but the
/// target collection is context-dependent and a sized/borrowing collect is
/// possible, so flagging it would produce false positives.
///
/// # How
///
/// A [`syn::visit::Visit`] walker recurses into every expression in the block,
/// so allocations nested inside `if`/`match`/closure/loop bodies are caught.
/// The walker is allocation-free itself (it only pushes to a `Vec` on a hit).
fn check_no_alloc_violations(block: &syn::Block) -> Vec<String> {
    let mut visitor = NoAllocVisitor { violations: Vec::new() };
    syn::visit::visit_block(&mut visitor, block);
    visitor.violations
}

/// Render a [`syn::Path`] as a `::`-joined identifier string for comparison
/// (e.g. `String::from`, `Vec::new`). Used by the macro- and call-detection
/// arms of [`NoAllocVisitor`].
fn syn_path_to_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// T69: `syn::visit::Visit` impl that collects heap-allocation violations.
struct NoAllocVisitor {
    violations: Vec<String>,
}

impl<'ast> syn::visit::Visit<'ast> for NoAllocVisitor {
    fn visit_expr_macro(&mut self, i: &'ast syn::ExprMacro) {
        let name = syn_path_to_string(&i.mac.path);
        if matches!(
            name.as_str(),
            "vec"
                | "format"
                | "println"
                | "print"
                | "eprintln"
                | "eprint"
                | "write"
                | "writeln"
        ) {
            self.violations
                .push(format!("`{name}!` macro allocates on the heap"));
        }
        syn::visit::visit_expr_macro(self, i);
    }

    fn visit_expr_call(&mut self, i: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path_expr) = i.func.as_ref() {
            let name = syn_path_to_string(&path_expr.path);
            if matches!(
                name.as_str(),
                "String::from"
                    | "String::new"
                    | "String::from_utf8"
                    | "Vec::new"
                    | "Vec::with_capacity"
                    | "Box::new"
            ) {
                self.violations
                    .push(format!("`{name}(...)` allocates on the heap"));
            }
        }
        syn::visit::visit_expr_call(self, i);
    }

    fn visit_expr_method_call(&mut self, i: &'ast syn::ExprMethodCall) {
        let method = i.method.to_string();
        if matches!(method.as_str(), "to_string" | "to_owned") {
            self.violations
                .push(format!("`.{method}()` allocates a new String on the heap"));
        }
        syn::visit::visit_expr_method_call(self, i);
    }
}

#[cfg(test)]
mod no_alloc_tests {
    //! T69 inline unit tests for the @no-alloc allocation scanner.
    use super::*;

    fn block_from(src: &str) -> syn::Block {
        let item: syn::ItemFn = syn::parse_quote!(
            fn __no_alloc_probe() { #src }
        );
        *item.block
    }

    #[test]
    fn detects_vec_macro() {
        let b = block_from("let v = vec![1, 2, 3];");
        let v = check_no_alloc_violations(&b);
        assert!(v.iter().any(|s| s.contains("vec!")), "got {v:?}");
    }

    #[test]
    fn detects_box_new() {
        let b = block_from("let b = Box::new(42);");
        let v = check_no_alloc_violations(&b);
        assert!(v.iter().any(|s| s.contains("Box::new")), "got {v:?}");
    }

    #[test]
    fn detects_string_from() {
        let b = block_from("let s = String::from(\"hi\");");
        let v = check_no_alloc_violations(&b);
        assert!(v.iter().any(|s| s.contains("String::from")), "got {v:?}");
    }

    #[test]
    fn detects_to_string_method() {
        let b = block_from("let s = 5.to_string();");
        let v = check_no_alloc_violations(&b);
        assert!(v.iter().any(|s| s.contains("to_string")), "got {v:?}");
    }

    #[test]
    fn detects_nested_allocation() {
        // Allocations inside an if-body are caught (deep recursion).
        let b = block_from("if c { let s = format!(\"{}\", x); }");
        let v = check_no_alloc_violations(&b);
        assert!(v.iter().any(|s| s.contains("format!")), "got {v:?}");
    }

    #[test]
    fn clean_block_has_no_violations() {
        let b = block_from("let n = 1 + 2; let m = n * 3;");
        let v = check_no_alloc_violations(&b);
        assert!(v.is_empty(), "pure arithmetic must not flag: {v:?}");
    }

    #[test]
    fn does_not_flag_collect() {
        // .collect() is intentionally not flagged (may or may not allocate).
        let b = block_from("let v: Vec<i64> = (0..3).collect();");
        let v = check_no_alloc_violations(&b);
        assert!(
            !v.iter().any(|s| s.contains("collect")),
            "collect must NOT be flagged: {v:?}"
        );
    }
}

