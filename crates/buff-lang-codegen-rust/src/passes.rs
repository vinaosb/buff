//! T77/T78: compiler optimization passes (AST-level, before codegen).
//!
//! These passes walk the Buff AST and transform it to eliminate dead code
//! (T77) and propagate constants (T78). They are pure functions: they take
//! `&[Decl]` and return `Vec<Decl>`, never mutating the input. The passes
//! are applied via the caller chaining them before [`generate_rust`] so the
//! default path is unchanged (backwards compatibility — existing callers
//! and snapshot tests are unaffected).
//!
//! # Conservatism
//!
//! Both passes are deliberately conservative. They only transform code when
//! the transformation is provably safe:
//!
//! - **DCE** only removes `let` bindings whose value is a **pure literal**
//!   (no function/method calls, no I/O) and whose name is never referenced
//!   anywhere in the enclosing function body. It only removes functions that
//!   are **never called** AND not `main` AND not `@test`/`@bench` AND not
//!   exported.
//! - **Const-prop** only replaces a name with a literal when the binding is
//!   `let x = <literal>`, x is **never assigned to** (`x = ...` or `x += ...`),
//!   and every use of x is in a pure expression context.

use std::collections::{BTreeMap, BTreeSet};

use buff_lang_ast::{
    common::Block,
    expr::{Expr, Literal},
    stmt::Stmt,
    Decl, FuncDecl,
};

// ===========================================================================
// T77: Dead Code Elimination
// ===========================================================================

/// T77: Remove dead code from a slice of declarations.
///
/// See the module-level docs for what is and isn't removed.
pub fn dead_code_elimination(decls: &[Decl]) -> Vec<Decl> {
    let called_fns = collect_called_functions(decls);
    let exported_fns = collect_exported_fn_names(decls);

    decls
        .iter()
        .filter_map(|decl| {
            if let Some(func) = func_decl_from(decl) {
                if is_dead_function(func, &called_fns, &exported_fns) {
                    return None;
                }
            }
            Some(eliminate_dead_bindings_in_decl(decl))
        })
        .collect()
}

fn is_dead_function(
    func: &FuncDecl,
    called: &BTreeSet<String>,
    exported: &BTreeSet<String>,
) -> bool {
    if func.name.name == "main" {
        return false;
    }
    if exported.contains(&func.name.name) {
        return false;
    }
    if func.attributes.iter().any(|a| {
        matches!(
            a.name.name.as_str(),
            "test" | "bench" | "should_panic" | "ignore"
        )
    }) {
        return false;
    }
    if called.contains(&func.name.name) {
        return false;
    }
    if func.is_extern {
        return false;
    }
    true
}

fn func_decl_from(decl: &Decl) -> Option<&FuncDecl> {
    match decl {
        Decl::FuncDecl(f) => Some(f),
        Decl::ExportDecl(e) => match e.inner.as_ref() {
            Decl::FuncDecl(f) => Some(f),
            _ => None,
        },
        _ => None,
    }
}

fn collect_exported_fn_names(decls: &[Decl]) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for decl in decls {
        if let Decl::ExportDecl(e) = decl {
            if let Decl::FuncDecl(f) = e.inner.as_ref() {
                set.insert(f.name.name.clone());
            }
        }
    }
    set
}

fn collect_called_functions(decls: &[Decl]) -> BTreeSet<String> {
    let mut called = BTreeSet::new();
    for decl in decls {
        match decl {
            Decl::FuncDecl(f) => collect_called_in_block(&f.body, &mut called),
            Decl::ExportDecl(e) => {
                if let Decl::FuncDecl(f) = e.inner.as_ref() {
                    collect_called_in_block(&f.body, &mut called);
                }
            }
            Decl::ExtendBlock(ext) => {
                for method in &ext.methods {
                    collect_called_in_block(&method.body, &mut called);
                }
            }
            Decl::TraitDecl(t) => {
                for default in &t.defaults {
                    collect_called_in_block(&default.body, &mut called);
                }
            }
            _ => {}
        }
    }
    called
}

fn collect_called_in_block(block: &Block, called: &mut BTreeSet<String>) {
    for stmt in &block.stmts {
        collect_called_in_stmt(stmt, called);
    }
}

