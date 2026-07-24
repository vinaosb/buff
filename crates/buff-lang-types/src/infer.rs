//! Local type inference for the Buff language.
//!
//! [`TypeInferencer`] walks the AST bottom-up, assigning a [`Type`] to each
//! expression and reporting [`TypeError`]s with source span information when
//! operands are incompatible.
//!
//! v1.0 ships primitives, collections, user-defined types, full inference
//! from literals/identifiers/operators/calls, exhaustiveness checking, and
//! recursion detection.

use buff_lang_ast::{Block, Expr, Ident, InterpPart, Literal, Stmt, TypeRef, UnaryOp};
use buff_lang_error::{Diagnostic, ErrorCode, Span, TypeError};
use std::collections::BTreeMap;

use crate::env::TypeEnv;
use crate::prelude;
use crate::prelude_types;
use crate::promote::{assignable_to, promote_binary};
use crate::ty::Type;

/// A registry of user-defined generic type declarations (T37).
///
/// Maps a user struct/enum name to its declared generic arity (the number of
/// type parameters). Built by walking the top-level `Decl`s once (structs +
/// enums with `type_params`); consulted by
/// [`typeref_to_type_with_user`] so a source annotation like `Pair<Int,
/// String>` can resolve to a [`Type::User`] carrying the bound arguments,
/// instead of falling through to `None` / `Type::Unknown`.
///
/// The arity check defends against arity-mismatched annotations
/// (`Pair<Int>` when `Pair<T, U>` is declared): a mismatch defers to
/// rustc (returns `None`) rather than producing a malformed `User` type.
/// This is the same defer-to-rustc stance the pre-T37 resolver took for ALL
/// user generics, so the only behaviour change is the NEW happy path
/// (matching arity → resolved `Type::User`).
#[derive(Debug, Clone, Default)]
pub struct UserGenericDecls {
    inner: BTreeMap<String, usize>,
}

impl UserGenericDecls {
    /// Build an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a user type's declared generic arity. Idempotent — the last
    /// registration wins (mirrors Rust's "one definition per name" rule).
    pub fn register(&mut self, name: impl Into<String>, arity: usize) {
        self.inner.insert(name.into(), arity);
    }

    /// Look up the declared arity for `name`, if it is a registered
    /// user-defined generic type.
    pub fn arity_of(&self, name: &str) -> Option<usize> {
        self.inner.get(name).copied()
    }

    /// Returns `true` if `name` is a registered user-defined type (generic or
    /// not — a 0-arity user struct is still registered so bare references
    /// resolve to `Type::User { args: [] }`).
    pub fn contains(&self, name: &str) -> bool {
        self.inner.contains_key(name)
    }
}

/// A registry of trait implementations (T75b — associated types in traits).
///
/// Maps `(trait_name, target_type_name)` pairs to their associated-type
/// bindings, so that a reference like `Container::Item` (for some target
/// type known to implement `Container`) can be resolved to the concrete
/// [`TypeRef`] the implementor chose.
///
/// Built by walking the top-level `Decl::ImplBlock`s once; consulted by
/// [`TypeInferencer::resolve_associated_type`] so a trait method return
/// type like `Item` (where `Item` is an associated type of the trait being
/// implemented) can be substituted with the concrete binding. The
/// codegen-rust pass also consults this registry when lowering trait
/// method signatures that reference associated types.
///
/// # Why names, not TypeRefs
///
/// The registry keys on PLAIN STRING names (`"Container"`, `"Box"`) rather
/// than full [`TypeRef`]s. This keeps the lookup O(1) and avoids the
/// pattern-matching overhead of destructuring `TypeRef::Named` at every
/// query site. Generic trait impls (`impl Iterable<Int> for Vec<Int>`)
/// are deferred — when they arrive, the key shape will widen to include
/// the generic arguments.
///
/// # Migration: purely additive
///
/// T75b introduces this registry as a NEW field on [`TypeInferencer`],
/// defaulting to empty. Existing inferencer behaviour is unchanged when
/// no `ImplBlock`s are registered (every lookup returns `None`, same as
/// the pre-T75b "unknown associated type" fallback).
#[derive(Debug, Clone, Default)]
pub struct TraitImplRegistry {
    /// `(trait_name, target_type_name) -> { assoc_type_name -> TypeRef }`.
    /// A `BTreeMap` of `BTreeMap`s for deterministic iteration order
    /// (matches the project-wide BTreeMap preference for codegen
    /// determinism — see the codegen-rust AGENTS.md).
    inner: BTreeMap<(String, String), BTreeMap<String, TypeRef>>,
}

impl TraitImplRegistry {
    /// Build an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an `impl Trait for Target` block's associated-type
    /// bindings. Each `(trait, target)` pair may be registered once;
    /// re-registration with the same pair overwrites (mirrors Rust's
    /// "coherence" rule, though we do not enforce it — rustc will).
    ///
    /// Trait and target names are extracted from the [`ImplBlock`]'s
    /// [`TypeRef::Named`] fields; non-named shapes (generic trait impls)
    /// are silently skipped (deferred feature).
    pub fn register_impl_block(&mut self, impl_block: &buff_lang_ast::ImplBlock) {
        let trait_name = match &impl_block.trait_name {
            TypeRef::Named { name, .. } => name.name.clone(),
            // Generic trait impls are deferred — skip silently.
            _ => return,
        };
        let target_name = match &impl_block.target {
            TypeRef::Named { name, .. } => name.name.clone(),
            // Generic targets are deferred — skip silently.
            _ => return,
        };
        let bindings = self
            .inner
            .entry((trait_name, target_name))
            .or_default();
        for b in &impl_block.type_bindings {
            bindings.insert(b.name.name.clone(), b.target.clone());
        }
    }

    /// Bulk-register every `Decl::ImplBlock` in `decls`. Convenience for
    /// drivers that walk the top-level decl list once.
    pub fn register_from_decls<'a>(&mut self, decls: impl IntoIterator<Item = &'a buff_lang_ast::Decl>) {
        for decl in decls {
            if let buff_lang_ast::Decl::ImplBlock(impl_block) = decl {
                self.register_impl_block(impl_block);
            }
        }
    }

    /// Resolve an associated-type reference. Given a trait name
    /// (`"Container"`), a target type that is known to implement that
    /// trait (`"Box"`), and an associated-type name (`"Item"`), returns
    /// the concrete [`TypeRef`] the implementor chose, or `None` if no
    /// matching impl is registered.
    pub fn resolve_associated_type(
        &self,
        trait_name: &str,
        target_name: &str,
        assoc_name: &str,
    ) -> Option<&TypeRef> {
        self.inner
            .get(&(trait_name.to_string(), target_name.to_string()))
            .and_then(|bindings| bindings.get(assoc_name))
    }

    /// Returns `true` if any impl is registered for `(trait_name, target_name)`.
    pub fn has_impl(&self, trait_name: &str, target_name: &str) -> bool {
        self.inner
            .contains_key(&(trait_name.to_string(), target_name.to_string()))
    }
}

/// A local type inferencer. Owns a [`TypeEnv`] that accumulates bindings as
/// `let` declarations are processed.
pub struct TypeInferencer {
    env: TypeEnv,
    /// T37: registry of user-defined generic type declarations. Consulted by
    /// the let-annotation resolution path so a `let p: Pair<Int, String> =`
    /// annotation resolves to a `Type::User` (instead of `Unknown`). Seeded
    /// by the codegen driver (which walks the top-level `Decl`s once and
    /// calls [`Self::register_user_generics`]). Empty by default — keeps
    /// standalone/test inferencers behaving exactly as before T37.
    user_generic_decls: UserGenericDecls,
    /// T75b: registry of trait implementations. Consulted by
    /// [`Self::resolve_associated_type`] so that a trait method's return
    /// type that names an associated type (`fn get(...) -> Item`) can be
    /// resolved to the concrete [`TypeRef`] chosen by the implementor of
    /// that trait for the inferred receiver type. Seeded by the codegen
    /// driver (which walks the top-level `Decl::ImplBlock`s once and
    /// calls [`Self::register_trait_impls`]). Empty by default — keeps
    /// standalone/test inferencers behaving exactly as before T75b.
    trait_impls: TraitImplRegistry,
}

impl TypeInferencer {
    /// Creates a fresh inferencer with an empty environment.
    pub fn new() -> Self {
        Self {
            env: TypeEnv::new(),
            user_generic_decls: UserGenericDecls::new(),
            trait_impls: TraitImplRegistry::new(),
        }
    }

    /// Register a single user-defined generic type declaration (T37). `name`
    /// is the user struct/enum identifier; `arity` is the number of declared
    /// type parameters (0 for a non-generic user type). Idempotent.
    pub fn register_user_generic(&mut self, name: impl Into<String>, arity: usize) {
        self.user_generic_decls.register(name, arity);
    }

    /// Replace the user-generic registry wholesale (T37). Convenience for
    /// drivers that build a [`UserGenericDecls`] from the full `Decl` list
    /// once and install it in one call.
    pub fn set_user_generic_decls(&mut self, decls: UserGenericDecls) {
        self.user_generic_decls = decls;
    }

    /// T75b: register a single trait implementation block. Records its
    /// associated-type bindings in the [`TraitImplRegistry`] so subsequent
    /// calls to [`Self::resolve_associated_type`] can find them.
    pub fn register_trait_impl(&mut self, impl_block: &buff_lang_ast::ImplBlock) {
        self.trait_impls.register_impl_block(impl_block);
    }

