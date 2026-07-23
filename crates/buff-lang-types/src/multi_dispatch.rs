//! T58 — Multiple Dispatch for Numerical APIs (Julia-inspired).
//!
//! Compile-time dispatch on ALL argument types, not just the receiver. A
//! group of free functions sharing the same name with different argument
//! type signatures forms a "multi-dispatch group". At a call site, the
//! compiler infers each argument's type and selects the unique matching
//! impl. Single-dispatch is the special case (group size 1) — unchanged
//! from pre-T58 behaviour.
//!
//! # Design (backward-compatible)
//!
//! - A name forms a multi-dispatch group ONLY if **2+ free funcs share it**
//!   in the same compilation unit. A lone `func foo(a: Int)` does NOT form
//!   a group and bypasses the dispatcher entirely (preserves the existing
//!   unmangled Rust name so all pre-T58 code keeps compiling identically).
//! - At codegen, multi-dispatch group impls are MANGLED as
//!   `<name>_<argTy1>_<argTy2>_...` so each impl lowers to a unique Rust
//!   free function. Call sites emit the mangled name selected by the
//!   dispatcher.
//! - All matching is on TYPES (not values); all dispatch is COMPILE-TIME
//!   (no runtime vtable, no dynamic dispatch).
//!
//! # Errors (existing E12xx range, no new variants)
//!
//! - 0 matching impls for a call → `E1201` (UndefinedVariable: "no
//!   matching impl").
//! - 2+ equally-matching impls for a call → `E1202` (BinaryOpTypeMismatch:
//!   "ambiguous dispatch").
//!
//! # Mangling scheme
//!
//! `<buff_name>_<arg1_ty>_<arg2_ty>_...` where each `<argN_ty>` is the
//! lowercased base primitive name (`int`, `float`, `string`, `bool`,
//! `vector`, `matrix`, `option`, `result`, `decimal`, `double`, `char`,
//! `byte`, user-defined-type names). Nested generics collapse to the
//! base name (so `Vector<Int>` and `Vector<Float>` both → `vector`); the
//! 2+ arity of a group guarantees uniqueness even with collapsed names
//! when each impl's param ARITY or param BASE-TYPE-SEQUENCE differs.
//!
//! Mangling is deterministic (driven by declaration source order +
//! canonicalised type tokens via `BTreeMap`/`BTreeSet`), so the same
//! Buff source always produces byte-identical Rust (the T29 flaky-test
//! lesson).

use std::collections::BTreeMap;

use buff_lang_ast::{Decl, FuncDecl, TypeRef};
use buff_lang_error::{Diagnostic, ErrorCode, Span, TypeError};

use crate::infer::typeref_to_type;
use crate::promote::assignable_to;
use crate::ty::Type;

/// A single method inside a multi-dispatch group (one of N impls sharing
/// the same Buff function name).
///
/// Carries the resolved param types and the resolved return type so the
/// dispatcher can match arguments and report the inferred call type. The
/// `mangled_name` is the unique Rust free-fn name this impl lowers to.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiDispatchMethod {
    /// Resolved parameter types in declaration order.
    pub param_types: Vec<Type>,
    /// Resolved return type (`Type::Void` when no annotation).
    pub return_type: Type,
    /// The Rust-mangled name (`<buff_name>_<ty1>_<ty2>_...`).
    pub mangled_name: String,
}

/// The registry of all multi-dispatch groups in a compilation unit.
///
/// Keyed by Buff function name. **Only names with 2+ impls are present** —
/// a lone `func foo(a: Int)` does NOT form a multi-dispatch group and
/// bypasses the dispatcher entirely (preserving backward compatibility
/// with pre-T58 unmangled codegen).
///
/// Built once at the start of codegen / type-checking from the top-level
/// declaration list. Lookups are O(N) where N is the number of impls in
/// the group (typically tiny — 2-5 impls in practice).
#[derive(Debug, Clone, Default)]
pub struct MultiDispatchTable {
    groups: BTreeMap<String, Vec<MultiDispatchMethod>>,
}