fn collect_called_in_stmt(stmt: &Stmt, called: &mut BTreeSet<String>) {
    match stmt {
        Stmt::LetDecl { value, .. } | Stmt::LetPattern { value, .. } => {
            collect_called_in_expr(value, called);
        }
        Stmt::Assignment { target, value, .. } => {
            collect_called_in_expr(target, called);
            collect_called_in_expr(value, called);
        }
        Stmt::ExprStmt(e, _) => collect_called_in_expr(e, called),
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                collect_called_in_expr(e, called);
            }
        }
        Stmt::ForIn { iter, body, .. } => {
            collect_called_in_expr(iter, called);
            collect_called_in_block(body, called);
        }
        Stmt::ForWhile { cond, body, .. } => {
            collect_called_in_expr(cond, called);
            collect_called_in_block(body, called);
        }
        Stmt::ForLet { value, body, .. } => {
            collect_called_in_expr(value, called);
            collect_called_in_block(body, called);
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for c in conditions {
                match c {
                    buff_lang_ast::GuardCondition::Let { value, .. } => {
                        collect_called_in_expr(value, called);
                    }
                    buff_lang_ast::GuardCondition::Bool(e) => {
                        collect_called_in_expr(e, called);
                    }
                }
            }
            collect_called_in_block(else_block, called);
        }
        Stmt::Defer { expr, .. } => collect_called_in_expr(expr, called),
        Stmt::ComptimeBlock { body, .. } => collect_called_in_block(body, called),
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

fn collect_called_in_expr(expr: &Expr, called: &mut BTreeSet<String>) {
    match expr {
        Expr::FuncCall { callee, args, .. } => {
            if let Expr::Ident(name, _) = callee.as_ref() {
                called.insert(name.name.clone());
            }
            collect_called_in_expr(callee, called);
            for a in args {
                collect_called_in_expr(a, called);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_called_in_expr(receiver, called);
            for a in args {
                collect_called_in_expr(a, called);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_called_in_expr(lhs, called);
            collect_called_in_expr(rhs, called);
        }
        Expr::UnaryOp { operand, .. } => collect_called_in_expr(operand, called),
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_called_in_expr(cond, called);
            collect_called_in_block(then_block, called);
            if let Some(eb) = else_block {
                collect_called_in_block(eb, called);
            }
        }
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_called_in_expr(v, called);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_called_in_expr(scrutinee, called);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_called_in_expr(g, called);
                }
                collect_called_in_block(&arm.body, called);
            }
        }
        Expr::Lambda { body, .. } => collect_called_in_block(body, called),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_called_in_expr(e, called);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_called_in_expr(base, called);
            for i in indices {
                collect_called_in_expr(i, called);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e, _) = part {
                    collect_called_in_expr(e, called);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_called_in_expr(k, called);
                collect_called_in_expr(v, called);
            }
        }
        Expr::Try { expr, .. } | Expr::SuspendExpr { inner: expr, .. } => {
            collect_called_in_expr(expr, called);
        }
        Expr::Spawn { task, .. } => collect_called_in_expr(task, called),
        Expr::Range { start, end, .. } => {
            collect_called_in_expr(start, called);
            collect_called_in_expr(end, called);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            collect_called_in_expr(value, called);
            collect_called_in_block(then_block, called);
            if let Some(eb) = else_block {
                collect_called_in_block(eb, called);
            }
        }
        Expr::TupleLit(elements, _) => {
            for e in elements {
                collect_called_in_expr(e, called);
            }
        }
        Expr::NamedArg { value, .. } => collect_called_in_expr(value, called),
        Expr::Ident(_, _) | Expr::Literal(_, _) => {}
    }
}

// --- Binding-level DCE (dead let removal) ---

fn eliminate_dead_bindings_in_decl(decl: &Decl) -> Decl {
    match decl {
        Decl::FuncDecl(f) => {
            let mut new_fn = f.clone();
            new_fn.body = eliminate_dead_bindings_in_block(&f.body);
            Decl::FuncDecl(new_fn)
        }
        Decl::ExportDecl(e) => {
            let inner = Box::new(eliminate_dead_bindings_in_decl(&e.inner));
            Decl::ExportDecl(buff_lang_ast::ExportDecl {
                inner,
                span: e.span,
            })
        }
        Decl::ExtendBlock(ext) => {
            let mut new_ext = ext.clone();
            for method in &mut new_ext.methods {
                method.body = eliminate_dead_bindings_in_block(&method.body);
            }
            Decl::ExtendBlock(new_ext)
        }
        Decl::TraitDecl(t) => {
            let mut new_t = t.clone();
            for default in &mut new_t.defaults {
                default.body = eliminate_dead_bindings_in_block(&default.body);
            }
            Decl::TraitDecl(new_t)
        }
        other => other.clone(),
    }
}

fn eliminate_dead_bindings_in_block(block: &Block) -> Block {
    let mut used_names = BTreeSet::new();
    collect_ident_names_in_block(block, &mut used_names);

    let new_stmts = block
        .stmts
        .iter()
        .filter(|s| !is_dead_let(s, &used_names))
        .map(|s| eliminate_dead_bindings_in_stmt(s))
        .collect();

    Block {
        stmts: new_stmts,
        span: block.span,
    }
}