    /// T75b: bulk-register every `Decl::ImplBlock` in `decls`. Convenience
    /// for codegen drivers that walk the top-level decl list once. Mirrors
    /// the T37 [`Self::set_user_generic_decls`] pattern.
    pub fn register_trait_impls<'a>(
        &mut self,
        decls: impl IntoIterator<Item = &'a buff_lang_ast::Decl>,
    ) {
        self.trait_impls.register_from_decls(decls);
    }

    /// T75b: replace the trait-impl registry wholesale (mirrors
    /// [`Self::set_user_generic_decls`]).
    pub fn set_trait_impls(&mut self, registry: TraitImplRegistry) {
        self.trait_impls = registry;
    }

    /// T75b: resolve an associated-type reference. Given a trait name, a
    /// target type name (the type known to implement that trait), and an
    /// associated-type name, returns the concrete [`TypeRef`] chosen by
    /// the implementor. Returns `None` when no matching impl is
    /// registered (the pre-T75b behaviour — the caller falls through to
    /// `Type::Unknown` and lets rustc catch downstream issues).
    pub fn resolve_associated_type(
        &self,
        trait_name: &str,
        target_name: &str,
        assoc_name: &str,
    ) -> Option<&TypeRef> {
        self.trait_impls
            .resolve_associated_type(trait_name, target_name, assoc_name)
    }

    /// T75b: borrow the trait-impl registry (read-only). Lets codegen-rust
    /// consult the same registry the inferencer uses, without re-walking
    /// the decl list.
    pub fn trait_impls(&self) -> &TraitImplRegistry {
        &self.trait_impls
    }

    /// Pre-binds `name` to `ty` in the environment. Useful for seeding the
    /// inferencer with known bindings (e.g. function parameters) before
    /// inference, or for testing.
    pub fn bind(&mut self, name: &str, ty: Type) {
        self.env.insert(name, ty);
    }

    /// Returns the inferred type of `name`, if it is bound in the environment.
    pub fn lookup(&self, name: &str) -> Option<&Type> {
        self.env.lookup(name)
    }

    /// Returns a reference to the underlying environment.
    pub fn env(&self) -> &TypeEnv {
        &self.env
    }

    /// Infers the [`Type`] of an expression.
    ///
    /// Returns an `Err(TypeError)` for operands that cannot be typed together.
    pub fn infer_expr(&mut self, expr: &Expr) -> Result<Type, TypeError> {
        match expr {
            Expr::Literal(lit, span) => self.infer_literal(lit, *span),
            Expr::Ident(name, span) => {
                // T28: `None` is a prelude Option variant, NOT a keyword. It
                // resolves to `Option<T>` with a fresh (Unknown) inner type —
                // the inner is pinned by context (e.g. a `let x: Option<Int>
                // = None` annotation) or stays Unknown until a later use.
                if name.name == "None" {
                    return Ok(Type::option(Type::Unknown));
                }
                self.lookup_ident(name, *span)
            }
            Expr::BinaryOp { op, lhs, rhs, span } => self.infer_binary(op, lhs, rhs, *span),
            Expr::UnaryOp { op, operand, span } => self.infer_unary(op, operand, *span),
            Expr::IfExpr {
                cond,
                then_block,
                else_block,
                span,
            } => self.infer_if(cond, then_block, else_block, *span),
            // T96: standard-library prelude. A bare-ident callee whose name
            // is a recognised prelude function is resolved WITHOUT an import
            // — its return type is computed from the inferred arg types via
            // `prelude::return_type`. Non-prelude free-function calls stay
            // `Unknown` (full user-call resolution arrives later).
            Expr::FuncCall { callee, args, .. } => {
                // T28: `Some(x)` is a prelude Option constructor, NOT a
                // keyword and NOT a user function. It wraps its single
                // argument's type in `Option<T>`. `None` (no args) is handled
                // in the `Expr::Ident` arm above, but a defensive `None()`
                // call shape also yields `Option<Unknown>` for robustness.
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if name.name == "Some" && args.len() == 1 {
                        let inner = self.infer_expr(&args[0])?;
                        return Ok(Type::option(inner));
                    }
                    if name.name == "None" && args.is_empty() {
                        return Ok(Type::option(Type::Unknown));
                    }
                    // T30: `Ok(x)` and `Err(e)` are prelude Result constructors,
                    // NOT keywords and NOT user functions. `Ok(x)` wraps its
                    // argument's type in `Result<T, Unknown>` (the Err type is
                    // pinned by context or stays Unknown). `Err(e)` wraps its
                    // argument's type in `Result<Unknown, E>` symmetrically.
                    // Neither is a reserved keyword.
                    if name.name == "Ok" && args.len() == 1 {
                        let ok_ty = self.infer_expr(&args[0])?;
                        return Ok(Type::result(ok_ty, Type::Unknown));
                    }
                    if name.name == "Err" && args.len() == 1 {
                        let err_ty = self.infer_expr(&args[0])?;
                        return Ok(Type::result(Type::Unknown, err_ty));
                    }
                }
                // T96: standard-library prelude. A bare-ident callee whose name
                // is a recognised prelude function is resolved WITHOUT an import
                // — its return type is computed from the inferred arg types via
                // `prelude::return_type`. Non-prelude free-function calls stay
                // `Unknown` (full user-call resolution arrives later).
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if let Some(fn_) = prelude::lookup(&name.name) {
                        let mut arg_tys = Vec::with_capacity(args.len());
                        for a in args {
                            arg_tys.push(self.infer_expr(a)?);
                        }
                        return Ok(prelude::return_type(fn_, &arg_tys));
                    }
                }
                // v0.5: real call resolution.
                Ok(Type::Unknown)
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                // T124b: prelude-types registry. A `Type.method(args)` call
                // whose receiver is a bare `Expr::Ident` naming a prelude
                // type (DateTime, Date, Time, Duration, Instant) is resolved
                // via the prelude-types table — NO `import` required. This
                // is the generalisation of the existing `Matrix.new(...)`
                // special case below, established so future v1.4 stdlib
                // tasks (Regex, Math, URL, Hash, ...) can extend the
                // registry without rewriting the inferencer.
                if let Expr::Ident(id, _) = receiver.as_ref() {
                    if let Some((ptype, pmethod)) =
                        prelude_types::assoc_fn_lookup(&id.name, &method.name)
                    {
                        let mut arg_tys = Vec::with_capacity(args.len());
                        for a in args {
                            arg_tys.push(self.infer_expr(a)?);
                        }
                        if let Some(ret) =
                            prelude_types::assoc_fn_return_type(ptype, pmethod, &arg_tys)
                        {
                            return Ok(ret);
                        }
                        // Unrecognised (type, method) pair: fall through to
                        // the default Unknown path so the codegen emits the
                        // call as a plain method call (and Rust will
                        // diagnose the typo).
                    }
                }
                // T24: `Matrix.new(rows, cols)` infers as `Matrix<T>` where
                // the element type is unknown without further evidence (a
                // type annotation `let m: Matrix<Int> = ...` or a subsequent
                // 2-D index). We return `Matrix<Unknown>` so the variant
                // flows through to codegen, which emits the flat-storage
                // struct regardless.
                if method.name == "new" {
                    if let Expr::Ident(id, _) = receiver.as_ref() {
                        if id.name == "Matrix" {
                            return Ok(Type::matrix(Type::Unknown));
                        }
                    }
                }
                // T77: Expected-type driven inference for the `.map()` /
                // `.filter()` collection combinators. When the receiver
                // infers to `Vector<T>` and the single argument is a lambda,
                // the element type `T` is propagated as the EXPECTED type of
                // the lambda's single parameter (see
                // [`infer_expr_expected`]). This lets `{ x => x * 2 }` infer
                // `x` from the receiver without an explicit annotation.
                //
                // - `.map(lambda)`  -> `Vector<body_result_type>`
                // - `.filter(lambda)` -> `Vector<T>` (element type preserved;
                //   the body's Bool-ness is not enforced here — v0.5 treats
                //   type mismatches as warnings).
                //
                // Non-Vector receivers, multi-arg calls, and non-lambda
                // args fall through to the `Unknown` default (no regression
                // of the pre-T77 path).
                if matches!(method.name.as_str(), "map" | "filter") && args.len() == 1 {
                    if let Expr::Lambda { .. } = &args[0] {
                        let recv_ty = self.infer_expr(receiver)?;
                        if let Type::Vector(elem_ty) = &recv_ty {
                            let body_ty = self.infer_expr_expected(&args[0], Some(elem_ty))?;
                            let result_elem = if method.name == "map" {
                                body_ty
                            } else {
                                // filter preserves the element type.
                                (**elem_ty).clone()
                            };
                            return Ok(Type::vector(result_elem));
                        }
                    }
                }
                Ok(Type::Unknown)
            }
            Expr::SuspendExpr { inner, .. } => self.infer_expr(inner),
            // T23: A collection literal infers `Vector<T>`. For all-integer
            // literals the element width is auto-detected via T22 range
            // analysis (`[1, 2, 3]` -> `Vector<Int<8>>`). For a single
            // non-integer element kind the element type is that kind. Empty
            // or mixed literals fall back to `Vector<Int<64>>` (Buff's
            // default Int width) so a bare `let v = []` still type-checks
            // against a plain Int element.
            Expr::ArrayLit { elements, .. } => {
                Ok(Type::vector(self.infer_collection_element(elements)?))
            }
            // T23/T24/T82: Indexing `base[i]` (1 index) yields the element
            // type when `base` is a `Vector<T>` OR the value type when
            // `base` is a `Map<K, V>` (T82); `base[row, col]` (2 indices)
            // yields the element type when `base` is a `Matrix<T>`. Any
            // other shape yields `Unknown` (a later type check can reject
            // e.g. string indexing). The `indices` vec arity drives the
            // dispatch — single-index stays the T23 Vector / T82 Map path,
            // two-index takes the T24 Matrix path.
            //
            // T82 map-index semantics: `m[key]` ALWAYS yields a value of
            // the map's value type `V` (never panics, never `Option<V>`);
            // the codegen lowers the read to
            // `m.get(&key).cloned().unwrap_or_default()` so a missing key
            // returns the default for `V`. This type rule keeps the
            // surface type simple (the value type, not Option<V>) which
            // matches Buff's "no panic on missing keys" convention.
            Expr::Index { base, indices, .. } => {
                let base_ty = self.infer_expr(base)?;
                if indices.len() == 2 {
                    match base_ty {
                        Type::Matrix(elem) => Ok((*elem).clone()),
                        _ => Ok(Type::Unknown),
                    }
                } else if indices.len() == 1 {
                    match base_ty {
                        Type::Vector(elem) => Ok((*elem).clone()),
                        // T82: `m[key]` yields the map's value type V.
                        Type::Map(_, val) => Ok((*val).clone()),
                        _ => Ok(Type::Unknown),
                    }
                } else {
                    Ok(Type::Unknown)
                }
            }
            // T21: A string interpolation always evaluates to String.
            // Each embedded expression is visited (so its sub-types are
            // checked) but the parts themselves don't affect the result.
            Expr::StringInterp { parts, .. } => {
                for part in parts {
                    if let InterpPart::Expr(e, _) = part {
                        self.infer_expr(e)?;
                    }
                }
                Ok(Type::string())
            }
            // T25: A map literal infers `Map<K, V>`. Both key and value
            // types come from the first entry (uniformity is enforced by a
            // future task — for v0.5 we accept heterogeneous entries and
            // pick the first's kind as the canonical type). An empty map
            // (`{:}`) falls back to `Map<Int<64>, Int<64>>` so a bare
            // `let m = {:}` still type-checks.
            Expr::MapLit { entries, .. } => {
                let (key_ty, val_ty) = if let Some((k, v)) = entries.first() {
                    // `infer_collection_element` takes a `&[Expr]` slice;
                    // use `std::slice::from_ref` to avoid an unnecessary
                    // clone of the entry (clippy: cloned_ref_to_slice_refs).
                    let kt = self.infer_collection_element(std::slice::from_ref(k))?;
                    let vt = self.infer_collection_element(std::slice::from_ref(v))?;
                    (kt, vt)
                } else {
                    (Type::int_default(), Type::int_default())
                };
                // Visit the remaining entries so their sub-types are checked
                // (and so dependent inference side-effects run).
                for (k, v) in entries.iter().skip(1) {
                    let _ = self.infer_expr(k);
                    let _ = self.infer_expr(v);
                }
                Ok(Type::map(key_ty, val_ty))
            }
            // v0.5: lambda/struct/match inference.
            Expr::Lambda { .. } | Expr::StructInit { .. } | Expr::MatchExpr { .. } => {
                Ok(Type::Unknown)
            }
            // T30: `expr?` yields the Ok type `T` of a `Result<T, E>`. When
            // the operand infers to a known `Result(T, E)`, return `T`;
            // otherwise (Unknown, Option, etc.) fall back to `Unknown` so the
            // value flows through to codegen without a hard type error
            // (matches v0.5's type-errors-as-warnings policy).
            Expr::Try { expr, .. } => {
                let inner_ty = self.infer_expr(expr)?;
                match inner_ty {
                    Type::Result(ok, _) => Ok((*ok).clone()),
                    _ => Ok(Type::Unknown),
                }
            }
            // T31: `spawn expr` yields a `Task<T>` (Buff's alias for
            // Rust's `tokio::task::JoinHandle<T>`). The inner `T` is the
            // task body's return type. For v0.5 we leave it `Unknown`
            // because the type-inferencer doesn't yet track `Task<T>` as
            // a first-class `Type` variant — codegen handles it via the
            // `t.result()` → `.await` rewrite, which yields the inner `T`
            // at the await site.
            Expr::Spawn { task, .. } => {
                // Visit the task body so sub-inference runs, but the spawn
                // expression itself returns Unknown (the Task<T> wrapper
                // is opaque at the type level for v0.5).
                let _ = self.infer_expr(task)?;
                Ok(Type::Unknown)
            }
            // T84: `start..end` — infer both bounds, return `Range<T>`
            // where `T` is the element type. The element type is taken
            // from the first non-`Unknown` bound (start preferred, end
            // as fallback); when both are `Unknown` we fall back to
            // `Int<64>` (Buff's default Int) so a `Range<Int>` always
            // has a concrete element type. The `inclusive` flag is
            // preserved in the AST (`Expr::Range { inclusive }`) so
            // codegen can pick `..` vs `..=`; the TYPE layer treats
            // both shapes uniformly as `Range<T>` (Rust's
            // `Range<T>` / `RangeInclusive<T>` are distinct types but
            // Buff surfaces a single `Range<T>` abstraction — the
            // user-facing difference is purely syntactic).
            //
            // This replaces the T68 stub (`Ok(Type::Unknown)`). The
            // earlier stub was correct for codegen (which lowers the
            // AST directly via `lower_range`, ignoring the inferred
            // type) but lost the range-ness at the type layer, so
            // `:type 0..10` in the REPL printed `Unknown`. With a real
            // `Range<T>` variant, the type system is now range-aware.
            Expr::Range { start, end, .. } => {
                let start_ty = self.infer_expr(start)?;
                let end_ty = self.infer_expr(end)?;
                let elem = if !matches!(start_ty, Type::Unknown) {
                    start_ty
                } else if !matches!(end_ty, Type::Unknown) {
                    end_ty
                } else {
                    // Both bounds indeterminate — default to Int<64>
                    // (Buff's default integer width). This keeps the
                    // range element type concrete even when the bounds
                    // are themselves unannotated identifiers.
                    Type::int_default()
                };
                Ok(Type::range(elem))
            }
            // T72: `if let PAT = EXPR { then } else { else }` — infer the
            // value for side effects (binding the pattern's names to Unknown
            // since v0.5 doesn't track per-binding types through patterns),
            // then walk both blocks. The whole expression is `()` (unit)
            // when used as a statement, which is the common case. Mirrors
            // the IfExpr treatment: we don't unify the branch types.
            Expr::IfLet {
                pattern,
                value,
                then_block,
                else_block,
                ..
            } => {
                let _ = self.infer_expr(value)?;
                // Bind each pattern name to Unknown (v0.5 deferral — Rust
                // does the real per-binding inference at codegen time).
                for b in pattern.bindings() {
                    self.env.insert(&b.name, Type::Unknown);
                }
                for s in &then_block.stmts {
                    let _ = self.infer_stmt(s)?;
                }
                if let Some(eb) = else_block {
                    for s in &eb.stmts {
                        let _ = self.infer_stmt(s)?;
                    }
                }
                Ok(Type::Unknown)
            }
            // T103: `(e1, e2, ...)` — a tuple literal infers
            // `Type::Tuple([T1, T2, ...])` where each `Ti` is the inferred
            // type of the corresponding element. The 2+-element rule lives
            // at parse time, so this variant always carries 2+ members.
            // Each element is independently inferred (no unification — a
            // tuple `(Int, String)` keeps heterogeneous element types).
            Expr::TupleLit(members, _) => {
                let mut member_tys = Vec::with_capacity(members.len());
                for m in members {
                    member_tys.push(self.infer_expr(m)?);
                }
                Ok(Type::tuple(member_tys))
            }
            // T105: a named arg `name: value` infers the value's type. The
            // name is metadata for codegen reorder; it carries no type of
            // its own. The enclosing FuncCall/MethodCall inference decides
            // the call's overall type (typically Unknown in v0.5).
            Expr::NamedArg { value, .. } => self.infer_expr(value),
        }
    }

    /// Infers the type of `expr` with an optional EXPECTED-type hint (T77).
    ///
    /// `expected` is currently consumed only by [`Expr::Lambda`], where it is
    /// interpreted as the expected type of the lambda's SINGLE parameter —
    /// i.e. the element type propagated down from a `.map()` / `.filter()`
    /// receiver (`Vector<T>`). All other expressions ignore `expected` and
    /// behave identically to [`infer_expr`].
    ///
    /// This is an **additive** helper: existing `infer_expr(&expr)` callers
    /// are unchanged (they effectively pass `expected = None`). The
    /// [`Expr::MethodCall`] inference arm uses this to propagate the
    /// receiver's element type into a lambda argument.
    ///
    /// # Lambda semantics
    ///
    /// - With `expected = Some(T)` and a single-param lambda: the param name
    ///   is bound to `T` in the type environment, the body is inferred, and
    ///   the body's tail type is returned as the lambda's result type.
    ///   (Buff's `Type` enum has no function variant in v0.5, so the lambda
    ///   "type" itself is its body's type; callers like `.map()` compose the
    ///   final `Vector<R>` themselves.)
    /// - With `expected = None`, a multi-param lambda, or any other shape:
    ///   falls back to the v0.5 default (`Type::Unknown`) so the existing
    ///   closures/codegen path is unaffected.
    pub fn infer_expr_expected(
        &mut self,
        expr: &Expr,
        expected: Option<&Type>,
    ) -> Result<Type, TypeError> {
        match expr {
            Expr::Lambda { params, body, .. } => {
                // Without an expected param type, keep the v0.5 fallback so
                // we don't regress the existing closure/codegen path.
                let elem_ty = match expected {
                    Some(t) => t.clone(),
                    None => return Ok(Type::Unknown),
                };
                // Only single-param lambdas are supported by the
                // map/filter combinators. Multi-param lambdas fall back to
                // Unknown (a v0.5 deferral — Rust does the real inference at
                // codegen time).
                if params.len() != 1 {
                    return Ok(Type::Unknown);
                }
                // Bind the param name to the expected element type, then
                // infer the body's tail type. The lambda's RESULT type IS
                // the body's tail type for the purpose of `.map()` result
                // composition.
                self.env.insert(&params[0].name.name, elem_ty);
                self.infer_block_tail(body)
            }
            // All other expressions ignore `expected` and delegate to the
            // plain inference path.
            _ => self.infer_expr(expr),
        }
    }

    fn infer_literal(&self, lit: &Literal, _span: Span) -> Result<Type, TypeError> {
        Ok(match lit {
            Literal::Int(_) => Type::int_default(),
            Literal::Float(_) => Type::float_default(),
            Literal::Double(_) => Type::double(),
            Literal::Bool(_) => Type::bool(),
            Literal::String(_) => Type::string(),
            Literal::Byte(_) => Type::byte(),
            // T21: `'A'`, `'é'`, `'🚀'` infer as the Char type (one scalar).
            Literal::Char(_) => Type::char(),
            // T20: `99.90m` infers as the 128-bit fixed-point Decimal type
            // (NOT Double/Float), so it stays exact and runs on CPU only.
            Literal::Decimal(_) => Type::Decimal,
            // T79: Regex literal infers as `String` to match the v0.5 codegen
            // stub (which emits the pattern as a plain String literal). When
            // real `Regex::new` codegen lands in v1.0, this should become a
            // dedicated `Type::Regex` (or a structured type wrapping the
            // pattern + compile-time-validated flag).
            Literal::Regex(_) => Type::string(),
        })
    }

    /// Infer the element type of a collection literal (T23).
    ///
    /// - All-integer literals: auto-width via T22 `collection_int_width`
    ///   (`[1, 2, 3]` -> `Int<8>`; `[300]` -> `Int<16>`).
    /// - All-same primitive literal kind (Bool/Char/Byte/Float/Double/String):
    ///   that kind (the first element's).
    /// - Empty or mixed: `Int<64>` (Buff's default Int width) so a bare
    ///   `let v = []` type-checks against a plain Int element.
    fn infer_collection_element(&mut self, elements: &[Expr]) -> Result<Type, TypeError> {
        // T83: NESTED collection literal preservation. If the first
        // element is itself an `ArrayLit` or `MapLit`, recurse via
        // [`Self::infer_expr`] so the FULLY-NESTED type is preserved
        // (inner-first). Without this short-circuit, `[[1, 2], [3, 4]]`
        // fell through to the default-Int fallback and inferred as
        // `Vector<Int<64>>` (flattened) instead of the correct
        // `Vector<Vector<Int>>`. Similarly `{"a": {"b": 1}}` now
        // correctly infers as `Map<String, Map<String, Int>>`.
        //
        // We check ONLY the first element (consistent with the existing
        // first-element-wins policy for non-nested literals below).
        // Heterogeneous mixes like `[1, [2, 3]]` continue to fall
        // through to the default-Int fallback — surfacing a type
        // mismatch downstream rather than guessing.
        //
        // The signature is `&mut self` (changed from `&self` in T83)
        // specifically to allow this recursive `infer_expr` call.
        // Both existing callers (`ArrayLit` and `MapLit` arms of
        // [`Self::infer_expr`]) already have `&mut self`, so the
        // signature change is backward-compatible at the call sites.
        if let Some(first) = elements.first() {
            if matches!(first, Expr::ArrayLit { .. } | Expr::MapLit { .. }) {
                return self.infer_expr(first);
            }
        }
        // Collect integer literal values for auto-width detection. We
        // recognise both `Literal::Int(v)` and `UnaryOp(Neg, Literal::Int(v))`
        // (the parser-realistic form for negative numbers, since `-200` lexes
        // as a unary minus on `200`).
        let mut int_values: Vec<i128> = Vec::new();
        let mut all_int = !elements.is_empty();
        for e in elements {
            if let Some(v) = const_int_value(e) {
                int_values.push(v);
            } else {
                all_int = false;
                break;
            }
        }
        if all_int {
            let width = crate::range_analysis::collection_int_width(&int_values);
            return Ok(Type::Int { width });
        }
        // Non-empty, non-all-int: try the first element's literal kind.
        // Single collapsed pattern (avoids clippy's collapsible-nested-if-let).
        if let Some(Expr::Literal(lit, _)) = elements.first() {
            return self.infer_literal(lit, Span::dummy());
        }
        // Empty or mixed/non-literal: default Int<64>.
        Ok(Type::int_default())
    }

    fn lookup_ident(&self, name: &Ident, span: Span) -> Result<Type, TypeError> {
        self.env.lookup(&name.name).cloned().ok_or_else(|| {
            let mut diag = Diagnostic::error(format!("undefined variable: {}", name.name), span)
                .with_code(ErrorCode::UndefinedVariable);
            // T63: attach a "did you mean `X`?" help note when a prelude
            // fn/type name is close to the unknown identifier. The candidate
            // list is the implicit prelude (free fns + types) — the most
            // common cause of an undefined-variable error is a typo of a
            // builtin the user expected to be in scope.
            if let Some(msg) =
                buff_lang_error::suggest_with_message(&name.name, &prelude_suggestion_candidates())
            {
                diag = diag.with_note(format!("help: {msg}"));
            }
            TypeError::new(diag)
        })
    }

    fn infer_binary(
        &mut self,
        op: &buff_lang_ast::BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
    ) -> Result<Type, TypeError> {
        let lhs_ty = self.infer_expr(lhs)?;
        let rhs_ty = self.infer_expr(rhs)?;

        use buff_lang_ast::BinaryOp;
        match op {
            // Comparison operators always yield Bool, provided the operands
            // are comparable (either equal or numerically promotable).
            BinaryOp::Eq
            | BinaryOp::Neq
            | BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::Lte
            | BinaryOp::Gte => {
                if lhs_ty == rhs_ty || promote_binary(&lhs_ty, &rhs_ty).is_some() {
                    Ok(Type::Bool)
                } else {
                    Err(TypeError::new(
                        Diagnostic::error(format!("cannot compare {lhs_ty} with {rhs_ty}"), span)
                            .with_code(ErrorCode::BinaryOpTypeMismatch),
                    ))
                }
            }
            // Logical operators require Bool on both sides.
            BinaryOp::And | BinaryOp::Or => {
                if lhs_ty != Type::Bool || rhs_ty != Type::Bool {
                    return Err(TypeError::new(
                        Diagnostic::error(
                            format!("logical operators require Bool, found {lhs_ty} and {rhs_ty}"),
                            span,
                        )
                        .with_code(ErrorCode::BinaryOpTypeMismatch),
                    ));
                }
                Ok(Type::Bool)
            }
            // Arithmetic operators — promote operands.
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                // T124b: prelude-datetime arithmetic. chrono (and std::time)
                // impl `Add<Duration>` / `Sub<Duration>` on the datetime
                // family, so `dt + Duration.days(7)` and `dt - dt2` are
                // legal at the Rust level. The type inferencer must accept
                // these combinations and produce the right result type
                // (DateTime + Duration -> DateTime, DateTime - DateTime ->
                // Duration, etc.).
                if let Some(dt_result) = datetime_arith_result(op, &lhs_ty, &rhs_ty) {
                    return Ok(dt_result);
                }
                promote_binary(&lhs_ty, &rhs_ty).ok_or_else(|| {
                    TypeError::new(
                        Diagnostic::error(
                            format!("cannot apply operator to {lhs_ty} and {rhs_ty}"),
                            span,
                        )
                        .with_code(ErrorCode::BinaryOpTypeMismatch),
                    )
                })
            }
            // Bitwise / shift operators — integers only.
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                if !lhs_ty.is_integer_like() || !rhs_ty.is_integer_like() {
                    return Err(TypeError::new(
                        Diagnostic::error(
                            format!(
                                "bitwise operators require integers, found {lhs_ty} and {rhs_ty}"
                            ),
                            span,
                        )
                        .with_code(ErrorCode::BinaryOpTypeMismatch),
                    ));
                }
                promote_binary(&lhs_ty, &rhs_ty).ok_or_else(|| {
                    TypeError::new(
                        Diagnostic::error(
                            format!("cannot apply bitwise operator to {lhs_ty} and {rhs_ty}"),
                            span,
                        )
                        .with_code(ErrorCode::BinaryOpTypeMismatch),
                    )
                })
            }
            // Plain assignment — result type is the lhs.
            BinaryOp::Assign => Ok(lhs_ty),
            // Compound assignment — operands must be promotable.
            BinaryOp::AddAssign
            | BinaryOp::SubAssign
            | BinaryOp::MulAssign
            | BinaryOp::DivAssign
            | BinaryOp::ModAssign => {
                if promote_binary(&lhs_ty, &rhs_ty).is_some() {
                    Ok(lhs_ty)
                } else {
                    Err(TypeError::new(
                        Diagnostic::error(format!("cannot assign {rhs_ty} to {lhs_ty}"), span)
                            .with_code(ErrorCode::AssignTypeMismatch),
                    ))
                }
            }
            // T101: null coalescing `??` — result type is the RHS type
            // (the unwrap_or default). The LHS must be an Option<T> or
            // Result<T,E>; the RHS must be assignable to T. For v0.5 we
            // return the RHS type (the default value's type).
            BinaryOp::NullCoalesce => Ok(rhs_ty),
        }
    }

    fn infer_unary(&mut self, op: &UnaryOp, operand: &Expr, span: Span) -> Result<Type, TypeError> {
        let operand_ty = self.infer_expr(operand)?;
        match op {
            UnaryOp::Neg => {
                if operand_ty.is_numeric() {
                    Ok(operand_ty)
                } else {
                    Err(TypeError::new(
                        Diagnostic::error(
                            format!("unary - requires a numeric type, found {operand_ty}"),
                            span,
                        )
                        .with_code(ErrorCode::InvalidUnaryOperand),
                    ))
                }
            }
            UnaryOp::Not => {
                if operand_ty == Type::Bool {
                    Ok(Type::Bool)
                } else {
                    Err(TypeError::new(
                        Diagnostic::error(
                            format!("unary ! requires Bool, found {operand_ty}"),
                            span,
                        )
                        .with_code(ErrorCode::InvalidUnaryOperand),
                    ))
                }
            }
            UnaryOp::BitNot => {
                if operand_ty.is_integer_like() {
                    Ok(operand_ty)
                } else {
                    Err(TypeError::new(
                        Diagnostic::error(
                            format!("unary ~ requires an integer, found {operand_ty}"),
                            span,
                        )
                        .with_code(ErrorCode::InvalidUnaryOperand),
                    ))
                }
            }
        }
    }

    fn infer_if(
        &mut self,
        cond: &Expr,
        then_block: &Block,
        else_block: &Option<Block>,
        span: Span,
    ) -> Result<Type, TypeError> {
        let cond_ty = self.infer_expr(cond)?;
        if cond_ty != Type::Bool {
            return Err(TypeError::new(
                Diagnostic::error(format!("if condition must be Bool, found {cond_ty}"), span)
                    .with_code(ErrorCode::IfConditionMustBeBool),
            ));
        }
        let then_ty = self.infer_block_tail(then_block)?;
        if let Some(else_b) = else_block {
            let else_ty = self.infer_block_tail(else_b)?;
            if then_ty != else_ty {
                return Err(TypeError::new(
                    Diagnostic::error(
                        format!("if/else branches have different types: {then_ty} vs {else_ty}"),
                        span,
                    )
                    .with_code(ErrorCode::IfBranchTypeMismatch),
                ));
            }
            Ok(then_ty)
        } else {
            Ok(Type::Void)
        }
    }

    /// Infers the "tail" type of a block — the type of its last statement.
    fn infer_block_tail(&mut self, block: &Block) -> Result<Type, TypeError> {
        let mut last_ty = Type::Void;
        for stmt in &block.stmts {
            last_ty = self.infer_stmt(stmt)?;
        }
        Ok(last_ty)
    }

    /// Infers the type produced by a statement.
    ///
    /// `let` declarations update the environment; expression statements yield
    /// their value type; `return` yields its operand's type (or `Void`).
    pub fn infer_stmt(&mut self, stmt: &Stmt) -> Result<Type, TypeError> {
        match stmt {
            Stmt::LetDecl {
                name,
                value,
                ty,
                span,
                ..
            } => {
                let value_ty = self.infer_expr(value)?;
                if let Some(annotated_ref) = ty {
                    // T37: consult the user-aware resolver so annotations
                    // referencing user-defined generic types
                    // (`let p: Pair<Int, String> = ...`) resolve to a real
                    // `Type::User` instead of falling through to Unknown.
                    // Built-in generics are unaffected (the user-aware
                    // resolver delegates to `typeref_to_type` first).
                    if let Some(annotated_ty) =
                        typeref_to_type_with_user(annotated_ref, &self.user_generic_decls)
                    {
                        if !assignable_to(&annotated_ty, &value_ty) {
                            // T28: null-safety. When a value of type
                            // `Option<T>` is bound/used where the BARE inner
                            // type `T` (non-Option) is expected (e.g.
                            // `let y: Int = x` where `x: Option<Int>`), the
                            // diagnostic carries the exact suffix
                            // `. Use if-let or ?? to unwrap.` so the user
                            // knows the escape hatch. The `??` operator is
                            // implemented in T101 (deferred); the message
                            // mentions it now per the T28 contract.
                            let msg = if is_null_safety_violation(&annotated_ty, &value_ty) {
                                format!(
                                    "expected {annotated_ty}, found {value_ty}. Use if-let or ?? to unwrap."
                                )
                            } else {
                                format!("expected {annotated_ty}, found {value_ty}")
                            };
                            return Err(TypeError::new(
                                Diagnostic::error(msg, *span)
                                    .with_code(ErrorCode::AssignTypeMismatch),
                            ));
                        }
                        self.env.insert(&name.name, annotated_ty.clone());
                        return Ok(annotated_ty);
                    }
                    // Unrecognised annotation (user types, generics) — defer to v0.5.
                }
                self.env.insert(&name.name, value_ty.clone());
                Ok(value_ty)
            }
            Stmt::ExprStmt(expr, _) => self.infer_expr(expr),
            Stmt::Return(Some(expr), _) => self.infer_expr(expr),
            Stmt::Return(None, _) => Ok(Type::Void),
            Stmt::Assignment { .. } => Ok(Type::Void),
            Stmt::Break(_) | Stmt::Continue(_) => Ok(Type::Void),
            Stmt::ForIn { .. } | Stmt::ForWhile { .. } => Ok(Type::Void),
            // T71: destructuring let. v0.5 deferral — the per-binding types
            // can't be split out without knowing the tuple/struct shape, so
            // each binding is recorded as `Type::Unknown` (the value type is
            // still inferred for any type-annotation check). This keeps
            // downstream uses compiling (Unknown is permissive); Rust does the
            // real per-field inference at codegen.
            Stmt::LetPattern { pattern, value, .. } => {
                let _ = self.infer_expr(value)?;
                for b in pattern.bindings() {
                    self.env.insert(&b.name, Type::Unknown);
                }
                Ok(Type::Void)
            }
            // T72: `for let PAT = EXPR { body }` — infer the value, bind
            // each pattern name to Unknown (v0.5 deferral), walk the body.
            // The whole statement is `()` (Void), matching ForIn/ForWhile.
            Stmt::ForLet {
                pattern,
                value,
                body,
                ..
            } => {
                let _ = self.infer_expr(value)?;
                for b in pattern.bindings() {
                    self.env.insert(&b.name, Type::Unknown);
                }
                for s in &body.stmts {
                    let _ = self.infer_stmt(s)?;
                }
                Ok(Type::Void)
            }
            // T73: `guard <conds> else { block }` — infer each condition's
            // value/expr; for `let` conditions, bind each pattern name to
            // Unknown (v0.5 deferral — same as ForLet/LetPattern). The
            // let-bindings are introduced IN THE ENCLOSING SCOPE (the
            // guard-passthrough path), so subsequent statements can read
            // them. Walk the else-block for its side effects on the env.
            // The whole statement is `()` (Void).
            Stmt::Guard {
                conditions,
                else_block,
                ..
            } => {
                for c in conditions {
                    match c {
                        buff_lang_ast::GuardCondition::Let { pattern, value, .. } => {
                            let _ = self.infer_expr(value)?;
                            for b in pattern.bindings() {
                                self.env.insert(&b.name, Type::Unknown);
                            }
                        }
                        buff_lang_ast::GuardCondition::Bool(e) => {
                            let _ = self.infer_expr(e)?;
                        }
                    }
                }
                for s in &else_block.stmts {
                    let _ = self.infer_stmt(s)?;
                }
                Ok(Type::Void)
            }
            // T100: `defer EXPR` — infer the deferred expression for its
            // side effects on the env (no bindings introduced). The whole
            // statement is `()` (Void).
            Stmt::Defer { expr, .. } => {
                let _ = self.infer_expr(expr)?;
                Ok(Type::Void)
            }
            Stmt::ComptimeBlock { body, .. } => {
                for s in &body.stmts {
                    let _ = self.infer_stmt(s)?;
                }
                Ok(Type::Void)
            }
        }
    }
}