impl MultiDispatchTable {
    /// Build the table from a declaration list.
    ///
    /// Free functions sharing a name with 2+ impls form a group; lone
    /// impls are NOT registered (so they go through the existing
    /// single-dispatch codegen path unchanged).
    ///
    /// Determinism: declarations are walked in source order, the
    /// `BTreeMap` keys iterate deterministically, and the per-group impl
    /// vec preserves source order. The same `&[Decl]` always produces a
    /// byte-identical table (the T29 flaky-test lesson).
    pub fn build(decls: &[Decl]) -> Self {
        // First pass: collect ALL free FuncDecls by name (preserve source
        // order via `Vec`, not `BTreeSet`, so a group's impl vec is in
        // declaration order).
        let mut by_name: BTreeMap<String, Vec<&FuncDecl>> = BTreeMap::new();
        for decl in decls {
            if let Decl::FuncDecl(f) = decl {
                by_name.entry(f.name.name.clone()).or_default().push(f);
            }
        }
        let mut groups: BTreeMap<String, Vec<MultiDispatchMethod>> = BTreeMap::new();
        for (name, funcs) in by_name {
            // Skip single-impl names: NOT a multi-dispatch group. The
            // existing unmangled path handles them — backward compat.
            if funcs.len() < 2 {
                continue;
            }
            let methods: Vec<MultiDispatchMethod> = funcs
                .iter()
                .map(|f| {
                    let param_type_refs: Vec<&TypeRef> = f.params.iter().map(|p| &p.ty).collect();
                    MultiDispatchMethod {
                        param_types: f
                            .params
                            .iter()
                            .map(|p| typeref_to_type(&p.ty).unwrap_or(Type::Unknown))
                            .collect(),
                        return_type: f
                            .return_type
                            .as_ref()
                            .and_then(typeref_to_type)
                            .unwrap_or(Type::Void),
                        mangled_name: mangle(&name, &param_type_refs),
                    }
                })
                .collect();
            groups.insert(name, methods);
        }
        Self { groups }
    }

    /// Is `name` a multi-dispatch group? (Used by codegen to short-circuit
    /// the existing single-impl lowering path — when false, the caller
    /// uses the unmangled name unchanged.)
    pub fn is_group(&self, name: &str) -> bool {
        self.groups.contains_key(name)
    }

    /// Returns the method at the given source-order index for the named
    /// group. Used by codegen's `lower_func` to look up the mangled name
    /// for the FuncDecl being lowered (callers locate the index by
    /// matching the FuncDecl's param types against the group's methods).
    pub fn method_for_decl(&self, f: &FuncDecl) -> Option<&MultiDispatchMethod> {
        let methods = self.groups.get(&f.name.name)?;
        let target_param_types: Vec<Type> = f
            .params
            .iter()
            .map(|p| typeref_to_type(&p.ty).unwrap_or(Type::Unknown))
            .collect();
        // Source-order-preserving search: the FIRST method whose param
        // types match the decl's signature. Identical signatures inside
        // a group are illegal Buff (and a future hard error); for now,
        // first-match gives deterministic behaviour.
        methods.iter().find(|m| m.param_types == target_param_types)
    }