fn is_dead_let(stmt: &Stmt, used_names: &BTreeSet<String>) -> bool {
    match stmt {
        Stmt::LetDecl { name, value, .. } => {
            !used_names.contains(&name.name) && expr_is_pure_literal(value)
        }
        _ => false,
    }
}

fn expr_is_pure_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Literal(_, _) | Expr::Ident(_, _) => true,
        Expr::BinaryOp { lhs, rhs, .. } => expr_is_pure_literal(lhs) && expr_is_pure_literal(rhs),
        Expr::UnaryOp { operand, .. } => expr_is_pure_literal(operand),
        Expr::ArrayLit { elements, .. } => elements.iter().all(expr_is_pure_literal),
        _ => false,
    }
}

fn eliminate_dead_bindings_in_stmt(stmt: &Stmt) -> Stmt {
    match stmt {
        Stmt::ForIn {
            var,
            iter,
            body,
            span,
        } => Stmt::ForIn {
            var: var.clone(),
            iter: iter.clone(),
            body: eliminate_dead_bindings_in_block(body),
            span: *span,
        },
        Stmt::ForWhile { cond, body, span } => Stmt::ForWhile {
            cond: cond.clone(),
            body: eliminate_dead_bindings_in_block(body),
            span: *span,
        },
        Stmt::ForLet {
            pattern,
            value,
            body,
            span,
        } => Stmt::ForLet {
            pattern: pattern.clone(),
            value: value.clone(),
            body: eliminate_dead_bindings_in_block(body),
            span: *span,
        },
        Stmt::Guard {
            conditions,
            else_block,
            span,
        } => Stmt::Guard {
            conditions: conditions.clone(),
            else_block: eliminate_dead_bindings_in_block(else_block),
            span: *span,
        },
        Stmt::ComptimeBlock { body, span } => Stmt::ComptimeBlock {
            body: eliminate_dead_bindings_in_block(body),
            span: *span,
        },
        other => other.clone(),
    }
}

// --- Identifier collection (for liveness) ---

fn collect_ident_names_in_block(block: &Block, names: &mut BTreeSet<String>) {
    for stmt in &block.stmts {
        collect_ident_names_in_stmt(stmt, names);
    }
}

fn collect_ident_names_in_stmt(stmt: &Stmt, names: &mut BTreeSet<String>) {
    match stmt {
        Stmt::LetDecl { value, .. } | Stmt::LetPattern { value, .. } => {
            collect_ident_names_in_expr(value, names);
        }
        Stmt::Assignment { target, value, .. } => {
            collect_ident_names_in_expr(target, names);
            collect_ident_names_in_expr(value, names);
        }
        Stmt::ExprStmt(e, _) => collect_ident_names_in_expr(e, names),
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                collect_ident_names_in_expr(e, names);
            }
        }
        Stmt::ForIn { iter, body, .. } => {
            collect_ident_names_in_expr(iter, names);
            collect_ident_names_in_block(body, names);
        }
        Stmt::ForWhile { cond, body, .. } => {
            collect_ident_names_in_expr(cond, names);
            collect_ident_names_in_block(body, names);
        }
        Stmt::ForLet { value, body, .. } => {
            collect_ident_names_in_expr(value, names);
            collect_ident_names_in_block(body, names);
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for c in conditions {
                match c {
                    buff_lang_ast::GuardCondition::Let { value, .. } => {
                        collect_ident_names_in_expr(value, names);
                    }
                    buff_lang_ast::GuardCondition::Bool(e) => {
                        collect_ident_names_in_expr(e, names);
                    }
                }
            }
            collect_ident_names_in_block(else_block, names);
        }
        Stmt::Defer { expr, .. } => collect_ident_names_in_expr(expr, names),
        Stmt::ComptimeBlock { body, .. } => collect_ident_names_in_block(body, names),
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