impl Default for TypeInferencer {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract a compile-time `i128` value from an integer-literal expression,
/// recognising both `Literal::Int(v)` and `UnaryOp(Neg, Literal::Int(v))`
/// (the parser-realistic form for negative numbers). Returns `None` for any
/// non-integer-literal expression.
fn const_int_value(expr: &Expr) -> Option<i128> {
    match expr {
        Expr::Literal(Literal::Int(v), _) => Some(*v as i128),
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand,
            ..
        } => const_int_value(operand).map(|v| -v),
        _ => None,
    }
}

/// Converts a parse-time [`TypeRef`] into a resolved [`Type`] for the
/// primitive names recognised in v1.0.
///
/// Returns `None` for unrecognised names (full generics beyond builtin
/// collections, function types) — these are post-v1.0 work.
///
/// ## T28 — `Option<T>`
///
/// The built-in `Option<T>` type is recognised in two structural shapes:
///
/// - `TypeRef::Option(inner, _)` — the dedicated AST variant (hand-built
///   ASTs / tests).
/// - `TypeRef::Generic { base: Named("Option"), args: [inner], .. }` — the
///   shape the parser produces for source annotations like `Option<Int>`
///   (the parser treats `Option<T>` as a plain generic application; see
///   `parse_type_ref`).
///
/// Both lower to [`Type::Option`] with the inner type resolved recursively
/// (an unresolvable inner falls back to [`Type::Unknown`] so the Option
/// wrapper still flows through — this lets `let x: Option<MyEnum> = None`
/// type-check at the wrapper level even before user-enum resolution lands).
///
/// ## T13 — User-defined generic types
///
/// User-defined generic types (`struct Pair<T, U>`, `func id<T>(x: T) -> T`)
/// are **deferred to rustc** for type inference and monomorphization. This
/// function returns `None` for any `TypeRef::Generic` whose base name is NOT
/// one of the built-in prelude types (`Option`, `Result`). The caller (the
/// type inferencer / codegen) then treats the type as [`Type::Unknown`] and
/// lets rustc perform the actual monomorphization at each call site.
///
/// This is the documented MVP approach (per the T13 task spec): "substitute
/// type params at call site — no full Hindley-Milner needed; rustc does the
/// heavy lifting via monomorphization". A full Buff-side generic inference
/// engine (Hindley-Milner or rustc-query-based) is a future task.
///
/// **Built-in generic resolution is NOT broken**: `Option<T>`, `Result<T, E>`,
/// `Vector<T>`, `Map<K, V>`, `Channel<T>` continue to resolve exactly as
/// before. The `Vector`/`Map`/`Channel` families are NOT handled here (they
/// resolve at the expression-inference level from literals and constructor
/// calls, not from type annotations); only `Option` and `Result` are handled
/// in this function's `Generic` arm.
///
/// ## T58 — `pub(crate)` visibility
///
/// Exposed at `pub(crate)` so the `multi_dispatch` module can resolve
/// `FuncDecl` parameter types when building the dispatch table. Kept
/// crate-local (NOT `pub`) because the function's contract is unstable
/// — it returns `None` for user-defined types (structs/enums), which is
/// a v0.5 deferral the multi-dispatch table inherits (multi-dispatch on
/// user types falls through to `Type::Unknown` and the dispatcher treats
/// the params as opaque, which is the documented v1.19 scope).
pub(crate) fn typeref_to_type(ty: &TypeRef) -> Option<Type> {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "Int" => Some(Type::int_default()),
            "Float" => Some(Type::float_default()),
            "Double" => Some(Type::double()),
            "Bool" => Some(Type::bool()),
            "String" => Some(Type::string()),
            "Char" => Some(Type::char()),
            "Byte" => Some(Type::byte()),
            "Decimal" => Some(Type::Decimal),
            "Void" => Some(Type::Void),
            // T124b: prelude-types. Source annotations like
            // `let dt: DateTime = ...` resolve via the registry so the
            // null-safety / assignment check can compare prelude types.
            "DateTime" => Some(Type::DateTime),
            "Date" => Some(Type::Date),
            "Time" => Some(Type::Time),
            "Duration" => Some(Type::Duration),
            "Instant" => Some(Type::Instant),
            _ => None,
        },
        // T28: dedicated `Option<T>` AST variant.
        TypeRef::Option(inner, _) => Some(Type::option(
            typeref_to_type(inner).unwrap_or(Type::Unknown),
        )),
        // T28: source annotations `Option<Int>` parse as a generic
        // application whose base name is "Option". Recognise it here so a
        // `let x: Option<Int> = Some(42)` annotation resolves to a real
        // `Type::Option(Int<64>)` and the null-safety check can fire.
        //
        // T30: source annotations `Result<T, E>` parse as a generic
        // application whose base name is "Result" with 2 args. Recognise it
        // so a `let x: Result<Int, Error> = Ok(42)` annotation resolves to a
        // real `Type::Result(Int<64>, Unknown)` (the Error user-enum falls
        // back to Unknown — matching v0.5's user-type resolution gap).
        TypeRef::Generic { base, args, .. } => {
            if let TypeRef::Named { name, .. } = base.as_ref() {
                if name.name == "Option" && args.len() == 1 {
                    let inner = typeref_to_type(&args[0]).unwrap_or(Type::Unknown);
                    return Some(Type::option(inner));
                }
                if name.name == "Result" && args.len() == 2 {
                    let ok_ty = typeref_to_type(&args[0]).unwrap_or(Type::Unknown);
                    let err_ty = typeref_to_type(&args[1]).unwrap_or(Type::Unknown);
                    return Some(Type::result(ok_ty, err_ty));
                }
            }
            None
        }
        // T76: union types `A | B | C`. Resolve each member recursively;
        // unresolvable members fall back to `Unknown` so the Union wrapper
        // still flows through codegen.
        TypeRef::Union(members, _) => {
            let resolved: Vec<Type> = members
                .iter()
                .map(|m| typeref_to_type(m).unwrap_or(Type::Unknown))
                .collect();
            Some(Type::Union(resolved))
        }
        // T103: tuple types `(T, U, ...)`. Resolve each member recursively;
        // unresolvable members fall back to `Unknown` so the Tuple wrapper
        // still flows through codegen. A `TypeRef::Tuple` always carries 2+
        // members (the parser's single-element disambiguation), so no
        // single-element `Type::Tuple` is produced here.
        TypeRef::Tuple(members, _) => {
            let resolved: Vec<Type> = members
                .iter()
                .map(|m| typeref_to_type(m).unwrap_or(Type::Unknown))
                .collect();
            Some(Type::Tuple(resolved))
        }
        _ => None,
    }
}

