//! # buff-lang-codegen-wgsl
//!
//! Buff AST → WGSL compute shader source codegen.
//!
//! This crate lowers a single Buff lambda of the form `{ x => <numeric expr> }`
//! into a complete WGSL `@compute` shader that maps the lambda over a storage
//! buffer element-wise. The shader's binding layout is **stable** so the
//! runtime crate (T45) can hardcode a matching [`wgpu::BindGroupLayout`].
//!
//! # Scope (T44 — Wave 10)
//!
//! - ✅ Single-parameter numeric map lambda lowering.
//! - ✅ Arithmetic, comparison, logical, bitwise binary operators.
//! - ✅ Unary negation, NOT, bit-NOT.
//! - ✅ Numeric literals (`Float`, `Int`, `Bool`, `Byte`).
//! - ✅ **f64 / `Double` rejection** (RED spec — WGSL has no f64).
//! - ✅ Deterministic, byte-stable output.
//! - ❌ Multi-statement lambda bodies (deferred — runtime CPU-fallback).
//! - ❌ Calls, struct init, match, indexing (deferred).
//! - ❌ f16 enable-directive emission (deferred to feature-detection task).
//! - ❌ Compiling/validating the shader (T45's job — this crate emits STRINGS).
//!
//! # Why raw-string output is correct here
//!
//! Buff's Rust codegen uses `syn`/`quote`/`prettyplease` (no raw strings
//! allowed — project hard rule). But WGSL has no Rust-native `syn`
//! equivalent, and `naga`/`wgpu` parsers panic on invalid input rather than
//! return structured errors. So the **shader source text IS the artifact** —
//! this is the ONE Buff crate where deterministic `format!`-based string
//! generation is the intended design. We centralize it in [`shader::render_shader`]
//! so the template is one place to audit, and the body lowerer is one place
//! to extend.
//!
//! # Entry API
//!
//! Two equivalent entry points — pick whichever matches your call site:
//!
//! ```
//! use buff_lang_codegen_wgsl::{generate_wgsl, WgslCodegen, WgslOptions, WgslScalarType};
//! # use buff_lang_ast::{Expr, Literal, common::{Block, Ident, Param}, op::BinaryOp, ty::TypeRef};
//! # use buff_lang_error::Span;
//! # fn mk_lambda() -> Expr {
//! #     let span = Span::dummy();
//! #     let x = Expr::Ident(Ident::new("x", span), span);
//! #     let two = Expr::Literal(Literal::Float(2.0), span);
//! #     let body_expr = Expr::BinaryOp { op: BinaryOp::Mul, lhs: Box::new(x), rhs: Box::new(two), span };
//! #     let body = Block { stmts: vec![buff_lang_ast::Stmt::ExprStmt(body_expr, span)], span };
//! #     Expr::Lambda {
//! #         params: vec![Param {
//! #             name: Ident::new("x", span),
//! #             ty: TypeRef::Named { name: Ident::new("Float", span), span },
//! #             default_value: None,
//! #             span,
//! #         }],
//! #         body,
//! #         return_type: None,
//! #         span,
//! #     }
//! # }
//! # let lambda = mk_lambda();
//!
//! // 1. Functional — zero configuration.
//! let shader = generate_wgsl(&lambda).unwrap();
//!
//! // 2. Struct — overrides workgroup size / element type / bindings.
//! let opts = WgslOptions {
//!     workgroup_size: 128,
//!     element_type: WgslScalarType::F32,
//!     ..WgslOptions::default()
//! };
//! let shader = WgslCodegen::with_options(opts).generate(&lambda).unwrap();
//! ```
//!
//! Both produce byte-identical output for the same `(lambda, opts)`.
//!
//! [`wgpu::BindGroupLayout`]: https://docs.rs/wgpu/latest/wgpu/struct.BindGroupLayout.html

pub mod error;
pub mod lower;
pub mod shader;
pub mod ty;

pub use error::WgslError;
pub use lower::lower_expr;
pub use shader::{render_shader, WgslOptions};
pub use ty::{filter_buff_type_name, filter_literal, resolve_param_type, WgslScalarType};

use buff_lang_ast::common::Block;
use buff_lang_ast::expr::Expr;
use buff_lang_ast::stmt::Stmt;

