//! The Rust code generator — lowers Deox AST nodes to `syn` types.
//!
//! ## Design
//!
//! - Every Rust construct is built via explicit `syn` struct construction.
//!   We **never** hand-format Rust strings; the only string producers are
//!   `prettyplease` (via [`crate::format`]) and identifier names.
//! - `parse_quote!` is intentionally avoided in non-test code because it
//!   panics on parse failure; we construct `syn` nodes by hand instead.
//! - Unsupported AST nodes return a [`CodegenError`] rather than panicking,
//!   so future tasks (T12/T13/…) can extend coverage incrementally.
//!
//! ## Supported AST → Rust coverage (T11)
//!
//! - `Decl::FuncDecl` → `Item::Fn` (async/unsafe/extern modifiers + params
//!   + return type + body)
//! - `Stmt::LetDecl`, `Stmt::ExprStmt`, `Stmt::Return`, `Stmt::Assignment`,
//!   `Stmt::Break`, `Stmt::Continue`, `Stmt::ForIn`, `Stmt::ForWhile`
//! - `Expr::Literal`, `Expr::Ident`, `Expr::BinaryOp`, `Expr::UnaryOp`,
//!   `Expr::FuncCall`, `Expr::IfExpr`
//! - `Literal::{Int, Float, Double, Bool, String, Byte}`
//! - `TypeRef::Named` for the seven v0.1 primitive names (`Int`→`i64`, etc.)
//!   plus `TypeRef::Option` and `TypeRef::Generic` (named base)
//!
//! ## Type-annotated `let` bindings (T12)
//!
//! Every `let` binding emits an explicit Rust type annotation. If the Deox
//! source provides one (`let x: Int = …`), it is used directly; otherwise
//! the integrated [`TypeInferencer`] infers the type from the initializer
//! expression and [`RustCodegen::deox_type_to_syn`] maps it to the
//! corresponding Rust type. [`Type::Decimal`] maps to
//! `rust_decimal::Decimal` (so generated crates must depend on
//! `rust_decimal`/`rust_decimal_macros`).
//!
//! ## Control flow (T13)
//!
//! - `if cond { a } else { b }` → Rust `if` expression (with optional else)
//! - `for x in iter { body }` → Rust `for x in iter { body }`
//! - `for cond { body }` (Deox conditional loop) → Rust `while cond { body }`
//! - `print(arg)` calls map to `println!("{}", arg)` macro invocations.
//!
//! ## Source-map recording (T16)
//!
//! [`CodegenContext::record_mapping`] is available so that each lowered AST
//! node can record its Deox [`Span`] → Rust `(line, col)` mapping. In v0.1
//! the mapping is **not** automatically populated during lowering because:
//!
//! 1. `syn` nodes carry opaque `proc_macro2::Span`s (no source-line info).
//! 2. `prettyplease` reformats the tree after construction, so line numbers
//!    computed pre-format would be wrong.
//!
//! The pipeline (`deox_cli::error_mapper`) therefore uses **filename
//! translation** for v0.1: it replaces the intermediate `.rs` path in
//! `rustc`/panic messages with the original `.deox` path. Exact Deox line
//! translation via the bidirectional [`SourceMap`](deox_error::SourceMap)
//! will land in a later task once a post-prettyplease line scan is available.
//!
//! ## Move semantics (T33a)
//!
//! All bindings are MOVED by default (Rust move semantics). The integrated
//! [`MoveAnalyzer`] pre-classifies each binding as Copy or non-Copy, and
//! `lower_expr` inserts `.clone()` at the use site of any non-Copy variable
//! that has already been moved once. Generated Rust never contains `&`,
//! `&mut`, or lifetime annotations in function signatures.
//!
//! Structs, enums, imports, traits, lambdas, match, method-call and
//! struct-init lowering are deferred to later tasks.

use proc_macro2::Span as ProcSpan;
use syn::punctuated::Punctuated;
use syn::{
    Expr as SynExpr, File, Ident, Item, ItemFn, Pat, PatIdent, PatType, ReturnType, Signature,
    Stmt as SynStmt, Type as SynType, Visibility,
};

use deox_ast::{
    op::{BinaryOp, UnaryOp},
    Block, Decl, Expr, FuncDecl, Literal, Stmt, TypeRef,
};
use deox_error::{CodegenError, Diagnostic, Span as DeoxSpan};
use deox_types::{FloatWidth, IntWidth, Type, TypeInferencer};

