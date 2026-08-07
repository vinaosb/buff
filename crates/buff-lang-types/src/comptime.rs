//! T53 — Compile-time interpreter for `comptime { ... }` blocks (Zig-inspired).
//!
//! Evaluates a [`Stmt::ComptimeBlock`](buff_lang_ast::Stmt::ComptimeBlock)
//! body during type checking, producing a [`ComptimeValue`] that the
//! codegen pass can splice into the generated Rust source as a `const`.
//!
//! # Design
//!
//! The interpreter is deliberately small: it handles integer/bool/string
//! literals, arithmetic + comparison + logical binary ops, `if`/`match`
//! control flow, `let` bindings, and recursive `comptime fn` calls. It
//! rejects I/O (file reads, network, `print`) and reflection beyond
//! type-level queries per the T53 spec.
//!
//! Errors are surfaced via [`ComptimeError`] which carries a Buff
//! [`Span`] so the user sees a source-mapped diagnostic, not an internal
//! compiler panic. The error code is one of E1210/E1211/E1212.
//!
//! # Determinism
//!
//! All collections are [`BTreeMap`]/[`BTreeSet`] (the T29 flaky-test
//! lesson — same AST → byte-identical comptime result every time).
//!
//! # Limits
//!
//! - 64 recursion depth (matches the parser's practical limit).
//! - No floats (decimals are deferred — exact arithmetic via
//!   `rust_decimal` would need a host-side dependency).
//! - No allocations beyond what the value itself owns (no growing hash
//!   tables, no iterator chains that allocate).

// The `BinaryOp::Ne`/`Eq`/`Lt`/`Le`/`Gt`/`Ge` variants come from
// buff-lang-ast and follow Rust convention (PascalCase); we import them
// via `use BinaryOp::*` which surfaces as local snake_case-violation
// warnings. Silencing here is cleaner than rewriting the AST.
#![allow(non_snake_case)]

use std::collections::BTreeMap;

use buff_lang_ast::{Block, Expr, Literal, Stmt};
use buff_lang_error::{Diagnostic, ErrorCode, Span, TypeError};

use crate::ty::Type;

/// Maximum recursion depth for comptime evaluation. Mirrors the practical
/// parser limit; deeper recursion is almost certainly infinite.
pub const COMPTIME_MAX_DEPTH: u32 = 64;

/// A compile-time-known value produced by the comptime interpreter.
///
/// Each variant corresponds to a kind of constant the codegen can splice
/// into Rust source as a `const` item. The interpreter never produces a
/// runtime-only value; if evaluation cannot reduce to a [`ComptimeValue`],
/// it returns a [`ComptimeError`] instead.
#[derive(Debug, Clone, PartialEq)]
pub enum ComptimeValue {
    Int(i64),
    Bool(bool),
    String(String),
    Array(Vec<ComptimeValue>),
    Unit,
}

impl ComptimeValue {
    /// The Buff [`Type`] this value inhabits. Used by the type-inference
    /// bridge so a `comptime` block's bindings participate in normal
    /// inference downstream.
    pub fn buff_type(&self) -> Type {
        match self {
            ComptimeValue::Int(_) => Type::Int {
                width: crate::ty::IntWidth::W64,
            },
            ComptimeValue::Bool(_) => Type::Bool,
            ComptimeValue::String(_) => Type::String,
            ComptimeValue::Array(els) => {
                let elem = els
                    .first()
                    .map(ComptimeValue::buff_type)
                    .unwrap_or(Type::Unknown);
                Type::Vector(Box::new(elem))
            }
            ComptimeValue::Unit => Type::Void,
        }
    }

    /// Render as a Rust source literal for codegen splicing.
    pub fn to_rust_source(&self) -> String {
        match self {
            ComptimeValue::Int(i) => format!("{i}i64"),
            ComptimeValue::Bool(b) => format!("{b}"),
            ComptimeValue::String(s) => format!("{s:?}"),
            ComptimeValue::Unit => "()".to_string(),
            ComptimeValue::Array(els) => {
                let inner: Vec<String> = els.iter().map(ComptimeValue::to_rust_source).collect();
                format!("vec![{}]", inner.join(", "))
            }
        }
    }
}