/// The default entry point: lower `lambda` into a complete WGSL compute shader
/// using [`WgslOptions::default()`] (workgroup_size=64, f32, bindings 0/1).
///
/// # Errors
/// - [`WgslError::NotMapLambda`] if `lambda` is not an `Expr::Lambda` with
///   exactly one parameter.
/// - [`WgslError::UnsupportedParamType`] if the parameter's type annotation
///   names a non-WGSL-native type (e.g. `Double`).
/// - [`WgslError::InvalidLambdaBody`] if the body has zero or >1 top-level
///   statements.
/// - [`WgslError::UnsupportedExpr`] if the body references an unsupported AST
///   node (calls, struct init, etc.).
/// - [`WgslError::UnsupportedType`] if the body contains a `Literal::Double`
///   or other non-WGSL-native literal.
///
/// # Example
/// ```
/// # use buff_lang_codegen_wgsl::generate_wgsl;
/// # use buff_lang_ast::{Expr, Literal, common::{Block, Ident, Param}, op::BinaryOp, ty::TypeRef};
/// # use buff_lang_error::Span;
/// # let span = Span::dummy();
/// # let x = Expr::Ident(Ident::new("x", span), span);
/// # let two = Expr::Literal(Literal::Float(2.0), span);
/// # let body_expr = Expr::BinaryOp { op: BinaryOp::Mul, lhs: Box::new(x), rhs: Box::new(two), span };
/// # let body = Block { stmts: vec![buff_lang_ast::Stmt::ExprStmt(body_expr, span)], span };
/// # let lambda = Expr::Lambda {
/// #     params: vec![Param {
/// #         name: Ident::new("x", span),
/// #         ty: TypeRef::Named { name: Ident::new("Float", span), span },
/// #         default_value: None,
/// #         span,
/// #     }],
/// #     body,
/// #     return_type: None,
/// #     span,
/// # };
/// let shader = generate_wgsl(&lambda).unwrap();
/// assert!(shader.contains("@compute @workgroup_size(64)"));
/// ```
pub fn generate_wgsl(lambda: &Expr) -> Result<String, WgslError> {
    WgslCodegen::default().generate(lambda)
}

/// Lower `lambda` with explicit [`WgslOptions`] (non-default workgroup size,
/// element type, or bindings).
pub fn generate_wgsl_with_options(lambda: &Expr, opts: &WgslOptions) -> Result<String, WgslError> {
    WgslCodegen::with_options(*opts).generate(lambda)
}

/// The codegen state. Currently holds only [`WgslOptions`], but is exposed as
/// a struct so future tasks can attach caches (e.g. an inline-`enable f16;`
/// flag) without breaking the entry API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WgslCodegen {
    opts: WgslOptions,
}

impl WgslCodegen {
    /// Construct with explicit options.
    #[must_use]
    pub const fn with_options(opts: WgslOptions) -> Self {
        Self { opts }
    }

    /// Read-only access to the configured options.
    #[must_use]
    pub const fn options(&self) -> &WgslOptions {
        &self.opts
    }

    /// Lower `lambda` into a complete WGSL compute shader.
    ///
    /// This is the same as [`generate_wgsl`] but lets you reuse the codegen
    /// across multiple lambdas with the same options (the codegen is
    /// stateless beyond `opts`, so this is purely a stylistic choice).
    pub fn generate(&self, lambda: &Expr) -> Result<String, WgslError> {
        let (param_name, body_expr) = extract_map_lambda(lambda)?;
        // Resolve the parameter type (default f32 if un-annotated).
        // The resolved scalar is stored in opts.element_type by callers who
        // want a non-default element type. When the param has an explicit
        // type annotation that disagrees with opts.element_type, the
        // annotation wins (we don't silently retype user data on the GPU).
        let param_ty_ref = lambda_param_type_ref(lambda)?;
        let resolved = resolve_param_type(param_ty_ref.as_ref())?;
        // If the caller did NOT override the element type, adopt the param's
        // resolved type. (When they DID override — element_type != f32 default
        // — we keep their choice; this lets callers explicitly cast.)
        let mut effective_opts = self.opts;
        if effective_opts.element_type == WgslScalarType::F32 && resolved != WgslScalarType::F32 {
            effective_opts.element_type = resolved;
        }
        let body_wgsl = lower_expr(&body_expr, &param_name)?;
        render_shader(&effective_opts, &param_name, &body_wgsl)
    }
}

// ---------------------------------------------------------------------------
// AST extraction helpers
// ---------------------------------------------------------------------------

/// Extract `(param_name, body_expr)` from a single-parameter map lambda.
///
/// Steps:
/// 1. Confirm `lambda` is `Expr::Lambda` (else `NotMapLambda`).
/// 2. Confirm exactly 1 param (else `NotMapLambda`).
/// 3. Confirm body has exactly 1 statement that is an `ExprStmt` (else
///    `InvalidLambdaBody`).
fn extract_map_lambda(lambda: &Expr) -> Result<(String, Expr), WgslError> {
    let (params, body, _return_type) = match lambda {
        Expr::Lambda {
            params,
            body,
            return_type,
            ..
        } => (params, body, return_type),
        other => {
            return Err(WgslError::NotMapLambda {
                got: other_kind(other).to_string(),
            });
        }
    };
    if params.len() != 1 {
        return Err(WgslError::NotMapLambda {
            got: format!("{} params", params.len()),
        });
    }
    let body_expr = extract_single_expr_body(body)?;
    let param_name = params[0].name.name.clone();
    Ok((param_name, body_expr))
}

