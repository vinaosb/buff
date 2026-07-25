//! T105a - extern-crate emit-on-demand detection (program_uses_* AST walkers, part 1) (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move - no logic changes. Child module of rust_codegen so it
//! inherits the parent imports via use super::* (zero per-module import lists).
//! Functions are pub(super) so the parent reaches them through the glob below.

use super::*;

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
pub(super) fn program_uses_matrix(decls: &[Decl]) -> bool {
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
pub(super) fn block_uses_matrix(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_matrix)
}

/// Check a single statement (and its nested expressions) for Matrix.new.
pub(super) fn stmt_uses_matrix(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_matrix(value),
        Stmt::Assignment { target, value, .. } => {
            expr_uses_matrix(target) || expr_uses_matrix(value)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_matrix(iter) || block_uses_matrix(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_matrix(cond) || block_uses_matrix(body),
        // T72: `for let PAT = EXPR { body }` — value + body may use Matrix.
        Stmt::ForLet { value, body, .. } => expr_uses_matrix(value) || block_uses_matrix(body),
        // T73: `guard <conds> else { block }` — conditions + else may use
        // Matrix (any `Matrix.new` in a condition or the else-block triggers
        // emit-on-demand). Let-value and Bool-expr both count.
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_matrix(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_matrix(e),
            }) || block_uses_matrix(else_block)
        }
        // T100: `defer EXPR` — the deferred expression may use Matrix.
        Stmt::Defer { expr, .. } => expr_uses_matrix(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_matrix(body),
    }
}

/// Recursively scan an expression tree for a `Matrix.new(...)` MethodCall.
pub(super) fn expr_uses_matrix(expr: &Expr) -> bool {
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
            InterpPart::Expr(e, _) => expr_uses_matrix(e),
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
        // T72: `if let PAT = EXPR { then } else { else }` — recurse into
        // value + both blocks (pattern carries no Matrix construction).
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_matrix(value)
                || block_uses_matrix(then_block)
                || else_block.as_ref().is_some_and(block_uses_matrix)
        }
        // T103: `(e1, e2, ...)` — recurse into each element.
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_matrix),
        // T105: a named arg `name: value` — recurse into the value.
        Expr::NamedArg { value, .. } => expr_uses_matrix(value),
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
pub(super) fn matrix_struct_items() -> Vec<Item> {
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
pub(super) fn program_uses_error(decls: &[Decl]) -> bool {
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
pub(super) fn block_uses_error(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_error)
}

/// Check a single statement (and its nested expressions) for `Error(...)`.
pub(super) fn stmt_uses_error(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_error(value),
        Stmt::Assignment { target, value, .. } => expr_uses_error(target) || expr_uses_error(value),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_error(iter) || block_uses_error(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_error(cond) || block_uses_error(body),
        // T72: `for let PAT = EXPR { body }` — value + body may use Error.
        Stmt::ForLet { value, body, .. } => expr_uses_error(value) || block_uses_error(body),
        // T73: `guard <conds> else { block }` — conditions + else may use
        // Error (any `Error(...)` in a condition or the else-block triggers
        // emit-on-demand of the builtin Error struct).
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_error(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_error(e),
            }) || block_uses_error(else_block)
        }
        // T100: `defer EXPR` — the deferred expression may use Error.
        Stmt::Defer { expr, .. } => expr_uses_error(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_error(body),
    }
}

/// Recursively scan an expression tree for an `Error(...)` constructor call.
pub(super) fn expr_uses_error(expr: &Expr) -> bool {
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
            InterpPart::Expr(e, _) => expr_uses_error(e),
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
        // T72: `if let PAT = EXPR { then } else { else }` — recurse into
        // value + both blocks (pattern carries no Error construction).
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_error(value)
                || block_uses_error(then_block)
                || else_block.as_ref().is_some_and(block_uses_error)
        }
        // T103: `(e1, e2, ...)` — recurse into each element.
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_error),
        // T105: a named arg `name: value` — recurse into the value.
        Expr::NamedArg { value, .. } => expr_uses_error(value),
    }
}