/// Returns `true` when assigning `value` to `annotated` is a **null-safety
/// violation** (T28): the value is an `Option<T>` but the target is a bare,
/// non-Option type. This is the case that triggers the extended diagnostic
/// suffix (`. Use if-let or ?? to unwrap.`).
///
/// Concretely: `is_null_safety_violation(Int, Option<Int>)` is `true`, but
/// `is_null_safety_violation(Option<Int>, Option<Int>)` is `false` (Option→Option
/// is fine, handled by normal equality) and `is_null_safety_violation(Int,
/// String)` is `false` (a plain type mismatch, not a null-safety issue).
pub(crate) fn is_null_safety_violation(annotated: &Type, value: &Type) -> bool {
    matches!(value, Type::Option(_)) && !matches!(annotated, Type::Option(_))
}

/// T37 — user-aware type-reference resolution.
///
/// Resolves a [`TypeRef`] to a [`Type`] exactly like [`typeref_to_type`] for
/// all built-in shapes (primitives, `Option`, `Result`, unions, tuples), and
/// ADDITIONALLY resolves user-defined generic applications when `user_decls`
/// knows about them.
///
/// # Built-in generics are NOT broken
///
/// `Vector`/`Map`/`Channel` continue to resolve at the expression-inference
/// level (unchanged). `Option<T>` and `Result<T, E>` continue to resolve here
/// via the same `Generic`-arm special-case as before — the user-decl lookup
/// is consulted ONLY when the builtin arms have already returned `None`, so
/// a user type shadowing a builtin name (e.g. a user `Option` struct) cannot
/// hijack builtin resolution.
///
/// # User-generic substitution
///
/// When `ty` is a `TypeRef::Generic { base: Named(name), args, .. }` whose
/// `name` is registered in `user_decls` with MATCHING arity, each arg is
/// resolved recursively (via this same function, so nested user generics and
/// builtins compose), and a [`Type::User { name, args }`] is returned. An
/// arity mismatch defers to rustc (returns `None`) — mirroring the pre-T37
/// behaviour so existing snapshots stay byte-identical for malformed
/// annotations.
///
/// When `ty` is a bare `TypeRef::Named(name)` whose `name` is registered with
/// arity 0 (a non-generic user struct/enum), a `Type::User { name, args: [] }`
/// is returned. This lets `let p: Point = ...` carry a real type instead of
/// `Unknown`.
pub(crate) fn typeref_to_type_with_user(
    ty: &TypeRef,
    user_decls: &UserGenericDecls,
) -> Option<Type> {
    // First, try the builtin resolver. If it succeeds, the type is a builtin
    // (primitive / Option / Result / Union / Tuple) — return as-is. This
    // guarantees built-in generic resolution is NOT broken by T37: the
    // user-decl registry is never consulted for a builtin name.
    if let Some(resolved) = typeref_to_type(ty) {
        return Some(resolved);
    }
    // Builtin resolution returned None — consult the user-decl registry.
    match ty {
        TypeRef::Generic { base, args, .. } => {
            if let TypeRef::Named { name, .. } = base.as_ref() {
                if let Some(declared_arity) = user_decls.arity_of(&name.name) {
                    // Arity must match the declaration; a mismatch defers to
                    // rustc (None) rather than emitting a malformed User type.
                    if declared_arity == args.len() {
                        // Recursively resolve each argument so nested user
                        // generics (`Outer<Pair<Int, String>>`) and builtins
                        // (`Tree<Option<Int>>`) compose. Unresolvable args
                        // fall back to Unknown (the same stance as Union/Tuple).
                        let resolved_args: Vec<Type> = args
                            .iter()
                            .map(|a| typeref_to_type_with_user(a, user_decls).unwrap_or(Type::Unknown))
                            .collect();
                        return Some(Type::user(name.name.clone(), resolved_args));
                    }
                }
            }
            None
        }
        TypeRef::Named { name, .. } => {
            // A bare user type reference. Only resolve when registered with
            // arity 0 (a non-generic user struct/enum). A registered generic
            // referenced without args (e.g. `Pair` when `Pair<T, U>` is
            // declared) defers to rustc — the user omitted required args.
            if user_decls.arity_of(&name.name) == Some(0) {
                return Some(Type::user(name.name.clone(), Vec::new()));
            }
            None
        }
        _ => None,
    }
}

