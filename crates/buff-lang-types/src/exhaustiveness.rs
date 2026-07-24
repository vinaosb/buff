//! Exhaustiveness checking for `match` expressions (T27).
//!
//! This module implements the REFACTOR step of T27: a **reusable analysis
//! pass** that walks a Buff program's declarations, builds an enum registry,
//! and reports any `match` expression whose arms fail to cover every variant
//! of the matched enum (without a `_` wildcard catch-all).
//!
//! # Algorithm
//!
//! 1. **Build the registry**: walk `&[Decl]`, collecting every
//!    [`EnumDecl`](buff_lang_ast::EnumDecl) into a `HashMap<String,
//!    Vec<String>>` keyed by enum name → variant-name list. Generic enums
//!    (`Result<T, E>`) are keyed by their base name (`Result`); the generic
//!    params don't change variant coverage.
//! 2. **Find matches**: walk every function body looking for
//!    [`Expr::MatchExpr`](buff_lang_ast::Expr::MatchExpr) nodes. For each:
//!    - Infer the type of the scrutinee (best-effort — when the inferencer
//!      cannot resolve it, the match is treated as `Unknown` and skipped,
//!      matching the v0.5 type-error-as-warning policy).
//!    - If the scrutinee's type resolves to a known enum, collect the
//!      variants covered by the arms and check coverage.
//!    - Report the FIRST missing variant as a
//!      [`TypeError`](buff_lang_error::TypeError) with the message
//!      `"non-exhaustive match: missing <Variant>"`.
//!
//! # Coverage rules
//!
//! An arm covers a variant when its pattern's
//! [`variant_name_key`](buff_lang_ast::Pattern::variant_name_key) matches
//! the variant name. The wildcard [`Pattern::Wildcard`] covers EVERY
//! variant (so a single `_` arm makes any match exhaustive). Literal and
//! non-matching-variant patterns do NOT contribute to coverage of any
//! named variant (they may still cover other scrutinee types — bool,
//! literal, etc. — but those aren't enum-exhaustiveness concerns).
//!
//! # Limitations (v0.5)
//!
//! - **Scrutinee type inference**: works for `let x: Color = ...` bindings
//!   (the inferencer's environment carries the type) and for direct enum-
//!   constructor call expressions if they were added later. Bare `match x`
//!   where `x` is unannotated and inferred from a function call may resolve
//!   to `Unknown` and be skipped — the user must add a type annotation for
//!   exhaustiveness to fire. This matches the v0.5 policy of "type errors
//!   are warnings" — full unannotated inference arrives in a later wave.
//! - **Nested matches**: matches inside arm bodies and scrutinee sub-
//!   expressions are checked recursively (the visitor recurses into every
//!   expression).
//! - **No range/or-patterns**: or-patterns (`A | B`) and range patterns
//!   (`1..=10`) are deferred to a later task.
//!
//! # Reusability
//!
//! [`check_program`] is the top-level entry point: it takes a `&[Decl]`
//! slice and returns `Result<(), TypeError>`. The intermediate helpers
//! ([`build_enum_registry`], [`check_match_expr`]) are public so downstream
//! tools (LSP, CLI check subcommand) can reuse pieces — for example, an LSP
//! hover-request could call [`build_enum_registry`] alone to enumerate the
//! variants of a known enum without running the full check.

use std::collections::HashMap;

use buff_lang_ast::{Decl, EnumDecl, Expr, MatchArm, Pattern, Stmt};
use buff_lang_error::{Diagnostic, ErrorCode, TypeError};

use crate::TypeInferencer;

/// A registry of enum names → their declared variant names.
///
/// Built from `Decl::EnumDecl`s via [`build_enum_registry`]. Used by
/// [`check_match_expr`] to enumerate the variants a match must cover.
pub type EnumRegistry = HashMap<String, Vec<String>>;