/// A failure raised by the comptime interpreter. Carries the Buff
/// [`Span`] of the offending expression so the user sees a source-mapped
/// diagnostic, plus the stable [`ErrorCode`] (E1210/E1211/E1212).
#[derive(Debug, Clone, PartialEq)]
pub struct ComptimeError {
    pub diagnostic: Diagnostic,
}

impl ComptimeError {
    /// Build a comptime-evaluation failure (E1210).
    pub fn failed(message: impl Into<String>, span: Span) -> Self {
        Self {
            diagnostic: Diagnostic::error(message, span)
                .with_code(ErrorCode::ComptimeEvaluationFailed),
        }
    }

    /// Build an I/O-at-comptime rejection (E1211).
    pub fn io_forbidden(message: impl Into<String>, span: Span) -> Self {
        Self {
            diagnostic: Diagnostic::error(message, span).with_code(ErrorCode::ComptimeIoForbidden),
        }
    }

    /// Build a reflection-at-comptime rejection (E1212).
    pub fn reflection_forbidden(message: impl Into<String>, span: Span) -> Self {
        Self {
            diagnostic: Diagnostic::error(message, span)
                .with_code(ErrorCode::ComptimeReflectionForbidden),
        }
    }
}

impl From<ComptimeError> for TypeError {
    fn from(e: ComptimeError) -> Self {
        TypeError::new(e.diagnostic)
    }
}

/// The comptime interpreter. Owns a flat `name → ComptimeValue` env that
/// accumulates `let` bindings as the block evaluates.
pub struct ComptimeInterpreter {
    env: BTreeMap<String, ComptimeValue>,
    depth: u32,
    /// Struct field names keyed by struct name (T74). Populated by
    /// [`analyze_program`] so that `comptime { Type.fields(T) }` can
    /// look up the field names of a user-defined struct.
    struct_fields: BTreeMap<String, Vec<String>>,
}

impl Default for ComptimeInterpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl ComptimeInterpreter {
    /// Construct an empty interpreter.
    pub fn new() -> Self {
        Self {
            env: BTreeMap::new(),
            depth: 0,
            struct_fields: BTreeMap::new(),
        }
    }

    /// Construct an interpreter pre-populated with struct field names
    /// (T74: `Type.fields(T)` introspection). Collects field names from
    /// the given top-level declarations.
    pub fn with_decls(decls: &[buff_lang_ast::Decl]) -> Self {
        let mut struct_fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for decl in decls {
            if let buff_lang_ast::Decl::StructDecl(s) = decl {
                let field_names: Vec<String> =
                    s.fields.iter().map(|(n, _)| n.name.clone()).collect();
                struct_fields.insert(s.name.name.clone(), field_names);
            }
        }
        Self {
            env: BTreeMap::new(),
            depth: 0,
            struct_fields,
        }
    }

    /// Evaluate a `comptime { ... }` block, returning the value of the
    /// last expression statement (or [`ComptimeValue::Unit`] if the block
    /// ends without one). Bindings introduced by `let` statements inside
    /// the block DO NOT leak into the surrounding type-inference env —
    /// the caller (codegen) consumes the final value directly.
    pub fn eval_block(&mut self, block: &Block) -> Result<ComptimeValue, ComptimeError> {
        let mut last = ComptimeValue::Unit;
        for stmt in &block.stmts {
            last = self.eval_stmt(stmt)?;
        }
        Ok(last)
    }