/// T124b: prelude-datetime arithmetic result-type helper.
///
/// chrono (and `std::time::Instant`) impl `Add<Duration>` and `Sub<Duration>`
/// on the datetime family, and `Sub<Self>` yields a `Duration` for paired
/// datetimes. This helper captures the legal combinations so the type
/// inferencer can accept `dt + Duration.days(7)` and `dt1 - dt2` without
/// falling through to the numeric `promote_binary` path (which would reject
/// them as non-numeric).
///
/// Returns `Some(result_ty)` for legal combinations, `None` otherwise (so
/// the caller falls through to the default promote/error path).
fn datetime_arith_result(op: &buff_lang_ast::BinaryOp, lhs: &Type, rhs: &Type) -> Option<Type> {
    use buff_lang_ast::BinaryOp;
    match (op, lhs, rhs) {
        // `<datetime> + Duration -> <datetime>` — chrono `DateTime + TimeDelta`,
        // `NaiveDate + TimeDelta`, `NaiveTime + TimeDelta`, `Instant + Duration`.
        (
            BinaryOp::Add,
            t @ (Type::DateTime | Type::Date | Type::Time | Type::Instant),
            Type::Duration,
        ) => Some(t.clone()),
        (
            BinaryOp::Add,
            Type::Duration,
            t @ (Type::DateTime | Type::Date | Type::Time | Type::Instant),
        ) => Some(t.clone()),
        // `<datetime> - Duration -> <datetime>` — chrono `Sub<TimeDelta>`.
        (
            BinaryOp::Sub,
            t @ (Type::DateTime | Type::Date | Type::Time | Type::Instant),
            Type::Duration,
        ) => Some(t.clone()),
        // `<datetime> - <same-type datetime> -> Duration` — chrono `Sub<Self>` yields
        // a `TimeDelta` for paired DateTime / NaiveDate / NaiveTime, and
        // `std::time::Instant - Instant = std::time::Duration`.
        (BinaryOp::Sub, Type::DateTime, Type::DateTime)
        | (BinaryOp::Sub, Type::Date, Type::Date)
        | (BinaryOp::Sub, Type::Time, Type::Time)
        | (BinaryOp::Sub, Type::Instant, Type::Instant) => Some(Type::Duration),
        // `Duration + Duration -> Duration` — chrono TimeDelta + TimeDelta.
        (BinaryOp::Add, Type::Duration, Type::Duration) => Some(Type::Duration),
        // `Duration - Duration -> Duration` — chrono TimeDelta - TimeDelta.
        (BinaryOp::Sub, Type::Duration, Type::Duration) => Some(Type::Duration),
        // Every other combination falls through to the default path
        // (numeric promotion or type error).
        _ => None,
    }
}