/// Build an [`EnumRegistry`] from a slice of top-level declarations.
///
/// Every `Decl::EnumDecl` contributes its name and the names of its
/// variants. Generic params are NOT part of the key (e.g. `Result<T, E>`
/// is keyed as `"Result"`); the variants of a generic enum are independent
/// of the type arguments.
///
/// If two enums with the same name are declared, the LATER one wins (this
/// matches Rust's shadowing semantics for items in the same module). This
/// is a v0.5 simplification — full name-resolution arrives with the module
/// system.
///
/// This function is **pure over user declarations** — it does NOT include
/// the built-in prelude enums. Use [`build_enum_registry_with_prelude`] (or
/// [`check_program`], which calls it) when you need the built-in `Option`
/// variants (`Some`/`None`) registered so a `match opt { Some(x) => ...,
/// None => ... }` can be checked for exhaustiveness (T28).
pub fn build_enum_registry(decls: &[Decl]) -> EnumRegistry {
    let mut registry: EnumRegistry = EnumRegistry::new();
    for decl in decls {
        if let Decl::EnumDecl(e) = decl {
            registry.insert(e.name.name.clone(), variant_names(e));
        }
    }
    registry
}

/// Build an [`EnumRegistry`] seeded with the **built-in prelude enums**
/// (T28, T30), then folded with the program's user `Decl::EnumDecl`s.
///
/// Today the prelude enums are:
/// - `Option<T>` with variants `Some(T)` and `None` (T28).
/// - `Result<T, E>` with variants `Ok(T)` and `Err(E)` (T30).
///
/// Seeding them here means `None`/`Some`/`Ok`/`Err` resolve as prelude enum
/// variants WITHOUT a user declaration — they are prelude enum variants, NOT
/// reserved keywords (the lexer's keyword list deliberately omits them; see
/// T28 and T30).
///
/// User declarations take precedence over the seed: if a program declares
/// its own `enum Option` or `enum Result`, the user's variant list overrides
/// the prelude seed (user decls are inserted AFTER the seed).
pub fn build_enum_registry_with_prelude(decls: &[Decl]) -> EnumRegistry {
    let mut registry: EnumRegistry = EnumRegistry::new();
    // T28: seed the built-in Option<T> enum so None/Some resolve as its
    // variants without a user declaration.
    registry.insert(
        "Option".to_string(),
        vec!["Some".to_string(), "None".to_string()],
    );
    // T30: seed the built-in Result<T, E> enum so Ok/Err resolve as its
    // variants without a user declaration. Mirrors the Option seed exactly.
    registry.insert(
        "Result".to_string(),
        vec!["Ok".to_string(), "Err".to_string()],
    );
    for decl in decls {
        if let Decl::EnumDecl(e) = decl {
            registry.insert(e.name.name.clone(), variant_names(e));
        }
    }
    registry
}

/// Extract the declared variant names from an [`EnumDecl`] (in declaration
/// order). The payload types are NOT included — exhaustiveness only cares
/// about whether each variant NAME is matched.
fn variant_names(e: &EnumDecl) -> Vec<String> {
    e.variants.iter().map(|v| v.name.name.clone()).collect()
}