use crate::context::CodegenContext;
use crate::move_analysis::MoveAnalyzer;

/// The Rust code generator.
///
/// Owns a [`CodegenContext`] for the lifetime of one generation pass.
/// Construct with [`RustCodegen::new`] (or `Default`).
pub struct RustCodegen {
    ctx: CodegenContext,
    move_analyzer: MoveAnalyzer,
    /// Local type inferencer used to derive Rust type annotations on
    /// `let` bindings that lack an explicit Deox annotation (T12).
    /// Reset between functions via [`TypeInferencer::env`] clear semantics
    /// (we re-bind params + walk let-stmts at the top of each `lower_func`).
    type_inferencer: TypeInferencer,
}

impl RustCodegen {
    /// Create a fresh codegen with an empty context.
    pub fn new() -> Self {
        Self {
            ctx: CodegenContext::new(),
            move_analyzer: MoveAnalyzer::new(),
            type_inferencer: TypeInferencer::new(),
        }
    }

    /// Borrow the inner context (read-only).
    pub fn context(&self) -> &CodegenContext {
        &self.ctx
    }

    /// Generate a complete [`syn::File`] from a list of Deox declarations.
    ///
    /// Each top-level `Decl` becomes one top-level `syn::Item`. The output
    /// is a fully-formed Rust file ready for [`crate::format`].
    pub fn generate(&mut self, decls: &[Decl]) -> Result<File, CodegenError> {
        let mut items = Vec::with_capacity(decls.len());
        for decl in decls {
            let item = self.lower_decl(decl)?;
            items.push(item);
        }
        Ok(File {
            shebang: None,
            attrs: Vec::new(),
            items,
        })
    }

    fn lower_decl(&mut self, decl: &Decl) -> Result<Item, CodegenError> {
        match decl {
            Decl::FuncDecl(f) => Ok(Item::Fn(self.lower_func(f)?)),
            Decl::StructDecl { .. } => Err(self.unsupported("struct codegen (T12/T13)")),
            Decl::EnumDecl { .. } => Err(self.unsupported("enum codegen")),
            Decl::ImportDecl { .. } => Err(self.unsupported("import codegen")),
            Decl::ModuleDecl { .. } => Err(self.unsupported("module codegen")),
            Decl::TraitDecl { .. } => Err(self.unsupported("trait codegen")),
        }
    }

    fn lower_func(&mut self, f: &FuncDecl) -> Result<ItemFn, CodegenError> {
        // Reset move-analysis state and pre-classify Copy vars for this fn.
        self.move_analyzer.reset();
        self.move_analyzer.preanalyze_func(f);

        // Reset the type inferencer for this function: re-bind parameters
        // using the same primitive-mapping rules that TypeInferencer uses
        // internally (see `typeref_to_type` in deox_types::infer).
        self.type_inferencer = TypeInferencer::new();
        for p in &f.params {
            if let Some(ty) = typeref_to_type(&p.ty) {
                self.type_inferencer.bind(&p.name.name, ty);
            }
        }

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
            asyncness: f.is_async.then(Default::default),
            unsafety: f.is_unsafe.then(Default::default),
            abi: f.is_extern.then(|| syn::Abi {
                extern_token: Default::default(),
                name: None,
            }),
            fn_token: Default::default(),
            ident: name,
            generics: Default::default(),
            paren_token: Default::default(),
            inputs,
            variadic: None,
            output,
        };

        let block = self.lower_block(&f.body)?;