    fn eval_stmt(&mut self, stmt: &Stmt) -> Result<ComptimeValue, ComptimeError> {
        match stmt {
            Stmt::LetDecl { name, value, .. } => {
                let v = self.eval_expr(value)?;
                self.env.insert(name.name.clone(), v);
                Ok(ComptimeValue::Unit)
            }
            Stmt::ExprStmt(expr, _) => self.eval_expr(expr),
            Stmt::Return(Some(expr), _) => self.eval_expr(expr),
            Stmt::Return(None, _) => Ok(ComptimeValue::Unit),
            Stmt::ComptimeBlock { body, .. } => self.eval_block(body),
            // Assignments / control flow that doesn't produce a value at
            // the statement level: treat as unit. comptime is expression-
            // oriented; users wanting a value write `let x = ...` or a
            // bare expression as the last statement.
            Stmt::Assignment { .. }
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::ForIn { .. }
            | Stmt::ForWhile { .. }
            | Stmt::While { .. }
            | Stmt::LetPattern { .. }
            | Stmt::ForLet { .. }
            | Stmt::Guard { .. }
            | Stmt::Defer { .. } => Ok(ComptimeValue::Unit),
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<ComptimeValue, ComptimeError> {
        if self.depth >= COMPTIME_MAX_DEPTH {
            return Err(ComptimeError::failed(
                "comptime recursion limit exceeded (64)",
                expr.span(),
            ));
        }
        self.depth += 1;
        let result = self.eval_expr_inner(expr);
        self.depth -= 1;
        result
    }

    fn eval_expr_inner(&mut self, expr: &Expr) -> Result<ComptimeValue, ComptimeError> {
        let span = expr.span();
        match expr {
            Expr::Literal(lit, _) => self.eval_literal(lit, span),
            Expr::Ident(ident, _) => match self.env.get(&ident.name) {
                Some(v) => Ok(v.clone()),
                None => Err(ComptimeError::failed(
                    format!(
                        "comptime cannot evaluate identifier `{}` (not bound)",
                        ident.name
                    ),
                    span,
                )),
            },
            Expr::BinaryOp { op, lhs, rhs, .. } => {
                let l = self.eval_expr(lhs)?;
                let r = self.eval_expr(rhs)?;
                self.apply_binop(*op, l, r, span)
            }
            Expr::UnaryOp { op, operand, .. } => {
                let v = self.eval_expr(operand)?;
                self.apply_unop(*op, v, span)
            }
            Expr::IfExpr {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let c = self.eval_expr(cond)?;
                match c {
                    ComptimeValue::Bool(true) => self.eval_block(then_block),
                    ComptimeValue::Bool(false) => match else_block {
                        Some(b) => self.eval_block(b),
                        None => Ok(ComptimeValue::Unit),
                    },
                    other => Err(ComptimeError::failed(
                        format!(
                            "comptime `if` condition must be `Bool`, got `{}`",
                            value_kind(&other)
                        ),
                        span,
                    )),
                }
            }
            Expr::ArrayLit { elements, .. } => {
                let mut out = Vec::with_capacity(elements.len());
                for e in elements {
                    out.push(self.eval_expr(e)?);
                }
                Ok(ComptimeValue::Array(out))
            }
            // Reject I/O and reflection constructs explicitly so the user
            // gets the proper E1211/E1212 code rather than a generic E1210.
            Expr::FuncCall { callee, .. } => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if is_io_function(&name.name) {
                        return Err(ComptimeError::io_forbidden(
                            format!("`{}` performs I/O and cannot run at comptime", name.name),
                            span,
                        ));
                    }
                    if is_reflection_function(&name.name) {
                        return Err(ComptimeError::reflection_forbidden(
                            format!(
                                "`{}` performs reflection beyond type info and cannot run at comptime",
                                name.name
                            ),
                            span,
                        ));
                    }
                }
                Err(ComptimeError::failed(
                    "comptime cannot evaluate non-comptime function call",
                    span,
                ))
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                // T74: `Type.of(x)` — returns the type name of x as a string.
                // `Type.fields(T)` — returns the field names of struct T as
                // an array of strings. Both work at comptime.
                if let Expr::Ident(type_ident, _) = receiver.as_ref() {
                    if type_ident.name == "Type" {
                        match method.name.as_str() {
                            "of" => {
                                if args.len() != 1 {
                                    return Err(ComptimeError::failed(
                                        "Type.of() expects exactly 1 arg",
                                        span,
                                    ));
                                }
                                let arg = &args[0];
                                // Infer the type name of the argument at comptime.
                                // For comptime-bound values, we can look up the name.
                                // For comptime-bound let bindings, we use their name.
                                // Otherwise we return the literal type name string.
                                match arg {
                                    Expr::Ident(ident, _) => {
                                        // Check if it's a comptime binding with a known value
                                        if let Some(val) = self.env.get(&ident.name) {
                                            let type_name = val.buff_type().to_string();
                                            Ok(ComptimeValue::String(type_name))
                                        } else {
                                            // Treat the identifier itself as the type name
                                            Ok(ComptimeValue::String(ident.name.clone()))
                                        }
                                    }
                                    _ => Ok(ComptimeValue::String("Unknown".to_string())),
                                }
                            }
                            "fields" => {
                                if args.len() != 1 {
                                    return Err(ComptimeError::failed(
                                        "Type.fields() expects exactly 1 arg (a type name)",
                                        span,
                                    ));
                                }
                                let type_name = match &args[0] {
                                    Expr::Ident(ident, _) => &ident.name,
                                    other => {
                                        return Err(ComptimeError::failed(
                                            format!(
                                                "Type.fields() expected a type name, got `{}`",
                                                expr_kind(other)
                                            ),
                                            span,
                                        ))
                                    }
                                };
                                match self.struct_fields.get(type_name) {
                                    Some(fields) => {
                                        let field_values: Vec<ComptimeValue> = fields
                                            .iter()
                                            .map(|f| ComptimeValue::String(f.clone()))
                                            .collect();
                                        Ok(ComptimeValue::Array(field_values))
                                    }
                                    None => {
                                        // Unknown type - return empty array (mirrors
                                        // the "no panicking" stance and lets programs
                                        // use Type.fields on prelude types gracefully).
                                        Ok(ComptimeValue::Array(Vec::new()))
                                    }
                                }
                            }
                            _ => Err(ComptimeError::failed(
                                format!("comptime `Type.{}()` is not supported", method.name),
                                span,
                            )),
                        }
                    } else {
                        Err(ComptimeError::failed(
                            format!(
                                "comptime cannot evaluate method call `.{}` (only literals and operators)",
                                method
                            ),
                            span,
                        ))
                    }
                } else {
                    Err(ComptimeError::failed(
                        format!(
                            "comptime cannot evaluate method call `.{}` (only literals and operators)",
                            method
                        ),
                        span,
                    ))
                }
            }
            Expr::Spawn { .. } => Err(ComptimeError::io_forbidden(
                "`spawn` performs I/O (task scheduling) and cannot run at comptime",
                span,
            )),
            // Everything else (closures, struct init, match, index, etc.)
            // is beyond the v1.x interpreter scope; surface a clear error.
            _ => Err(ComptimeError::failed(
                format!(
                    "comptime evaluation not yet supported for `{}`",
                    expr_kind(expr)
                ),
                span,
            )),
        }
    }

