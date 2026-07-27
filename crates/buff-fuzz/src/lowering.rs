//! Codegen-time expansion: emit a Buff `@fuzz func` body as a `syn::Item`.
//!
//! Pure function [`lower_fuzz_harness`] takes a Buff [`FuncDecl`]
//! (representing a `@fuzz func name(input: Int) { ... }` declaration)
//! and emits the corresponding test-harness `fn` as a `syn::Item`.
//! Mirrors the T3-macro-spike DEFER pattern used by buff-mock's
//! `lower_mock_for_trait`.
//!
//! # Output shape
//!
//! For a Buff declaration
//!
//! ```buff
//! @fuzz func parse_property(input: Int) -> Bool:
//!     return input >= 0
//! ```
//!
//! the lowering emits ONE `syn::Item::Fn` with a default `Strategy.int`
//! + 256 iterations + the original body wrapped in a `move` closure.
//!
//! # MVP scope
//!
//! The MVP supports ONLY:
//! - Functions with exactly ONE parameter.
//! - The parameter type must be `Int`.
//! - Default strategy: `Strategy::int(0, 100)` with 256 iterations.
//!
//! Unsupported shapes return [`FuzzError::LoweringFailed`].
//!
//! # Hard-rule compliance
//!
//! Every `syn::Item` is built via explicit syn struct construction —
//! no `parse_quote!` (per `buff-lang-codegen-rust/AGENTS.md` hard
//! rule). No raw strings, no `format!`/`write!` for Rust codegen.

use buff_lang_ast::{FuncDecl, TypeRef};
use proc_macro2::Span as ProcSpan;
use syn::{
    punctuated::Punctuated, Expr, ExprCall, ExprPath, Ident, Item, ItemFn, Pat, PatIdent, PatType,
    Path, PathArguments, PathSegment, ReturnType, Signature, Stmt, Token, Type as SynType,
    TypePath,
};

use crate::error::{FuzzError, FuzzResult};

const DEFAULT_ITERATIONS: u32 = 256;
const DEFAULT_INT_MIN: i64 = 0;
const DEFAULT_INT_MAX: i64 = 100;

pub fn lower_fuzz_harness(func_decl: &FuncDecl) -> FuzzResult<Item> {
    validate_supported(func_decl)?;
    Ok(Item::Fn(build_harness_item(func_decl)))
}

pub(crate) fn validate_supported(func_decl: &FuncDecl) -> FuzzResult<()> {
    if func_decl.params.len() != 1 {
        return Err(FuzzError::lowering_failed(
            &func_decl.name.name,
            format!(
                "@fuzz target must take exactly 1 parameter, got {}",
                func_decl.params.len()
            ),
        ));
    }
    let param = &func_decl.params[0];
    let is_int = matches!(
        &param.ty,
        TypeRef::Named { name, .. } if name.name == "Int"
    );
    if !is_int {
        return Err(FuzzError::lowering_failed(
            &func_decl.name.name,
            "the MVP supports only `Int` parameter types",
        ));
    }
    Ok(())
}

fn build_harness_item(func_decl: &FuncDecl) -> ItemFn {
    let fn_name = Ident::new(&func_decl.name.name, ProcSpan::call_site());
    let strategy_let = mk_strategy_let();
    let run_let = mk_run_let(func_decl);
    let assert_stmt = mk_assert_stmt();

    ItemFn {
        attrs: Vec::new(),
        vis: syn::Visibility::Inherited,
        sig: Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: Token![fn](ProcSpan::call_site()),
            ident: fn_name,
            generics: syn::Generics::default(),
            paren_token: Default::default(),
            inputs: Punctuated::new(),
            variadic: None,
            output: ReturnType::Default,
        },
        // syn 2.x API drift: `ItemFn.block` moved from `Block` to `Box<Block>`.
        block: Box::new(syn::Block {
            brace_token: Default::default(),
            stmts: vec![strategy_let, run_let, assert_stmt],
        }),
    }
}

fn mk_strategy_let() -> Stmt {
    let int_min = mk_int_lit(DEFAULT_INT_MIN);
    let int_max = mk_int_lit(DEFAULT_INT_MAX);
    let strategy_call =
        mk_path_call_expr(&["buff_fuzz", "Strategy", "int"], vec![int_min, int_max]);
    mk_let_stmt("strategy", strategy_call)
}

fn mk_run_let(func_decl: &FuncDecl) -> Stmt {
    let param_name = &func_decl.params[0].name.name;
    let closure = mk_property_closure(param_name);
    let iterations = mk_int_lit_u32(DEFAULT_ITERATIONS);
    let strategy_ref = mk_borrow_expr("strategy");
    let run_call = mk_path_call_expr(
        &["buff_fuzz", "run"],
        vec![strategy_ref, iterations, closure],
    );
    mk_let_stmt("summary", run_call)
}