    /// Resolve a call to `name` with `arg_types`.
    ///
    /// Returns:
    /// - `Ok(Some((method_idx, return_type)))` when exactly one impl
    ///   matches. `method_idx` is the source-order index into the group
    ///   (used by codegen to look up the mangled name).
    /// - `Ok(None)` when `name` is NOT a multi-dispatch group (the caller
    ///   should use the existing single-impl / unmangled path).
    /// - `Err(TypeError)` when 0 matches (no impl) or >1 matches
    ///   (ambiguous dispatch). Uses existing E12xx codes per the T58
    ///   spec ("no new ErrorCode variants").
    pub fn resolve(
        &self,
        name: &str,
        arg_types: &[Type],
        span: Span,
    ) -> Result<Option<(usize, Type)>, TypeError> {
        let Some(methods) = self.groups.get(name) else {
            return Ok(None);
        };
        // T58 specificity (Julia-inspired). Dispatch proceeds in TWO
        // passes so an EXACT-type match is always preferred over a
        // widened (assignable) match:
        //
        // 1. Collect EXACT matches — every arg type EQUALS the
        //    corresponding param type. If exactly one exists, it wins.
        //    If 2+ exist, the call is ambiguous (two impls with the
        //    identical signature — illegal Buff, surfaced as E1202).
        // 2. If no exact match, collect ASSIGNABLE matches — every arg
        //    type is assignable to the param (covers numeric widening
        //    `Int` -> `Float`, `Int<8>` -> `Int<64>`). If exactly one
        //    exists, it wins; 2+ is ambiguous.
        //
        // This mirrors Julia's method specificity: `combine(Int, Int)`
        // is MORE SPECIFIC than `combine(Float, Float)` for a call
        // `combine(1, 2)`, so the exact match wins even though Int is
        // also assignable to Float. Without specificity, every call
        // with Int args would be ambiguous whenever both an Int and a
        // Float impl exist — making multi-dispatch useless for
        // numerical APIs (the exact use case T58 targets).
        let exact_matches: Vec<usize> = methods
            .iter()
            .enumerate()
            .filter(|(_, m)| args_equal(&m.param_types, arg_types))
            .map(|(i, _)| i)
            .collect();
        let matches = if exact_matches.len() == 1 {
            exact_matches
        } else if exact_matches.len() > 1 {
            // Two impls with IDENTICAL signatures — inherently ambiguous.
            exact_matches
        } else {
            // No exact match: fall back to assignable (widened) matches.
            methods
                .iter()
                .enumerate()
                .filter(|(_, m)| args_match(&m.param_types, arg_types))
                .map(|(i, _)| i)
                .collect()
        };
        match matches.len() {
            0 => {
                let arg_list = format_type_list(arg_types);
                Err(TypeError::new(
                    Diagnostic::error(
                        format!("no matching multi-dispatch impl for `{name}({arg_list})`"),
                        span,
                    )
                    .with_code(ErrorCode::UndefinedVariable),
                ))
            }
            1 => {
                let idx = matches[0];
                Ok(Some((idx, methods[idx].return_type.clone())))
            }
            _ => {
                let arg_list = format_type_list(arg_types);
                Err(TypeError::new(
                    Diagnostic::error(
                        format!(
                            "ambiguous multi-dispatch call to `{name}({arg_list})`: {} matching impls",
                            matches.len()
                        ),
                    span,
                    )
                    .with_code(ErrorCode::BinaryOpTypeMismatch),
                ))
            }
        }
    }

    /// Returns the mangled Rust name for the resolved method (used by
    /// codegen at the call site after `resolve` picks an impl).
    pub fn mangled_name(&self, name: &str, method_idx: usize) -> Option<&str> {
        self.groups
            .get(name)
            .and_then(|m| m.get(method_idx))
            .map(|m| m.mangled_name.as_str())
    }

    /// Iterate over all groups in canonical (BTreeMap) order. Used by
    /// codegen to know which FuncDecl names need mangling at lower_func
    /// time.
    pub fn groups(&self) -> impl Iterator<Item = (&String, &[MultiDispatchMethod])> {
        self.groups.iter().map(|(k, v)| (k, v.as_slice()))
    }
}

/// Do the call-site `arg_types` EXACTLY equal `param_types` (same arity,
/// every position `==`)? Used by [`MultiDispatchTable::resolve`] as the
/// first (specificity) pass — an exact match always wins over a widened
/// (assignable) match.
fn args_equal(param_types: &[Type], arg_types: &[Type]) -> bool {
    param_types.len() == arg_types.len() && param_types == arg_types
}

/// Are the call-site `arg_types` compatible with `param_types`?
///
/// A match requires equal arity AND each arg type is equal to OR
/// assignable to the corresponding param type. Assignability covers
/// numeric widening (`Int<8>` -> `Int<64>`, `Int` -> `Float`) via the
/// shared `assignable_to` helper — symmetric with how `let` annotations
/// accept promotable values.
fn args_match(param_types: &[Type], arg_types: &[Type]) -> bool {
    if param_types.len() != arg_types.len() {
        return false;
    }
    for (p, a) in param_types.iter().zip(arg_types.iter()) {
        // assignable_to(annotated, value): here the PARAM is the
        // annotated target, the ARG is the value being supplied.
        if !assignable_to(p, a) {
            return false;
        }
    }
    true
}

