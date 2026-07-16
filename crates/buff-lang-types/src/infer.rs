//! Local type inference for the Buff language (v0.1).
//!
//! [`TypeInferencer`] walks the AST bottom-up, assigning a [`Type`] to each
//! expression and reporting [`TypeError`]s with source span information when
//! operands are incompatible.
//!
//! v0.1 supports only primitive types and local inference from literals,
//! identifiers, and operators. Function/method calls return [`Type::Unknown`]
//! (full inference arrives in v0.5).

use buff_lang_ast::{Block, Expr, Ident, Literal, Stmt, TypeRef, UnaryOp};
use buff_lang_error::{Diagnostic, Span, TypeError};

use crate::env::TypeEnv;
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
            Expr::Ident(name, span) => self.lookup_ident(name, *span),
            Expr::BinaryOp { op, lhs, rhs, span } => self.infer_binary(op, lhs, rhs, *span),
            Expr::UnaryOp { op, operand, span } => self.infer_unary(op, operand, *span),
            Expr::IfExpr {
                cond,
                then_block,
                else_block,
                span,
            } => self.infer_if(cond, then_block, else_block, *span),
            // v0.5: real call resolution.
            Expr::FuncCall { .. } | Expr::MethodCall { .. } => Ok(Type::Unknown),
            Expr::SuspendExpr { inner, .. } => self.infer_expr(inner),
            // v0.5: lambda/struct/match inference.
            Expr::Lambda { .. } | Expr::StructInit { .. } | Expr::MatchExpr { .. } => {
                Ok(Type::Unknown)
            }
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
        })
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
                            return Err(TypeError::new(Diagnostic::error(
                                format!("expected {annotated_ty}, found {value_ty}"),
                                *span,
                            )));
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
        }
    }
}

impl Default for TypeInferencer {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts a parse-time [`TypeRef`] into a resolved [`Type`] for the
/// primitive names recognised in v0.1.
///
/// Returns `None` for unrecognised names (user types, generics, function
/// types) — these are deferred to v0.5.
fn typeref_to_type(ty: &TypeRef) -> Option<Type> {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "Int" => Some(Type::int_default()),
            "Float" => Some(Type::float_default()),
            "Double" => Some(Type::double()),
            "Bool" => Some(Type::bool()),
            "String" => Some(Type::string()),
            "Byte" => Some(Type::byte()),
            "Decimal" => Some(Type::Decimal),
            "Void" => Some(Type::Void),
            _ => None,
        },
        _ => None,
    }
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
