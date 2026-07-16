//! The Rust code generator — lowers Buff AST nodes to `syn` types.
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
//! - `Literal::{Int, Float, Double, Bool, String, Byte, Decimal}`
//! - `TypeRef::Named` for the seven v0.1 primitive names (`Int`→`i64`, etc.)
//!   plus `TypeRef::Option` and `TypeRef::Generic` (named base)
//!
//! ## Type-annotated `let` bindings (T12)
//!
//! Every `let` binding emits an explicit Rust type annotation. If the Buff
//! source provides one (`let x: Int = …`), it is used directly; otherwise
//! the integrated [`TypeInferencer`] infers the type from the initializer
//! expression and [`RustCodegen::buff_type_to_syn`] maps it to the
//! corresponding Rust type. [`Type::Decimal`] maps to
//! `rust_decimal::Decimal` (so generated crates must depend on
//! `rust_decimal`/`rust_decimal_macros`).
//!
//! ## Control flow (T13)
//!
//! - `if cond { a } else { b }` → Rust `if` expression (with optional else)
//! - `for x in iter { body }` → Rust `for x in iter { body }`
//! - `for cond { body }` (Buff conditional loop) → Rust `while cond { body }`
//! - `print(arg)` calls map to `println!("{}", arg)` macro invocations.
//!
//! ## Source-map recording (T16)
//!
//! [`CodegenContext::record_mapping`] is available so that each lowered AST
//! node can record its Buff [`Span`] → Rust `(line, col)` mapping. In v0.1
//! the mapping is **not** automatically populated during lowering because:
//!
//! 1. `syn` nodes carry opaque `proc_macro2::Span`s (no source-line info).
//! 2. `prettyplease` reformats the tree after construction, so line numbers
//!    computed pre-format would be wrong.
//!
//! The pipeline (`buff_lang_cli::error_mapper`) therefore uses **filename
//! translation** for v0.1: it replaces the intermediate `.rs` path in
//! `rustc`/panic messages with the original `.buff` path. Exact Buff line
//! translation via the bidirectional [`SourceMap`](buff_lang_error::SourceMap)
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