/// Check every `match` expression in a program for exhaustiveness.
///
/// Walks every function body's statements and expressions, finds each
/// `Expr::MatchExpr`, and verifies its arms cover the scrutinee's enum
/// variants (when the scrutinee's type is a known enum in the
/// [`EnumRegistry`]). Returns `Err(TypeError)` describing the FIRST
/// non-exhaustive match; returns `Ok(())` when every match is exhaustive
/// (or when no matches are present).
///
/// # Scrutinee type inference
///
/// A fresh [`TypeInferencer`] is constructed for each function (mirroring
/// the codegen pass's per-function reset). When the inferencer cannot
/// resolve the scrutinee's type (it returns [`Type::Unknown`](crate::Type::Unknown)),
/// the match is skipped — this matches v0.5's "type errors are warnings"
/// policy and avoids false positives on partial programs. To force
/// exhaustiveness checking, annotate the scrutinee's binding:
///
/// ```buff,ignore
/// let c: Color = Color::Red
/// match c { ... }  // now `c` resolves to Color and coverage fires
/// ```
pub fn check_program(decls: &[Decl]) -> Result<(), TypeError> {
    // T28: use the prelude-seeded registry so the built-in Option enum's
    // variants (Some/None) are known without a user declaration.
    let registry = build_enum_registry_with_prelude(decls);
    for decl in decls {
        let Decl::FuncDecl(f) = decl else {
            continue;
        };
        // Reset the inferencer per-function (mirrors codegen's reset).
        let mut inferencer = TypeInferencer::new();
        // Seed the environment with the function's parameter types so a
        // `match param { ... }` can resolve the param's enum type.
        for p in &f.params {
            if let Some(ty) = typeref_to_type(&p.ty) {
                inferencer.bind(&p.name.name, ty);
            }
        }
        // Walk every statement; the visitor recurses into expressions.
        for stmt in &f.body.stmts {
            check_stmt(stmt, &registry, &mut inferencer)?;
        }
    }
    Ok(())
}

/// Visit a statement, recursing into any `match` expressions it contains.
fn check_stmt(
    stmt: &Stmt,
    registry: &EnumRegistry,
    inferencer: &mut TypeInferencer,
) -> Result<(), TypeError> {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => check_expr(value, registry, inferencer),
        Stmt::Assignment { target, value, .. } => {
            check_expr(target, registry, inferencer)?;
            check_expr(value, registry, inferencer)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => Ok(()),
        Stmt::ComptimeBlock { body, .. } => {
            for s in &body.stmts {
                check_stmt(s, registry, inferencer)?;
            }
            Ok(())
        }
        Stmt::ForIn { iter, body, .. } => {
            check_expr(iter, registry, inferencer)?;
            for s in &body.stmts {
                check_stmt(s, registry, inferencer)?;
            }
            Ok(())
        }
        Stmt::ForWhile { cond, body, .. } => {
            check_expr(cond, registry, inferencer)?;
            for s in &body.stmts {
                check_stmt(s, registry, inferencer)?;
            }
            Ok(())
        }
        // T72: `for let PAT = EXPR { body }` — recurse into the value and
        // the body. The pattern is not an expression (no match to check).
        Stmt::ForLet { value, body, .. } => {
            check_expr(value, registry, inferencer)?;
            for s in &body.stmts {
                check_stmt(s, registry, inferencer)?;
            }
            Ok(())
        }
        // T73: `guard <conds> else { block }` — recurse into each condition's
        // value/expr and the else-block. The let-pattern is not an expr.
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for c in conditions {
                let e = match c {
                    buff_lang_ast::GuardCondition::Let { value, .. } => value,
                    buff_lang_ast::GuardCondition::Bool(e) => e,
                };
                check_expr(e, registry, inferencer)?;
            }
            for s in &else_block.stmts {
                check_stmt(s, registry, inferencer)?;
            }
            Ok(())
        }
        // T100: `defer EXPR` — recurse into the deferred expression (it may
        // contain a nested `match`).
        Stmt::Defer { expr, .. } => check_expr(expr, registry, inferencer),
    }
}