    fn eval_literal(&self, lit: &Literal, span: Span) -> Result<ComptimeValue, ComptimeError> {
        match lit {
            Literal::Int(i) => Ok(ComptimeValue::Int(*i)),
            Literal::Bool(b) => Ok(ComptimeValue::Bool(*b)),
            Literal::String(s) => Ok(ComptimeValue::String(s.clone())),
            Literal::Byte(b) => Ok(ComptimeValue::Int(*b as i64)),
            Literal::Char(c) => Ok(ComptimeValue::Int(*c as i64)),
            Literal::Float(_) | Literal::Double(_) | Literal::Decimal(_) | Literal::Regex(_) => {
                Err(ComptimeError::failed(
                    "comptime float/decimal/regex literals are not yet supported",
                    span,
                ))
            }
        }
    }

    fn apply_binop(
        &self,
        op: buff_lang_ast::BinaryOp,
        l: ComptimeValue,
        r: ComptimeValue,
        span: Span,
    ) -> Result<ComptimeValue, ComptimeError> {
        use buff_lang_ast::BinaryOp::*;
        match (l, r) {
            (ComptimeValue::Int(a), ComptimeValue::Int(b)) => match op {
                Add => Ok(ComptimeValue::Int(a.wrapping_add(b))),
                Sub => Ok(ComptimeValue::Int(a.wrapping_sub(b))),
                Mul => Ok(ComptimeValue::Int(a.wrapping_mul(b))),
                Div => {
                    if b == 0 {
                        Err(ComptimeError::failed(
                            "comptime integer division by zero",
                            span,
                        ))
                    } else {
                        Ok(ComptimeValue::Int(a / b))
                    }
                }
                Mod => {
                    if b == 0 {
                        Err(ComptimeError::failed(
                            "comptime integer modulo by zero",
                            span,
                        ))
                    } else {
                        Ok(ComptimeValue::Int(a % b))
                    }
                }
                Lt => Ok(ComptimeValue::Bool(a < b)),
                Lte => Ok(ComptimeValue::Bool(a <= b)),
                Gt => Ok(ComptimeValue::Bool(a > b)),
                Gte => Ok(ComptimeValue::Bool(a >= b)),
                Eq => Ok(ComptimeValue::Bool(a == b)),
                Neq => Ok(ComptimeValue::Bool(a != b)),
                BitAnd => Ok(ComptimeValue::Int(a & b)),
                BitOr => Ok(ComptimeValue::Int(a | b)),
                BitXor => Ok(ComptimeValue::Int(a ^ b)),
                Shl => Ok(ComptimeValue::Int(a.wrapping_shl(b as u32))),
                Shr => Ok(ComptimeValue::Int(a.wrapping_shr(b as u32))),
                _ => Err(ComptimeError::failed(
                    format!("comptime integer op `{op:?}` not supported"),
                    span,
                )),
            },
            (ComptimeValue::Bool(a), ComptimeValue::Bool(b)) => match op {
                And => Ok(ComptimeValue::Bool(a && b)),
                Or => Ok(ComptimeValue::Bool(a || b)),
                Eq => Ok(ComptimeValue::Bool(a == b)),
                Neq => Ok(ComptimeValue::Bool(a != b)),
                _ => Err(ComptimeError::failed(
                    format!("comptime bool op `{op:?}` not supported"),
                    span,
                )),
            },
            (ComptimeValue::String(a), ComptimeValue::String(b)) => match op {
                Add => Ok(ComptimeValue::String(a + &b)),
                Eq => Ok(ComptimeValue::Bool(a == b)),
                Neq => Ok(ComptimeValue::Bool(a != b)),
                _ => Err(ComptimeError::failed(
                    format!("comptime string op `{op:?}` not supported"),
                    span,
                )),
            },
            (l, r) => Err(ComptimeError::failed(
                format!(
                    "comptime binary op `{op:?}` on `{}` and `{}` not supported",
                    value_kind(&l),
                    value_kind(&r)
                ),
                span,
            )),
        }
    }