/// Extract a lambda's parameter's [`buff_lang_ast::ty::TypeRef`] (the type
/// annotation, if any). Returns `Ok(None)` for non-lambdas (unreachable in
/// practice — `extract_map_lambda` filters first).
fn lambda_param_type_ref(lambda: &Expr) -> Result<Option<buff_lang_ast::ty::TypeRef>, WgslError> {
    if let Expr::Lambda { params, .. } = lambda {
        if params.len() == 1 {
            return Ok(Some(params[0].ty.clone()));
        }
    }
    Ok(None)
}

/// Pull the single `ExprStmt` out of a lambda body block, or error.
fn extract_single_expr_body(body: &Block) -> Result<Expr, WgslError> {
    match body.stmts.len() {
        1 => match &body.stmts[0] {
            Stmt::ExprStmt(expr, _) => Ok(expr.clone()),
            Stmt::LetDecl { .. } | Stmt::LetPattern { .. } => Err(WgslError::InvalidLambdaBody {
                count: 1,
                hint: " (got a `let` declaration; T44 supports only a single expression body)"
                    .to_string(),
            }),
            Stmt::Assignment { .. } => Err(WgslError::InvalidLambdaBody {
                count: 1,
                hint: " (got an assignment; T44 supports only a single expression body)"
                    .to_string(),
            }),
            Stmt::Return(_, _) => Err(WgslError::InvalidLambdaBody {
                count: 1,
                hint: " (got a `return` statement; T44 supports only a single expression body)"
                    .to_string(),
            }),
            Stmt::Break(_) | Stmt::Continue(_) => Err(WgslError::InvalidLambdaBody {
                count: 1,
                hint: " (got a flow-control statement; T44 supports only a single expression body)"
                    .to_string(),
            }),
            Stmt::ForIn { .. } | Stmt::ForWhile { .. } | Stmt::While { .. } | Stmt::ForLet { .. } => {
                Err(WgslError::InvalidLambdaBody {
                    count: 1,
                    hint: " (got a loop statement; T44 supports only a single expression body)"
                        .to_string(),
                })
            }
            Stmt::Guard { .. } => Err(WgslError::InvalidLambdaBody {
                count: 1,
                hint: " (got a guard statement; T44 supports only a single expression body)"
                    .to_string(),
            }),
            Stmt::Defer { .. } => Err(WgslError::InvalidLambdaBody {
                count: 1,
                hint: " (got a defer statement; T44 supports only a single expression body)"
                    .to_string(),
            }),
            // T53: comptime block - WGSL lambdas do not support comptime
            // (CPU fallback path). Surgical stub; T53 owns the full
            // comptime lowering.
            Stmt::ComptimeBlock { .. } => Err(WgslError::InvalidLambdaBody {
                count: 1,
                hint: " (got a comptime block; T44 supports only a single expression body)"
                    .to_string(),
            }),
        },
        0 => Err(WgslError::InvalidLambdaBody {
            count: 0,
            hint: String::new(),
        }),
        n => Err(WgslError::InvalidLambdaBody {
            count: n,
            hint: " (T44 supports only a single expression body; multi-statement bodies are deferred to a later task)"
                .to_string(),
        }),
    }
}

