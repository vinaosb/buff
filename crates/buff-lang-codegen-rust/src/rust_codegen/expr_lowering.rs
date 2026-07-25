//! T105a - expr construct/pattern/literal/op lowering: lambda..make_unary_op (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move of `impl RustCodegen` methods into this child module so the
//! parent file shrinks. Methods are pub(super); the parent declares only
//! `mod <name>;` (inherent methods resolve by type, no `use` needed). Child
//! inherits parent imports via use super::* and may call the parent private
//! methods (descendant privacy) and the extracted helper modules.

use super::*;

impl RustCodegen {
    /// Lower a minimal closure `{ params => expr }` to a Rust closure
    /// `|p1, p2| body` (T23 + T34 capture analysis).
    ///
    /// Param types are inferred by Rust — we emit no annotations (matching
    /// Buff's "hide the types" philosophy). The body is a single expression
    /// in T23's minimal shape; if the parser produced a multi-statement
    /// block, it is lowered as a block expression.
    ///
    /// # T34: variable capture
    ///
    /// Before lowering the body, we compute the set of variables CAPTURED
    /// by this closure (free vars of body minus params minus closure-local
    /// lets) via [`buff_lang_types::closure_captures`] — the shared
    /// capture analysis extracted from T33's spawn free-var walker. The
    /// capture set is pushed onto [`Self::closure_capture_stack`] so that
    /// [`Self::lower_expr`]'s `Expr::Ident` arm can emit captured
    /// variables plainly WITHOUT calling [`MoveAnalyzer::needs_clone`].
    ///
    /// Rust closures handle capture automatically (by ref or by move based
    /// on how the body uses the variable). Buff's job is only to AVOID
    /// inserting spurious `.clone()` calls for captured-variable uses
    /// INSIDE the closure body — without the capture stack, a non-Copy
    /// captured var used twice in a closure would get a wrong `.clone()`
    /// on its second use (MoveAnalyzer would see it as "use after move").
    pub(super) fn lower_lambda(
        &mut self,
        params: &[buff_lang_ast::common::Param],
        body: &Block,
    ) -> Result<SynExpr, CodegenError> {
        // Build the closure parameter patterns: `|p1, p2, ...|`.
        let mut pats: Punctuated<Pat, syn::Token![,]> = Punctuated::new();
        for p in params {
            pats.push(Pat::Ident(PatIdent {
                attrs: Vec::new(),
                ident: ast_ident_to_syn(&p.name),
                by_ref: None,
                mutability: None,
                subpat: None,
            }));
        }
        // T34: compute captures and push onto the stack so the body-
        // lowering path knows which idents are captured (and should
        // bypass needs_clone). Popped after the body is lowered so the
        // stack correctly reflects the enclosing scope on exit.
        //
        // We ALSO insert the closure's own PARAM names into the pushed
        // set: closure params are fresh bindings owned by the closure
        // body, and Rust handles their ownership within the body (Copy
        // params are copied, non-Copy by-value uses are Rust's concern).
        // Without this, a param used multiple times in the body (e.g.
        // `|x| x * x + x`) would get a spurious `.clone()` from the
        // MoveAnalyzer on its second+ use — a pre-existing T23 limitation
        // that T34's capture-aware codegen naturally fixes by treating
        // params the same as captures (bypass needs_clone inside the body).
        let mut bypass_set = buff_lang_types::closure_captures(params, body);
        for p in params {
            bypass_set.insert(p.name.name.clone());
        }
        self.closure_capture_stack.push(bypass_set);
        // Body: a single ExprStmt lowers to a bare expression; otherwise a
        // block expression.
        let body_expr = self.lower_lambda_body(body);
        // Always pop, even if body lowering errored, so the stack stays
        // balanced across error recovery paths.
        self.closure_capture_stack.pop();
        let body_expr = body_expr?;
        Ok(SynExpr::Closure(syn::ExprClosure {
            attrs: Vec::new(),
            lifetimes: Default::default(),
            constness: None,
            movability: None,
            asyncness: None,
            capture: None,
            or1_token: Default::default(),
            or2_token: Default::default(),
            inputs: pats,
            output: ReturnType::Default,
            body: Box::new(body_expr),
        }))
    }

    /// Lower a lambda body. If the block is a single `ExprStmt`, lower that
    /// expression directly (so `|x| x * 2` not `|x| { x * 2 }`); otherwise
    /// lower the block as a `syn::Expr::Block`.
    pub(super) fn lower_lambda_body(&mut self, body: &Block) -> Result<SynExpr, CodegenError> {
        if body.stmts.len() == 1 {
            if let Stmt::ExprStmt(e, _) = &body.stmts[0] {
                return self.lower_expr(e);
            }
        }
        let block = self.lower_block(body)?;
        Ok(SynExpr::Block(syn::ExprBlock {
            attrs: Vec::new(),
            label: None,
            block,
        }))
    }