/// T63 — Build the candidate list of implicit-prelude names (free fns +
/// prelude types) used by [`TypeInferencer::lookup_ident`] to attach
/// "did you mean `X`?" help notes to undefined-variable errors.
///
/// The list is the union of [`prelude::PreludeFn::ALL`] and
/// [`prelude_types::PreludeType::ALL`] source-names. It is rebuilt on each
/// undefined-variable error (an error path, so the cost is irrelevant) —
/// keeping it out of a static avoids pulling `lazy_static` / `once_cell`
/// into the leaf-adjacent types crate.
fn prelude_suggestion_candidates() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::with_capacity(64);
    for &pf in prelude::PreludeFn::ALL {
        names.push(pf.name());
    }
    for &pt in prelude_types::PreludeType::ALL {
        names.push(pt.name());
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::{BinaryOp, Literal};

    fn span() -> Span {
        Span::dummy()
    }

    #[test]
    fn infer_all_literals() {
        let mut inf = TypeInferencer::new();
        let cases = [
            (Expr::Literal(Literal::Int(42), span()), Type::int_default()),
            (
                Expr::Literal(Literal::Float(2.5), span()),
                Type::float_default(),
            ),
            (Expr::Literal(Literal::Double(9.9), span()), Type::double()),
            (Expr::Literal(Literal::Bool(true), span()), Type::bool()),
            (
                Expr::Literal(Literal::String("hi".into()), span()),
                Type::string(),
            ),
            (Expr::Literal(Literal::Byte(0xFF), span()), Type::byte()),
        ];
        for (expr, expected) in cases {
            assert_eq!(inf.infer_expr(&expr).unwrap(), expected);
        }
    }

    #[test]
    fn infer_neg_preserves_numeric_type() {
        let mut inf = TypeInferencer::new();
        let e = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(Expr::Literal(Literal::Int(5), span())),
            span: span(),
        };
        assert_eq!(inf.infer_expr(&e).unwrap(), Type::int_default());
    }

    #[test]
    fn infer_add_promotes_to_float() {
        let mut inf = TypeInferencer::new();
        let e = Expr::BinaryOp {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Literal(Literal::Int(1), span())),
            rhs: Box::new(Expr::Literal(Literal::Float(2.0), span())),
            span: span(),
        };
        assert_eq!(inf.infer_expr(&e).unwrap(), Type::float_default());
    }

    #[test]
    fn infer_logical_error_on_int() {
        let mut inf = TypeInferencer::new();
        let e = Expr::BinaryOp {
            op: BinaryOp::And,
            lhs: Box::new(Expr::Literal(Literal::Int(1), span())),
            rhs: Box::new(Expr::Literal(Literal::Int(2), span())),
            span: span(),
        };
        assert!(inf.infer_expr(&e).is_err());
    }

    #[test]
    fn infer_not_on_int_errors() {
        let mut inf = TypeInferencer::new();
        let e = Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::Literal(Literal::Int(5), span())),
            span: span(),
        };
        assert!(inf.infer_expr(&e).is_err());
    }

    #[test]
    fn let_decl_with_annotation_mismatch_errors() {
        let mut inf = TypeInferencer::new();
        let stmt = Stmt::LetDecl {
            name: buff_lang_ast::Ident::new("x", span()),
            value: Expr::Literal(Literal::String("hello".into()), span()),
            mutable: false,
            ty: Some(TypeRef::Named {
                name: buff_lang_ast::Ident::new("Int", span()),
                span: span(),
            }),
            span: span(),
        };
        assert!(inf.infer_stmt(&stmt).is_err());
    }

    // -----------------------------------------------------------------------
    // T13 — Generics: verify that built-in generic resolution is NOT broken
    // by the type_params AST migration, and that user-defined generics
    // defer to rustc (return None from typeref_to_type).
    // -----------------------------------------------------------------------

    fn named(name: &str) -> TypeRef {
        TypeRef::Named {
            name: buff_lang_ast::Ident::new(name, span()),
            span: span(),
        }
    }

    fn generic(base: &str, args: &[&str]) -> TypeRef {
        TypeRef::Generic {
            base: Box::new(named(base)),
            args: args.iter().map(|a| named(a)).collect(),
            span: span(),
        }
    }

    #[test]
    fn t13_option_generic_still_resolves() {
        // Option<Int> must resolve to Type::Option(Int<64>).
        let ty = generic("Option", &["Int"]);
        let resolved = typeref_to_type(&ty);
        assert_eq!(
            resolved,
            Some(Type::option(Type::int_default())),
            "Option<Int> must still resolve after the T13 type_params migration"
        );
    }

    #[test]
    fn t13_result_generic_still_resolves() {
        // Result<Int, String> must resolve to Type::Result(Int<64>, String).
        let ty = generic("Result", &["Int", "String"]);
        let resolved = typeref_to_type(&ty);
        assert_eq!(
            resolved,
            Some(Type::result(Type::int_default(), Type::string())),
            "Result<Int, String> must still resolve after the T13 type_params migration"
        );
    }

    #[test]
    fn t13_user_defined_generic_defers_to_rustc() {
        // User-defined generics like Pair<T, U> are NOT resolved by the Buff
        // type inferencer — they defer to rustc for monomorphization.
        // typeref_to_type returns None (the existing fallthrough behavior).
        let ty = generic("Pair", &["Int", "String"]);
        let resolved = typeref_to_type(&ty);
        assert_eq!(
            resolved, None,
            "user-defined generics defer to rustc (typeref_to_type returns None)"
        );
    }

    #[test]
    fn t13_generic_func_param_type_defers_to_rustc() {
        // A generic func parameter like `x: T` (where T is a type param)
        // resolves to None from typeref_to_type (T is not a builtin name).
        // The type inferencer treats it as Unknown; rustc handles the actual
        // type binding at monomorphization time.
        let ty = named("T");
        let resolved = typeref_to_type(&ty);
        assert_eq!(
            resolved, None,
            "type-param names resolve to None (rustc handles binding)"
        );
    }

    // -----------------------------------------------------------------------
    // T37 — User-defined generic type resolution (v1.25 language-features
    // batch). The builtin resolver (`typeref_to_type`) is unchanged; the
    // user-aware resolver (`typeref_to_type_with_user`) adds a `Type::User`
    // result for registered user generics with matching arity.
    // -----------------------------------------------------------------------

    #[test]
    fn t37_builtin_option_still_resolves_under_user_aware_resolver() {
        // The user-aware resolver MUST delegate to the builtin resolver
        // first, so Option<Int> still resolves to Type::Option(Int<64>)
        // even with a populated user-decl registry.
        let mut decls = UserGenericDecls::new();
        decls.register("Pair", 2);
        let ty = generic("Option", &["Int"]);
        let resolved = typeref_to_type_with_user(&ty, &decls);
        assert_eq!(
            resolved,
            Some(Type::option(Type::int_default())),
            "Option<Int> must still resolve via the user-aware resolver"
        );
    }

    #[test]
    fn t37_user_generic_with_matching_arity_resolves() {
        // Pair<Int, String> registered with arity 2 resolves to
        // Type::User { name: "Pair", args: [Int<64>, String] }.
        let mut decls = UserGenericDecls::new();
        decls.register("Pair", 2);
        let ty = generic("Pair", &["Int", "String"]);
        let resolved = typeref_to_type_with_user(&ty, &decls);
        assert_eq!(
            resolved,
            Some(Type::user("Pair", vec![Type::int_default(), Type::string()])),
            "Pair<Int, String> must resolve to Type::User with bound args"
        );
    }

    #[test]
    fn t37_user_generic_arity_mismatch_defers_to_rustc() {
        // Pair<Int> when Pair<T, U> is declared (arity 2) — arity mismatch
        // defers to rustc (returns None), preserving pre-T37 behaviour.
        let mut decls = UserGenericDecls::new();
        decls.register("Pair", 2);
        let ty = generic("Pair", &["Int"]);
        let resolved = typeref_to_type_with_user(&ty, &decls);
        assert_eq!(
            resolved, None,
            "arity-mismatched user generic defers to rustc (None)"
        );
    }

    #[test]
    fn t37_user_generic_not_registered_defers_to_rustc() {
        // An unregistered user generic (no decl in the registry) defers to
        // rustc — the resolver cannot invent a Type::User without a decl.
        let decls = UserGenericDecls::new();
        let ty = generic("Pair", &["Int", "String"]);
        let resolved = typeref_to_type_with_user(&ty, &decls);
        assert_eq!(
            resolved, None,
            "unregistered user generic defers to rustc (None)"
        );
    }

    #[test]
    fn t37_bare_user_type_resolves_when_registered_zero_arity() {
        // A bare user type `Point` registered with arity 0 resolves to
        // Type::User { name: "Point", args: [] }.
        let mut decls = UserGenericDecls::new();
        decls.register("Point", 0);
        let ty = named("Point");
        let resolved = typeref_to_type_with_user(&ty, &decls);
        assert_eq!(
            resolved,
            Some(Type::user("Point", Vec::new())),
            "bare user type registered with arity 0 resolves to Type::User"
        );
    }

    #[test]
    fn t37_nested_user_generic_resolves_recursively() {
        // Outer<Pair<Int, String>> — the outer arg is itself a user generic
        // that must resolve recursively.
        let mut decls = UserGenericDecls::new();
        decls.register("Outer", 1);
        decls.register("Pair", 2);
        let inner = generic("Pair", &["Int", "String"]);
        let ty = TypeRef::Generic {
            base: Box::new(named("Outer")),
            args: vec![inner],
            span: span(),
        };
        let resolved = typeref_to_type_with_user(&ty, &decls);
        assert_eq!(
            resolved,
            Some(Type::user(
                "Outer",
                vec![Type::user(
                    "Pair",
                    vec![Type::int_default(), Type::string()]
                )]
            )),
            "nested user generics resolve recursively"
        );
    }

    #[test]
    fn t37_user_generic_mixed_with_builtin_arg_resolves() {
        // Tree<Option<Int>> — a user generic whose arg is a builtin generic.
        let mut decls = UserGenericDecls::new();
        decls.register("Tree", 1);
        let inner = generic("Option", &["Int"]);
        let ty = TypeRef::Generic {
            base: Box::new(named("Tree")),
            args: vec![inner],
            span: span(),
        };
        let resolved = typeref_to_type_with_user(&ty, &decls);
        assert_eq!(
            resolved,
            Some(Type::user("Tree", vec![Type::option(Type::int_default())])),
            "user generic with builtin arg resolves recursively"
        );
    }

    // ---------------------------------------------------------------------------
    // T75b: TraitImplRegistry — associated-type resolution.
    // ---------------------------------------------------------------------------

    fn t75b_named_ty(name: &str) -> TypeRef {
        TypeRef::Named {
            name: buff_lang_ast::Ident::new(name, span()),
            span: span(),
        }
    }

    fn t75b_impl_block(
        trait_name: &str,
        target_name: &str,
        bindings: &[(&str, &str)],
    ) -> buff_lang_ast::ImplBlock {
        buff_lang_ast::ImplBlock {
            trait_name: t75b_named_ty(trait_name),
            target: t75b_named_ty(target_name),
            type_bindings: bindings
                .iter()
                .map(|(n, t)| buff_lang_ast::AssociatedTypeBinding {
                    name: buff_lang_ast::Ident::new(*n, span()),
                    target: t75b_named_ty(t),
                    span: span(),
                })
                .collect(),
            methods: Vec::new(),
            span: span(),
        }
    }

    #[test]
    fn t75b_empty_registry_resolves_nothing() {
        let reg = TraitImplRegistry::new();
        assert!(reg
            .resolve_associated_type("Container", "Box", "Item")
            .is_none());
        assert!(!reg.has_impl("Container", "Box"));
    }

    #[test]
    fn t75b_registry_resolves_registered_binding() {
        let mut reg = TraitImplRegistry::new();
        reg.register_impl_block(&t75b_impl_block(
            "Container",
            "Box",
            &[("Item", "Int")],
        ));
        assert!(reg.has_impl("Container", "Box"));
        let resolved = reg
            .resolve_associated_type("Container", "Box", "Item")
            .expect("registered binding must resolve");
        assert!(
            matches!(resolved, TypeRef::Named { name, .. } if name.name == "Int"),
            "expected Item -> Int, got {resolved:?}"
        );
    }

    #[test]
    fn t75b_registry_multiple_bindings_independent() {
        let mut reg = TraitImplRegistry::new();
        reg.register_impl_block(&t75b_impl_block(
            "Map",
            "Dict",
            &[("Key", "String"), ("Value", "Int")],
        ));
        let key = reg
            .resolve_associated_type("Map", "Dict", "Key")
            .expect("Key binding must resolve");
        let val = reg
            .resolve_associated_type("Map", "Dict", "Value")
            .expect("Value binding must resolve");
        assert!(
            matches!(key, TypeRef::Named { name, .. } if name.name == "String"),
            "Key -> String"
        );
        assert!(
            matches!(val, TypeRef::Named { name, .. } if name.name == "Int"),
            "Value -> Int"
        );
    }

    #[test]
    fn t75b_registry_unknown_assoc_returns_none() {
        let mut reg = TraitImplRegistry::new();
        reg.register_impl_block(&t75b_impl_block(
            "Container",
            "Box",
            &[("Item", "Int")],
        ));
        // Assoc name NOT in the registered bindings → None.
        assert!(reg
            .resolve_associated_type("Container", "Box", "Other")
            .is_none());
    }

    #[test]
    fn t75b_registry_unknown_target_returns_none() {
        let mut reg = TraitImplRegistry::new();
        reg.register_impl_block(&t75b_impl_block(
            "Container",
            "Box",
            &[("Item", "Int")],
        ));
        // Different target type that has no registered impl → None.
        assert!(reg
            .resolve_associated_type("Container", "Bag", "Item")
            .is_none());
        assert!(!reg.has_impl("Container", "Bag"));
    }

    #[test]
    fn t75b_register_from_decls_walks_impl_blocks() {
        use buff_lang_ast::Decl;
        let decls: Vec<Decl> = vec![
            Decl::ImplBlock(t75b_impl_block("A", "X", &[("T", "Int")])),
            Decl::ImplBlock(t75b_impl_block("B", "Y", &[("U", "String")])),
        ];
        let mut reg = TraitImplRegistry::new();
        reg.register_from_decls(decls.iter());
        assert!(reg.has_impl("A", "X"));
        assert!(reg.has_impl("B", "Y"));
        assert!(!reg.has_impl("A", "Y"));
    }

    #[test]
    fn t75b_inferencer_delegates_to_registry() {
        let mut inf = TypeInferencer::new();
        inf.register_trait_impl(&t75b_impl_block(
            "Container",
            "Box",
            &[("Item", "Int")],
        ));
        let resolved = inf
            .resolve_associated_type("Container", "Box", "Item")
            .expect("inferencer must consult its registry");
        assert!(
            matches!(resolved, TypeRef::Named { name, .. } if name.name == "Int"),
            "expected Item -> Int via inferencer, got {resolved:?}"
        );
        // Unknown reference returns None (the pre-T75b fallback).
        assert!(inf
            .resolve_associated_type("Container", "Box", "Other")
            .is_none());
    }
}