fn collect_ident_names_in_expr(expr: &Expr, names: &mut BTreeSet<String>) {
    match expr {
        Expr::Ident(name, _) => {
            names.insert(name.name.clone());
        }
        Expr::FuncCall { callee, args, .. } => {
            collect_ident_names_in_expr(callee, names);
            for a in args {
                collect_ident_names_in_expr(a, names);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_ident_names_in_expr(receiver, names);
            for a in args {
                collect_ident_names_in_expr(a, names);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_ident_names_in_expr(lhs, names);
            collect_ident_names_in_expr(rhs, names);
        }
        Expr::UnaryOp { operand, .. } => collect_ident_names_in_expr(operand, names),
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_ident_names_in_expr(cond, names);
            collect_ident_names_in_block(then_block, names);
            if let Some(eb) = else_block {
                collect_ident_names_in_block(eb, names);
            }
        }
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_ident_names_in_expr(v, names);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_ident_names_in_expr(scrutinee, names);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_ident_names_in_expr(g, names);
                }
                collect_ident_names_in_block(&arm.body, names);
            }
        }
        Expr::Lambda { body, .. } => collect_ident_names_in_block(body, names),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_ident_names_in_expr(e, names);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_ident_names_in_expr(base, names);
            for i in indices {
                collect_ident_names_in_expr(i, names);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e, _) = part {
                    collect_ident_names_in_expr(e, names);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_ident_names_in_expr(k, names);
                collect_ident_names_in_expr(v, names);
            }
        }
        Expr::Try { expr, .. } | Expr::SuspendExpr { inner: expr, .. } => {
            collect_ident_names_in_expr(expr, names);
        }
        Expr::Spawn { task, .. } => collect_ident_names_in_expr(task, names),
        Expr::Range { start, end, .. } => {
            collect_ident_names_in_expr(start, names);
            collect_ident_names_in_expr(end, names);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            collect_ident_names_in_expr(value, names);
            collect_ident_names_in_block(then_block, names);
            if let Some(eb) = else_block {
                collect_ident_names_in_block(eb, names);
            }
        }
        Expr::TupleLit(elements, _) => {
            for e in elements {
                collect_ident_names_in_expr(e, names);
            }
        }
        Expr::NamedArg { value, .. } => collect_ident_names_in_expr(value, names),
        Expr::Literal(_, _) => {}
    }
}

// ===========================================================================
// T78: Constant Propagation
// ===========================================================================

/// T78: Propagate constant values throughout the AST.
///
/// For each `let x = <literal>` where `x` is **never mutated** (`x = ...`) and
/// **never re-declared** (no second `let x` in any scope), replace every
/// `Expr::Ident(x)` reference with the literal value. This runs BEFORE DCE so
/// the propagated-away `let x = <literal>` binding can then be eliminated as
/// dead code.
///
/// # Conservatism
///
/// - Only **literal** values are propagated (`Literal::Int`, `Literal::Bool`,
///   `Literal::String`, etc.). Compound expressions (`x + 1`) are not folded.
/// - If a name appears in **multiple** `let` bindings (shadowing), it is NOT
///   propagated (avoids incorrect substitution across scopes).
/// - If a name appears as an **assignment target** (`x = ...` or `x += ...`),
///   it is NOT propagated (the value may change at runtime).
pub fn constant_propagation(decls: &[Decl]) -> Vec<Decl> {
    decls.iter().map(propagate_in_decl).collect()
}

fn propagate_in_decl(decl: &Decl) -> Decl {
    match decl {
        Decl::FuncDecl(f) => {
            let const_map = build_const_map(&f.body);
            let mut new_fn = f.clone();
            new_fn.body = propagate_in_block(&f.body, &const_map);
            Decl::FuncDecl(new_fn)
        }
        Decl::ExportDecl(e) => {
            let inner = Box::new(propagate_in_decl(&e.inner));
            Decl::ExportDecl(buff_lang_ast::ExportDecl {
                inner,
                span: e.span,
            })
        }
        Decl::ExtendBlock(ext) => {
            let mut new_ext = ext.clone();
            for method in &mut new_ext.methods {
                let map = build_const_map(&method.body);
                method.body = propagate_in_block(&method.body, &map);
            }
            Decl::ExtendBlock(new_ext)
        }
        Decl::TraitDecl(t) => {
            let mut new_t = t.clone();
            for default in &mut new_t.defaults {
                let map = build_const_map(&default.body);
                default.body = propagate_in_block(&default.body, &map);
            }
            Decl::TraitDecl(new_t)
        }
        other => other.clone(),
    }
}

/// Build a map of `name → Literal` for constant bindings that are safe to
/// propagate (never mutated, never re-declared).
fn build_const_map(block: &Block) -> BTreeMap<String, Literal> {
    let mut candidates: BTreeMap<String, Literal> = BTreeMap::new();
    let mut let_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut mutated: BTreeSet<String> = BTreeSet::new();

    collect_const_info_block(block, &mut candidates, &mut let_counts, &mut mutated);

    // Filter out names that are mutated or declared more than once.
    candidates.retain(|name, _| {
        let count = let_counts.get(name).copied().unwrap_or(0);
        count <= 1 && !mutated.contains(name)
    });

    candidates
}

