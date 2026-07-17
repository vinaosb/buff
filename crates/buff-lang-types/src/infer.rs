//! Local type inference for the Buff language (v0.1).
//!
//! [`TypeInferencer`] walks the AST bottom-up, assigning a [`Type`] to each
//! expression and reporting [`TypeError`]s with source span information when
//! operands are incompatible.
//!
//! v0.1 supports only primitive types and local inference from literals,
//! identifiers, and operators. Function/method calls return [`Type::Unknown`]
//! (full inference arrives in v0.5).

use buff_lang_ast::{Block, Expr, Ident, InterpPart, Literal, Stmt, TypeRef, UnaryOp};
use buff_lang_error::{Diagnostic, Span, TypeError};

use crate::env::TypeEnv;
use crate::prelude;
use crate::promote::{assignable_to, promote_binary};
use crate::ty::Type;

/// A local type inferencer. Owns a [`TypeEnv`] that accumulates bindings as
/// `let` declarations are processed.
pub struct TypeInferencer {
    env: TypeEnv,
}

impl TypeInferencer {
    /// Creates a fresh inferencer with an empty environment.
    pub fn new() -> Self {
        Self {
            env: TypeEnv::new(),
        }
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
            // T23/T24: Indexing `base[i]` (1 index) yields the element type
            // when `base` is a `Vector<T>`; `base[row, col]` (2 indices)
            // yields the element type when `base` is a `Matrix<T>`. Any
            // other shape yields `Unknown` (a later type check can reject
            // e.g. string indexing). The `indices` vec arity drives the
            // dispatch — single-index stays the T23 Vector path, two-index
            // takes the T24 Matrix path.
            Expr::Index { base, indices, .. } => {
                let base_ty = self.infer_expr(base)?;
                if indices.len() == 2 {
                    match base_ty {
                        Type::Matrix(elem) => Ok((*elem).clone()),
                        _ => Ok(Type::Unknown),
                    }
                } else {
                    match base_ty {
                        Type::Vector(elem) => Ok((*elem).clone()),
                        _ => Ok(Type::Unknown),
                    }
                }
            }
            // T21: A string interpolation always evaluates to String.
            // Each embedded expression is visited (so its sub-types are
            // checked) but the parts themselves don't affect the result.
            Expr::StringInterp { parts, .. } => {
                for part in parts {
                    if let InterpPart::Expr(e) = part {
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
            // T68: `start..end` — infer both bounds, return Unknown (range
            // is an expression-level construct; the type system doesn't
            // track range types in v0.5).
            Expr::Range { start, end, .. } => {
                let _ = self.infer_expr(start)?;
                let _ = self.infer_expr(end)?;
                Ok(Type::Unknown)
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
    fn infer_collection_element(&self, elements: &[Expr]) -> Result<Type, TypeError> {
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
            TypeError::new(Diagnostic::error(
                format!("undefined variable: {}", name.name),
                span,
            ))
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
                    Err(TypeError::new(Diagnostic::error(
                        format!("cannot compare {lhs_ty} with {rhs_ty}"),
                        span,
                    )))
                }
            }
            // Logical operators require Bool on both sides.
            BinaryOp::And | BinaryOp::Or => {
                if lhs_ty != Type::Bool || rhs_ty != Type::Bool {
                    return Err(TypeError::new(Diagnostic::error(
                        format!("logical operators require Bool, found {lhs_ty} and {rhs_ty}"),
                        span,
                    )));
                }
                Ok(Type::Bool)
            }
            // Arithmetic operators — promote operands.
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                promote_binary(&lhs_ty, &rhs_ty).ok_or_else(|| {
                    TypeError::new(Diagnostic::error(
                        format!("cannot apply operator to {lhs_ty} and {rhs_ty}"),
                        span,
                    ))
                })
            }
            // Bitwise / shift operators — integers only.
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                if !lhs_ty.is_integer_like() || !rhs_ty.is_integer_like() {
                    return Err(TypeError::new(Diagnostic::error(
                        format!("bitwise operators require integers, found {lhs_ty} and {rhs_ty}"),
                        span,
                    )));
                }
                promote_binary(&lhs_ty, &rhs_ty).ok_or_else(|| {
                    TypeError::new(Diagnostic::error(
                        format!("cannot apply bitwise operator to {lhs_ty} and {rhs_ty}"),
                        span,
                    ))
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
                    Err(TypeError::new(Diagnostic::error(
                        format!("cannot assign {rhs_ty} to {lhs_ty}"),
                        span,
                    )))
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
                    Err(TypeError::new(Diagnostic::error(
                        format!("unary - requires a numeric type, found {operand_ty}"),
                        span,
                    )))
                }
            }
            UnaryOp::Not => {
                if operand_ty == Type::Bool {
                    Ok(Type::Bool)
                } else {
                    Err(TypeError::new(Diagnostic::error(
                        format!("unary ! requires Bool, found {operand_ty}"),
                        span,
                    )))
                }
            }
            UnaryOp::BitNot => {
                if operand_ty.is_integer_like() {
                    Ok(operand_ty)
                } else {
                    Err(TypeError::new(Diagnostic::error(
                        format!("unary ~ requires an integer, found {operand_ty}"),
                        span,
                    )))
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
            return Err(TypeError::new(Diagnostic::error(
                format!("if condition must be Bool, found {cond_ty}"),
                span,
            )));
        }
        let then_ty = self.infer_block_tail(then_block)?;
        if let Some(else_b) = else_block {
            let else_ty = self.infer_block_tail(else_b)?;
            if then_ty != else_ty {
                return Err(TypeError::new(Diagnostic::error(
                    format!("if/else branches have different types: {then_ty} vs {else_ty}"),
                    span,
                )));
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
                    if let Some(annotated_ty) = typeref_to_type(annotated_ref) {
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
                            return Err(TypeError::new(Diagnostic::error(msg, *span)));
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
/// primitive names recognised in v0.1.
///
/// Returns `None` for unrecognised names (user types, generics, function
/// types) — these are deferred to v0.5.
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
fn typeref_to_type(ty: &TypeRef) -> Option<Type> {
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
}