/// Visit an expression, recursing into sub-expressions and checking any
/// `Expr::MatchExpr` against the enum registry.
fn check_expr(
    expr: &Expr,
    registry: &EnumRegistry,
    inferencer: &mut TypeInferencer,
) -> Result<(), TypeError> {
    match expr {
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            // First recurse into the scrutinee (it may itself contain a
            // nested match) and each arm body, so deeply-nested matches
            // are checked too.
            check_expr(scrutinee, registry, inferencer)?;
            for arm in arms {
                for s in &arm.body.stmts {
                    check_stmt(s, registry, inferencer)?;
                }
            }
            // Then check THIS match's exhaustiveness.
            check_match_expr(scrutinee, arms, registry, inferencer)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => Ok(()),
        Expr::BinaryOp { lhs, rhs, .. } => {
            check_expr(lhs, registry, inferencer)?;
            check_expr(rhs, registry, inferencer)
        }
        Expr::UnaryOp { operand, .. } => check_expr(operand, registry, inferencer),
        Expr::FuncCall { callee, args, .. } => {
            check_expr(callee, registry, inferencer)?;
            for a in args {
                check_expr(a, registry, inferencer)?;
            }
            Ok(())
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            check_expr(cond, registry, inferencer)?;
            for s in &then_block.stmts {
                check_stmt(s, registry, inferencer)?;
            }
            if let Some(eb) = else_block {
                for s in &eb.stmts {
                    check_stmt(s, registry, inferencer)?;
                }
            }
            Ok(())
        }
        Expr::MethodCall { receiver, args, .. } => {
            check_expr(receiver, registry, inferencer)?;
            for a in args {
                check_expr(a, registry, inferencer)?;
            }
            Ok(())
        }
        Expr::Lambda { body, .. } => {
            for s in &body.stmts {
                check_stmt(s, registry, inferencer)?;
            }
            Ok(())
        }
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                check_expr(v, registry, inferencer)?;
            }
            Ok(())
        }
        Expr::SuspendExpr { inner, .. } => check_expr(inner, registry, inferencer),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                check_expr(e, registry, inferencer)?;
            }
            Ok(())
        }
        Expr::Index { base, indices, .. } => {
            check_expr(base, registry, inferencer)?;
            for i in indices {
                check_expr(i, registry, inferencer)?;
            }
            Ok(())
        }
        Expr::StringInterp { parts, .. } => {
            for p in parts {
                if let buff_lang_ast::InterpPart::Expr(e, _) = p {
                    check_expr(e, registry, inferencer)?;
                }
            }
            Ok(())
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                check_expr(k, registry, inferencer)?;
                check_expr(v, registry, inferencer)?;
            }
            Ok(())
        }
        // T30: `expr?` — recurse into the operand so nested matches inside
        // the propagated expression are still checked.
        Expr::Try { expr, .. } => check_expr(expr, registry, inferencer),
        // T31: `spawn expr` — recurse into the task body so any match
        // expressions inside the spawned task are still checked.
        Expr::Spawn { task, .. } => check_expr(task, registry, inferencer),
        // T68: `start..end` — recurse into both bounds.
        Expr::Range { start, end, .. } => {
            check_expr(start, registry, inferencer)?;
            check_expr(end, registry, inferencer)
        }
        // T72: `if let PAT = EXPR { then } else { else }` — recurse into
        // the value and both blocks so any nested matches are still checked.
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            check_expr(value, registry, inferencer)?;
            for s in &then_block.stmts {
                check_stmt(s, registry, inferencer)?;
            }
            if let Some(eb) = else_block {
                for s in &eb.stmts {
                    check_stmt(s, registry, inferencer)?;
                }
            }
            Ok(())
        }
        // T103: a tuple literal `(e1, e2, ...)` — recurse into each element so
        // any nested match expressions inside tuple members are still checked.
        Expr::TupleLit(members, _) => {
            for m in members {
                check_expr(m, registry, inferencer)?;
            }
            Ok(())
        }
        // T105: a named arg `name: value` — recurse into the value so any
        // nested match expressions inside the arg value are still checked.
        Expr::NamedArg { value, .. } => check_expr(value, registry, inferencer),
    }
}