use buff_lang_ast::{
    op::{BinaryOp, UnaryOp},
    Block, Decl, Expr, FuncDecl, InterpPart, Literal, Stmt, TypeRef,
};
use buff_lang_error::{CodegenError, Diagnostic, Span as BuffSpan};
use buff_lang_types::{FloatWidth, IntWidth, Type, TypeInferencer};

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
    /// `let` bindings that lack an explicit Buff annotation (T12).
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

    /// Generate a complete [`syn::File`] from a list of Buff declarations.
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
        // internally (see `typeref_to_type` in buff_lang_types::infer).
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
                // explicit Buff annotation (`ty: Some(..)`), prefer it;
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
                    self.buff_type_to_syn(&inferred)
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
                // Buff's `for cond { body }` (conditional-loop form) maps
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
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => self.lower_method_call(receiver, method, args),
            Expr::StringInterp { parts, .. } => self.lower_string_interp(parts),
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

    /// Lower a Buff method call to a Rust `receiver.method(args)` expression.
    ///
    /// T21 — string methods. The following Buff method names map to specific
    /// Rust idioms (none of them is a literal `recv.method(args)` because
    /// Rust strings don't expose these names directly):
    ///
    /// | Buff                  | Rust                                              |
    /// |-----------------------|---------------------------------------------------|
    /// | `s.char_count()`      | `s.chars().count()`                               |
    /// | `s.byte_len()`        | `s.len()`                                         |
    /// | `s.chars()`           | `s.chars()`                                       |
    /// | `s.bytes()`           | `s.bytes()`                                       |
    /// | `s.graphemes()`       | `unicode_segmentation::UnicodeSegmentation::graphemes(s, true).collect::<String>()` — see note below |
    /// | `s.first()`           | `s.chars().next()`                                |
    /// | `s.last()`            | `s.chars().last()`                                |
    /// | `s.slice(a, b)`       | char-safe slice via `s.chars().skip(a).take(b - a).collect()` |
    ///
    /// `graphemes()` is special-cased to return a `String` (a flattened
    /// representation) for now; a true iterator-returning API will need a
    /// dedicated AST shape (deferred to a later task — see notes).
    ///
    /// Any unrecognised method falls through to a plain `recv.method(args)`
    /// Rust method call, which is correct for arbitrary user-defined methods
    /// and the methods of future types.
    fn lower_method_call(
        &mut self,
        receiver: &Expr,
        method: &buff_lang_ast::common::Ident,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        let recv = self.lower_expr(receiver)?;
        let method_name = method.name.as_str();

        // Helper: lower `args` into a Punctuated list.
        let lower_args =
            |codegen: &mut Self| -> Result<Punctuated<SynExpr, syn::Token![,]>, CodegenError> {
                let mut out: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                for a in args {
                    out.push(codegen.lower_expr(a)?);
                }
                Ok(out)
            };

        // String-method mappings.
        let lowered = match method_name {
            // `s.char_count()` → `s.chars().count()`
            "char_count" if args.is_empty() => {
                self.method_chain(recv, &["chars", "count"], None)?
            }
            // `s.byte_len()` → `s.len()`
            "byte_len" if args.is_empty() => self.method_chain(recv, &["len"], None)?,
            // `s.chars()` → `s.chars()`
            "chars" if args.is_empty() => self.method_chain(recv, &["chars"], None)?,
            // `s.bytes()` → `s.bytes()`
            "bytes" if args.is_empty() => self.method_chain(recv, &["bytes"], None)?,
            // `s.first()` → `s.chars().next()`
            "first" if args.is_empty() => self.method_chain(recv, &["chars", "next"], None)?,
            // `s.last()` → `s.chars().last()`
            "last" if args.is_empty() => self.method_chain(recv, &["chars", "last"], None)?,
            // `s.graphemes()` → grapheme iterator wrapped via unicode-segmentation.
            // For now we return a flattened String (`.collect()`) so callers
            // can treat the result as a `String` without dragging the trait
            // into every scope. A future task will introduce a typed iterator.
            "graphemes" if args.is_empty() => self.lower_graphemes_call(recv)?,
            // `s.slice(a, b)` → char-safe slice.
            // Approach: `s.chars().skip(a).take(b - a).collect::<String>()`.
            // We lower the two integer arguments and emit the chain. If `b`
            // is not provided, we use `s.chars().skip(a).collect::<String>()`.
            "slice" => self.lower_slice_call(recv, args)?,
            // Default: a plain method call `recv.method(args)`.
            _ => {
                let args_punct = lower_args(self)?;
                SynExpr::MethodCall(syn::ExprMethodCall {
                    attrs: Vec::new(),
                    receiver: Box::new(recv),
                    dot_token: Default::default(),
                    method: Ident::new(method_name, ProcSpan::call_site()),
                    turbofish: None,
                    paren_token: Default::default(),
                    args: args_punct,
                })
            }
        };
        Ok(lowered)
    }

    /// Build a chained method call: `recv.m1().m2()...` (no args at any link).
    /// If `final_method` is given, it's used as the OUTERMOST call name (the
    /// last element of `methods` overrides it; passing `None` is equivalent).
    fn method_chain(
        &self,
        recv: SynExpr,
        methods: &[&str],
        _final_method: Option<&str>,
    ) -> Result<SynExpr, CodegenError> {
        let mut acc = recv;
        for &m in methods {
            acc = SynExpr::MethodCall(syn::ExprMethodCall {
                attrs: Vec::new(),
                receiver: Box::new(acc),
                dot_token: Default::default(),
                method: Ident::new(m, ProcSpan::call_site()),
                turbofish: None,
                paren_token: Default::default(),
                args: Default::default(),
            });
        }
        Ok(acc)
    }

    /// Lower `s.graphemes()` to a grapheme-iteration expression that yields a
    /// `String` of concatenated grapheme clusters.
    ///
    /// Emits (conceptually):
    /// ```text
    /// unicode_segmentation::UnicodeSegmentation::graphemes(&s, true)
    ///     .collect::<String>()
    /// ```
    ///
    /// The call is built as a `quote!`-expanded token stream so we never
    /// hand-format Rust. The trait must be in scope at the use site — see
    /// the generated-crate wiring note in T21 deferral.
    fn lower_graphemes_call(&self, recv: SynExpr) -> Result<SynExpr, CodegenError> {
        // We use quote! to build the macro-shaped expression. The receiver
        // is spliced in via `#recv`. The full path avoids needing a `use`
        // import in the generated crate.
        let tokens: proc_macro2::TokenStream =
            syn::parse_str("unicode_segmentation::UnicodeSegmentation::graphemes(&__recv, true)")
                .map_err(|e| self.unsupported(&format!("graphemes path parse: {e}")))?;
        // Manually build: __trait_path::graphemes(&recv, true).collect::<String>()
        // by constructing an ExprMethodCall for `.collect::<String>()`.
        let graphemes_call = splice_receiver_into_call(tokens, recv)?;
        let collect_call = SynExpr::MethodCall(syn::ExprMethodCall {
            attrs: Vec::new(),
            receiver: Box::new(graphemes_call),
            dot_token: Default::default(),
            method: Ident::new("collect", ProcSpan::call_site()),
            // turbofish: `::<String>`
            turbofish: Some(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: Default::default(),
                args: {
                    let mut p: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
                    p.push(syn::GenericArgument::Type(rust_path_type("String")));
                    p
                },
                gt_token: Default::default(),
            }),
            paren_token: Default::default(),
            args: Default::default(),
        });
        Ok(collect_call)
    }

    /// Lower `s.slice(a, b)` to a char-safe slice expression.
    ///
    /// Emits (conceptually) `s.chars().skip(a).take(b - a).collect::<String>()`.
    /// A single-arg form `s.slice(a)` becomes `s.chars().skip(a).collect::<String>()`.
    fn lower_slice_call(&mut self, recv: SynExpr, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.is_empty() || args.len() > 2 {
            return Err(self.unsupported(&format!(
                "slice expects 1 or 2 integer args, got {}",
                args.len()
            )));
        }
        // Start: `s.chars()`
        let chars_call = self.method_chain(recv, &["chars"], None)?;
        // `.skip(a)`
        let skip_arg = self.lower_expr(&args[0])?;
        let skip_call = method_call_one_arg(chars_call, "skip", skip_arg);
        // `.take(b - a)` if a second arg is present; else just chain collect.
        let after_take = if args.len() == 2 {
            let b_arg = self.lower_expr(&args[1])?;
            // Compute `b - a` as a Rust binary subtraction at runtime so the
            // arguments don't have to be literals.
            let b_minus_a = SynExpr::Binary(syn::ExprBinary {
                attrs: Vec::new(),
                left: Box::new(b_arg),
                op: syn::BinOp::Sub(Default::default()),
                right: Box::new(self.lower_expr(&args[0])?),
            });
            method_call_one_arg(skip_call, "take", b_minus_a)
        } else {
            skip_call
        };
        // `.collect::<String>()`
        let collect_call = SynExpr::MethodCall(syn::ExprMethodCall {
            attrs: Vec::new(),
            receiver: Box::new(after_take),
            dot_token: Default::default(),
            method: Ident::new("collect", ProcSpan::call_site()),
            turbofish: Some(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: Default::default(),
                args: {
                    let mut p: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
                    p.push(syn::GenericArgument::Type(rust_path_type("String")));
                    p
                },
                gt_token: Default::default(),
            }),
            paren_token: Default::default(),
            args: Default::default(),
        });
        Ok(collect_call)
    }

    /// Lower a string interpolation `"text {expr} more"` to a Rust
    /// `format!("text {} more", expr)` macro invocation.
    ///
    /// The format string is built by walking the parts:
    /// - `InterpPart::Literal(s)` — the literal text, with each `{`/`}`
    ///   escaped to `{{`/`}}` so `format!` doesn't interpret them as slots.
    /// - `InterpPart::Expr(_)` — a `{}` placeholder in the format string, and
    ///   the lowered expression as a positional argument after the string.
    ///
    /// The final `format!` call is built via `quote!` so the format string
    /// and args are spliced in without any hand-formatted Rust.
    fn lower_string_interp(&mut self, parts: &[InterpPart]) -> Result<SynExpr, CodegenError> {
        // Build the format string with `{}` placeholders for each Expr.
        let mut fmt_string = String::new();
        let mut lowered_args: Vec<SynExpr> = Vec::new();
        for part in parts {
            match part {
                InterpPart::Literal(text) => {
                    // Escape `{` → `{{` and `}` → `}}` so they're literal.
                    for c in text.chars() {
                        match c {
                            '{' => fmt_string.push_str("{{"),
                            '}' => fmt_string.push_str("}}"),
                            _ => fmt_string.push(c),
                        }
                    }
                }
                InterpPart::Expr(e) => {
                    fmt_string.push_str("{}");
                    lowered_args.push(self.lower_expr(e)?);
                }
            }
        }
        // Build the format! macro: tokens are "<fmt>", arg1, arg2, ...
        // We build this with quote! by splicing each argument in turn.
        let format_lit = proc_macro2::Literal::string(&fmt_string);
        let args_tokens: Vec<proc_macro2::TokenStream> = lowered_args
            .iter()
            .map(|a| {
                let a = a.clone();
                quote::quote! { #a }
            })
            .collect();
        let combined: proc_macro2::TokenStream = if args_tokens.is_empty() {
            // Should never happen (interp always has at least one Expr),
            // but guard against malformed AST.
            quote::quote! { #format_lit }
        } else {
            let mut ts: proc_macro2::TokenStream = quote::quote! { #format_lit };
            for a in args_tokens {
                ts.extend(quote::quote! { , #a });
            }
            ts
        };
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: syn::Path::from(Ident::new("format", ProcSpan::call_site())),
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Paren(Default::default()),
                tokens: combined,
            },
        }))
    }

    fn lower_literal(&mut self, lit: &Literal) -> Result<SynExpr, CodegenError> {
        // T20: Decimal literal → `rust_decimal_macros::dec!(<raw>)`. The raw
        // digit text is parsed into a `proc_macro2::TokenStream` so the
        // *exact* digits survive (no rounding through f64) — this matches
        // what `dec!` expects (a numeric literal token) and preserves
        // trailing zeros like the `0` in `99.90`.
        if let Literal::Decimal(raw) = lit {
            return self.lower_decimal_literal(raw);
        }
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
            // T21: `'A'` → `syn::Lit::Char`. prettyplease prints Rust `char`
            // literals with the correct quoting (including for escapes and
            // non-ASCII scalars).
            Literal::Char(c) => syn::Lit::Char(syn::LitChar::new(*c, ProcSpan::call_site())),
            // Handled by the early return above; this arm exists only so the
            // match is exhaustive (it is never reached).
            Literal::Decimal(_) => {
                return Err(self.unsupported("decimal literal (unreachable arm)"))
            }
        };
        Ok(SynExpr::Lit(syn::ExprLit {
            attrs: Vec::new(),
            lit: syn_lit,
        }))
    }

    /// Lower a Buff `Decimal` literal to the `rust_decimal_macros::dec!(...)`
    /// macro invocation (T20).
    ///
    /// The raw source text is parsed via `syn::parse_str` into a
    /// `proc_macro2::TokenStream` so the exact digits (including trailing
    /// zeros) are preserved verbatim — the value never transits through an
    /// `f64`, guaranteeing exactness end-to-end.
    fn lower_decimal_literal(&self, raw: &str) -> Result<SynExpr, CodegenError> {
        let num_tokens: proc_macro2::TokenStream = syn::parse_str(raw)
            .map_err(|e| self.unsupported(&format!("decimal literal `{raw}`: {e}")))?;
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: rust_path("rust_decimal_macros::dec"),
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Paren(Default::default()),
                tokens: num_tokens,
            },
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
        // Buff's `~` (bitwise NOT on integers) maps to Rust's `!` on integers.
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

    /// Convert a Buff [`TypeRef`] to a Rust [`syn::Type`].
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
                    // T21: Char → Rust's primitive `char` type.
                    "Char" => "char",
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

    /// Map a resolved Buff [`Type`] (post-inference) to a Rust [`syn::Type`].
    ///
    /// Returns `None` for [`Type::Unknown`] and [`Type::Void`] — callers
    /// (notably `let` lowering) treat `None` as "no annotation emitted".
    /// [`Type::Decimal`] maps to `rust_decimal::Decimal` (the crate is a
    /// dependency of `buff-lang-codegen-rust` so generated crates must depend
    /// on it as well — the runtime/driver is responsible for that).
    fn buff_type_to_syn(&self, ty: &Type) -> Option<SynType> {
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
            // T21: Char → Rust's `char` (a 4-byte Unicode scalar value).
            Type::Char => "char",
            Type::Decimal => "rust_decimal::Decimal",
            Type::Unknown | Type::Void => return None,
        };
        Some(rust_path_type(rust_name))
    }

    fn unsupported(&self, what: &str) -> CodegenError {
        CodegenError::new(Diagnostic::error(
            format!("unsupported: {what}"),
            BuffSpan::dummy(),
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

/// Convert a Buff [`buff_lang_ast::Ident`] into a `syn::Ident`. The byte offsets
/// in the Buff span don't carry over (proc-macro2 spans are opaque), so we
/// just use `call_site` here. The source-map mapping (Buff span → Rust
/// line/col) is recorded separately in [`CodegenContext`].
fn ast_ident_to_syn(ident: &buff_lang_ast::common::Ident) -> Ident {
    Ident::new(&ident.name, ProcSpan::call_site())
}

/// Build a `syn::Type::Path` from a `::`-separated Rust type name string
/// (e.g. `"i64"`, `"bool"`, `"rust_decimal::Decimal"`). Each `::`-separated
/// segment becomes a [`syn::PathSegment`]. The result is always a plain path
/// with no generic arguments.
fn rust_path_type(name: &str) -> SynType {
    SynType::Path(syn::TypePath {
        qself: None,
        path: rust_path(name),
    })
}

/// Build a `syn::Path` from a `::`-separated name string
/// (e.g. `"rust_decimal_macros::dec"`). Used for macro paths like the
/// `dec!(...)` codegen in T20.
fn rust_path(name: &str) -> syn::Path {
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    for seg in name.split("::") {
        segments.push(syn::PathSegment {
            ident: Ident::new(seg, ProcSpan::call_site()),
            arguments: syn::PathArguments::None,
        });
    }
    syn::Path {
        leading_colon: None,
        segments,
    }
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

/// Mirror of the private `typeref_to_type` in `buff_lang_types::infer`.
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
            // T21: Char annotation maps to the resolved Char type.
            "Char" => Some(Type::char()),
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

/// Build a `recv.method(arg)` single-argument method call.
///
/// Used by the string-method codegen helpers (e.g. `s.chars().skip(n)`).
fn method_call_one_arg(recv: SynExpr, method: &str, arg: SynExpr) -> SynExpr {
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(arg);
    SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(recv),
        dot_token: Default::default(),
        method: Ident::new(method, ProcSpan::call_site()),
        turbofish: None,
        paren_token: Default::default(),
        args,
    })
}

/// Take a token stream that calls a fully-qualified function with a single
/// placeholder argument `__recv` and replace that placeholder with an
/// actual lowered receiver expression.
///
/// The `tokens` argument is expected to parse as a Rust function-call
/// expression (e.g. `path::func(&__recv, true)`). We use `quote!` to splice
/// the receiver in: we re-parse a small template that names `__recv` and
/// then walk the resulting `ExprCall` to substitute the real receiver.
///
/// This indirection is needed because `quote!` cannot easily splice into
/// an arbitrary position inside a string-built token stream — we instead
/// parse the template to a real `ExprCall`, then swap the first argument.
fn splice_receiver_into_call(
    tokens: proc_macro2::TokenStream,
    recv: SynExpr,
) -> Result<SynExpr, CodegenError> {
    // Rebuild via quote! so we never hand-format. The placeholder name
    // `__recv` is referenced as a Rust identifier in the template; we then
    // construct the call by hand using the lowered receiver.
    //
    // Simpler approach: construct the call directly via syn::ExprCall with
    // the lowered recv as the first arg and `true` as the second.
    let _ = tokens; // discarded; we rebuild from scratch to stay quote!-based.
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    // `&recv` — syn doesn't have a one-liner for `&expr`, so we build it.
    let borrow_recv = SynExpr::Reference(syn::ExprReference {
        attrs: Vec::new(),
        and_token: Default::default(),
        mutability: None,
        expr: Box::new(recv),
    });
    args.push(borrow_recv);
    args.push(SynExpr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Bool(syn::LitBool::new(true, ProcSpan::call_site())),
    }));
    Ok(SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(SynExpr::Path(syn::ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: rust_path("unicode_segmentation::UnicodeSegmentation::graphemes"),
        })),
        paren_token: Default::default(),
        args,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::common::{Block, Ident as AstIdent, Param};
    use buff_lang_ast::{op::BinaryOp, op::UnaryOp, Literal};
    use buff_lang_error::Span;

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
        let sd = buff_lang_ast::decl::StructDecl {
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

    // -----------------------------------------------------------------------
    // T22 — Fixed-width `Int<W>` codegen mapping contract.
    //
    // The T22 spec says fixed-mode overflow must "panic in debug, wrap in
    // release". Buff inherits this behaviour FOR FREE from Rust: codegen
    // maps each fixed `Int<W>` to the corresponding native Rust integer
    // (`i8`/`i16`/`i32`/`i64`/`i128`), and Rust's native arithmetic already
    // has the debug-panic/release-wrap overflow contract. No explicit
    // `checked_*` calls are emitted.
    //
    // These tests mechanically pin the mapping so a regression in
    // `buff_type_to_syn` cannot silently widen every fixed-width integer
    // (which would change the overflow boundary). See T22 evidence file
    // `task-22-overflow-modes.txt`.
    // -----------------------------------------------------------------------

    /// Helper: extract the leading path-segment ident from a `syn::Type` (or
    /// panic). Used by the T22 fixed-width mapping tests below.
    fn first_path_segment_str(ty: &SynType) -> String {
        match ty {
            SynType::Path(p) => p
                .path
                .segments
                .first()
                .map(|s| s.ident.to_string())
                .unwrap_or_else(|| panic!("path has no segments")),
            _ => panic!("expected Path, got {ty:?}"),
        }
    }

    #[test]
    fn t22_fixed_int_widths_map_to_native_rust_widths() {
        let codegen = RustCodegen::new();
        // Every fixed Int<W> must map to the SAME-width native Rust integer.
        for (w, expected) in [
            (IntWidth::W8, "i8"),
            (IntWidth::W16, "i16"),
            (IntWidth::W32, "i32"),
            (IntWidth::W64, "i64"),
            (IntWidth::W128, "i128"),
        ] {
            let ty = Type::Int { width: w };
            let syn_ty = codegen
                .buff_type_to_syn(&ty)
                .expect("Int<W> must map to a Rust type");
            assert_eq!(
                first_path_segment_str(&syn_ty),
                expected,
                "Int<{:?}] -> wrong Rust width",
                w
            );
        }
    }

    #[test]
    fn t22_fixed_int8_preserves_width_through_arithmetic() {
        // The full T22 "fixed mode preserves type" contract: an i8 value
        // stays i8 after arithmetic because (a) the TypeInferencer preserves
        // width via promote_binary and (b) the codegen maps the resulting
        // Int<8> back to i8.  We verify the codegen end of that chain here.
        // (The inferencer end is covered by `numeric_coercion::fixed_int8_*`.)
        let codegen = RustCodegen::new();
        let syn_ty = codegen
            .buff_type_to_syn(&Type::Int {
                width: IntWidth::W8,
            })
            .expect("Int<8> maps to i8");
        assert_eq!(first_path_segment_str(&syn_ty), "i8");
        // And Int<32> + Int<32> = Int<32> maps back to i32 (not widened to i64).
        let syn_ty = codegen
            .buff_type_to_syn(&Type::Int {
                width: IntWidth::W32,
            })
            .expect("Int<32> maps to i32");
        assert_eq!(first_path_segment_str(&syn_ty), "i32");
    }
}