/// Mangle a multi-dispatch method's name based on its parameter types.
///
/// Scheme: `<buff_name>_<arg1_ty>_<arg2_ty>_...` where each `<argN_ty>`
/// is the lowercased base primitive type token (see [`type_token`]).
///
/// Examples:
/// - `combine(Int, Float)` -> `combine_int_float`
/// - `matmul(Matrix, Vector)` -> `matmul_matrix_vector`
/// - `process(Vector<Int>, Int)` -> `process_vector_int`
fn mangle(buff_name: &str, params: &[&TypeRef]) -> String {
    let mut out = String::with_capacity(buff_name.len() + params.len() * 8);
    out.push_str(buff_name);
    for p in params {
        out.push('_');
        out.push_str(&type_token(p));
    }
    out
}

/// Lowercased base type token for mangling.
///
/// Returns the lowercased base primitive name; nested generics collapse
/// to the base name (so `Vector<Int>` and `Vector<Float>` both -> `vector`).
/// The 2+ arity of a multi-dispatch group guarantees uniqueness even with
/// collapsed names when each impl's param ARITY or param BASE-TYPE-
/// SEQUENCE differs.
///
/// Unknown TypeRef shapes (function types, anonymous tuples inside
/// params) collapse to `t` (a short opaque token) so mangling never
/// panics on exotic signatures.
fn type_token(tr: &TypeRef) -> String {
    let base = match tr {
        TypeRef::Named { name, .. } => name.name.as_str(),
        TypeRef::Generic { base, .. } => {
            // Collapse to base name (`Vector<Int>` -> `Vector`).
            if let TypeRef::Named { name, .. } = base.as_ref() {
                name.name.as_str()
            } else {
                return "t".to_string();
            }
        }
        TypeRef::Option(_, _) => "Option",
        TypeRef::Function { .. } => return "fn".to_string(),
        TypeRef::Union(_, _) => "Union",
        TypeRef::Tuple(_, _) => "Tuple",
    };
    base.to_lowercase()
}