    /// Lower a map literal `{"k": v, ...}` (or empty `{:}`) to Rust's
    /// `std::collections::HashMap::from([("k", v), ...])` (T25).
    ///
    /// Each entry's key and value are lowered independently and spliced into
    /// the outer array as Rust tuples. The fully-qualified path
    /// `std::collections::HashMap::from` is used (not a bare `HashMap::from`
    /// with a `use` import) so generated programs need NO import wiring.
    ///
    /// For an empty literal `{:}` we emit `HashMap::from([])` (Rust infers the
    /// key/value types from the `let`-binding annotation, which the codegen's
    /// type inferencer drives).
    ///
    /// The output is built via `quote!` so the outer `::from([...])` shell and
    /// the comma-separated tuple entries are constructed without any
    /// hand-formatted Rust strings.
    pub(super) fn lower_map_lit(
        &mut self,
        entries: &[(Expr, Expr)],
    ) -> Result<SynExpr, CodegenError> {
        // Lower each (key, value) pair into a Rust tuple expression. We
        // build the tuple via `syn::ExprTuple` so it's a real AST node
        // (not a token stream).
        let mut lowered_entries: Vec<SynExpr> = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let k_e = self.lower_expr(k)?;
            let v_e = self.lower_expr(v)?;
            let mut tuple_elems: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
            tuple_elems.push(k_e);
            tuple_elems.push(v_e);
            // A trailing comma is required by Rust for single-element tuples;
            // for 2-element tuples it's optional but harmless. We always add
            // one for uniformity.
            let tuple = SynExpr::Tuple(syn::ExprTuple {
                attrs: Vec::new(),
                paren_token: Default::default(),
                elems: tuple_elems,
            });
            lowered_entries.push(tuple);
        }
        // Build the outer `[<entries>]` array literal as a Rust expression
        // via `quote!`. Each lowered tuple is spliced in comma-separated.
        let mut entries_tokens: proc_macro2::TokenStream = proc_macro2::TokenStream::new();
        for (i, e) in lowered_entries.iter().enumerate() {
            if i > 0 {
                entries_tokens.extend(quote::quote! { , });
            }
            let e = e.clone();
            entries_tokens.extend(quote::quote! { #e });
        }
        // `std::collections::HashMap::from([<entries_tokens>])`
        let tokens: proc_macro2::TokenStream = quote::quote! {
            std::collections::HashMap::from([#entries_tokens])
        };
        syn::parse2(tokens)
            .map_err(|e| self.unsupported(&format!("map literal codegen parse: {e}")))
    }

    /// Lower a Buff struct-init expression `Type { field: value, ... }` to a
    /// Rust [`syn::ExprStruct`] of the same shape (T26).
    ///
    /// Each field is a `field: <lowered_value>` pair; the type path uses the
    /// struct name verbatim (Buff's struct names ARE Rust struct names — no
    /// renaming). The output is built via `quote!` so the brace-delimited
    /// body and comma-separated fields are constructed without any
    /// hand-formatted Rust strings.
    ///
    /// This mirrors the source form 1:1 because Buff deliberately matches
    /// Rust's struct-init syntax ( braces + named fields + colon ).
    pub(super) fn lower_struct_init(
        &mut self,
        type_name: &buff_lang_ast::common::Ident,
        fields: &[(buff_lang_ast::common::Ident, Expr)],
    ) -> Result<SynExpr, CodegenError> {
        // Lower each field value first.
        let mut lowered_fields: Vec<(Ident, SynExpr)> = Vec::with_capacity(fields.len());
        for (fname, fval) in fields {
            let v = self.lower_expr(fval)?;
            lowered_fields.push((ast_ident_to_syn(fname), v));
        }
        // Build `Type { f1: v1, f2: v2, ... }` via `quote!`. Splice each
        // field as `#fname: #fval` (both are syn expressions/idents that
        // `quote!` can interpolate).
        let type_path = rust_path(&type_name.name);
        let mut fields_tokens: proc_macro2::TokenStream = proc_macro2::TokenStream::new();
        for (i, (fname, fval)) in lowered_fields.iter().enumerate() {
            if i > 0 {
                fields_tokens.extend(quote::quote! { , });
            }
            fields_tokens.extend(quote::quote! { #fname: #fval });
        }
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #type_path { #fields_tokens }
        };
        syn::parse2(tokens)
            .map_err(|e| self.unsupported(&format!("struct init codegen parse: {e}")))
    }

    /// Lower a Buff `match scrutinee { arms }` to a Rust `syn::ExprMatch`
    /// (T27).
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// match <scrutinee> {
    ///     <pattern> => <body>,
    ///     <pattern> => <body>,
    ///     ...
    /// }
    /// ```
    ///
    /// Each arm's body is a `Block` (the parser wraps the single body
    /// expression in a one-statement block). The arm pattern goes through
    /// [`Self::lower_pattern`]; the body goes through [`Self::lower_block`].
    ///
    /// This mirrors the source form 1:1 because Buff deliberately matches
    /// Rust's `match` syntax. Exhaustiveness is checked separately by the
    /// `buff-lang-types` analysis pass; if a match is non-exhaustive the
    /// type-checker flags it BEFORE codegen runs (codegen assumes the match
    /// is well-formed).
    pub(super) fn lower_match_expr(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
    ) -> Result<SynExpr, CodegenError> {
        let scrut = self.lower_expr(scrutinee)?;
        // T86 + tail-expression fix: ALWAYS strip the trailing `;` on the
        // LAST statement of each arm body block. The parser wraps each arm
        // body as a one-stmt `Block { stmts: [Stmt::ExprStmt(e)] }`, and
        // the default [`Self::lower_block`] emits that as `{ e; }`
        // (statement with semi → block type `()`). For match arms this is
        // almost never what you want — arm bodies should YIELD the arm's
        // value type, not `()`. Turning `{ e; }` into `{ e }` (tail
        // expression → block yields the value) is strictly more general:
        // `{ e }` is valid Rust whether you want the value or want to
        // discard it, while `{ e; }` forces `()` and produces a type
        // mismatch whenever the match is used in a value context
        // (explicit `return match ...`, implicit tail-expression return,
        // `let v = match ...`, or even `match` as a sub-expression).
        //
        // Originally (T86) the strip was gated on
        // [`Self::return_position_depth`] > 0 so only explicit
        // `return match n { ... }` got the strip. That left the implicit
        // tail-expression case broken:
        //     func code_str(code: ErrorCode) -> String:
        //         match code {
        //             UnexpectedChar => "E1001",
        //             ...
        //         }
        // Here the match is the function body's LAST expression (implicit
        // return), but `return_position_depth` is 0 because there's no
        // `Stmt::Return` on the lowering stack. The generated
        // `{ "E1001"; }` arms yield `()`, mismatching the `-> String`
        // signature. Always stripping fixes both the explicit and implicit
        // cases uniformly.
        //
        // [`Self::return_position_depth`] is still incremented/decremented
        // by the `Stmt::Return` lowering arm for backward compatibility
        // and potential future consumers, but
        // [`Self::lower_match_expr`] no longer reads it.
        let mut arms_syn: Vec<syn::Arm> = Vec::with_capacity(arms.len());
        for arm in arms {
            let pat = self.lower_pattern(&arm.pattern, false)?;
            // T40: lower the optional `if <cond>` guard to a syn::Arm guard.
            // `Some(v) if v > 0 => ...` lowers to Rust's `Some(v) if v > 0 =>
            // ...` 1:1 (Rust match-arm guards use the identical `if` syntax,
            // so no translation is needed — just lower the guard expression).
            // This syn version's `Arm::guard` is `Option<(If, Box<Expr>)>` —
            // a tuple of the `if` keyword token + the condition expression.
            let guard = if let Some(guard_expr) = &arm.guard {
                Some((
                    syn::Token![if](ProcSpan::call_site()),
                    Box::new(self.lower_expr(guard_expr)?),
                ))
            } else {
                None
            };
            // The parser wraps the body expression in a one-statement
            // `ExprStmt` block. We lower the block and use it as the arm
            // body — Rust accepts a block as an arm body. If the block has
            // a single trailing expression, prettyplease will format it
            // back as `pat => expr,`; if it's multiple statements, the
            // block form `pat => { ... },` is emitted (also valid Rust).
            let mut body_block = self.lower_block(&arm.body)?;
            // Always strip: match arm bodies should yield a value, never
            // `()`. See the long comment above for rationale.
            strip_trailing_semi_on_last_expr_stmt(&mut body_block);
            let body_expr = SynExpr::Block(syn::ExprBlock {
                attrs: Vec::new(),
                label: None,
                block: body_block,
            });
            arms_syn.push(syn::Arm {
                attrs: Vec::new(),
                pat,
                guard,
                fat_arrow_token: Default::default(),
                body: Box::new(body_expr),
                comma: Some(Default::default()),
            });
        }
        Ok(SynExpr::Match(syn::ExprMatch {
            attrs: Vec::new(),
            match_token: Default::default(),
            expr: Box::new(scrut),
            brace_token: Default::default(),
            arms: arms_syn,
        }))
    }

    /// T82: lower a Map indexing READ `m[key]` to
    /// `m.get(&key).cloned().unwrap_or_default()`.
    ///
    /// Buff's "no panic on missing keys" convention: a missing key returns
    /// the default for the map's value type `V`, NEVER a Rust panic.
    /// Rust's native `m[key]` would panic on missing key (HashMap's
    /// `Index` impl unwraps the `get` result); we lower to the safe form
    /// so the user never sees a runtime panic from a map lookup.
    ///
    /// The chain is built explicitly (not via `parse_quote!`, which is
    /// banned in non-test code):
    ///
    /// 1. `m.get(&key)` — returns `Option<&V>`. The `&key` borrows the
    ///    key so the lookup doesn't consume it.
    /// 2. `.cloned()` — converts `Option<&V>` to `Option<V>` (requires
    ///    `V: Clone`; Buff's move-by-default codegen already inserts
    ///    `.clone()` everywhere, so `V: Clone` is satisfied for all
    ///    types Buff can put in a Map).
    /// 3. `.unwrap_or_default()` — converts `Option<V>` to `V`, using
    ///    `Default::default()` when the key was missing. Requires
    ///    `V: Default`; numeric / bool / String / Vec / HashMap types
    ///    all impl `Default` in std, and user structs get `#[derive(Default)]`
    ///    automatically via the codegen derive list (see
    ///    [`derive_and_repr_attrs`]).
    ///
    /// The `&key` reference is built via `syn::ExprReference` so we
    /// never hand-format a `&` token.
    pub(super) fn lower_map_index_read(
        &mut self,
        base: &Expr,
        key: &Expr,
    ) -> Result<SynExpr, CodegenError> {
        let base_e = self.lower_expr(base)?;
        let key_e = self.lower_expr(key)?;
        // `&key` — borrow the key so the lookup doesn't consume it.
        let ref_key = SynExpr::Reference(syn::ExprReference {
            attrs: Vec::new(),
            and_token: Default::default(),
            mutability: None,
            expr: Box::new(key_e),
        });
        // `m.get(&key)` — returns Option<&V>.
        let get_call = method_call_one_arg(base_e, "get", ref_key);
        // `.cloned()` — Option<&V> -> Option<V>.
        let cloned = SynExpr::MethodCall(syn::ExprMethodCall {
            attrs: Vec::new(),
            receiver: Box::new(get_call),
            dot_token: Default::default(),
            method: Ident::new("cloned", ProcSpan::call_site()),
            turbofish: None,
            paren_token: Default::default(),
            args: Punctuated::new(),
        });
        // `.unwrap_or_default()` — Option<V> -> V (Default fallback).
        let unwrapped = SynExpr::MethodCall(syn::ExprMethodCall {
            attrs: Vec::new(),
            receiver: Box::new(cloned),
            dot_token: Default::default(),
            method: Ident::new("unwrap_or_default", ProcSpan::call_site()),
            turbofish: None,
            paren_token: Default::default(),
            args: Punctuated::new(),
        });
        Ok(unwrapped)
    }

    /// T82: lower a Map indexing WRITE `m[key] = value` to
    /// `m.insert(key, value)`.
    ///
    /// Returns the lowered statement shape (`m.insert(key, value)` as a
    /// `SynStmt::Expr(_, Some(semi))` method-call statement). Rust's
    /// `HashMap::insert` returns `Option<V>` (the previous value if the
    /// key existed); the return is discarded at statement position,
    /// matching Buff's `m[k] = v` whose result is unit.
    ///
    /// The codegen dispatches here from [`Self::lower_stmt`]'s
    /// `Stmt::Assignment` arm when the target is `Expr::Index { base,
    /// indices: [key] }` AND `base` infers to `Map<K, V>`. Other shapes
    /// (Vector indexing, Matrix indexing, simple Ident assignment) take
    /// the existing paths.
    pub(super) fn lower_map_index_write(
        &mut self,
        base: &Expr,
        key: &Expr,
        value: &Expr,
    ) -> Result<SynStmt, CodegenError> {
        let base_e = self.lower_expr(base)?;
        let key_e = self.lower_expr(key)?;
        let value_e = self.lower_expr(value)?;
        let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
        args.push(key_e);
        args.push(value_e);
        let insert_call = SynExpr::MethodCall(syn::ExprMethodCall {
            attrs: Vec::new(),
            receiver: Box::new(base_e),
            dot_token: Default::default(),
            method: Ident::new("insert", ProcSpan::call_site()),
            turbofish: None,
            paren_token: Default::default(),
            args,
        });
        Ok(SynStmt::Expr(insert_call, Some(Default::default())))
    }

    /// Lower `expr?` to Rust's native `?` operator (T30 REFACTOR step).
    ///
    /// This is the extracted error-propagation codegen helper. It builds a
    /// `syn::ExprTry` wrapping the lowered operand, which `prettyplease`
    /// prints as `<expr>?`. Rust's `?` performs exactly the early-return
    /// propagation the task requires (`match expr { Ok(v) => v, Err(e) =>
    /// return Err(e.into()) }`), so we delegate to it rather than emitting
    /// the explicit match. The enclosing Buff function must lower to a Rust
    /// function returning `Result<T, E>` — which it does whenever the user
    /// writes a `Result<T, E>` return-type annotation, the only context
    /// where `?` is meaningful.
    ///
    /// Design choice (documented in the task): option (a) — Rust-native `?` —
    /// over option (b) — the explicit match. (a) is simpler, equally correct,
    /// and produces cleaner Rust that rustc optimises identically.
    pub(super) fn lower_try(&mut self, expr: &Expr) -> Result<SynExpr, CodegenError> {
        let inner = self.lower_expr(expr)?;
        Ok(SynExpr::Try(syn::ExprTry {
            attrs: Vec::new(),
            expr: Box::new(inner),
            question_token: Default::default(),
        }))
    }

    /// Lower the prelude error constructor `Error(arg)` to
    /// `Err(Error::new(arg))` (T30).
    ///
    /// `Error("msg")` in Buff is sugar for an `Err` value carrying a
    /// freshly-constructed `Error` (the builtin error type emitted on-demand
    /// by [`Self::generate`]). It maps to `Err(Error::new(arg))` so a
    /// `return Error("msg")` produces an early `Err` return without the user
    /// writing `Err(...)` themselves.
    ///
    /// The single argument is lowered as a normal expression and spliced
    /// into the `Error::new(...)` call via `quote!` (so no hand-formatted
    /// Rust). The outer `Err(...)` is a path call built the same way.
    pub(super) fn lower_error_constructor(
        &mut self,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!(
                "Error() expects exactly 1 arg, got {}",
                args.len()
            )));
        }
        let arg = self.lower_expr(&args[0])?;
        // `Error::new(#arg)` — built via quote! so the path + arg splice
        // without hand-formatted Rust. The explicit type annotation pins the
        // `parse2` target so type inference doesn't fall back to `()`.
        let inner_call_tokens: proc_macro2::TokenStream = quote::quote! {
            Error::new(#arg)
        };
        let inner_call: SynExpr = syn::parse2(inner_call_tokens)
            .map_err(|e| self.unsupported(&format!("Error() codegen parse: {e}")))?;
        // Wrap in `Err(...)`.
        let tokens: proc_macro2::TokenStream = quote::quote! {
            Err(#inner_call)
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("Err() codegen parse: {e}")))
    }

    /// T31: lower `spawn <expr>` to Rust's `tokio::spawn(async move { <expr> })`.
    ///
    /// The task body becomes the body of an `async move` closure so the
    /// spawned task owns its captured variables (Buff hides borrow-checker
    /// pain from users; the generated Rust must be move-clean). The result
    /// is a `tokio::task::JoinHandle<T>` — Buff's `Task<T>` is a thin alias
    /// for this type, and the only `.await` on a Task lands at the
    /// `t.result()` site (see [`Self::lower_method_call`]).
    ///
    /// Built via `quote!` so the `tokio::spawn(async move { ... })` shape
    /// is constructed from real `syn` tokens rather than hand-formatted
    /// Rust. The single string producer remains `prettyplease::unparse`.
    pub(super) fn lower_spawn(&mut self, task: &Expr) -> Result<SynExpr, CodegenError> {
        // T31: bump async-block depth so async calls inside the task body
        // still get `.await` (the `async move { ... }` block IS an async
        // context, even if the spawning fn is sync).
        self.async_block_depth += 1;
        // T33: bump spawn depth so ident uses inside the task body get
        // rewritten to `Arc::clone(&x)` (for Arc-shared bindings) instead
        // of moving or deep-cloning. Reset on exit so idents outside the
        // spawn go back to the regular move/clone path.
        self.spawn_depth += 1;
        let task_expr = self.lower_expr(task)?;
        self.spawn_depth -= 1;
        self.async_block_depth -= 1;
        let tokens: proc_macro2::TokenStream = quote::quote! {
            tokio::spawn(async move { #task_expr })
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("spawn codegen parse: {e}")))
    }

    /// T68: lower `start..end` (exclusive) or `start..=end` (inclusive) to a
    /// Rust range expression.
    ///
    /// Exclusive range `0..10` → Rust `0..10` via `syn::ExprRange`.
    /// Inclusive range `0..=10` → Rust `0..=10` via `syn::ExprRange`.
    ///
    /// Built via `quote!` so the `..` / `..=` operator is constructed from
    /// real `syn` tokens rather than hand-formatted Rust.
    pub(super) fn lower_range(
        &mut self,
        start: &Expr,
        end: &Expr,
        inclusive: bool,
    ) -> Result<SynExpr, CodegenError> {
        let start_e = self.lower_expr(start)?;
        let end_e = self.lower_expr(end)?;
        let tokens: proc_macro2::TokenStream = if inclusive {
            quote::quote! { #start_e ..= #end_e }
        } else {
            quote::quote! { #start_e .. #end_e }
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("range codegen parse: {e}")))
    }

    /// T31: lower `block(<expr>)` to a one-shot tokio runtime block.
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// {
    ///     let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    ///     rt.block_on(<expr>)
    /// }
    /// ```
    ///
    /// This is the SYNC context form — it spins up a fresh current-thread
    /// runtime and blocks the calling thread on the async expression. It's
    /// the bridge between sync code and an async future when no runtime is
    /// already running.
    ///
    /// # `block()` inside an async fn — DEADLOCK RISK warning
    ///
    /// If `block()` is called from inside an async fn (the current fn is in
    /// the propagated async set), we emit a [`Diagnostic::warning`]
    /// explaining the deadlock risk: the runtime worker thread is blocked
    /// on `block_on`, so any future scheduled on the same worker can never
    /// run, deadlocking the program. The warning is appended to
    /// [`Self::warnings`]; the codegen still emits the (broken) Rust so the
    /// user can see what they wrote and decide how to refactor (usually:
    /// remove `block()` and let the async fn `return` the future directly).
    pub(super) fn lower_block_call(&mut self, expr: &Expr) -> Result<SynExpr, CodegenError> {
        // Warn if we're inside an async fn — block_on in async is a deadlock.
        if self.current_fn_is_async() {
            let span = expr.span();
            self.warnings.push(
                Diagnostic::warning(
                    "`block()` inside an async function can deadlock the runtime",
                    span,
                )
                .with_code(ErrorCode::AsyncBlockDeadlock)
                .with_note(
                    "block_on parks the current worker thread, preventing any future \
                     scheduled on the same worker from running. Consider returning the \
                     future directly instead of blocking on it.",
                ),
            );
        }
        let arg = self.lower_expr(expr)?;
        // Build the one-shot runtime block via quote! — no hand-formatted
        // Rust string. The expect() message is intentionally lowercase +
        // no trailing period per the conventions doc.
        let tokens: proc_macro2::TokenStream = quote::quote! {
            {
                let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
                rt.block_on(#arg)
            }
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("block() codegen parse: {e}")))
    }

    /// T31: is the function we're currently lowering in the propagated
    /// async set? Used by [`Self::lower_block_call`] to decide whether to
    /// emit the deadlock warning.
    ///
    /// Returns `false` when called outside `lower_func` (e.g. when lowering
    /// a free-floating expression in tests).
    pub(super) fn current_fn_is_async(&self) -> bool {
        match &self.current_fn_name {
            Some(name) => self.async_fns.contains(name),
            None => false,
        }
    }

    /// T31: are we currently inside an async context? True when either:
    ///   - the current fn is async (per [`Self::current_fn_is_async`]), OR
    ///   - we're inside one or more `async move { ... }` blocks (e.g.
    ///     inside a `spawn` body — the spawned task is itself async even
    ///     when the spawner is sync).
    ///
    /// Drives the `.await` insertion decision in [`Self::lower_expr`].
    pub(super) fn in_async_context(&self) -> bool {
        self.current_fn_is_async() || self.async_block_depth > 0
    }

    /// T34: should `name` bypass [`MoveAnalyzer::needs_clone`] because we're
    /// inside a closure body and `name` is either a **captured variable**
    /// or a **closure parameter**?
    ///
    /// Checks the top-of-stack entry in [`Self::closure_capture_stack`].
    /// Returns `false` when not inside any closure body (empty stack).
    pub(super) fn is_captured_in_closure(&self, name: &str) -> bool {
        match self.closure_capture_stack.last() {
            Some(bypass) => bypass.contains(name),
            None => false,
        }
    }

    /// Lower a Buff [`Pattern`] to a Rust [`syn::Pat`] (T27 / T71).
    ///
    /// Mapping:
    /// - [`Pattern::Wildcard`] → `syn::Pat::Wild` (`_`)
    /// - [`Pattern::Ident(name, _)`] → `syn::Pat::Ident(name)`. Rust resolves
    ///   whether the name is a unit variant or a fresh binding using type
    ///   information (matching Buff's deferred-resolve approach).
    /// - [`Pattern::Literal(lit, _)`] → `syn::Pat::Lit` (literal pattern).
    /// - [`Pattern::Variant { variant, subpatterns, .. }`] →
    ///   - if `subpatterns` is empty: `syn::Pat::Path` (`Variant` alone — a
    ///     unit variant reference; we never reach this from the parser since
    ///     unit variants come through `Pattern::Ident`, but the arm covers
    ///     hand-constructed ASTs from tests).
    ///   - else: `syn::Pat::TupleStruct` with one sub-pattern per slot. The
    ///     path is just `Variant` (no enum prefix) — Rust resolves it when
    ///     the enum is in scope. The `enum_name` field of the AST node is
    ///     ignored at codegen (the parser fills it with `""`).
    /// - [`Pattern::Tuple(subs, _)`] → `syn::Pat::Tuple` (`(a, b)`). T71.
    /// - [`Pattern::Struct { name, fields, .. }`] → `syn::Pat::Struct`
    ///   (`Point { x, y }`). Shorthand fields (name == binding name) are
    ///   reproduced as shorthand (no colon). T71.
    ///
    /// `mutable` (T71) — when `true`, every [`Pattern::Ident`] binding is
    /// emitted with `mut` (e.g. `mut x`). Match-arm callers pass `false`
    /// (patterns never carry `mut` in Buff syntax); the `let`-destructuring
    /// caller passes the binding's `mutable` flag so `let mut (a, b) = ...`
    /// lowers to `let (mut a, mut b) = ...`. `mutable` propagates recursively
    /// into sub-patterns so nested bindings all pick it up.
    pub(super) fn lower_pattern(
        &mut self,
        pat: &Pattern,
        mutable: bool,
    ) -> Result<Pat, CodegenError> {
        let syn_pat: Pat = match pat {
            Pattern::Wildcard(_) => Pat::Wild(syn::PatWild {
                attrs: Vec::new(),
                underscore_token: Default::default(),
            }),
            Pattern::Ident(name, _) => {
                // T85: bare user-defined enum variant used as a match
                // pattern. The parser encodes a bare `Red` as
                // `Pattern::Ident("Red")` because at parse time it
                // cannot tell variant-vs-binding apart. Here we resolve
                // the ambiguity: if `Red` is a known user-defined enum
                // variant, lower to the qualified path pattern
                // `Color::Red`; otherwise keep the binding-pattern
                // lowering (`Pat::Ident`). Without this, rustc treats
                // bare `Red` in a match arm as a fresh binding that
                // matches ANY value (silently shadowing the variant) —
                // a particularly nasty bug because it typechecks but
                // does the wrong thing at runtime.
                if let Some(enum_name) = self.user_enum_variants.get(&name.name) {
                    Pat::Path(syn::PatPath {
                        attrs: Vec::new(),
                        qself: None,
                        path: two_segment_path(enum_name, &name.name),
                    })
                } else {
                    Pat::Ident(PatIdent {
                        attrs: Vec::new(),
                        ident: ast_ident_to_syn(name),
                        by_ref: None,
                        mutability: mutable.then(Default::default),
                        subpat: None,
                    })
                }
            }
            Pattern::Literal(lit, _) => {
                let lit_expr = self.lower_literal(lit)?;
                // `syn::Pat::Lit` is an alias for `syn::ExprLit` in syn 2.0
                // (see `syn::pat.rs`: `ExprLit as PatLit`). So a literal
                // pattern is constructed exactly like a literal expression:
                // wrap the `syn::Lit` in an `ExprLit` and hand it to
                // `Pat::Lit(...)`.
                let expr_lit = match lit_expr {
                    SynExpr::Lit(el) => el,
                    other => {
                        return Err(self.unsupported(&format!(
                            "literal pattern codegen expected Lit, got {other:?}"
                        )))
                    }
                };
                Pat::Lit(expr_lit)
            }
            Pattern::Variant {
                enum_name,
                variant,
                subpatterns,
                ..
            } => {
                // T85: resolve the variant's owning enum. The parser
                // leaves `enum_name` empty for bare `Variant(...)`
                // patterns; we look up the owning enum from the
                // user-enum-variant registry built in [`Self::generate`].
                // If `enum_name` was explicitly written by the user
                // (`Color::Red(x)`) we use it as-is. If the registry has
                // no entry (e.g. prelude `Some`/`None`/`Ok`/`Err`, or
                // the variant belongs to an enum declared in another
                // file we don't see) we fall back to the bare variant
                // name — preserving the prior codegen shape so existing
                // snapshots for Option/Result stay byte-identical.
                let resolved_enum: Option<&str> = if !enum_name.name.is_empty() {
                    Some(enum_name.name.as_str())
                } else {
                    self.user_enum_variants
                        .get(&variant.name)
                        .map(String::as_str)
                };
                let path = match resolved_enum {
                    Some(en) => two_segment_path(en, &variant.name),
                    None => syn::Path::from(ast_ident_to_syn(variant)),
                };
                if subpatterns.is_empty() {
                    // Unit variant via path. Build `syn::Pat::Path` with a
                    // single-segment (or two-segment, T85) path.
                    Pat::Path(syn::PatPath {
                        attrs: Vec::new(),
                        qself: None,
                        path,
                    })
                } else {
                    // Tuple-struct variant: `Variant(subpat1, subpat2, ...)`.
                    let mut elems: Punctuated<Pat, syn::Token![,]> = Punctuated::new();
                    for sub in subpatterns {
                        elems.push(self.lower_pattern(sub, mutable)?);
                    }
                    Pat::TupleStruct(syn::PatTupleStruct {
                        attrs: Vec::new(),
                        qself: None,
                        path,
                        paren_token: Default::default(),
                        elems,
                    })
                }
            }
            Pattern::Tuple(subs, _) => {
                // T71: tuple destructuring `(a, b, ...)`.
                let mut elems: Punctuated<Pat, syn::Token![,]> = Punctuated::new();
                for sub in subs {
                    elems.push(self.lower_pattern(sub, mutable)?);
                }
                Pat::Tuple(syn::PatTuple {
                    attrs: Vec::new(),
                    paren_token: Default::default(),
                    elems,
                })
            }
            Pattern::Struct {
                name, fields, rest, ..
            } => {
                // T71: struct destructuring `Name { field: subpat, ... }`.
                // Hand-built via `syn::PatStruct` + `syn::FieldPat` (syn 2.0
                // renamed the field type `PatField`→`FieldPat`). Shorthand
                // (immutable + field name == binding name) is reproduced
                // without a colon: `Point { x }` not `Point { x: x }`.
                // T41: the `rest` flag lowers to a Rust `..` rest pattern
                // (`Point { x, .. }`) via `syn::PatStruct::rest = Some(..)`.
                let mut field_pats: Punctuated<syn::FieldPat, syn::Token![,]> = Punctuated::new();
                for (field_name, subpat) in fields {
                    let is_shorthand = !mutable
                        && matches!(subpat, Pattern::Ident(id, _) if id.name == field_name.name);
                    let lowered = self.lower_pattern(subpat, mutable)?;
                    field_pats.push(syn::FieldPat {
                        attrs: Vec::new(),
                        member: ast_ident_to_syn(field_name).into(),
                        colon_token: if is_shorthand {
                            None
                        } else {
                            Some(Default::default())
                        },
                        pat: Box::new(lowered),
                    });
                }
                Pat::Struct(syn::PatStruct {
                    attrs: Vec::new(),
                    qself: None,
                    path: syn::Path::from(ast_ident_to_syn(name)),
                    brace_token: Default::default(),
                    fields: field_pats,
                    // T41: `..` rest pattern. When the Buff struct pattern
                    // carries `rest = true`, emit Rust's `..` rest token so
                    // unmentioned fields are ignored. prettyplease renders it
                    // as `Point { x, .. }`.
                    rest: if *rest {
                        Some(syn::PatRest {
                            attrs: Vec::new(),
                            dot2_token: Default::default(),
                        })
                    } else {
                        None
                    },
                })
            }
            Pattern::Or(alts, _) => {
                // T39: or-pattern `A | B | C`. Lower each alternative via the
                // shared `lower_pattern` (so nested or-patterns and all other
                // shapes compose) and build a `syn::Pat::Or`. The leading `|`
                // is implicit in syn's PatOr (prettyplease emits it between
                // alternatives). An empty alts vec is a parser invariant
                // violation (parse_pattern always pushes ≥2); defensively
                // fall back to Wild to avoid a panic.
                if alts.len() < 2 {
                    return Ok(Pat::Wild(syn::PatWild {
                        attrs: Vec::new(),
                        underscore_token: Default::default(),
                    }));
                }
                let mut cases: Punctuated<Pat, syn::Token![|]> = Punctuated::new();
                for alt in alts {
                    cases.push(self.lower_pattern(alt, mutable)?);
                }
                Pat::Or(syn::PatOr {
                    attrs: Vec::new(),
                    leading_vert: None,
                    cases,
                })
            }
        };
        Ok(syn_pat)
    }

    pub(super) fn lower_literal(&mut self, lit: &Literal) -> Result<SynExpr, CodegenError> {
        // T20: Decimal literal → `rust_decimal_macros::dec!(<raw>)`. The raw
        // digit text is parsed into a `proc_macro2::TokenStream` so the
        // *exact* digits survive (no rounding through f64) — this matches
        // what `dec!` expects (a numeric literal token) and preserves
        // trailing zeros like the `0` in `99.90`.
        if let Literal::Decimal(raw) = lit {
            return self.lower_decimal_literal(raw);
        }
        // Self-host: Always wrap string literals in .to_string() so they
        // are String type (not &str). This allows passing string literals
        // to String-typed function parameters without a codegen gap.
        // Rust's deref coercion handles the reverse (&str params receive
        // String via auto-deref), so this is always safe.
        if let Literal::String(s) = lit {
            let lit_expr = SynExpr::Lit(syn::ExprLit {
                attrs: Vec::new(),
                lit: syn::Lit::Str(syn::LitStr::new(s, ProcSpan::call_site())),
            });
            return Ok(SynExpr::MethodCall(syn::ExprMethodCall {
                attrs: Vec::new(),
                receiver: Box::new(lit_expr),
                dot_token: Default::default(),
                method: syn::Ident::new("to_string", ProcSpan::call_site()),
                turbofish: None,
                args: Punctuated::new(),
                paren_token: Default::default(),
            }));
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
            // T79: Regex literal — CODEGEN DEFERRED in v0.5. The generated
            // Cargo project has NO `regex` crate dependency (T32-style dep
            // injection is a separate v1.0 task), so emitting
            // `regex::Regex::new(...)` would fail to compile downstream. As a
            // documented stub we emit the raw pattern text as a plain String
            // literal (valid standalone Rust) so the pipeline stays green.
            // Real `Regex::new` lowering + Cargo-project dep wiring arrives
            // in v1.0. See `Literal::Regex` on the AST for the deferral note.
            Literal::Regex(p) => syn::Lit::Str(syn::LitStr::new(p, ProcSpan::call_site())),
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
    pub(super) fn lower_decimal_literal(&self, raw: &str) -> Result<SynExpr, CodegenError> {
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

    pub(super) fn make_binary_op(
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
            // T101: `a ?? b` → `a.unwrap_or(b)` via quote! + syn::parse2.
            BinaryOp::NullCoalesce => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #lhs.unwrap_or(#rhs)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("null coalesce codegen parse: {e}")))?
            }
        };
        Ok(result)
    }

    pub(super) fn bin_arith(&self, op: syn::BinOp, lhs: SynExpr, rhs: SynExpr) -> SynExpr {
        SynExpr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(lhs),
            op,
            right: Box::new(rhs),
        })
    }

    pub(super) fn make_unary_op(
        &mut self,
        op: UnaryOp,
        operand: SynExpr,
    ) -> Result<SynExpr, CodegenError> {
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
}