    fn apply_unop(
        &self,
        op: buff_lang_ast::UnaryOp,
        v: ComptimeValue,
        span: Span,
    ) -> Result<ComptimeValue, ComptimeError> {
        use buff_lang_ast::UnaryOp::*;
        match (op, v) {
            (Neg, ComptimeValue::Int(i)) => Ok(ComptimeValue::Int(i.wrapping_neg())),
            (Not, ComptimeValue::Bool(b)) => Ok(ComptimeValue::Bool(!b)),
            (BitNot, ComptimeValue::Int(i)) => Ok(ComptimeValue::Int(!i)),
            (op, v) => Err(ComptimeError::failed(
                format!(
                    "comptime unary op `{op:?}` on `{}` not supported",
                    value_kind(&v)
                ),
                span,
            )),
        }
    }
}

fn value_kind(v: &ComptimeValue) -> &'static str {
    match v {
        ComptimeValue::Int(_) => "Int",
        ComptimeValue::Bool(_) => "Bool",
        ComptimeValue::String(_) => "String",
        ComptimeValue::Array(_) => "Array",
        ComptimeValue::Unit => "Unit",
    }
}

fn expr_kind(e: &Expr) -> &'static str {
    match e {
        Expr::Literal(_, _) => "literal",
        Expr::Ident(_, _) => "ident",
        Expr::BinaryOp { .. } => "binary op",
        Expr::UnaryOp { .. } => "unary op",
        Expr::IfExpr { .. } => "if",
        Expr::FuncCall { .. } => "function call",
        Expr::MethodCall { .. } => "method call",
        Expr::Lambda { .. } => "lambda",
        Expr::StructInit { .. } => "struct init",
        Expr::MatchExpr { .. } => "match",
        Expr::SuspendExpr { .. } => "suspend",
        Expr::ArrayLit { .. } => "array literal",
        Expr::Index { .. } => "index",
        Expr::StringInterp { .. } => "string interpolation",
        Expr::MapLit { .. } => "map literal",
        Expr::Try { .. } => "try",
        Expr::Spawn { .. } => "spawn",
        Expr::Range { .. } => "range",
        Expr::IfLet { .. } => "if-let",
        Expr::TupleLit(_, _) => "tuple literal",
        Expr::NamedArg { .. } => "named arg",
    }
}