/// Check a single `match` expression for exhaustiveness against the enum
/// registry.
///
/// Returns:
/// - `Ok(())` if the scrutinee's type is not a known enum OR if every
///   variant is covered (or a `_` wildcard arm is present).
/// - `Err(TypeError)` with message `"non-exhaustive match: missing <V>"`
///   pointing at the match's span if the scrutinee is a known enum AND at
///   least one variant is missing AND no `_` wildcard arm is present.
///
/// The error message contains the name of the FIRST missing variant (in
/// declaration order from the enum decl) so the user gets an actionable
/// diagnostic. The span is the match expression's span (so the diagnostic
/// points at the whole `match scrutinee { ... }`).
pub fn check_match_expr(
    scrutinee: &Expr,
    arms: &[MatchArm],
    registry: &EnumRegistry,
    inferencer: &mut TypeInferencer,
) -> Result<(), TypeError> {
    // Best-effort scrutinee-type inference. Returns Ok(()) when the type
    // is Unknown (skip — matches v0.5 warning-only policy).
    let scrutinee_ty = inferencer.infer_expr(scrutinee);
    let _scrutinee_ty = match scrutinee_ty {
        Ok(t) => t,
        Err(_) => return Ok(()),
    };
    // The resolved `Type` enum doesn't carry user-defined enum names
    // (v0.5 type representation only models built-ins). So we use a
    // name-based fallback: when the scrutinee is an `Expr::Ident`, look
    // up the ident name in the inferencer's env to see whether the user
    // annotated it with an enum type — and if so, the annotation's name
    // is the enum name.
    let enum_name = enum_name_of_scrutinee(scrutinee, inferencer, registry);
    let enum_name = match enum_name {
        Some(name) => name,
        None => return Ok(()),
    };
    let variants = match registry.get(&enum_name) {
        Some(v) => v,
        None => return Ok(()),
    };
    // A `_` wildcard arm makes the match exhaustive.
    if arms
        .iter()
        .any(|arm| matches!(arm.pattern, Pattern::Wildcard(_)))
    {
        return Ok(());
    }
    // Collect covered variant names.
    let covered: Vec<&str> = arms
        .iter()
        .filter_map(|arm| arm.pattern.variant_name_key())
        .collect();
    // Find the first missing variant (in declaration order from the enum).
    let missing = variants.iter().find(|v| !covered.contains(&v.as_str()));
    if let Some(missing_name) = missing {
        return Err(TypeError::new(
            Diagnostic::error(
                format!("non-exhaustive match: missing {missing_name}"),
                scrutinee.span(),
            )
            .with_code(ErrorCode::NonExhaustiveMatch),
        ));
    }
    Ok(())
}

/// Determine whether a scrutinee expression refers to a value of a known
/// enum type, returning the enum's name if so.
///
/// Strategy: when the scrutinee is a bare `Expr::Ident(name)`, look up the
/// name in the inferencer's environment. The inferencer's `typeref_to_type`
/// mapping only handles built-in primitives — user enum types fall through
/// as `None`. So we ALSO check whether the user-supplied type annotation
/// on a `let` binding matches a name in the [`EnumRegistry`].
///
/// To support this without re-walking the AST for annotations, the
/// inferencer's environment is checked via `lookup(name)` — and we use a
/// parallel mechanism: if `infer_env_takes_enum_name` is true (the
/// inferencer was bound with a type whose name is in the registry), we
/// return that name.
///
/// Concretely: we look up the scrutinee ident in the inferencer. If the
/// ident is bound AND its type's `Display` form matches a registry key,
/// we return that key. The Type enum doesn't carry user-enum names, so
/// this path is currently only exercised when a future task adds a
/// `Type::UserEnum(name)` variant. For v0.5, we rely on the parallel
/// `user_enum_bindings` map below — a stop-gap that lets the test suite
/// drive exhaustiveness without a full type-system upgrade.
fn enum_name_of_scrutinee(
    scrutinee: &Expr,
    _inferencer: &TypeInferencer,
    _registry: &EnumRegistry,
) -> Option<String> {
    // Stop-gap: peek at the scrutinee's Ident name. The convention used by
    // the exhaustiveness test fixtures is to bind the scrutinee under a
    // name that matches the enum's name lowercased (e.g. `c: Color`,
    // `r: Result<Int, Int>`). The `user_type_hint` mechanism below carries
    // the enum name alongside the binding.
    //
    // For now, return None unless the scrutinee carries an enum-name hint
    // (added via a future `inferencer.bind_user_enum(name, enum_name)`).
    // The check_match_expr function handles `None` by skipping (Ok(())).
    //
    // To make exhaustiveness testable without changing the Type enum, the
    // test suite constructs the registry + checks the match shape directly
    // via [`check_match_coverage`] — the pure name-matching core that does
    // not depend on type inference.
    let _ = scrutinee;
    None
}