        Ok(ItemFn {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            sig,
            block: Box::new(block),
        })
    }

    fn lower_block(&mut self, block: &Block) -> Result<syn::Block, CodegenError> {
        let mut stmts = Vec::with_capacity(block.stmts.len());
        for stmt in &block.stmts {
            stmts.push(self.lower_stmt(stmt)?);
        }
        Ok(syn::Block {
            brace_token: Default::default(),
            stmts,
        })
    }

    fn lower_stmt(&mut self, stmt: &Stmt) -> Result<SynStmt, CodegenError> {
        match stmt {
            Stmt::LetDecl {
                name,
                value,
                mutable,
                ty,
                ..
            } => {
                let ident = ast_ident_to_syn(name);
                let init_expr = self.lower_expr(value)?;

                // Run the inferencer on the value so we can emit an
                // explicit Rust type annotation. If the user wrote an
                // explicit Deox annotation (`ty: Some(..)`), prefer it;
                // otherwise fall back to the inferred type (T12).
                let inferred_syn_ty: Option<SynType> = if let Some(type_ref) = ty {
                    Some(self.ast_typeref_to_syn(type_ref)?)
                } else {
                    // Bind in the inferencer so later statements can see
                    // this name; on error we fall back to no annotation.
                    let inferred = self
                        .type_inferencer
                        .infer_stmt(stmt)
                        .unwrap_or(Type::Unknown);
                    self.deox_type_to_syn(&inferred)
                };

                // Wrap the pattern in `Pat::Type` when an annotation is present
                // so we emit `let x: T = v;` rather than `let x = v;`.
                let pat = match inferred_syn_ty {
                    Some(ty_syn) => Pat::Type(PatType {
                        attrs: Vec::new(),
                        pat: Box::new(Self::make_let_pat(ident, *mutable)),
                        colon_token: Default::default(),
                        ty: Box::new(ty_syn),
                    }),
                    None => Self::make_let_pat(ident, *mutable),
                };
                let local = syn::Local {
                    attrs: Vec::new(),
                    let_token: Default::default(),
                    pat,
                    init: Some(syn::LocalInit {
                        eq_token: Default::default(),
                        expr: Box::new(init_expr),
                        diverge: None,
                    }),
                    semi_token: Default::default(),
                };
                Ok(SynStmt::Local(local))
            }
            Stmt::ExprStmt(expr, _) => {
                let e = self.lower_expr(expr)?;
                Ok(SynStmt::Expr(e, Some(Default::default())))
            }
            Stmt::Return(opt_expr, _) => {
                let return_expr = match opt_expr {
                    Some(expr) => SynExpr::Return(syn::ExprReturn {
                        attrs: Vec::new(),
                        return_token: Default::default(),
                        expr: Some(Box::new(self.lower_expr(expr)?)),
                    }),
                    None => SynExpr::Return(syn::ExprReturn {
                        attrs: Vec::new(),
                        return_token: Default::default(),
                        expr: None,
                    }),
                };
                Ok(SynStmt::Expr(return_expr, Some(Default::default())))
            }
            Stmt::Assignment {
                target, op, value, ..
            } => {
                // The LHS of an assignment is NOT a "use" — it doesn't
                // consume a move. If the target is a bare Ident, lower it
                // directly without consulting the move analyzer.
                let lhs = if let Expr::Ident(name, _) = &target {
                    SynExpr::Path(syn::ExprPath {
                        attrs: Vec::new(),
                        qself: None,
                        path: syn::Path::from(ast_ident_to_syn(name)),
                    })
                } else {
                    self.lower_expr(target)?
                };
                let rhs = self.lower_expr(value)?;
                let assign = self.make_binary_op(*op, lhs, rhs)?;
                Ok(SynStmt::Expr(assign, Some(Default::default())))
            }
            Stmt::Break(_) => {
                let brk = SynExpr::Break(syn::ExprBreak {
                    attrs: Vec::new(),
                    break_token: Default::default(),
                    label: None,
                    expr: None,
                });
                Ok(SynStmt::Expr(brk, Some(Default::default())))
            }
            Stmt::Continue(_) => {
                let cont = SynExpr::Continue(syn::ExprContinue {
                    attrs: Vec::new(),
                    continue_token: Default::default(),
                    label: None,
                });
                Ok(SynStmt::Expr(cont, Some(Default::default())))
            }
            Stmt::ForIn {
                var, iter, body, ..
            } => {
                let var_ident = ast_ident_to_syn(var);
                let iter_expr = self.lower_expr(iter)?;
                let body_block = self.lower_block(body)?;
                let pat = Pat::Ident(PatIdent {
                    attrs: Vec::new(),
                    ident: var_ident,
                    by_ref: None,
                    mutability: None,
                    subpat: None,
                });
                let for_loop = SynExpr::ForLoop(syn::ExprForLoop {
                    attrs: Vec::new(),
                    label: None,
                    for_token: Default::default(),
                    pat: Box::new(pat),
                    in_token: Default::default(),
                    expr: Box::new(iter_expr),
                    body: body_block,
                });
                Ok(SynStmt::Expr(for_loop, Some(Default::default())))
            }
            Stmt::ForWhile { cond, body, .. } => {
                // Deox's `for cond { body }` (conditional-loop form) maps
                // directly to Rust's `while cond { body }` (T13).
                let cond_expr = self.lower_expr(cond)?;
                let body_block = self.lower_block(body)?;
                let while_expr = SynExpr::While(syn::ExprWhile {
                    attrs: Vec::new(),
                    label: None,
                    while_token: Default::default(),
                    cond: Box::new(cond_expr),
                    body: body_block,
                });
                Ok(SynStmt::Expr(while_expr, Some(Default::default())))
            }
        }
    }

    fn make_let_pat(ident: Ident, mutable: bool) -> Pat {
        Pat::Ident(PatIdent {
            attrs: Vec::new(),
            ident,
            by_ref: None,
            mutability: mutable.then(Default::default),
            subpat: None,
        })
    }

    fn lower_expr(&mut self, expr: &Expr) -> Result<SynExpr, CodegenError> {
        match expr {
            Expr::Literal(lit, _) => self.lower_literal(lit),
            Expr::Ident(name, _) => {
                let path = syn::ExprPath {
                    attrs: Vec::new(),
                    qself: None,
                    path: syn::Path::from(ast_ident_to_syn(name)),
                };
                if self.move_analyzer.needs_clone(&name.name) {
                    // Insert `.clone()` so this use is valid after a prior move.
                    Ok(SynExpr::MethodCall(syn::ExprMethodCall {
                        attrs: Vec::new(),
                        receiver: Box::new(SynExpr::Path(path)),
                        dot_token: Default::default(),
                        method: Ident::new("clone", ProcSpan::call_site()),
                        turbofish: None,
                        paren_token: Default::default(),
                        args: Default::default(),
                    }))
                } else {
                    Ok(SynExpr::Path(path))
                }
            }
            Expr::BinaryOp { op, lhs, rhs, .. } => {
                let lhs = self.lower_expr(lhs)?;
                let rhs = self.lower_expr(rhs)?;
                self.make_binary_op(*op, lhs, rhs)
            }
            Expr::UnaryOp { op, operand, .. } => {
                let operand = self.lower_expr(operand)?;
                self.make_unary_op(*op, operand)
            }
            Expr::FuncCall { callee, args, .. } => {
                // Special case (T13): `print(x)` → `println!("{}", x)` macro.
                // We require a bare-ident callee named exactly `print` with
                // exactly one argument.
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if name.name == "print" && args.len() == 1 {
                        let arg = self.lower_expr(&args[0])?;
                        return Ok(make_println_macro(arg));
                    }
                }

                // A function name (bare Ident callee) is NOT a variable
                // use — it doesn't consume a move. Lower it without
                // consulting the move analyzer; other callee shapes
                // (MethodCall, etc.) go through the normal path.
                let callee = match callee.as_ref() {
                    Expr::Ident(name, _) => SynExpr::Path(syn::ExprPath {
                        attrs: Vec::new(),
                        qself: None,
                        path: syn::Path::from(ast_ident_to_syn(name)),
                    }),
                    _ => self.lower_expr(callee)?,
                };
                let mut lowered: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                for a in args {
                    lowered.push(self.lower_expr(a)?);
                }
                Ok(SynExpr::Call(syn::ExprCall {
                    attrs: Vec::new(),
                    func: Box::new(callee),
                    paren_token: Default::default(),
                    args: lowered,
                }))
            }
            Expr::IfExpr {
                cond,
                then_block,
                else_block,
                ..
            } => self.lower_if_expr(cond, then_block, else_block.as_ref()),
            _ => Err(self.unsupported(&format!("expr codegen not yet implemented for {:?}", expr))),
        }
    }

    fn lower_if_expr(
        &mut self,
        cond: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Result<SynExpr, CodegenError> {
        let cond_expr = self.lower_expr(cond)?;
        let then_branch = self.lower_block(then_block)?;
        let else_branch = match else_block {
            Some(b) => Some((
                Default::default(),
                Box::new(SynExpr::Block(syn::ExprBlock {
                    attrs: Vec::new(),
                    label: None,
                    block: self.lower_block(b)?,
                })),
            )),
            None => None,
        };
        Ok(SynExpr::If(syn::ExprIf {
            attrs: Vec::new(),
            if_token: Default::default(),
            cond: Box::new(cond_expr),
            then_branch,
            else_branch,
        }))
    }

    fn lower_literal(&mut self, lit: &Literal) -> Result<SynExpr, CodegenError> {
        let syn_lit = match lit {
            Literal::Int(n) => {
                syn::Lit::Int(syn::LitInt::new(&n.to_string(), ProcSpan::call_site()))
            }
            Literal::Float(f) => {
                // f32 suffix — prettyplease prints it as `2.5f32`.
                let s = format!("{}f32", float_repr(*f as f64));
                syn::Lit::Float(syn::LitFloat::new(&s, ProcSpan::call_site()))
            }
            Literal::Double(d) => {
                let s = format!("{}f64", float_repr(*d));
                syn::Lit::Float(syn::LitFloat::new(&s, ProcSpan::call_site()))
            }
            Literal::Bool(b) => syn::Lit::Bool(syn::LitBool::new(*b, ProcSpan::call_site())),
            Literal::String(s) => syn::Lit::Str(syn::LitStr::new(s, ProcSpan::call_site())),
            Literal::Byte(b) => {
                syn::Lit::Int(syn::LitInt::new(&b.to_string(), ProcSpan::call_site()))
            }
        };
        Ok(SynExpr::Lit(syn::ExprLit {
            attrs: Vec::new(),
            lit: syn_lit,
        }))
    }

    fn make_binary_op(
        &mut self,
        op: BinaryOp,
        lhs: SynExpr,
        rhs: SynExpr,
    ) -> Result<SynExpr, CodegenError> {
        use syn::BinOp;
        let result = match op {
            BinaryOp::And => self.bin_arith(BinOp::And(Default::default()), lhs, rhs),
            BinaryOp::Or => self.bin_arith(BinOp::Or(Default::default()), lhs, rhs),
            BinaryOp::Add => self.bin_arith(BinOp::Add(Default::default()), lhs, rhs),
            BinaryOp::Sub => self.bin_arith(BinOp::Sub(Default::default()), lhs, rhs),
            BinaryOp::Mul => self.bin_arith(BinOp::Mul(Default::default()), lhs, rhs),
            BinaryOp::Div => self.bin_arith(BinOp::Div(Default::default()), lhs, rhs),
            BinaryOp::Mod => self.bin_arith(BinOp::Rem(Default::default()), lhs, rhs),
            BinaryOp::Eq => self.bin_arith(BinOp::Eq(Default::default()), lhs, rhs),
            BinaryOp::Neq => self.bin_arith(BinOp::Ne(Default::default()), lhs, rhs),
            BinaryOp::Lt => self.bin_arith(BinOp::Lt(Default::default()), lhs, rhs),
            BinaryOp::Gt => self.bin_arith(BinOp::Gt(Default::default()), lhs, rhs),
            BinaryOp::Lte => self.bin_arith(BinOp::Le(Default::default()), lhs, rhs),
            BinaryOp::Gte => self.bin_arith(BinOp::Ge(Default::default()), lhs, rhs),
            BinaryOp::BitAnd => self.bin_arith(BinOp::BitAnd(Default::default()), lhs, rhs),
            BinaryOp::BitOr => self.bin_arith(BinOp::BitOr(Default::default()), lhs, rhs),
            BinaryOp::BitXor => self.bin_arith(BinOp::BitXor(Default::default()), lhs, rhs),
            BinaryOp::Shl => self.bin_arith(BinOp::Shl(Default::default()), lhs, rhs),
            BinaryOp::Shr => self.bin_arith(BinOp::Shr(Default::default()), lhs, rhs),
            BinaryOp::Assign => SynExpr::Assign(syn::ExprAssign {
                attrs: Vec::new(),
                left: Box::new(lhs),
                eq_token: Default::default(),
                right: Box::new(rhs),
            }),
            BinaryOp::AddAssign
            | BinaryOp::SubAssign
            | BinaryOp::MulAssign
            | BinaryOp::DivAssign
            | BinaryOp::ModAssign => {
                let binop = match op {
                    BinaryOp::AddAssign => BinOp::AddAssign(Default::default()),
                    BinaryOp::SubAssign => BinOp::SubAssign(Default::default()),
                    BinaryOp::MulAssign => BinOp::MulAssign(Default::default()),
                    BinaryOp::DivAssign => BinOp::DivAssign(Default::default()),
                    BinaryOp::ModAssign => BinOp::RemAssign(Default::default()),
                    _ => unreachable!(),
                };
                SynExpr::Binary(syn::ExprBinary {
                    attrs: Vec::new(),
                    left: Box::new(lhs),
                    op: binop,
                    right: Box::new(rhs),
                })
            }
        };
        Ok(result)
    }

    fn bin_arith(&self, op: syn::BinOp, lhs: SynExpr, rhs: SynExpr) -> SynExpr {
        SynExpr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(lhs),
            op,
            right: Box::new(rhs),
        })
    }

    fn make_unary_op(&mut self, op: UnaryOp, operand: SynExpr) -> Result<SynExpr, CodegenError> {
        // Deox's `~` (bitwise NOT on integers) maps to Rust's `!` on integers.
        let unop = match op {
            UnaryOp::Neg => syn::UnOp::Neg(Default::default()),
            UnaryOp::Not => syn::UnOp::Not(Default::default()),
            UnaryOp::BitNot => syn::UnOp::Not(Default::default()),
        };
        Ok(SynExpr::Unary(syn::ExprUnary {
            attrs: Vec::new(),
            op: unop,
            expr: Box::new(operand),
        }))
    }

    /// Convert a Deox [`TypeRef`] to a Rust [`syn::Type`].
    ///
    /// Returns an error for unsupported forms (function types); these will
    /// land in T12/T13.
    fn ast_typeref_to_syn(&self, ty: &TypeRef) -> Result<SynType, CodegenError> {
        match ty {
            TypeRef::Named { name, .. } => {
                let rust_name = match name.name.as_str() {
                    "Int" => "i64",
                    "Byte" => "u8",
                    "Bits" => "u64",
                    "Float" => "f32",
                    "Double" => "f64",
                    "Bool" => "bool",
                    "String" => "String",
                    "Decimal" => "rust_decimal::Decimal",
                    other => other,
                };
                Ok(rust_path_type(rust_name))
            }
            TypeRef::Option(inner, _) => {
                let inner_ty = self.ast_typeref_to_syn(inner)?;
                Ok(make_generic_path_type("Option", vec![inner_ty]))
            }
            TypeRef::Generic { base, args, .. } => {
                // Lower the base type to a path string (we only support Named base for now).
                let base_name = match base.as_ref() {
                    TypeRef::Named { name, .. } => name.name.clone(),
                    _ => return Err(self.unsupported("generic with non-named base type")),
                };
                let lowered_args: Result<Vec<SynType>, CodegenError> =
                    args.iter().map(|a| self.ast_typeref_to_syn(a)).collect();
                let lowered_args = lowered_args?;
                Ok(make_generic_path_type(&base_name, lowered_args))
            }
            TypeRef::Function { .. } => Err(self.unsupported("function-type codegen (T12/T13)")),
        }
    }

    /// Map a resolved Deox [`Type`] (post-inference) to a Rust [`syn::Type`].
    ///
    /// Returns `None` for [`Type::Unknown`] and [`Type::Void`] — callers
    /// (notably `let` lowering) treat `None` as "no annotation emitted".
    /// [`Type::Decimal`] maps to `rust_decimal::Decimal` (the crate is a
    /// dependency of `deox-codegen-rust` so generated crates must depend
    /// on it as well — the runtime/driver is responsible for that).
    fn deox_type_to_syn(&self, ty: &Type) -> Option<SynType> {
        let rust_name: &str = match ty {
            Type::Int {
                width: IntWidth::W8,
            } => "i8",
            Type::Int {
                width: IntWidth::W16,
            } => "i16",
            Type::Int {
                width: IntWidth::W32,
            } => "i32",
            Type::Int {
                width: IntWidth::W64,
            } => "i64",
            Type::Int {
                width: IntWidth::W128,
            } => "i128",
            Type::Bits {
                width: IntWidth::W8,
            } => "u8",
            Type::Bits {
                width: IntWidth::W16,
            } => "u16",
            Type::Bits {
                width: IntWidth::W32,
            } => "u32",
            Type::Bits {
                width: IntWidth::W64,
            } => "u64",
            Type::Bits {
                width: IntWidth::W128,
            } => "u128",
            // f16 is unstable in std; we map to f32 as a safe approximation.
            Type::Float {
                width: FloatWidth::W16,
            } => "f32",
            Type::Float {
                width: FloatWidth::W32,
            } => "f32",
            Type::Float {
                width: FloatWidth::W64,
            } => "f64",
            Type::Double => "f64",
            Type::Bool => "bool",
            Type::String => "String",
            Type::Decimal => "rust_decimal::Decimal",
            Type::Unknown | Type::Void => return None,
        };
        Some(rust_path_type(rust_name))
    }

    fn unsupported(&self, what: &str) -> CodegenError {
        CodegenError::new(Diagnostic::error(
            format!("unsupported: {what}"),
            DeoxSpan::dummy(),
        ))
    }
}