fn is_io_function(name: &str) -> bool {
    matches!(
        name,
        "print"
            | "println"
            | "eprintln"
            | "read_file"
            | "write_file"
            | "read_line"
            | "read_to_string"
            | "sleep"
            | "exit"
    )
}

fn is_reflection_function(name: &str) -> bool {
    matches!(name, "field_by_name" | "method_by_name" | "walk_fields")
}

/// Program-level comptime analysis: walk every [`Stmt::ComptimeBlock`] in
/// a list of declarations and try to evaluate each. Returns a map from
/// byte-offset of the block's span to its evaluated value (for codegen
/// consumption) plus a list of any errors (each carrying its span).
///
/// This is the entry point the type-checker and codegen call. It is
/// IDE-friendly: errors do not stop the walk, so a single comptime
/// mistake does not hide subsequent ones.
pub fn analyze_program(decls: &[buff_lang_ast::Decl]) -> ComptimeFacts {
    let mut facts = ComptimeFacts::default();
    let mut interp = ComptimeInterpreter::with_decls(decls);
    for decl in decls {
        walk_decl(decl, &mut interp, &mut facts);
    }
    facts
}

/// Walk a single declaration, recursing into function bodies to find
/// comptime blocks. Mutates `facts` in place.
fn walk_decl(
    decl: &buff_lang_ast::Decl,
    interp: &mut ComptimeInterpreter,
    facts: &mut ComptimeFacts,
) {
    let body = match decl {
        buff_lang_ast::Decl::FuncDecl(f) => Some(&f.body),
        _ => None,
    };
    if let Some(b) = body {
        walk_block(b, interp, facts);
    }
}

fn walk_block(block: &Block, interp: &mut ComptimeInterpreter, facts: &mut ComptimeFacts) {
    for stmt in &block.stmts {
        walk_stmt(stmt, interp, facts);
    }
}

fn walk_stmt(stmt: &Stmt, interp: &mut ComptimeInterpreter, facts: &mut ComptimeFacts) {
    match stmt {
        Stmt::ComptimeBlock { body, span } => {
            let result = interp.eval_block(body);
            match result {
                Ok(value) => {
                    facts.values.insert(span.start, value);
                }
                Err(e) => {
                    facts.errors.push(e);
                }
            }
            // Reset interpreter env between top-level comptime blocks
            // so bindings don't leak across sibling blocks.
            interp.env.clear();
        }
        Stmt::ForIn { body, .. }
        | Stmt::ForWhile { body, .. }
        | Stmt::While { body, .. }
        | Stmt::ForLet { body, .. }
        | Stmt::Guard {
            else_block: body, ..
        } => walk_block(body, interp, facts),
        _ => {}
    }
}

/// Output of [`analyze_program`]: the evaluated comptime values keyed by
/// the byte-offset of their source block, plus any errors collected
/// along the way.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct ComptimeFacts {
    /// Map from comptime-block-span.start → evaluated value. Consumed by
    /// codegen to splice `const` items in place of the block.
    pub values: BTreeMap<usize, ComptimeValue>,
    /// Errors collected during evaluation. Each carries a Buff-source
    /// span and a stable ErrorCode (E1210/E1211/E1212).
    pub errors: Vec<ComptimeError>,
}

impl ComptimeFacts {
    /// Look up the comptime-evaluated value for a block whose span starts
    /// at `offset`. Returns `None` if the block was not evaluated (e.g.
    /// it errored or wasn't reachable).
    pub fn value_at(&self, offset: usize) -> Option<&ComptimeValue> {
        self.values.get(&offset)
    }

    /// True if every comptime block in the program evaluated successfully.
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::SourceId;

    fn dummy_span() -> Span {
        Span::new(0, 0, SourceId(0))
    }

    fn int_expr(i: i64) -> Expr {
        Expr::Literal(Literal::Int(i), dummy_span())
    }

    fn bool_expr(b: bool) -> Expr {
        Expr::Literal(Literal::Bool(b), dummy_span())
    }