// ---------------------------------------------------------------------------
// T124b — chrono / std::time emit-on-demand detection (prelude-types).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any reference to a prelude
/// datetime type (T124b). Returns `true` if at least one is found,
/// signalling [`RustCodegen::generate`] to record `"chrono"` in the
/// extern-crate set so the pipeline knows the generated Cargo project
/// depends on `chrono`.
///
/// Detection recognises BOTH:
/// - **Associated-function calls**: `DateTime.now()`, `Duration.days(7)`,
///   `Instant.now()`, `Date.today()`, etc. (receiver is a bare Ident
///   naming a prelude type).
/// - **Instance-method calls**: `dt.format(...)`, `dt.year()`, etc. The
///   receiver is NOT a bare type name (it's a value), so we conservatively
///   detect ANY call to the prelude instance-method names — false positives
///   are tolerable (they just trigger chrono registration, which is a
///   no-op if the program doesn't actually use chrono).
///
/// Source-level type annotations (`let dt: DateTime = ...`) are NOT
/// detected by this walker; they're handled by the codegen pass directly
/// via [`buff_lang_types::is_prelude_type`] when the annotation is
/// resolved. The two paths together cover every realistic chrono use.
pub(super) fn program_uses_chrono(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_chrono(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_chrono`]: scan a block's statements.
pub(super) fn block_uses_chrono(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_chrono)
}

/// Check a single statement (and its nested expressions) for chrono usage.
pub(super) fn stmt_uses_chrono(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl {
            value,
            ty: Some(ty),
            ..
        }
        | Stmt::LetPattern {
            value,
            ty: Some(ty),
            ..
        } => {
            // Source-level type annotation names a prelude type
            // (e.g. `let dt: DateTime = ...`). This counts even if the
            // value expression doesn't itself mention chrono.
            type_ref_names_prelude_type(ty) || expr_uses_chrono(value)
        }
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_chrono(value),
        Stmt::Assignment { target, value, .. } => {
            expr_uses_chrono(target) || expr_uses_chrono(value)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_chrono(iter) || block_uses_chrono(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_chrono(cond) || block_uses_chrono(body),
        Stmt::ForLet { value, body, .. } => expr_uses_chrono(value) || block_uses_chrono(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_chrono(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_chrono(e),
            }) || block_uses_chrono(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_chrono(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_chrono(body),
    }
}

/// Returns `true` iff `ty` (or any nested inner TypeRef) mentions a prelude
/// datetime type name. Used by [`stmt_uses_chrono`] to detect source-level
/// type annotations like `let dt: DateTime = ...`.
pub(super) fn type_ref_names_prelude_type(ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Named { name, .. } => buff_lang_types::is_prelude_type(&name.name),
        TypeRef::Option(inner, _) => type_ref_names_prelude_type(inner),
        TypeRef::Generic { base, args, .. } => {
            type_ref_names_prelude_type(base) || args.iter().any(type_ref_names_prelude_type)
        }
        _ => false,
    }
}

/// Recursively scan an expression tree for any prelude-type usage.
///
/// Detection is conservative: it triggers on any `Type.<assoc_fn>()`
/// shape whose receiver is a bare Ident naming a prelude type, OR on any
/// instance-method call whose method name is a recognised prelude
/// instance method (the receiver's inferred type is then checked at
/// codegen time by `lower_method_call`).
pub(super) fn expr_uses_chrono(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            // Associated-function call: `DateTime.now()`, `Duration.days(7)`, etc.
            // T124f: narrow to the datetime FAMILY only (DateTime / Date /
            // Time / Duration / Instant). The previous `is_prelude_type`
            // check was too broad - it flagged every prelude-type Ident
            // receiver, which after T124c/T124d/T124e/T124f includes
            // Log / Regex / Toml / Math / Random / Strings (none of which
            // lower to chrono). The `buff_type().is_prelude_datetime()`
            // round-trip captures exactly the 5 chrono types.
            if let Expr::Ident(id, _) = receiver.as_ref() {
                if let Some(ptype) = buff_lang_types::prelude_type_lookup(&id.name) {
                    if ptype.buff_type().is_prelude_datetime() {
                        return true;
                    }
                }
            }
            // Instance-method call: `dt.format(...)`, `dt.year()`, etc.
            // Conservative on the method NAME — the receiver's type is
            // resolved at codegen time, so we err on the side of "register
            // chrono" if the method name matches any prelude instance fn.
            if buff_lang_types::PreludeInstanceFn::ALL
                .iter()
                .any(|f| f.name() == method.name.as_str())
            {
                return true;
            }
            expr_uses_chrono(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_chrono(lhs) || expr_uses_chrono(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_chrono(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_chrono(callee) || args.iter().any(expr_uses_chrono)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_chrono(cond)
                || block_uses_chrono(then_block)
                || else_block.as_ref().is_some_and(block_uses_chrono)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_chrono(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_chrono),
        Expr::Index { base, indices, .. } => {
            expr_uses_chrono(base) || indices.iter().any(expr_uses_chrono)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_chrono(k) || expr_uses_chrono(v)),
        Expr::Lambda { body, .. } => block_uses_chrono(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_chrono(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_chrono(scrutinee) || arms.iter().any(|arm| block_uses_chrono(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_chrono(inner),
        Expr::Try { expr, .. } => expr_uses_chrono(expr),
        Expr::Spawn { task, .. } => expr_uses_chrono(task),
        Expr::Range { start, end, .. } => expr_uses_chrono(start) || expr_uses_chrono(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_chrono(value)
                || block_uses_chrono(then_block)
                || else_block.as_ref().is_some_and(block_uses_chrono)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_chrono),
        Expr::NamedArg { value, .. } => expr_uses_chrono(value),
    }
}

// ---------------------------------------------------------------------------
// T124c — tracing / tracing-subscriber emit-on-demand detection (Log module).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any `Log.<level>(...)` call
/// (T124c). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to:
/// 1. record `"tracing"` + `"tracing-subscriber"` in the extern-crate set
///    so the pipeline knows the generated Cargo project depends on both
///    crates;
/// 2. emit a `tracing_subscriber::fmt()...try_init()` statement at the top
///    of `main` so the program's log output is formatted (pretty in dev,
///    JSON in release) and level-filtered via the `BUFF_LOG` env var.
///
/// Detection recognises the `Log` namespace as the receiver of a method
/// call (`Log.info(...)`, `Log.error(...)`, ...). The method name is NOT
/// matched here — `Log` is a reserved prelude namespace, so any
/// `Log.<anything>()` triggers tracing registration. Codegen will surface
/// a clear error if `<anything>` is not one of debug/info/warn/error.
///
/// Mirrors the chrono detection pattern (T124b); the recursive walker
/// covers every Stmt / Expr variant that could host a `Log` call.
pub(super) fn program_uses_tracing(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_tracing(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_tracing`]: scan a block's statements.
pub(super) fn block_uses_tracing(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_tracing)
}