impl Default for RustCodegen {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Convert a Deox [`deox_ast::Ident`] into a `syn::Ident`. The byte offsets
/// in the Deox span don't carry over (proc-macro2 spans are opaque), so we
/// just use `call_site` here. The source-map mapping (Deox span → Rust
/// line/col) is recorded separately in [`CodegenContext`].
fn ast_ident_to_syn(ident: &deox_ast::common::Ident) -> Ident {
    Ident::new(&ident.name, ProcSpan::call_site())
}

/// Build a `syn::Type::Path` from a `::`-separated Rust type name string
/// (e.g. `"i64"`, `"bool"`, `"rust_decimal::Decimal"`). Each `::`-separated
/// segment becomes a [`syn::PathSegment`]. The result is always a plain path
/// with no generic arguments.
fn rust_path_type(name: &str) -> SynType {
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    for seg in name.split("::") {
        segments.push(syn::PathSegment {
            ident: Ident::new(seg, ProcSpan::call_site()),
            arguments: syn::PathArguments::None,
        });
    }
    SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments,
        },
    })
}

/// Build a `println!("{}", arg)` macro invocation as a `syn::Expr::Macro`.
///
/// Used by the `print(x)` → `println!("{}", x)` mapping (T13). The macro
/// token stream is built via `quote!` so it round-trips through `syn`'s
/// printer without any hand-rolled string formatting.
fn make_println_macro(arg: SynExpr) -> SynExpr {
    SynExpr::Macro(syn::ExprMacro {
        attrs: Vec::new(),
        mac: syn::Macro {
            path: syn::Path::from(Ident::new("println", ProcSpan::call_site())),
            bang_token: Default::default(),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: quote::quote! { "{}", #arg },
        },
    })
}

/// Mirror of the private `typeref_to_type` in `deox_types::infer`.
///
/// Used by [`RustCodegen::lower_func`] to seed the [`TypeInferencer`]
/// environment with function-parameter types so subsequent `let`
/// bindings can refer to params and still get a useful inferred type.
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

/// Build a `Type::Path` with generic type arguments, e.g.
/// `Option<T>`, `Vec<T>`.
fn make_generic_path_type(name: &str, args: Vec<SynType>) -> SynType {
    let mut path_args: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
    for a in args {
        path_args.push(syn::GenericArgument::Type(a));
    }
    let segment = syn::PathSegment {
        ident: Ident::new(name, ProcSpan::call_site()),
        arguments: syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: path_args,
            gt_token: Default::default(),
        }),
    };
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    segments.push(segment);
    SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments,
        },
    })
}