fn collect_const_info_block(
    block: &Block,
    candidates: &mut BTreeMap<String, Literal>,
    let_counts: &mut BTreeMap<String, usize>,
    mutated: &mut BTreeSet<String>,
) {
    for stmt in &block.stmts {
        collect_const_info_stmt(stmt, candidates, let_counts, mutated);
    }
}

fn collect_const_info_stmt(
    stmt: &Stmt,
    candidates: &mut BTreeMap<String, Literal>,
    let_counts: &mut BTreeMap<String, usize>,
    mutated: &mut BTreeSet<String>,
) {
    match stmt {
        Stmt::LetDecl { name, value, .. } => {
            *let_counts.entry(name.name.clone()).or_insert(0) += 1;
            if let Expr::Literal(lit, _) = value {
                candidates.insert(name.name.clone(), lit.clone());
            } else {
                // Non-literal value → not a const candidate.
                candidates.remove(&name.name);
            }
            collect_const_info_expr(value, candidates, let_counts, mutated);
        }
        Stmt::LetPattern { value, .. } => {
            collect_const_info_expr(value, candidates, let_counts, mutated);
        }
        Stmt::Assignment { target, value, .. } => {
            if let Expr::Ident(name, _) = target {
                mutated.insert(name.name.clone());
            }
            collect_const_info_expr(target, candidates, let_counts, mutated);
            collect_const_info_expr(value, candidates, let_counts, mutated);
        }
        Stmt::ExprStmt(e, _) => collect_const_info_expr(e, candidates, let_counts, mutated),
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                collect_const_info_expr(e, candidates, let_counts, mutated);
            }
        }
        Stmt::ForIn { iter, body, .. } => {
            collect_const_info_expr(iter, candidates, let_counts, mutated);
            collect_const_info_block(body, candidates, let_counts, mutated);
        }
        Stmt::ForWhile { cond, body, .. } => {
            collect_const_info_expr(cond, candidates, let_counts, mutated);
            collect_const_info_block(body, candidates, let_counts, mutated);
        }
        Stmt::ForLet { value, body, .. } => {
            collect_const_info_expr(value, candidates, let_counts, mutated);
            collect_const_info_block(body, candidates, let_counts, mutated);
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for c in conditions {
                match c {
                    buff_lang_ast::GuardCondition::Let { value, .. } => {
                        collect_const_info_expr(value, candidates, let_counts, mutated);
                    }
                    buff_lang_ast::GuardCondition::Bool(e) => {
                        collect_const_info_expr(e, candidates, let_counts, mutated);
                    }
                }
            }
            collect_const_info_block(else_block, candidates, let_counts, mutated);
        }
        Stmt::Defer { expr, .. } => {
            collect_const_info_expr(expr, candidates, let_counts, mutated);
        }
        Stmt::ComptimeBlock { body, .. } => {
            collect_const_info_block(body, candidates, let_counts, mutated);
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

#[allow(clippy::only_used_in_recursion)]
fn collect_const_info_expr(
    expr: &Expr,
    candidates: &mut BTreeMap<String, Literal>,
    let_counts: &mut BTreeMap<String, usize>,
    mutated: &mut BTreeSet<String>,
) {
    match expr {
        Expr::FuncCall { callee, args, .. } => {
            collect_const_info_expr(callee, candidates, let_counts, mutated);
            for a in args {
                collect_const_info_expr(a, candidates, let_counts, mutated);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_const_info_expr(receiver, candidates, let_counts, mutated);
            for a in args {
                collect_const_info_expr(a, candidates, let_counts, mutated);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_const_info_expr(lhs, candidates, let_counts, mutated);
            collect_const_info_expr(rhs, candidates, let_counts, mutated);
        }
        Expr::UnaryOp { operand, .. } => {
            collect_const_info_expr(operand, candidates, let_counts, mutated);
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_const_info_expr(cond, candidates, let_counts, mutated);
            collect_const_info_block(then_block, candidates, let_counts, mutated);
            if let Some(eb) = else_block {
                collect_const_info_block(eb, candidates, let_counts, mutated);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_const_info_expr(scrutinee, candidates, let_counts, mutated);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_const_info_expr(g, candidates, let_counts, mutated);
                }
                collect_const_info_block(&arm.body, candidates, let_counts, mutated);
            }
        }
        Expr::Lambda { body, .. } => {
            collect_const_info_block(body, candidates, let_counts, mutated);
        }
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_const_info_expr(e, candidates, let_counts, mutated);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_const_info_expr(base, candidates, let_counts, mutated);
            for i in indices {
                collect_const_info_expr(i, candidates, let_counts, mutated);
            }
        }
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_const_info_expr(v, candidates, let_counts, mutated);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e, _) = part {
                    collect_const_info_expr(e, candidates, let_counts, mutated);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_const_info_expr(k, candidates, let_counts, mutated);
                collect_const_info_expr(v, candidates, let_counts, mutated);
            }
        }
        Expr::Try { expr, .. } | Expr::SuspendExpr { inner: expr, .. } => {
            collect_const_info_expr(expr, candidates, let_counts, mutated);
        }
        Expr::Spawn { task, .. } => {
            collect_const_info_expr(task, candidates, let_counts, mutated);
        }
        Expr::Range { start, end, .. } => {
            collect_const_info_expr(start, candidates, let_counts, mutated);
            collect_const_info_expr(end, candidates, let_counts, mutated);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            collect_const_info_expr(value, candidates, let_counts, mutated);
            collect_const_info_block(then_block, candidates, let_counts, mutated);
            if let Some(eb) = else_block {
                collect_const_info_block(eb, candidates, let_counts, mutated);
            }
        }
        Expr::TupleLit(elements, _) => {
            for e in elements {
                collect_const_info_expr(e, candidates, let_counts, mutated);
            }
        }
        Expr::NamedArg { value, .. } => {
            collect_const_info_expr(value, candidates, let_counts, mutated);
        }
        Expr::Ident(_, _) | Expr::Literal(_, _) => {}
    }
}