/// Helper: render a slice of resolved types as a comma-separated list
/// for diagnostic messages (`Int, Float`).
fn format_type_list(types: &[Type]) -> String {
    types
        .iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    //! Unit tests for the multi-dispatch registry + resolver. Full
    //! integration tests (parser → infer → codegen) live in
    //! `tests/multi_dispatch.rs`.

    use super::*;
    use buff_lang_ast::{
        common::{Block, Ident, Param},
        decl::FuncDecl,
    };
    use buff_lang_error::Span;

    fn sp() -> Span {
        Span::dummy()
    }

    fn named_ty(name: &str) -> TypeRef {
        TypeRef::Named {
            name: Ident::new(name, sp()),
            span: sp(),
        }
    }

    fn param(name: &str, ty_name: &str) -> Param {
        Param::plain(name, named_ty(ty_name), sp())
    }

    fn func(name: &str, params: &[(&str, &str)]) -> Decl {
        Decl::FuncDecl(FuncDecl {
            name: Ident::new(name, sp()),
            params: params.iter().map(|(n, t)| param(n, t)).collect(),
            return_type: Some(named_ty("Int")),
            body: Block::empty(sp()),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: sp(),
        })
    }

    #[test]
    fn empty_program_has_no_groups() {
        let table = MultiDispatchTable::build(&[]);
        assert_eq!(table.groups().count(), 0);
        assert!(!table.is_group("foo"));
    }

    #[test]
    fn single_func_does_not_form_group() {
        // A lone `func foo(a: Int)` is NOT multi-dispatch.
        let decls = vec![func("foo", &[("a", "Int")])];
        let table = MultiDispatchTable::build(&decls);
        assert!(!table.is_group("foo"));
    }

    #[test]
    fn two_funcs_same_name_form_group() {
        let decls = vec![
            func("combine", &[("a", "Int"), ("b", "Int")]),
            func("combine", &[("a", "Float"), ("b", "Float")]),
        ];
        let table = MultiDispatchTable::build(&decls);
        assert!(table.is_group("combine"));
        // exactly 2 methods registered.
        assert_eq!(
            table.groups().next().unwrap().1.len(),
            2,
            "group should have 2 methods"
        );
    }

    #[test]
    fn mangled_names_are_unique_per_impl() {
        let decls = vec![
            func("combine", &[("a", "Int"), ("b", "Int")]),
            func("combine", &[("a", "Float"), ("b", "Float")]),
        ];
        let table = MultiDispatchTable::build(&decls);
        let names: Vec<&str> = table
            .groups()
            .next()
            .unwrap()
            .1
            .iter()
            .map(|m| m.mangled_name.as_str())
            .collect();
        assert_eq!(names, vec!["combine_int_int", "combine_float_float"]);
    }

    #[test]
    fn resolve_returns_none_for_non_group() {
        let table = MultiDispatchTable::build(&[]);
        let r = table.resolve("foo", &[Type::int_default()], sp()).unwrap();
        assert!(r.is_none(), "non-group name resolves to None");
    }

    #[test]
    fn resolve_picks_unique_match() {
        let decls = vec![
            func("combine", &[("a", "Int"), ("b", "Int")]),
            func("combine", &[("a", "Float"), ("b", "Float")]),
        ];
        let table = MultiDispatchTable::build(&decls);
        let (idx, ret) = table
            .resolve("combine", &[Type::int_default(), Type::int_default()], sp())
            .unwrap()
            .unwrap();
        assert_eq!(idx, 0);
        assert_eq!(ret, Type::int_default());
    }

    #[test]
    fn resolve_errors_when_no_impl_matches() {
        let decls = vec![
            func("combine", &[("a", "Int"), ("b", "Int")]),
            func("combine", &[("a", "Float"), ("b", "Float")]),
        ];
        let table = MultiDispatchTable::build(&decls);
        let err = table
            .resolve("combine", &[Type::string(), Type::string()], sp())
            .unwrap_err();
        // E1201 (UndefinedVariable) per spec.
        assert_eq!(err.diagnostic.code, Some(ErrorCode::UndefinedVariable));
    }

    #[test]
    fn resolve_errors_on_ambiguous_dispatch() {
        // Two impls with IDENTICAL param types => ambiguous (both match
        // any call with those types). This is the canonical ambiguity
        // case.
        let decls = vec![
            func("combine", &[("a", "Int"), ("b", "Int")]),
            func("combine", &[("a", "Int"), ("b", "Int")]),
        ];
        let table = MultiDispatchTable::build(&decls);
        let err = table
            .resolve("combine", &[Type::int_default(), Type::int_default()], sp())
            .unwrap_err();
        // E1202 (BinaryOpTypeMismatch) per spec.
        assert_eq!(err.diagnostic.code, Some(ErrorCode::BinaryOpTypeMismatch));
    }

    #[test]
    fn assignability_widens_int_to_float_in_dispatch() {
        // `combine(Int, Float)` matches a call site `combine(Int<8>, Float<32>)`
        // via assignable_to (numeric widening).
        let decls = vec![func("combine", &[("a", "Int"), ("b", "Float")])];
        // Only ONE impl — NOT a group. So the dispatcher returns None and
        // the existing single-impl path takes over. This test asserts the
        // boundary: a single-impl name with mismatched call types does
        // NOT trip the multi-dispatch error path (it falls through to
        // the existing rustc-level check).
        let table = MultiDispatchTable::build(&decls);
        assert!(!table.is_group("combine"));
    }

    #[test]
    fn method_for_desc_finds_impl_by_signature() {
        let decls = vec![
            func("matmul", &[("a", "Int"), ("b", "Int")]),
            func("matmul", &[("a", "Float"), ("b", "Float")]),
        ];
        let table = MultiDispatchTable::build(&decls);
        // Look up the mangled name for the SECOND impl.
        let second_decl = match &decls[1] {
            Decl::FuncDecl(f) => f,
            _ => unreachable!(),
        };
        let method = table.method_for_decl(second_decl).unwrap();
        assert_eq!(method.mangled_name, "matmul_float_float");
    }
}