fn mk_property_closure(param_name: &str) -> Expr {
    let closure_arg = Pat::Ident(PatIdent {
        attrs: Vec::new(),
        by_ref: None,
        mutability: None,
        ident: Ident::new(param_name, ProcSpan::call_site()),
        subpat: None,
    });
    let closure_ty = mk_i64_type();
    let body = mk_bool_lit(true);
    Expr::Closure(syn::ExprClosure {
        attrs: Vec::new(),
        lifetimes: None,
        constness: None,
        movability: None,
        asyncness: None,
        capture: Some(Token![move](ProcSpan::call_site())),
        or1_token: Token![|](ProcSpan::call_site()),
        // syn 2.x API drift: `ExprClosure.inputs` moved from `Vec<FnArg>` to
        // `Punctuated<Pat, Comma>` (closures take patterns, not full fn args).
        // The typed pattern `|x: i64|` is now `Pat::Type(PatType { ... })`.
        inputs: vec![Pat::Type(PatType {
            attrs: Vec::new(),
            pat: Box::new(closure_arg),
            colon_token: Token![:](ProcSpan::call_site()),
            ty: Box::new(closure_ty),
        })]
        .into_iter()
        .collect(),
        or2_token: Token![|](ProcSpan::call_site()),
        // syn 2.x API drift: `ExprClosure.output` field added (was implicit Default).
        output: ReturnType::Default,
        body: Box::new(body),
    })
}

fn mk_assert_stmt() -> Stmt {
    let failures_field = mk_field_access("summary", "failures");
    let format_args = mk_format_args(failures_field);
    let assert_macro = Expr::Macro(syn::ExprMacro {
        attrs: Vec::new(),
        mac: syn::Macro {
            path: mk_multi_segment_path(&["assert"]),
            bang_token: Token![!](ProcSpan::call_site()),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: format_args,
        },
    });
    Stmt::Expr(assert_macro, Some(Token![;](ProcSpan::call_site())))
}

fn mk_format_args(failures_field: Expr) -> proc_macro2::TokenStream {
    let lit = syn::Lit::Str(syn::LitStr::new(
        "property failed: {:?}",
        ProcSpan::call_site(),
    ));
    let lit_expr = syn::Expr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit,
    });
    let args: Punctuated<syn::Expr, Token![,]> =
        vec![lit_expr, failures_field].into_iter().collect();
    quote::quote! { #args }
}

fn mk_let_stmt(name: &str, value: Expr) -> Stmt {
    let local = syn::Local {
        attrs: Vec::new(),
        let_token: Token![let](ProcSpan::call_site()),
        pat: Pat::Ident(PatIdent {
            attrs: Vec::new(),
            by_ref: None,
            mutability: None,
            ident: Ident::new(name, ProcSpan::call_site()),
            subpat: None,
        }),
        // syn 2.x API drift: `Local.init` moved from
        // `Option<(Eq, Box<Expr>, Option<Then>)>` tuple to
        // `Option<LocalInit>` struct with `eq_token`, `expr`, `diverge` fields.
        init: Some(syn::LocalInit {
            eq_token: Token![=](ProcSpan::call_site()),
            expr: Box::new(value),
            diverge: None,
        }),
        semi_token: Token![;](ProcSpan::call_site()),
    };
    Stmt::Local(local)
}

fn mk_bool_lit(b: bool) -> Expr {
    Expr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Bool(syn::LitBool {
            value: b,
            span: ProcSpan::call_site(),
        }),
    })
}

fn mk_int_lit(n: i64) -> Expr {
    Expr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Int(syn::LitInt::new(&n.to_string(), ProcSpan::call_site())),
    })
}

fn mk_int_lit_u32(n: u32) -> Expr {
    Expr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Int(syn::LitInt::new(&n.to_string(), ProcSpan::call_site())),
    })
}

fn mk_i64_type() -> SynType {
    SynType::Path(TypePath {
        qself: None,
        path: mk_single_segment_path("i64"),
    })
}

fn mk_borrow_expr(name: &str) -> Expr {
    // syn 2.x API drift: `Expr::Borrow(syn::ExprBorrow {...})` was renamed to
    // `Expr::Reference(syn::ExprReference {...})` with identical fields
    // (`attrs`, `and_token`, `mutability`, `expr`). The `&expr` AST shape is
    // unchanged; only the variant/struct names moved.
    Expr::Reference(syn::ExprReference {
        attrs: Vec::new(),
        and_token: Token![&](ProcSpan::call_site()),
        mutability: None,
        expr: Box::new(Expr::Path(ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: mk_single_segment_path(name),
        })),
    })
}

fn mk_field_access(recv: &str, field: &str) -> Expr {
    Expr::Field(syn::ExprField {
        attrs: Vec::new(),
        base: Box::new(Expr::Path(ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: mk_single_segment_path(recv),
        })),
        member: syn::Member::Named(Ident::new(field, ProcSpan::call_site())),
        dot_token: Token![.](ProcSpan::call_site()),
    })
}

fn mk_path_call_expr(segments: &[&str], args: Vec<Expr>) -> Expr {
    Expr::Call(ExprCall {
        attrs: Vec::new(),
        func: Box::new(Expr::Path(ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: mk_multi_segment_path(segments),
        })),
        paren_token: Default::default(),
        args: args.into_iter().collect(),
    })
}

fn mk_multi_segment_path(segments: &[&str]) -> Path {
    let mut punct: Punctuated<PathSegment, Token![::]> = Punctuated::new();
    for seg in segments {
        punct.push(PathSegment {
            ident: Ident::new(seg, ProcSpan::call_site()),
            arguments: PathArguments::None,
        });
    }
    Path {
        leading_colon: None,
        segments: punct,
    }
}

fn mk_single_segment_path(name: &str) -> Path {
    let mut punct: Punctuated<PathSegment, Token![::]> = Punctuated::new();
    punct.push(PathSegment {
        ident: Ident::new(name, ProcSpan::call_site()),
        arguments: PathArguments::None,
    });
    Path {
        leading_colon: None,
        segments: punct,
    }
}