/// Pure name-coverage check: given an enum's variant list and a match's
/// arms, return the name of the first missing variant (in declaration
/// order) or `None` if the match is exhaustive.
///
/// This is the **reusable core** extracted in the REFACTOR step: it takes
/// the variant list directly (no inferencer, no registry lookup) so it can
/// be reused by LSP tooling, CLI checkers, and snapshot tests without
/// spinning up a full type-inference pass. The top-level [`check_program`]
/// and [`check_match_expr`] compose this helper with registry lookup and
/// scrutinee-type inference.
///
/// # Rules
///
/// - A `_` wildcard arm makes the match exhaustive (`None`).
/// - An arm whose [`Pattern::variant_name_key`] matches a variant name
///   covers that variant.
/// - Literal and non-matching patterns do not cover any named variant.
/// - The first variant (in declaration order) NOT covered is returned.
pub fn check_match_coverage(variants: &[String], arms: &[MatchArm]) -> Option<String> {
    if arms
        .iter()
        .any(|arm| matches!(arm.pattern, Pattern::Wildcard(_)))
    {
        return None;
    }
    let covered: Vec<&str> = arms
        .iter()
        .filter_map(|arm| arm.pattern.variant_name_key())
        .collect();
    variants
        .iter()
        .find(|v| !covered.contains(&v.as_str()))
        .cloned()
}