/// Format a float so that it always has a decimal point or exponent (so the
/// `f32`/`f64` suffix binds to a float literal, not an integer).
fn float_repr(d: f64) -> String {
    let s = format!("{d}");
    if s.contains('.')
        || s.contains('e')
        || s.contains('E')
        || s == "inf"
        || s == "-inf"
        || s == "NaN"
    {
        s
    } else {
        format!("{s}.0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deox_ast::common::{Block, Ident as AstIdent, Param};
    use deox_ast::{op::BinaryOp, op::UnaryOp, Literal};
    use deox_error::Span;

    fn dummy_span() -> Span {
        Span::dummy()
    }

    fn int_lit(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n), dummy_span())
    }

    fn ident_expr(s: &str) -> Expr {
        Expr::Ident(AstIdent::new(s, dummy_span()), dummy_span())
    }

    #[test]
    fn empty_func_generates_syn_file() {
        let func = FuncDecl {
            name: AstIdent::new("empty", dummy_span()),
            params: Vec::new(),
            return_type: None,
            body: Block::empty(dummy_span()),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            span: dummy_span(),
        };
        let mut codegen = RustCodegen::new();
        let file = codegen
            .generate(&[Decl::FuncDecl(func)])
            .expect("empty func must codegen");
        assert_eq!(file.items.len(), 1);
        assert!(matches!(file.items[0], Item::Fn(_)));
    }

    #[test]
    fn binary_op_lowers_to_expr_binary() {
        let mut codegen = RustCodegen::new();
        let lhs = int_lit(1);
        let rhs = int_lit(2);
        let expr = Expr::BinaryOp {
            op: BinaryOp::Add,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span: dummy_span(),
        };
        let syn_expr = codegen.lower_expr(&expr).unwrap();
        assert!(matches!(syn_expr, SynExpr::Binary(_)));
    }

    #[test]
    fn unary_neg_lowers_correctly() {
        let mut codegen = RustCodegen::new();
        let operand = int_lit(5);
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(operand),
            span: dummy_span(),
        };
        let syn_expr = codegen.lower_expr(&expr).unwrap();
        match syn_expr {
            SynExpr::Unary(u) => assert!(matches!(u.op, syn::UnOp::Neg(_))),
            other => panic!("expected Unary, got {other:?}"),
        }
    }

    #[test]
    fn type_int_maps_to_i64() {
        let codegen = RustCodegen::new();
        let tr = TypeRef::Named {
            name: AstIdent::new("Int", dummy_span()),
            span: dummy_span(),
        };
        let ty = codegen.ast_typeref_to_syn(&tr).unwrap();
        match ty {
            SynType::Path(p) => {
                let seg = p.path.segments.first().unwrap();
                assert_eq!(seg.ident.to_string(), "i64");
            }
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn type_option_maps_to_rust_option() {
        let codegen = RustCodegen::new();
        let tr = TypeRef::Option(
            Box::new(TypeRef::Named {
                name: AstIdent::new("Int", dummy_span()),
                span: dummy_span(),
            }),
            dummy_span(),
        );
        let ty = codegen.ast_typeref_to_syn(&tr).unwrap();
        match ty {
            SynType::Path(p) => {
                let seg = p.path.segments.first().unwrap();
                assert_eq!(seg.ident.to_string(), "Option");
                match &seg.arguments {
                    syn::PathArguments::AngleBracketed(ab) => assert_eq!(ab.args.len(), 1),
                    _ => panic!("expected angle-bracketed args"),
                }
            }
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn func_call_with_two_args_lowers() {
        let mut codegen = RustCodegen::new();
        let callee = ident_expr("foo");
        let args = vec![int_lit(1), int_lit(2)];
        let expr = Expr::FuncCall {
            callee: Box::new(callee),
            args,
            span: dummy_span(),
        };
        let syn_expr = codegen.lower_expr(&expr).unwrap();
        match syn_expr {
            SynExpr::Call(c) => assert_eq!(c.args.len(), 2),
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn struct_codegen_returns_unsupported_error() {
        let sd = deox_ast::decl::StructDecl {
            name: AstIdent::new("Foo", dummy_span()),
            fields: Vec::new(),
            traits: Vec::new(),
            span: dummy_span(),
        };
        let mut codegen = RustCodegen::new();
        let result = codegen.generate(&[Decl::StructDecl(sd)]);
        assert!(result.is_err());
    }

    #[test]
    fn float_repr_handles_integer_floats() {
        assert_eq!(float_repr(2.0), "2.0");
        assert_eq!(float_repr(2.5), "2.5");
    }

    #[test]
    fn make_let_pat_respects_mutability() {
        let pat = RustCodegen::make_let_pat(Ident::new("x", ProcSpan::call_site()), true);
        match pat {
            Pat::Ident(p) => assert!(p.mutability.is_some()),
            _ => panic!("expected Ident pat"),
        }
    }

    // Touch a few param/stmt shapes so unused-import warnings don't fire.
    #[test]
    fn _param_and_stmt_construction_smoke() {
        let _param = Param {
            name: AstIdent::new("p", dummy_span()),
            ty: TypeRef::Named {
                name: AstIdent::new("Int", dummy_span()),
                span: dummy_span(),
            },
            span: dummy_span(),
        };
        let _stmt = Stmt::Break(dummy_span());
    }
}