/// Short kind name for an [`Expr`] (used in the `NotMapLambda` error).
fn other_kind(expr: &Expr) -> &'static str {
    match expr {
        Expr::Literal(_, _) => "literal",
        Expr::Ident(_, _) => "identifier",
        Expr::BinaryOp { .. } => "binary op",
        Expr::UnaryOp { .. } => "unary op",
        Expr::IfExpr { .. } => "if expression",
        Expr::FuncCall { .. } => "function call",
        Expr::MethodCall { .. } => "method call",
        Expr::Lambda { .. } => "lambda", // unreachable (handled above) but kept for safety
        Expr::StructInit { .. } => "struct literal",
        Expr::MatchExpr { .. } => "match expression",
        Expr::SuspendExpr { .. } => "suspend expression",
        Expr::ArrayLit { .. } => "array literal",
        Expr::Index { .. } => "index expression",
        Expr::StringInterp { .. } => "string interpolation",
        Expr::MapLit { .. } => "map literal",
        Expr::Try { .. } => "try expression",
        Expr::Spawn { .. } => "spawn expression",
        Expr::Range { .. } => "range expression",
        Expr::IfLet { .. } => "if-let expression",
        Expr::TupleLit(_, _) => "tuple literal",
        Expr::NamedArg { .. } => "named argument",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::common::{Ident, Param};
    use buff_lang_ast::op::BinaryOp;
    use buff_lang_ast::ty::TypeRef;
    use buff_lang_ast::Literal;
    use buff_lang_error::Span;

    fn span() -> Span {
        Span::dummy()
    }

    fn ident(name: &str) -> Ident {
        Ident::new(name, span())
    }

    fn x_param(ty: Option<&str>) -> Param {
        let ty_ref = ty.map(|name| TypeRef::Named {
            name: ident(name),
            span: span(),
        });
        Param {
            name: ident("x"),
            ty: ty_ref.unwrap_or_else(|| TypeRef::Named {
                name: ident("Float"),
                span: span(),
            }),
            default_value: None,
            is_comptime: false,
            span: span(),
        }
    }

    fn x_ident() -> Expr {
        Expr::Ident(ident("x"), span())
    }

    fn float_lit(v: f32) -> Expr {
        Expr::Literal(Literal::Float(v), span())
    }

    fn double_lit(v: f64) -> Expr {
        Expr::Literal(Literal::Double(v), span())
    }

    fn int_lit(v: i64) -> Expr {
        Expr::Literal(Literal::Int(v), span())
    }

    fn binop(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span: span(),
        }
    }

    fn lambda(param: Param, body_expr: Expr) -> Expr {
        Expr::Lambda {
            params: vec![param],
            body: Block {
                stmts: vec![Stmt::ExprStmt(body_expr, span())],
                span: span(),
            },
            return_type: None,
            span: span(),
        }
    }

    #[test]
    fn public_api_qa_case_x_times_two() {
        // QA: {x => x * 2.0} → contains "@compute @workgroup_size(64)"
        let l = lambda(
            x_param(None),
            binop(BinaryOp::Mul, x_ident(), float_lit(2.0)),
        );
        let src = generate_wgsl(&l).unwrap();
        assert!(src.contains("@compute @workgroup_size(64)"));
        assert!(src.contains("output[i] = x * 2.0;"));
    }

    #[test]
    fn public_api_rejects_f64_param() {
        let l = lambda(
            x_param(Some("Double")),
            binop(BinaryOp::Mul, x_ident(), float_lit(2.0)),
        );
        let err = generate_wgsl(&l).unwrap_err();
        assert!(matches!(err, WgslError::UnsupportedType { .. }));
        assert!(err.to_string().contains("Float<64>"));
    }

    #[test]
    fn public_api_rejects_f64_body_literal() {
        let l = lambda(
            x_param(None),
            binop(BinaryOp::Mul, x_ident(), double_lit(2.0)),
        );
        let err = generate_wgsl(&l).unwrap_err();
        assert!(matches!(err, WgslError::UnsupportedType { .. }));
        assert!(err.to_string().contains("Float<64>"));
    }

    #[test]
    fn public_api_rejects_non_lambda() {
        let e = binop(BinaryOp::Add, int_lit(1), int_lit(2));
        let err = generate_wgsl(&e).unwrap_err();
        assert!(matches!(err, WgslError::NotMapLambda { .. }));
    }

    #[test]
    fn public_api_rejects_two_params() {
        let l = Expr::Lambda {
            params: vec![x_param(None), x_param(Some("y_param_internal"))],
            body: Block {
                stmts: vec![Stmt::ExprStmt(x_ident(), span())],
                span: span(),
            },
            return_type: None,
            span: span(),
        };
        let err = generate_wgsl(&l).unwrap_err();
        assert!(matches!(err, WgslError::NotMapLambda { .. }));
    }

    #[test]
    fn public_api_rejects_empty_body() {
        let l = Expr::Lambda {
            params: vec![x_param(None)],
            body: Block {
                stmts: vec![],
                span: span(),
            },
            return_type: None,
            span: span(),
        };
        let err = generate_wgsl(&l).unwrap_err();
        assert!(matches!(err, WgslError::InvalidLambdaBody { .. }));
    }

    #[test]
    fn options_round_trip() {
        let opts = WgslOptions {
            workgroup_size: 128,
            ..WgslOptions::default()
        };
        let cg = WgslCodegen::with_options(opts);
        assert_eq!(cg.options().workgroup_size, 128);
    }

    #[test]
    fn deterministic_byte_identical() {
        let l = lambda(
            x_param(None),
            binop(BinaryOp::Mul, x_ident(), float_lit(2.0)),
        );
        let a = generate_wgsl(&l).unwrap();
        let b = generate_wgsl(&l).unwrap();
        assert_eq!(a, b);
    }
}