// --- Propagation (replace Expr::Ident with the literal) ---

fn propagate_in_block(block: &Block, map: &BTreeMap<String, Literal>) -> Block {
    let stmts = block
        .stmts
        .iter()
        .map(|s| propagate_in_stmt(s, map))
        .collect();
    Block {
        stmts,
        span: block.span,
    }
}

fn propagate_in_stmt(stmt: &Stmt, map: &BTreeMap<String, Literal>) -> Stmt {
    match stmt {
        Stmt::LetDecl {
            name,
            value,
            mutable,
            ty,
            span,
        } => Stmt::LetDecl {
            name: name.clone(),
            value: propagate_in_expr(value, map),
            mutable: *mutable,
            ty: ty.clone(),
            span: *span,
        },
        Stmt::LetPattern {
            pattern,
            value,
            mutable,
            ty,
            span,
        } => Stmt::LetPattern {
            pattern: pattern.clone(),
            value: propagate_in_expr(value, map),
            mutable: *mutable,
            ty: ty.clone(),
            span: *span,
        },
        Stmt::Assignment {
            target,
            op,
            value,
            span,
        } => Stmt::Assignment {
            target: propagate_in_expr(target, map),
            op: *op,
            value: propagate_in_expr(value, map),
            span: *span,
        },
        Stmt::ExprStmt(e, span) => Stmt::ExprStmt(propagate_in_expr(e, map), *span),
        Stmt::Return(e, span) => Stmt::Return(e.as_ref().map(|e| propagate_in_expr(e, map)), *span),
        Stmt::ForIn {
            var,
            iter,
            body,
            span,
        } => Stmt::ForIn {
            var: var.clone(),
            iter: propagate_in_expr(iter, map),
            body: propagate_in_block(body, map),
            span: *span,
        },
        Stmt::ForWhile { cond, body, span } => Stmt::ForWhile {
            cond: propagate_in_expr(cond, map),
            body: propagate_in_block(body, map),
            span: *span,
        },
        Stmt::ForLet {
            pattern,
            value,
            body,
            span,
        } => Stmt::ForLet {
            pattern: pattern.clone(),
            value: propagate_in_expr(value, map),
            body: propagate_in_block(body, map),
            span: *span,
        },
        Stmt::Guard {
            conditions,
            else_block,
            span,
        } => Stmt::Guard {
            conditions: conditions
                .iter()
                .map(|c| match c {
                    buff_lang_ast::GuardCondition::Let {
                        pattern,
                        value,
                        span,
                    } => buff_lang_ast::GuardCondition::Let {
                        pattern: pattern.clone(),
                        value: propagate_in_expr(value, map),
                        span: *span,
                    },
                    buff_lang_ast::GuardCondition::Bool(e) => {
                        buff_lang_ast::GuardCondition::Bool(propagate_in_expr(e, map))
                    }
                })
                .collect(),
            else_block: propagate_in_block(else_block, map),
            span: *span,
        },
        Stmt::Defer { expr, span } => Stmt::Defer {
            expr: propagate_in_expr(expr, map),
            span: *span,
        },
        Stmt::ComptimeBlock { body, span } => Stmt::ComptimeBlock {
            body: propagate_in_block(body, map),
            span: *span,
        },
        other => other.clone(),
    }
}