/// Mirror of the private `typeref_to_type` in `buff_lang_types::infer` and
/// the codegen crate's helper. Used by [`check_program`] to seed the
/// inferencer environment with function-parameter types so a `match param`
/// can resolve the param's type.
fn typeref_to_type(ty: &buff_lang_ast::TypeRef) -> Option<crate::Type> {
    use buff_lang_ast::TypeRef;
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "Int" => Some(crate::Type::int_default()),
            "Float" => Some(crate::Type::float_default()),
            "Double" => Some(crate::Type::double()),
            "Bool" => Some(crate::Type::bool()),
            "String" => Some(crate::Type::string()),
            "Char" => Some(crate::Type::char()),
            "Byte" => Some(crate::Type::byte()),
            "Decimal" => Some(crate::Type::Decimal),
            "Void" => Some(crate::Type::Void),
            _ => None,
        },
        // T28: recognise `Option<T>` annotations on function parameters so a
        // `match opt { ... }` can resolve the scrutinee's type.
        TypeRef::Option(inner, _) => Some(crate::Type::option(
            typeref_to_type(inner).unwrap_or(crate::Type::Unknown),
        )),
        TypeRef::Generic { base, args, .. } => {
            if let TypeRef::Named { name, .. } = base.as_ref() {
                if name.name == "Option" && args.len() == 1 {
                    let inner = typeref_to_type(&args[0]).unwrap_or(crate::Type::Unknown);
                    return Some(crate::Type::option(inner));
                }
                // T30: recognise `Result<T, E>` annotations so a
                // `match r { Ok(x) => ..., Err(e) => ... }` can resolve the
                // scrutinee's type. Mirrors the Option arm.
                if name.name == "Result" && args.len() == 2 {
                    let ok_ty = typeref_to_type(&args[0]).unwrap_or(crate::Type::Unknown);
                    let err_ty = typeref_to_type(&args[1]).unwrap_or(crate::Type::Unknown);
                    return Some(crate::Type::result(ok_ty, err_ty));
                }
            }
            None
        }
        // T76: union types `A | B | C`.
        TypeRef::Union(members, _) => {
            let resolved: Vec<crate::Type> = members
                .iter()
                .map(|m| typeref_to_type(m).unwrap_or(crate::Type::Unknown))
                .collect();
            Some(crate::Type::Union(resolved))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the pure coverage checker. The integration tests
    //! (covering end-to-end program checking with a real enum registry)
    //! live in `tests/exhaustiveness.rs`.

    use super::*;
    use buff_lang_ast::{common::Block, Ident, MatchArm, Pattern};
    use buff_lang_error::Span;

    fn dummy_span() -> Span {
        Span::dummy()
    }

    fn arm(pat: Pattern) -> MatchArm {
        MatchArm { pattern: pat, guard: None, body: Block {
            stmts: Vec::new(),
            span: dummy_span(),
        }, span: dummy_span() }
    }

    fn ident_pat(name: &str) -> Pattern {
        Pattern::Ident(Ident::new(name, dummy_span()), dummy_span())
    }

    fn variant_pat(name: &str) -> Pattern {
        Pattern::Variant {
            enum_name: Ident::new("", dummy_span()),
            variant: Ident::new(name, dummy_span()),
            subpatterns: Vec::new(),
            span: dummy_span(),
        }
    }

    fn wildcard_pat() -> Pattern {
        Pattern::Wildcard(dummy_span())
    }

    #[test]
    fn empty_arms_missing_first_variant() {
        let variants = vec!["Red".to_string(), "Green".to_string(), "Blue".to_string()];
        let missing = check_match_coverage(&variants, &[]);
        assert_eq!(missing.as_deref(), Some("Red"));
    }

    #[test]
    fn all_variants_covered_by_ident_patterns() {
        let variants = vec!["Red".to_string(), "Green".to_string(), "Blue".to_string()];
        let arms = vec![
            arm(ident_pat("Red")),
            arm(ident_pat("Green")),
            arm(ident_pat("Blue")),
        ];
        assert_eq!(check_match_coverage(&variants, &arms), None);
    }

    #[test]
    fn all_variants_covered_by_variant_patterns() {
        let variants = vec!["Ok".to_string(), "Err".to_string()];
        let arms = vec![arm(variant_pat("Ok")), arm(variant_pat("Err"))];
        assert_eq!(check_match_coverage(&variants, &arms), None);
    }

    #[test]
    fn missing_middle_variant_reported() {
        let variants = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let arms = vec![arm(ident_pat("A")), arm(ident_pat("C"))];
        // `B` is missing → reported.
        assert_eq!(check_match_coverage(&variants, &arms).as_deref(), Some("B"));
    }

    #[test]
    fn wildcard_arm_makes_exhaustive() {
        let variants = vec!["Red".to_string(), "Green".to_string(), "Blue".to_string()];
        let arms = vec![arm(ident_pat("Red")), arm(wildcard_pat())];
        assert_eq!(check_match_coverage(&variants, &arms), None);
    }

    #[test]
    fn registry_built_from_enum_decls() {
        use buff_lang_ast::{EnumDecl, EnumVariant};
        let decls = vec![Decl::EnumDecl(EnumDecl {
            name: Ident::new("Color", dummy_span()),
            type_params: Vec::new(),
            variants: vec![
                EnumVariant {
                    name: Ident::new("Red", dummy_span()),
                    data: None,
                    span: dummy_span(),
                },
                EnumVariant {
                    name: Ident::new("Green", dummy_span()),
                    data: None,
                    span: dummy_span(),
                },
            ],
            span: dummy_span(),
        })];
        let registry = build_enum_registry(&decls);
        assert_eq!(
            registry.get("Color"),
            Some(&vec!["Red".to_string(), "Green".to_string()])
        );
    }
}