/// Check a single statement (and its nested expressions) for `Log.*(...)` usage.
pub(super) fn stmt_uses_tracing(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_tracing(value),
        Stmt::Assignment { target, value, .. } => {
            expr_uses_tracing(target) || expr_uses_tracing(value)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_tracing(iter) || block_uses_tracing(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_tracing(cond) || block_uses_tracing(body),
        Stmt::ForLet { value, body, .. } => expr_uses_tracing(value) || block_uses_tracing(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_tracing(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_tracing(e),
            }) || block_uses_tracing(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_tracing(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_tracing(body),
    }
}

/// Recursively scan an expression tree for a `Log.<method>(...)` call.
///
/// Detection is on the receiver NAME (`Log`) only — the method name is
/// validated at codegen time. This means a hypothetical user-defined
/// variable named `Log` whose method is called would trigger a false
/// positive (registering tracing unnecessarily); but since `Log` is a
/// reserved prelude namespace, the user can't legitimately bind to that
/// name anyway (shadowing it is the documented head-gun pattern from
/// the T124b registry).
pub(super) fn expr_uses_tracing(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            if let Expr::Ident(id, _) = receiver.as_ref() {
                if id.name == "Log" {
                    return true;
                }
            }
            // Conservatively flag any call whose method name matches a Log
            // level — same conservative strategy T124b uses for chrono
            // instance-method detection. The codegen arm will then either
            // emit a Log lowering or surface a clear error.
            if matches!(method.name.as_str(), "debug" | "info" | "warn" | "error") {
                // Only flag if the receiver could plausibly be Log (bare
                // Ident). We already covered `Log` above; other receivers
                // (values, calls) might be user methods that happen to
                // share the name — those should NOT trigger tracing
                // registration. So this branch is a no-op; we leave the
                // method-name check in place as documentation of the
                // design decision.
            }
            expr_uses_tracing(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_tracing(lhs) || expr_uses_tracing(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_tracing(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_tracing(callee) || args.iter().any(expr_uses_tracing)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_tracing(cond)
                || block_uses_tracing(then_block)
                || else_block.as_ref().is_some_and(block_uses_tracing)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_tracing(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_tracing),
        Expr::Index { base, indices, .. } => {
            expr_uses_tracing(base) || indices.iter().any(expr_uses_tracing)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_tracing(k) || expr_uses_tracing(v)),
        Expr::Lambda { body, .. } => block_uses_tracing(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_tracing(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_tracing(scrutinee) || arms.iter().any(|arm| block_uses_tracing(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_tracing(inner),
        Expr::Try { expr, .. } => expr_uses_tracing(expr),
        Expr::Spawn { task, .. } => expr_uses_tracing(task),
        Expr::Range { start, end, .. } => expr_uses_tracing(start) || expr_uses_tracing(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_tracing(value)
                || block_uses_tracing(then_block)
                || else_block.as_ref().is_some_and(block_uses_tracing)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_tracing),
        Expr::NamedArg { value, .. } => expr_uses_tracing(value),
    }
}

/// T124c: build the `tracing_subscriber` init statement emitted at the top
/// of `main` when the program uses the `Log` module.
///
/// Emits (conceptually):
///
/// ```rust,ignore
/// {
///     let __buff_log_filter = tracing_subscriber::EnvFilter::try_from_env("BUFF_LOG")
///         .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
///     let _ = if cfg!(debug_assertions) {
///         tracing_subscriber::fmt()
///             .with_env_filter(__buff_log_filter)
///             .try_init()
///     } else {
///         tracing_subscriber::fmt()
///             .with_env_filter(__buff_log_filter)
///             .json()
///             .try_init()
///     };
/// }
/// ```
///
/// # Design
///
/// - **`BUFF_LOG` env var** drives the level filter (RUST_LOG-style
///   directives: `BUFF_LOG=debug`, `BUFF_LOG=warn,buff::net=trace`).
///   Falls back to `"info"` when unset or unparseable via `unwrap_or_else`
///   (NO panic — matches Buff's "no panicking generated code" stance).
/// - **dev vs release**: `cfg!(debug_assertions)` is a RUNTIME check in
///   Rust (`cfg!` macro form, not `#[cfg]` attribute) so the same compiled
///   binary can be reused. Dev → pretty to stderr (the default
///   `tracing_subscriber::fmt()` formatter); release → JSON to stdout
///   (`.json()` layer).
/// - **`try_init()` not `init()`**: `try_init()` returns `Result` instead
///   of panicking on duplicate-global-subscriber. We discard the result
///   with `let _ = ...` — the SECOND init in a test/binary that already
///   has a subscriber is silently swallowed (Buff's "no panic" rule).
/// - **Single filter value**: built ONCE outside the `if`/`else`, then
///   MOVED into whichever branch runs. Rust's branch-evaluation semantics
///   permit this (only one branch executes at runtime, so the single
///   move is sound).
///
/// # Why a block statement (not bare)?
///
/// Wrapping in a `{ ... }` block scopes the `__buff_log_filter` binding
/// so it doesn't leak into the user's `main` body. The block evaluates
/// to `()` (the `let _ = ...` discards the `Result`), so it can stand
/// as a regular statement at the top of `main`'s body.
///
/// Built via `quote!` + `syn::parse2` (the standard pattern in this
/// module — the single string producer remains `prettyplease::unparse`).
/// On parse failure (unreachable — the template is compile-time-fixed)
/// we return `None` so the caller silently skips the init (defensive —
/// never panics in codegen).
pub(super) fn tracing_subscriber_init_stmt() -> Option<SynStmt> {
    let tokens: proc_macro2::TokenStream = quote::quote! {
        {
            let __buff_log_filter = tracing_subscriber::EnvFilter::try_from_env("BUFF_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
            let _ = if cfg!(debug_assertions) {
                tracing_subscriber::fmt()
                    .with_env_filter(__buff_log_filter)
                    .try_init()
            } else {
                tracing_subscriber::fmt()
                    .with_env_filter(__buff_log_filter)
                    .json()
                    .try_init()
            };
        }
    };
    syn::parse2::<SynStmt>(tokens).ok()
}

/// T114: auto-load `.env` at program start (like Bun/Deno/Node.js dotenv).
/// Emitted at the top of `main` to load KEY=VALUE pairs from `.env` into
/// the process environment. Does NOT override existing env vars (only sets
/// if absent). Simple parsing: one KEY=VALUE per line, skip `#` comments,
/// skip blank lines. No complex .env syntax (multiline, quotes).
pub(super) fn dotenv_auto_load_stmt() -> Option<SynStmt> {
    let tokens: proc_macro2::TokenStream = quote::quote! {
        {
            if let Ok(__buff_contents) = std::fs::read_to_string(".env") {
                for __buff_line in __buff_contents.lines() {
                    let __buff_line = __buff_line.trim();
                    if __buff_line.is_empty() || __buff_line.starts_with('#') {
                        continue;
                    }
                    if let Some((__buff_key, __buff_val)) = __buff_line.split_once('=') {
                        let __buff_k = __buff_key.trim().to_string();
                        let __buff_v = __buff_val.trim().to_string();
                        if !__buff_k.is_empty() && std::env::var(&__buff_k).is_err() {
                            unsafe { std::env::set_var(&__buff_k, &__buff_v); }
                        }
                    }
                }
            }
        }
    };
    syn::parse2::<SynStmt>(tokens).ok()
}

// ---------------------------------------------------------------------------
// T124d — regex emit-on-demand detection (Regex module).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any Regex usage (T124d):
/// `Regex.compile(...)`, `regex.match(...)`, `regex.find(...)`,
/// `regex.replace(...)`, `regex.captures(...)`. Returns `true` if at
/// least one is found, signalling [`RustCodegen::generate`] to record
/// `"regex"` in the extern-crate set so the pipeline knows the generated
/// Cargo project depends on the `regex` crate.
///
/// Detection recognises TWO shapes:
/// 1. **Associated function**: `Regex.compile(p)` — receiver is a bare
///    `Expr::Ident` naming the `Regex` prelude type.
/// 2. **Instance method**: `recv.match(...)` / `recv.find(...)` /
///    `recv.replace(...)` / `recv.captures(...)` — receiver is a value
///    whose inferred type is `Regex`. We can't do full type inference
///    at walker time (the integrated TypeInferencer lives in codegen),
///    so we conservatively flag ANY call whose method name matches one
///    of the four Regex instance methods. This mirrors the chrono
///    detection strategy (T124b): a false positive (e.g. a user type
///    with a `.find(...)` method) registers `regex` unnecessarily —
///    a no-op at the Cargo level (an unused dep). False negatives are
///    impossible because the assoc-fn shape (`Regex.compile`) is the
///    ONLY way to construct a Regex value at the surface.
///
/// Source-level type annotations like `let r: Regex = ...` also count
/// (mirroring the chrono walker), so a program that binds a Regex
/// without immediately calling it still registers the dep.
///
/// Mirrors the chrono (T124b) + tracing (T124c) detection patterns;
/// the recursive walker covers every Stmt / Expr variant that could
/// host a Regex call.
pub(super) fn program_uses_regex(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_regex(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_regex`]: scan a block's statements.
pub(super) fn block_uses_regex(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_regex)
}

/// Check a single statement (and its nested expressions) for Regex usage.
pub(super) fn stmt_uses_regex(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl {
            value,
            ty: Some(ty),
            ..
        }
        | Stmt::LetPattern {
            value,
            ty: Some(ty),
            ..
        } => {
            // Source-level type annotation names `Regex` (e.g.
            // `let r: Regex = ...`). This counts even if the value
            // expression doesn't itself mention regex.
            type_ref_names_regex(ty) || expr_uses_regex(value)
        }
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_regex(value),
        Stmt::Assignment { target, value, .. } => expr_uses_regex(target) || expr_uses_regex(value),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_regex(iter) || block_uses_regex(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_regex(cond) || block_uses_regex(body),
        Stmt::ForLet { value, body, .. } => expr_uses_regex(value) || block_uses_regex(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_regex(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_regex(e),
            }) || block_uses_regex(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_regex(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_regex(body),
    }
}

/// Returns `true` iff `ty` (or any nested inner TypeRef) mentions `Regex`.
/// Used by [`stmt_uses_regex`] to detect source-level type annotations
/// like `let r: Regex = ...`.
pub(super) fn type_ref_names_regex(ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Named { name, .. } => name.name == "Regex",
        TypeRef::Option(inner, _) => type_ref_names_regex(inner),
        TypeRef::Generic { base, args, .. } => {
            type_ref_names_regex(base) || args.iter().any(type_ref_names_regex)
        }
        _ => false,
    }
}

/// Recursively scan an expression tree for any Regex usage.
///
/// Detection is conservative: it triggers on:
/// - Any `Regex.<method>(...)` call (where `Regex` is the bare-ident
///   receiver — flags the assoc-fn `Regex.compile(p)` shape).
/// - Any `<recv>.<method>(...)` call whose method name matches one of
///   the four Regex instance methods (`match`/`find`/`replace`/`captures`).
pub(super) fn expr_uses_regex(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            // Associated-function call: `Regex.compile(p)`.
            if let Expr::Ident(id, _) = receiver.as_ref() {
                if id.name == "Regex" {
                    return true;
                }
            }
            // Instance-method call: `recv.match(...)`, `recv.find(...)`,
            // `recv.replace(...)`, `recv.captures(...)`. Conservative
            // on the method NAME — the receiver's type is resolved at
            // codegen time, so we err on the side of "register regex"
            // if the method name matches any Regex instance fn.
            // NOTE: `match` is a Buff keyword and won't parse from
            // source today (parser allows only `TokenKind::Ident(_)` in
            // method position), but AST-constructed test cases can
            // still produce an `Ident("match")`. We include it for
            // completeness so the walker stays in sync with the
            // registry's `PreludeInstanceFn::ALL`.
            if matches!(
                method.name.as_str(),
                "match" | "find" | "replace" | "captures"
            ) {
                return true;
            }
            expr_uses_regex(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_regex(lhs) || expr_uses_regex(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_regex(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_regex(callee) || args.iter().any(expr_uses_regex)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_regex(cond)
                || block_uses_regex(then_block)
                || else_block.as_ref().is_some_and(block_uses_regex)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_regex(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_regex),
        Expr::Index { base, indices, .. } => {
            expr_uses_regex(base) || indices.iter().any(expr_uses_regex)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_regex(k) || expr_uses_regex(v)),
        Expr::Lambda { body, .. } => block_uses_regex(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_regex(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_regex(scrutinee) || arms.iter().any(|arm| block_uses_regex(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_regex(inner),
        Expr::Try { expr, .. } => expr_uses_regex(expr),
        Expr::Spawn { task, .. } => expr_uses_regex(task),
        Expr::Range { start, end, .. } => expr_uses_regex(start) || expr_uses_regex(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_regex(value)
                || block_uses_regex(then_block)
                || else_block.as_ref().is_some_and(block_uses_regex)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_regex),
        Expr::NamedArg { value, .. } => expr_uses_regex(value),
    }
}

// ---------------------------------------------------------------------------
// T124e — toml emit-on-demand detection (Toml namespace module).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any `Toml.parse(...)` or
/// `Toml.stringify(...)` call (T124e). Returns `true` if at least one is
/// found, signalling [`RustCodegen::generate`] to record `"toml"` in the
/// extern-crate set so the pipeline knows the generated Cargo project
/// depends on the `toml` crate.
///
/// Detection recognises the `Toml` namespace as the receiver of a method
/// call (`Toml.parse(s)`, `Toml.stringify(v)`). The method name is NOT
/// matched here — `Toml` is a reserved prelude namespace, so any
/// `Toml.<anything>()` triggers `toml` registration. Codegen will surface
/// a clear error if `<anything>` is not one of parse/stringify.
///
/// Mirrors the tracing/regex detection patterns (T124c/T124d); the
/// recursive walker covers every Stmt / Expr variant that could host a
/// Toml call. Toml has NO instance methods (only assoc fns on the
/// namespace), so detection is simpler than Regex: only the bare-ident
/// receiver pattern (`Toml.method(...)`) is flagged.
pub(super) fn program_uses_toml(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_toml(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_toml`]: scan a block's statements.
pub(super) fn block_uses_toml(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_toml)
}

/// Check a single statement (and its nested expressions) for `Toml.*(...)`
/// usage.
pub(super) fn stmt_uses_toml(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_toml(value),
        Stmt::Assignment { target, value, .. } => expr_uses_toml(target) || expr_uses_toml(value),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_toml(iter) || block_uses_toml(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_toml(cond) || block_uses_toml(body),
        Stmt::ForLet { value, body, .. } => expr_uses_toml(value) || block_uses_toml(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_toml(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_toml(e),
            }) || block_uses_toml(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_toml(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_toml(body),
    }
}

/// Recursively scan an expression tree for a `Toml.<method>(...)` call.
///
/// Detection is on the receiver NAME (`Toml`) only — the method name is
/// validated at codegen time. This means a hypothetical user-defined
/// variable named `Toml` whose method is called would trigger a false
/// positive (registering toml unnecessarily); but since `Toml` is a
/// reserved prelude namespace, the user can't legitimately bind to that
/// name anyway. Same conservative strategy as the tracing walker (T124c).
pub(super) fn expr_uses_toml(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            if let Expr::Ident(id, _) = receiver.as_ref() {
                if id.name == "Toml" {
                    return true;
                }
            }
            // Conservatively flag any call whose method name matches a
            // Toml assoc fn — same conservative strategy T124c uses for
            // tracing instance-method detection. The codegen arm will
            // then either emit a Toml lowering or surface a clear error
            // if the receiver isn't actually the `Toml` namespace.
            if matches!(method.name.as_str(), "stringify") {
                // Only flag if the receiver could plausibly be Toml
                // (bare Ident). We already covered `Toml` above; other
                // receivers (values, calls) might be user methods that
                // happen to share the name — those should NOT trigger
                // toml registration. So this branch is a no-op; we
                // leave the method-name check in place as documentation
                // of the design decision.
            }
            expr_uses_toml(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_toml(lhs) || expr_uses_toml(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_toml(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_toml(callee) || args.iter().any(expr_uses_toml)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_toml(cond)
                || block_uses_toml(then_block)
                || else_block.as_ref().is_some_and(block_uses_toml)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_toml(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_toml),
        Expr::Index { base, indices, .. } => {
            expr_uses_toml(base) || indices.iter().any(expr_uses_toml)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_toml(k) || expr_uses_toml(v)),
        Expr::Lambda { body, .. } => block_uses_toml(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_toml(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_toml(scrutinee) || arms.iter().any(|arm| block_uses_toml(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_toml(inner),
        Expr::Try { expr, .. } => expr_uses_toml(expr),
        Expr::Spawn { task, .. } => expr_uses_toml(task),
        Expr::Range { start, end, .. } => expr_uses_toml(start) || expr_uses_toml(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_toml(value)
                || block_uses_toml(then_block)
                || else_block.as_ref().is_some_and(block_uses_toml)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_toml),
        Expr::NamedArg { value, .. } => expr_uses_toml(value),
    }
}

// ---------------------------------------------------------------------------
// T124f - rand emit-on-demand detection (Random namespace module).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any `Random.<method>(...)`
/// call (T124f). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to record `"rand"` in the extern-crate
/// set so the pipeline knows the generated Cargo project depends on
/// the `rand` crate.
///
/// Detection recognises the `Random` namespace as the receiver of a
/// method call (`Random.int(...)`, `Random.float()`, `Random.choice(v)`,
/// `Random.shuffle(v)`). The method name is NOT matched here - `Random`
/// is a reserved prelude namespace, so any `Random.<anything>()`
/// triggers `rand` registration. Codegen will surface a clear error if
/// `<anything>` is not one of int/float/choice/shuffle.
///
/// Mirrors the chrono/tracing/regex/toml detection patterns
/// (T124b/T124c/T124d/T124e); the recursive walker covers every Stmt /
/// Expr variant that could host a Random call. Random has NO instance
/// methods (only assoc fns on the namespace), so detection is simpler
/// than Regex: only the bare-ident receiver pattern (`Random.method(...)`)
/// is flagged.
///
/// Note: `Math` and `Strings` also ship in T124f but wrap Rust `std`
/// only (NO extern crate needed), so they have NO `program_uses_X`
/// walker - their generated code is fully standalone-rustc-compatible.
pub(super) fn program_uses_rand(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_rand(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_rand`]: scan a block's statements.
pub(super) fn block_uses_rand(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_rand)
}

/// Check a single statement (and its nested expressions) for `Random.*(...)`
/// usage.
pub(super) fn stmt_uses_rand(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_rand(value),
        Stmt::Assignment { target, value, .. } => expr_uses_rand(target) || expr_uses_rand(value),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_rand(iter) || block_uses_rand(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_rand(cond) || block_uses_rand(body),
        Stmt::ForLet { value, body, .. } => expr_uses_rand(value) || block_uses_rand(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_rand(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_rand(e),
            }) || block_uses_rand(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_rand(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_rand(body),
    }
}

/// Recursively scan an expression tree for a `Random.<method>(...)` call.
///
/// Detection is on the receiver NAME (`Random`) only - the method name
/// is validated at codegen time. This means a hypothetical user-defined
/// variable named `Random` whose method is called would trigger a false
/// positive (registering rand unnecessarily); but since `Random` is a
/// reserved prelude namespace, the user can't legitimately bind to that
/// name anyway. Same conservative strategy as the tracing/toml walkers
/// (T124c/T124e).
pub(super) fn expr_uses_rand(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall { receiver, .. } => {
            if let Expr::Ident(id, _) = receiver.as_ref() {
                if id.name == "Random" {
                    return true;
                }
            }
            expr_uses_rand(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_rand(lhs) || expr_uses_rand(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_rand(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_rand(callee) || args.iter().any(expr_uses_rand)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_rand(cond)
                || block_uses_rand(then_block)
                || else_block.as_ref().is_some_and(block_uses_rand)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_rand(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_rand),
        Expr::Index { base, indices, .. } => {
            expr_uses_rand(base) || indices.iter().any(expr_uses_rand)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_rand(k) || expr_uses_rand(v)),
        Expr::Lambda { body, .. } => block_uses_rand(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_rand(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_rand(scrutinee) || arms.iter().any(|arm| block_uses_rand(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_rand(inner),
        Expr::Try { expr, .. } => expr_uses_rand(expr),
        Expr::Spawn { task, .. } => expr_uses_rand(task),
        Expr::Range { start, end, .. } => expr_uses_rand(start) || expr_uses_rand(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_rand(value)
                || block_uses_rand(then_block)
                || else_block.as_ref().is_some_and(block_uses_rand)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_rand),
        Expr::NamedArg { value, .. } => expr_uses_rand(value),
    }
}

// ---------------------------------------------------------------------------
// T124g - tokio emit-on-demand detection (sleep() free fn).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any `sleep(...)` free-fn call
/// (T124g). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to record `"tokio"` in the extern-crate
/// set so the pipeline knows the generated Cargo project depends on
/// the `tokio` crate.
///
/// Detection recognises a `FuncCall` whose callee is the bare Ident
/// `sleep` (the prelude free fn introduced in T124g). The lowering
/// emits `tokio::time::sleep(<duration>).await` so any program using
/// `sleep` transitively requires tokio in `[dependencies]` (and the
/// enclosing fn MUST be async — async-propagation is the user's
/// responsibility today; future task can teach the T31 walker to flag
/// sleep-calling fns as async automatically).
///
/// Walker scope: NARROW. Flags ONLY `sleep(...)` calls — NOT every
/// async fn, NOT every `tokio::*` path fragment. The T124f gotcha
/// (chrono walker was originally over-broad, flagging namespace
/// modules) is the cautionary tale; this walker stays minimal so it
/// doesn't over-trigger on unrelated code. Same conservative
/// receiver-name-only strategy as the rand walker (T124f): a
/// hypothetical user-defined variable named `sleep` would trigger a
/// false positive, but since `sleep` is a reserved prelude name the
/// user can't legitimately bind to it.
///
/// Note: the existing v1.0 async lowering (`tokio::spawn`,
/// `tokio::runtime::Runtime`, `#[tokio::main]`) does NOT register
/// tokio in extern_crates — that path is single-file-rustc-only
/// (code-gen-only boundary, same as chrono/regex/toml/rand). This
/// walker is the FIRST time `tokio` enters extern_crates; the
/// existing async codegen paths don't need updating because their
/// `tokio::*` paths compile iff tokio is in the (deferred) Cargo
/// project's `[dependencies]`, which is exactly what this walker
/// signals.
pub(super) fn program_uses_tokio(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_tokio(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_tokio`]: scan a block's statements.
pub(super) fn block_uses_tokio(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_tokio)
}

/// Check a single statement (and its nested expressions) for `sleep(...)`
/// usage. Mirrors the `stmt_uses_rand` shape exactly.
pub(super) fn stmt_uses_tokio(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_tokio(value),
        Stmt::Assignment { target, value, .. } => expr_uses_tokio(target) || expr_uses_tokio(value),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_tokio(iter) || block_uses_tokio(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_tokio(cond) || block_uses_tokio(body),
        Stmt::ForLet { value, body, .. } => expr_uses_tokio(value) || block_uses_tokio(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_tokio(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_tokio(e),
            }) || block_uses_tokio(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_tokio(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_tokio(body),
    }
}

/// Recursively scan an expression tree for a `sleep(...)` free-fn call.
/// Same conservative bare-Ident-callee strategy as `expr_uses_rand`.
pub(super) fn expr_uses_tokio(expr: &Expr) -> bool {
    match expr {
        Expr::FuncCall { callee, args, .. } => {
            if let Expr::Ident(id, _) = callee.as_ref() {
                if id.name == "sleep" {
                    return true;
                }
            }
            expr_uses_tokio(callee) || args.iter().any(expr_uses_tokio)
        }
        Expr::MethodCall { receiver, .. } => expr_uses_tokio(receiver),
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_tokio(lhs) || expr_uses_tokio(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_tokio(operand),
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_tokio(cond)
                || block_uses_tokio(then_block)
                || else_block.as_ref().is_some_and(block_uses_tokio)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_tokio(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_tokio),
        Expr::Index { base, indices, .. } => {
            expr_uses_tokio(base) || indices.iter().any(expr_uses_tokio)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_tokio(k) || expr_uses_tokio(v)),
        Expr::Lambda { body, .. } => block_uses_tokio(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_tokio(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_tokio(scrutinee) || arms.iter().any(|arm| block_uses_tokio(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_tokio(inner),
        Expr::Try { expr, .. } => expr_uses_tokio(expr),
        Expr::Spawn { task, .. } => expr_uses_tokio(task),
        Expr::Range { start, end, .. } => expr_uses_tokio(start) || expr_uses_tokio(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_tokio(value)
                || block_uses_tokio(then_block)
                || else_block.as_ref().is_some_and(block_uses_tokio)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_tokio),
        Expr::NamedArg { value, .. } => expr_uses_tokio(value),
    }
}

// ---------------------------------------------------------------------------
// T124h - web module emit-on-demand detection (Base64 / Hex / URLEncode /
// UUID / URL modules). All five share the same detection shape: a
// `MethodCall` whose receiver is a bare Ident naming the prelude
// namespace (`Base64.encode(...)`, `UUID.v4()`, `URL.parse(...)`, ...).
// They differ ONLY in the namespace name, so the recursion is shared
// via `expr_uses_namespace` (takes the namespace name as a parameter).
// The five top-level walkers are thin wrappers.
//
// Walker scope: NARROW (per the T124f gotcha that chrono was originally
// over-broad). Each walker flags ONLY its specific receiver name - NOT
// every prelude-type Ident, NOT every method-name match. Same conservative
// receiver-name-only strategy as the rand / tokio walkers (T124f / T124g):
// a hypothetical user-defined variable named `Base64` / `Hex` / etc.
// would trigger a false positive, but since these are reserved prelude
// namespaces the user can't legitimately bind to them.
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any `<namespace>.<method>(...)`
/// call (T124h). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to record the corresponding Rust crate in
/// the extern-crate set.
///
/// Shared by all 5 web-module walkers. The `namespace` parameter is the
/// bare Ident name the walker matches against MethodCall receivers
/// (e.g. `"Base64"`, `"Hex"`, `"URLEncode"`, `"UUID"`, `"URL"`).
pub(super) fn program_uses_namespace(decls: &[Decl], namespace: &str) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_namespace(&f.body, namespace) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_namespace`]: scan a block's statements.
pub(super) fn block_uses_namespace(block: &Block, namespace: &str) -> bool {
    block
        .stmts
        .iter()
        .any(|s| stmt_uses_namespace(s, namespace))
}

/// Check a single statement (and its nested expressions) for
/// `<namespace>.<method>(...)` usage. Mirrors the `stmt_uses_rand` /
/// `stmt_uses_tokio` shape exactly.
pub(super) fn stmt_uses_namespace(stmt: &Stmt, namespace: &str) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_namespace(value, namespace),
        Stmt::Assignment { target, value, .. } => {
            expr_uses_namespace(target, namespace) || expr_uses_namespace(value, namespace)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => {
            expr_uses_namespace(iter, namespace) || block_uses_namespace(body, namespace)
        }
        Stmt::ForWhile { cond, body, .. } => {
            expr_uses_namespace(cond, namespace) || block_uses_namespace(body, namespace)
        }
        Stmt::ForLet { value, body, .. } => {
            expr_uses_namespace(value, namespace) || block_uses_namespace(body, namespace)
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => {
                    expr_uses_namespace(value, namespace)
                }
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_namespace(e, namespace),
            }) || block_uses_namespace(else_block, namespace)
        }
        Stmt::Defer { expr, .. } => expr_uses_namespace(expr, namespace),
        Stmt::ComptimeBlock { body, .. } => block_uses_namespace(body, namespace),
    }
}

/// Recursively scan an expression tree for a `<namespace>.<method>(...)`
/// call. Same conservative bare-Ident-receiver strategy as
/// `expr_uses_rand` / `expr_uses_tokio`.
pub(super) fn expr_uses_namespace(expr: &Expr, namespace: &str) -> bool {
    match expr {
        Expr::MethodCall { receiver, .. } => {
            if let Expr::Ident(id, _) = receiver.as_ref() {
                if id.name == namespace {
                    return true;
                }
            }
            expr_uses_namespace(receiver, namespace)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => {
            expr_uses_namespace(lhs, namespace) || expr_uses_namespace(rhs, namespace)
        }
        Expr::UnaryOp { operand, .. } => expr_uses_namespace(operand, namespace),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_namespace(callee, namespace)
                || args.iter().any(|a| expr_uses_namespace(a, namespace))
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_namespace(cond, namespace)
                || block_uses_namespace(then_block, namespace)
                || else_block
                    .as_ref()
                    .is_some_and(|b| block_uses_namespace(b, namespace))
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_namespace(e, namespace),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => {
            elements.iter().any(|e| expr_uses_namespace(e, namespace))
        }
        Expr::Index { base, indices, .. } => {
            expr_uses_namespace(base, namespace)
                || indices.iter().any(|i| expr_uses_namespace(i, namespace))
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_namespace(k, namespace) || expr_uses_namespace(v, namespace)),
        Expr::Lambda { body, .. } => block_uses_namespace(body, namespace),
        Expr::StructInit { fields, .. } => fields
            .iter()
            .any(|(_, v)| expr_uses_namespace(v, namespace)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            expr_uses_namespace(scrutinee, namespace)
                || arms
                    .iter()
                    .any(|arm| block_uses_namespace(&arm.body, namespace))
        }
        Expr::SuspendExpr { inner, .. } => expr_uses_namespace(inner, namespace),
        Expr::Try { expr, .. } => expr_uses_namespace(expr, namespace),
        Expr::Spawn { task, .. } => expr_uses_namespace(task, namespace),
        Expr::Range { start, end, .. } => {
            expr_uses_namespace(start, namespace) || expr_uses_namespace(end, namespace)
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_namespace(value, namespace)
                || block_uses_namespace(then_block, namespace)
                || else_block
                    .as_ref()
                    .is_some_and(|b| block_uses_namespace(b, namespace))
        }
        Expr::TupleLit(members, _) => members.iter().any(|m| expr_uses_namespace(m, namespace)),
        Expr::NamedArg { value, .. } => expr_uses_namespace(value, namespace),
    }
}

/// T124h: detect `Base64.encode(...)` / `Base64.decode(...)` calls.
pub(super) fn program_uses_base64(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "Base64")
}

/// T124h: detect `Hex.encode(...)` / `Hex.decode(...)` calls.
pub(super) fn program_uses_hex(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "Hex")
}

/// T124h: detect `URLEncode.encode(...)` / `URLEncode.decode(...)` calls.
/// The crate name is `percent-encoding` (with hyphen) - distinct from
/// the Buff namespace name `URLEncode` (no hyphen).
pub(super) fn program_uses_percent_encoding(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "URLEncode")
}

/// T124h: detect `UUID.v4()` / `UUID.v7()` / `UUID.parse(...)` calls.
pub(super) fn program_uses_uuid(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "UUID")
}

/// T124h: detect `URL.parse(...)` calls AND `url.scheme` / `url.host` /
/// `url.path` / `url.query(k)` instance method calls. The instance
/// methods require `url` too, so any program with a URL value's
/// accessor call needs the crate.
///
/// The instance-method detection uses the conservative method-name
/// strategy from `expr_uses_chrono` (T124b): flag any MethodCall whose
/// method name matches a URL instance method, regardless of receiver.
/// This is slightly broader than the namespace-only walkers above but
/// still narrow (only 4 specific method names: scheme/host/path/query).
/// False positives (user methods sharing these names) would
/// over-register `url` but never cause a missing-dependency rustc
/// failure (the registered crate just goes unused).
pub(super) fn program_uses_url(decls: &[Decl]) -> bool {
    // The namespace assoc-fn path: `URL.parse(s)` (bare Ident receiver).
    if program_uses_namespace(decls, "URL") {
        return true;
    }
    // The instance-method path: scan for any MethodCall whose method
    // name is a URL accessor (scheme/host/path/query). The receiver's
    // inferred type is checked at codegen time; we err on the side of
    // registering `url` if the name matches.
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_url_instance(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_url`]: scan a block for URL
/// instance-method calls (scheme/host/path/query).
pub(super) fn block_uses_url_instance(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_url_instance)
}

/// Check a single statement for URL instance-method calls.
pub(super) fn stmt_uses_url_instance(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_url_instance(value),
        Stmt::Assignment { target, value, .. } => {
            expr_uses_url_instance(target) || expr_uses_url_instance(value)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => {
            expr_uses_url_instance(iter) || block_uses_url_instance(body)
        }
        Stmt::ForWhile { cond, body, .. } => {
            expr_uses_url_instance(cond) || block_uses_url_instance(body)
        }
        Stmt::ForLet { value, body, .. } => {
            expr_uses_url_instance(value) || block_uses_url_instance(body)
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_url_instance(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_url_instance(e),
            }) || block_uses_url_instance(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_url_instance(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_url_instance(body),
    }
}

/// Recursively scan an expression tree for a URL instance-method call.
pub(super) fn expr_uses_url_instance(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            // URL instance method names: scheme/host/path/query. The
            // `path` name is shared with `std::path::Path` and other
            // Rust types - the over-registration is benign (the crate
            // is recorded but unused; rustc never errors on unused
            // dependencies when cargo registers them).
            if matches!(method.name.as_str(), "scheme" | "host" | "path" | "query") {
                return true;
            }
            expr_uses_url_instance(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => {
            expr_uses_url_instance(lhs) || expr_uses_url_instance(rhs)
        }
        Expr::UnaryOp { operand, .. } => expr_uses_url_instance(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_url_instance(callee) || args.iter().any(expr_uses_url_instance)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_url_instance(cond)
                || block_uses_url_instance(then_block)
                || else_block.as_ref().is_some_and(block_uses_url_instance)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_url_instance(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_url_instance),
        Expr::Index { base, indices, .. } => {
            expr_uses_url_instance(base) || indices.iter().any(expr_uses_url_instance)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_url_instance(k) || expr_uses_url_instance(v)),
        Expr::Lambda { body, .. } => block_uses_url_instance(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_url_instance(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            expr_uses_url_instance(scrutinee)
                || arms.iter().any(|arm| block_uses_url_instance(&arm.body))
        }
        Expr::SuspendExpr { inner, .. } => expr_uses_url_instance(inner),
        Expr::Try { expr, .. } => expr_uses_url_instance(expr),
        Expr::Spawn { task, .. } => expr_uses_url_instance(task),
        Expr::Range { start, end, .. } => {
            expr_uses_url_instance(start) || expr_uses_url_instance(end)
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_url_instance(value)
                || block_uses_url_instance(then_block)
                || else_block.as_ref().is_some_and(block_uses_url_instance)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_url_instance),
        Expr::NamedArg { value, .. } => expr_uses_url_instance(value),
    }
}