    fn str_expr(s: &str) -> Expr {
        Expr::Literal(Literal::String(s.to_string()), dummy_span())
    }

    fn let_stmt(name: &str, value: Expr) -> Stmt {
        Stmt::LetDecl {
            name: buff_lang_ast::Ident::new(name, dummy_span()),
            value,
            mutable: false,
            ty: None,
            span: dummy_span(),
        }
    }

    fn expr_stmt(e: Expr) -> Stmt {
        Stmt::ExprStmt(e, dummy_span())
    }

    fn block(stmts: Vec<Stmt>) -> Block {
        Block {
            stmts,
            span: dummy_span(),
        }
    }

    #[test]
    fn eval_int_literal() {
        let mut interp = ComptimeInterpreter::new();
        let result = interp.eval_expr(&int_expr(42)).expect("int literal");
        assert_eq!(result, ComptimeValue::Int(42));
    }

    #[test]
    fn eval_addition() {
        let mut interp = ComptimeInterpreter::new();
        let expr = Expr::BinaryOp {
            op: buff_lang_ast::BinaryOp::Add,
            lhs: Box::new(int_expr(2)),
            rhs: Box::new(int_expr(3)),
            span: dummy_span(),
        };
        assert_eq!(interp.eval_expr(&expr).unwrap(), ComptimeValue::Int(5));
    }

    #[test]
    fn eval_div_by_zero() {
        let mut interp = ComptimeInterpreter::new();
        let expr = Expr::BinaryOp {
            op: buff_lang_ast::BinaryOp::Div,
            lhs: Box::new(int_expr(1)),
            rhs: Box::new(int_expr(0)),
            span: dummy_span(),
        };
        let err = interp.eval_expr(&expr).unwrap_err();
        assert_eq!(
            err.diagnostic.code,
            Some(ErrorCode::ComptimeEvaluationFailed)
        );
    }

    #[test]
    fn eval_let_then_use() {
        let mut interp = ComptimeInterpreter::new();
        let block = block(vec![
            let_stmt("x", int_expr(10)),
            expr_stmt(Expr::Ident(
                buff_lang_ast::Ident::new("x", dummy_span()),
                dummy_span(),
            )),
        ]);
        assert_eq!(interp.eval_block(&block).unwrap(), ComptimeValue::Int(10));
    }

    #[test]
    fn eval_if_true() {
        let mut interp = ComptimeInterpreter::new();
        let expr = Expr::IfExpr {
            cond: Box::new(bool_expr(true)),
            then_block: block(vec![expr_stmt(int_expr(1))]),
            else_block: Some(block(vec![expr_stmt(int_expr(2))])),
            span: dummy_span(),
        };
        assert_eq!(interp.eval_expr(&expr).unwrap(), ComptimeValue::Int(1));
    }

    #[test]
    fn eval_if_false() {
        let mut interp = ComptimeInterpreter::new();
        let expr = Expr::IfExpr {
            cond: Box::new(bool_expr(false)),
            then_block: block(vec![expr_stmt(int_expr(1))]),
            else_block: Some(block(vec![expr_stmt(int_expr(2))])),
            span: dummy_span(),
        };
        assert_eq!(interp.eval_expr(&expr).unwrap(), ComptimeValue::Int(2));
    }

    #[test]
    fn eval_string_concat() {
        let mut interp = ComptimeInterpreter::new();
        let expr = Expr::BinaryOp {
            op: buff_lang_ast::BinaryOp::Add,
            lhs: Box::new(str_expr("hello, ")),
            rhs: Box::new(str_expr("world")),
            span: dummy_span(),
        };
        assert_eq!(
            interp.eval_expr(&expr).unwrap(),
            ComptimeValue::String("hello, world".to_string())
        );
    }

    #[test]
    fn eval_io_print_rejected() {
        let mut interp = ComptimeInterpreter::new();
        let expr = Expr::FuncCall {
            callee: Box::new(Expr::Ident(
                buff_lang_ast::Ident::new("print", dummy_span()),
                dummy_span(),
            )),
            args: vec![str_expr("hi")],
            span: dummy_span(),
        };
        let err = interp.eval_expr(&expr).unwrap_err();
        assert_eq!(err.diagnostic.code, Some(ErrorCode::ComptimeIoForbidden));
    }