fn propagate_in_expr(expr: &Expr, map: &BTreeMap<String, Literal>) -> Expr {
    match expr {
        Expr::Ident(name, span) => {
            if let Some(lit) = map.get(&name.name) {
                Expr::Literal(lit.clone(), *span)
            } else {
                expr.clone()
            }
        }
        Expr::FuncCall { callee, args, span } => Expr::FuncCall {
            callee: Box::new(propagate_in_expr(callee, map)),
            args: args.iter().map(|a| propagate_in_expr(a, map)).collect(),
            span: *span,
        },
        Expr::MethodCall {
            receiver,
            method,
            args,
            span,
        } => Expr::MethodCall {
            receiver: Box::new(propagate_in_expr(receiver, map)),
            method: method.clone(),
            args: args.iter().map(|a| propagate_in_expr(a, map)).collect(),
            span: *span,
        },
        Expr::BinaryOp { op, lhs, rhs, span } => Expr::BinaryOp {
            op: *op,
            lhs: Box::new(propagate_in_expr(lhs, map)),
            rhs: Box::new(propagate_in_expr(rhs, map)),
            span: *span,
        },
        Expr::UnaryOp { op, operand, span } => Expr::UnaryOp {
            op: *op,
            operand: Box::new(propagate_in_expr(operand, map)),
            span: *span,
        },
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            span,
        } => Expr::IfExpr {
            cond: Box::new(propagate_in_expr(cond, map)),
            then_block: propagate_in_block(then_block, map),
            else_block: else_block.as_ref().map(|b| propagate_in_block(b, map)),
            span: *span,
        },
        Expr::StructInit {
            type_name,
            fields,
            span,
        } => Expr::StructInit {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(n, v)| (n.clone(), propagate_in_expr(v, map)))
                .collect(),
            span: *span,
        },
        Expr::MatchExpr {
            scrutinee,
            arms,
            span,
        } => Expr::MatchExpr {
            scrutinee: Box::new(propagate_in_expr(scrutinee, map)),
            arms: arms
                .iter()
                .map(|arm| buff_lang_ast::MatchArm {
                    pattern: arm.pattern.clone(),
                    guard: arm.guard.as_ref().map(|g| propagate_in_expr(g, map)),
                    body: propagate_in_block(&arm.body, map),
                    span: arm.span,
                })
                .collect(),
            span: *span,
        },
        Expr::Lambda {
            params,
            body,
            return_type,
            span,
        } => Expr::Lambda {
            params: params.clone(),
            body: propagate_in_block(body, map),
            return_type: return_type.clone(),
            span: *span,
        },
        Expr::ArrayLit { elements, span } => Expr::ArrayLit {
            elements: elements.iter().map(|e| propagate_in_expr(e, map)).collect(),
            span: *span,
        },
        Expr::Index {
            base,
            indices,
            span,
        } => Expr::Index {
            base: Box::new(propagate_in_expr(base, map)),
            indices: indices.iter().map(|i| propagate_in_expr(i, map)).collect(),
            span: *span,
        },
        Expr::StringInterp { parts, span } => Expr::StringInterp {
            parts: parts
                .iter()
                .map(|p| match p {
                    buff_lang_ast::InterpPart::Literal(t) => {
                        buff_lang_ast::InterpPart::Literal(t.clone())
                    }
                    buff_lang_ast::InterpPart::Expr(e, spec) => buff_lang_ast::InterpPart::Expr(
                        Box::new(propagate_in_expr(e, map)),
                        spec.clone(),
                    ),
                })
                .collect(),
            span: *span,
        },
        Expr::MapLit { entries, span } => Expr::MapLit {
            entries: entries
                .iter()
                .map(|(k, v)| (propagate_in_expr(k, map), propagate_in_expr(v, map)))
                .collect(),
            span: *span,
        },
        Expr::Try { expr, span } => Expr::Try {
            expr: Box::new(propagate_in_expr(expr, map)),
            span: *span,
        },
        Expr::SuspendExpr { inner, span } => Expr::SuspendExpr {
            inner: Box::new(propagate_in_expr(inner, map)),
            span: *span,
        },
        Expr::Spawn { task, span } => Expr::Spawn {
            task: Box::new(propagate_in_expr(task, map)),
            span: *span,
        },
        Expr::Range {
            start,
            end,
            inclusive,
            span,
        } => Expr::Range {
            start: Box::new(propagate_in_expr(start, map)),
            end: Box::new(propagate_in_expr(end, map)),
            inclusive: *inclusive,
            span: *span,
        },
        Expr::IfLet {
            pattern,
            value,
            then_block,
            else_block,
            span,
        } => Expr::IfLet {
            pattern: pattern.clone(),
            value: Box::new(propagate_in_expr(value, map)),
            then_block: propagate_in_block(then_block, map),
            else_block: else_block.as_ref().map(|b| propagate_in_block(b, map)),
            span: *span,
        },
        Expr::TupleLit(elements, span) => Expr::TupleLit(
            elements.iter().map(|e| propagate_in_expr(e, map)).collect(),
            *span,
        ),
        Expr::NamedArg { name, value, span } => Expr::NamedArg {
            name: name.clone(),
            value: Box::new(propagate_in_expr(value, map)),
            span: *span,
        },
        Expr::Literal(_, _) => expr.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::common::Ident;
    use buff_lang_error::Span;

    fn let_decl(name: &str, value: Expr) -> Stmt {
        Stmt::LetDecl {
            name: Ident::new(name, Span::dummy()),
            value,
            mutable: false,
            ty: None,
            span: Span::dummy(),
        }
    }

    fn int_lit(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n), Span::dummy())
    }

    fn ident(name: &str) -> Expr {
        Expr::Ident(Ident::new(name, Span::dummy()), Span::dummy())
    }

    fn empty_block() -> Block {
        Block::empty(Span::dummy())
    }

    fn func(name: &str, body: Block) -> Decl {
        Decl::FuncDecl(FuncDecl {
            name: Ident::new(name, Span::dummy()),
            params: Vec::new(),
            return_type: None,
            body,
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            type_params: Vec::new(),
            span: Span::dummy(),
        })
    }

    #[test]
    fn dce_removes_unused_function() {
        let decls = vec![
            func(
                "main",
                Block {
                    stmts: vec![Stmt::ExprStmt(
                        Expr::FuncCall {
                            callee: Box::new(ident("helper")),
                            args: Vec::new(),
                            span: Span::dummy(),
                        },
                        Span::dummy(),
                    )],
                    span: Span::dummy(),
                },
            ),
            func("dead", empty_block()),
        ];
        let result = dead_code_elimination(&decls);
        let names: Vec<&str> = result
            .iter()
            .filter_map(|d| match d {
                Decl::FuncDecl(f) => Some(f.name.name.as_str()),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"main"), "main kept");
        assert!(!names.contains(&"dead"), "dead function removed: {names:?}");
    }

    #[test]
    fn dce_keeps_main() {
        let decls = vec![func("main", empty_block())];
        let result = dead_code_elimination(&decls);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn dce_removes_unused_let_binding() {
        let body = Block {
            stmts: vec![
                let_decl("unused", int_lit(42)),
                let_decl("used", int_lit(10)),
                Stmt::ExprStmt(ident("used"), Span::dummy()),
            ],
            span: Span::dummy(),
        };
        let decls = vec![func("main", body)];
        let result = dead_code_elimination(&decls);
        match &result[0] {
            Decl::FuncDecl(f) => {
                let names: Vec<&str> = f
                    .body
                    .stmts
                    .iter()
                    .filter_map(|s| match s {
                        Stmt::LetDecl { name, .. } => Some(name.name.as_str()),
                        _ => None,
                    })
                    .collect();
                assert!(!names.contains(&"unused"), "unused let removed: {names:?}");
                assert!(names.contains(&"used"), "used let kept: {names:?}");
            }
            _ => panic!("expected FuncDecl"),
        }
    }

    #[test]
    fn dce_keeps_let_with_side_effects() {
        let body = Block {
            stmts: vec![
                let_decl(
                    "x",
                    Expr::FuncCall {
                        callee: Box::new(ident("print")),
                        args: vec![Expr::Literal(Literal::String("hi".into()), Span::dummy())],
                        span: Span::dummy(),
                    },
                ),
                Stmt::ExprStmt(ident("other"), Span::dummy()),
            ],
            span: Span::dummy(),
        };
        let decls = vec![func("main", body)];
        let result = dead_code_elimination(&decls);
        match &result[0] {
            Decl::FuncDecl(f) => {
                assert_eq!(f.body.stmts.len(), 2, "side-effectful let NOT removed");
            }
            _ => panic!("expected FuncDecl"),
        }
    }

    #[test]
    fn dce_does_not_remove_called_function() {
        let decls = vec![
            func(
                "main",
                Block {
                    stmts: vec![Stmt::ExprStmt(
                        Expr::FuncCall {
                            callee: Box::new(ident("helper")),
                            args: Vec::new(),
                            span: Span::dummy(),
                        },
                        Span::dummy(),
                    )],
                    span: Span::dummy(),
                },
            ),
            func("helper", empty_block()),
        ];
        let result = dead_code_elimination(&decls);
        assert_eq!(result.len(), 2, "called function kept");
    }
}