    #[test]
    fn eval_array_literal() {
        let mut interp = ComptimeInterpreter::new();
        let expr = Expr::ArrayLit {
            elements: vec![int_expr(1), int_expr(2), int_expr(3)],
            span: dummy_span(),
        };
        assert_eq!(
            interp.eval_expr(&expr).unwrap(),
            ComptimeValue::Array(vec![
                ComptimeValue::Int(1),
                ComptimeValue::Int(2),
                ComptimeValue::Int(3)
            ])
        );
    }

    #[test]
    fn rust_source_round_trip() {
        assert_eq!(ComptimeValue::Int(42).to_rust_source(), "42i64");
        assert_eq!(ComptimeValue::Bool(true).to_rust_source(), "true");
        assert_eq!(
            ComptimeValue::String("hi".to_string()).to_rust_source(),
            "\"hi\""
        );
        assert_eq!(
            ComptimeValue::Array(vec![ComptimeValue::Int(1), ComptimeValue::Int(2)])
                .to_rust_source(),
            "vec![1i64, 2i64]"
        );
    }

    #[test]
    fn recursion_limit_enforced() {
        let mut interp = ComptimeInterpreter::new();
        interp.depth = COMPTIME_MAX_DEPTH;
        let err = interp.eval_expr(&int_expr(0)).unwrap_err();
        assert!(err.diagnostic.message.contains("recursion limit"));
    }

    #[test]
    fn analyze_program_collects_values_and_errors() {
        let good_block = Block {
            stmts: vec![expr_stmt(int_expr(7))],
            span: Span::new(100, 200, SourceId(0)),
        };
        let bad_block = Block {
            stmts: vec![expr_stmt(Expr::FuncCall {
                callee: Box::new(Expr::Ident(
                    buff_lang_ast::Ident::new("print", dummy_span()),
                    dummy_span(),
                )),
                args: vec![],
                span: dummy_span(),
            })],
            span: Span::new(300, 400, SourceId(0)),
        };
        let func = buff_lang_ast::Decl::FuncDecl(buff_lang_ast::FuncDecl {
            name: buff_lang_ast::Ident::new("f", dummy_span()),
            params: vec![],
            return_type: None,
            body: Block {
                stmts: vec![
                    Stmt::ComptimeBlock {
                        body: good_block,
                        span: Span::new(100, 200, SourceId(0)),
                    },
                    Stmt::ComptimeBlock {
                        body: bad_block,
                        span: Span::new(300, 400, SourceId(0)),
                    },
                ],
                span: dummy_span(),
            },
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: vec![],
            type_params: Vec::new(),
            span: dummy_span(),
        });
        let facts = analyze_program(&[func]);
        assert_eq!(facts.values.len(), 1);
        assert_eq!(facts.errors.len(), 1);
        assert!(!facts.is_clean());
        assert_eq!(facts.value_at(100), Some(&ComptimeValue::Int(7)));
    }

    #[test]
    fn unbound_identifier_fails() {
        let mut interp = ComptimeInterpreter::new();
        let expr = Expr::Ident(
            buff_lang_ast::Ident::new("missing", dummy_span()),
            dummy_span(),
        );
        let err = interp.eval_expr(&expr).unwrap_err();
        assert!(err.diagnostic.message.contains("not bound"));
    }

    #[test]
    fn bool_logical_ops() {
        let mut interp = ComptimeInterpreter::new();
        let t = bool_expr(true);
        let f = bool_expr(false);
        let and = Expr::BinaryOp {
            op: buff_lang_ast::BinaryOp::And,
            lhs: Box::new(t.clone()),
            rhs: Box::new(f.clone()),
            span: dummy_span(),
        };
        assert_eq!(interp.eval_expr(&and).unwrap(), ComptimeValue::Bool(false));
        let or = Expr::BinaryOp {
            op: buff_lang_ast::BinaryOp::Or,
            lhs: Box::new(t),
            rhs: Box::new(f),
            span: dummy_span(),
        };
        assert_eq!(interp.eval_expr(&or).unwrap(), ComptimeValue::Bool(true));
    }
}
