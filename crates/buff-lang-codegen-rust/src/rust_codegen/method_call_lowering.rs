//! T105a - method/builtin-call lowering: one_arg_method..matrix_new (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move of `impl RustCodegen` methods into this child module so the
//! parent file shrinks. Methods are pub(super); the parent declares only
//! `mod <name>;` (inherent methods resolve by type, no `use` needed). Child
//! inherits parent imports via use super::* and may call the parent private
//! methods (descendant privacy) and the extracted helper modules.

use super::*;

impl RustCodegen {

    /// Lower `abs(x)` → `(x).abs()`. Wrapping the receiver in parens
    /// ensures integer literals like `5` lower to `(5).abs()` rather than
    /// the ambiguous `5.abs()` (which Rust parses as a field access on a
    /// float literal `5.`).
    pub(super) fn lower_one_arg_method(
        &mut self,
        args: &[Expr],
        method: &str,
        wrap_parens: bool,
    ) -> Result<SynExpr, CodegenError> {
        let recv = self.lower_one_arg(args)?;
        let recv = if wrap_parens {
            wrap_in_parens(recv)
        } else {
            recv
        };
        Ok(method_call_no_args(recv, method))
    }

    /// Lower `min(a, b)` / `max(a, b)` → `(a).<method>(b)`.
    pub(super) fn lower_min_max(&mut self, args: &[Expr], method: &str) -> Result<SynExpr, CodegenError> {
        if args.len() != 2 {
            return Err(self.unsupported(&format!(
                "{method} expects exactly 2 args, got {}",
                args.len()
            )));
        }
        let a = wrap_in_parens(self.lower_expr(&args[0])?);
        let b = self.lower_expr(&args[1])?;
        Ok(method_call_one_arg(a, method, b))
    }

    /// Lower a float-returning unary math fn (`sqrt`/`floor`/`ceil`/`round`)
    /// to `((x) as f64).<method>()`. Coercing to `f64` first means int args
    /// compile without requiring the user to write `x as Double` manually.
    pub(super) fn lower_float_unary(&mut self, args: &[Expr], method: &str) -> Result<SynExpr, CodegenError> {
        let recv = self.lower_one_arg(args)?;
        let as_f64 = cast_to(recv, "f64");
        Ok(method_call_no_args(as_f64, method))
    }

    /// Lower `pow(base, exp)` — picks `.powf` for float bases and `.pow` for
    /// integer bases (Rust's `i64::pow` takes `u32`, hence the `as u32` cast).
    pub(super) fn lower_pow(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 2 {
            return Err(
                self.unsupported(&format!("pow expects exactly 2 args, got {}", args.len()))
            );
        }
        let base = wrap_in_parens(self.lower_expr(&args[0])?);
        let exp_raw = self.lower_expr(&args[1])?;
        // Infer the base type to choose `.pow` vs `.powf`. Inference errors
        // fall back to the integer form (which works for the common case).
        let base_ty = self
            .type_inferencer
            .infer_expr(&args[0])
            .unwrap_or(Type::Unknown);
        if base_ty.is_float_like() {
            let exp = cast_to(exp_raw, "f64");
            Ok(method_call_one_arg(base, "powf", exp))
        } else {
            let exp = cast_to(exp_raw, "u32");
            Ok(method_call_one_arg(base, "pow", exp))
        }
    }

    /// Lower a type conversion (`Int(x)` / `Float(x)` / `Bool(x)`).
    ///
    /// For String args we emit `.parse::<T>().unwrap_or(default)`; for
    /// numeric args we emit `(x) as T`. The `Bool` arm uses `x != 0` for
    /// numerics (Rust has no `as bool` cast) and `.parse::<bool>()` for
    /// strings.
    pub(super) fn lower_convert(
        &mut self,
        args: &[Expr],
        target: &str,
        kind: ConvKind,
    ) -> Result<SynExpr, CodegenError> {
        let arg = self.lower_one_arg(args)?;
        // Infer the arg's type to dispatch on the source category.
        let arg_ty = self
            .type_inferencer
            .infer_expr(&args[0])
            .unwrap_or(Type::Unknown);
        if matches!(arg_ty, Type::String) {
            // String → parse
            return Ok(parse_with_default(arg, target, &kind));
        }
        // Non-string → numeric coercion (`as T`) for Int/Float, or `!= 0` for Bool.
        match kind {
            ConvKind::Numeric => Ok(cast_to(arg, target)),
            ConvKind::Bool => {
                // `(x) != 0` — wrap the arg in parens so compound exprs bind right.
                let zero = make_int_lit_expr(0);
                Ok(make_binary_expr(
                    syn::BinOp::Ne(Default::default()),
                    wrap_in_parens(arg),
                    zero,
                ))
            }
        }
    }

    /// Lower `String(x)` → `(x).to_string()`. Works for any Rust `Display` type.
    pub(super) fn lower_to_string(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        let recv = self.lower_one_arg(args)?;
        Ok(method_call_no_args(recv, "to_string"))
    }

    /// Lower `print(x)` / `println(x)`.
    ///
    /// A bare string-literal arg lowers to `println!("the literal text")`
    /// — no `{}` placeholder (T96 acceptance). Any other arg lowers to
    /// `println!("{}", x)`.
    pub(super) fn lower_print(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!(
                "print/println expect exactly 1 arg, got {}",
                args.len()
            )));
        }
        // String-literal fast path: print("hello") → println!("hello").
        if let Expr::Literal(Literal::String(text), _) = &args[0] {
            return Ok(make_println_macro_literal(text));
        }
        // General path: print(x) → println!("{}", x).
        let arg = self.lower_expr(&args[0])?;
        Ok(make_println_macro(arg))
    }

    /// Lower `read_line()` → a block expression that reads one line of stdin.
    ///
    /// Emits (conceptually):
    /// ```text
    /// {
    ///     let mut __buff_prelude_line = String::new();
    ///     std::io::stdin().read_line(&mut __buff_prelude_line).ok();
    ///     __buff_prelude_line
    /// }
    /// ```
    ///
    /// The block is built via `quote!` and then re-parsed via
    /// `syn::parse2` (which returns a `Result`, unlike `parse_quote!`'s
    /// panic). The placeholder name `__buff_prelude_line` is intentionally
    /// ugly to avoid colliding with any user binding.
    pub(super) fn lower_read_line(&self) -> SynExpr {
        let tokens: proc_macro2::TokenStream = quote::quote! {{
            let mut __buff_prelude_line = String::new();
            std::io::stdin().read_line(&mut __buff_prelude_line).ok();
            __buff_prelude_line
        }};
        // `quote!`'s `{{...}}` produces a Rust block-expression token
        // stream; re-parse it as a `syn::Expr` (the top-level enum) so it
        // slots into the surrounding expression context. On the (unreachable)
        // parse failure we fall back to a bare `String::new()` call.
        match syn::parse2::<SynExpr>(tokens) {
            Ok(e) => e,
            Err(_) => {
                // Defensive fallback: never panic in codegen. The quote!
                // above is a compile-time-fixed template so a parse failure
                // is a codegen bug, not a user-facing condition.
                let path = rust_path("String::new");
                SynExpr::Call(syn::ExprCall {
                    attrs: Vec::new(),
                    func: Box::new(SynExpr::Path(syn::ExprPath {
                        attrs: Vec::new(),
                        qself: None,
                        path,
                    })),
                    paren_token: Default::default(),
                    args: Default::default(),
                })
            }
        }
    }

    /// T124g: lower `input()` / `input(prompt)` → a block expression
    /// that reads one line of stdin, optionally after printing a prompt.
    ///
    /// Emits (conceptually):
    /// ```text
    /// // input() - no prompt:
    /// {
    ///     let mut __buff_prelude_line = String::new();
    ///     std::io::stdin().read_line(&mut __buff_prelude_line).ok();
    ///     __buff_prelude_line.trim_end().to_string()
    /// }
    ///
    /// // input(prompt) - print prompt first, flush, then read:
    /// {
    ///     print!(<prompt>);
    ///     use std::io::Write;
    ///     std::io::stdout().flush().ok();
    ///     let mut __buff_prelude_line = String::new();
    ///     std::io::stdin().read_line(&mut __buff_prelude_line).ok();
    ///     __buff_prelude_line.trim_end().to_string()
    /// }
    /// ```
    ///
    /// Differences from `read_line()` (T99):
    /// - `input()` trims the trailing newline (`read_line()` does not).
    ///   This matches user expectations: `input()` returns the typed
    ///   text, not "text\n".
    /// - `input(prompt)` prints the prompt with `print!` (no newline)
    ///   and flushes stdout BEFORE reading. Without the flush, the
    ///   prompt may stay buffered in stdout's pipe until after the
    ///   read_line returns (interactive pipelines deadlock). The
    ///   `use std::io::Write;` brings the `flush` method into scope
    ///   for the block (the trait import is block-local so it doesn't
    ///   pollute the user's module).
    ///
    /// Arity: 0 or 1 args. 1 arg MUST be a String (the prompt). Any
    /// other arity surfaces as a codegen error.
    ///
    /// `.ok()` on read_line / flush elides I/O errors (Buff's
    /// panic-free generated-code stance — same as `read_line()`).
    pub(super) fn lower_input(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        // Arity check: 0 or 1 args.
        if args.len() > 1 {
            return Err(self.unsupported(&format!(
                "input() expects 0 or 1 arg (the prompt), got {}",
                args.len()
            )));
        }
        let tokens: proc_macro2::TokenStream = match args.first() {
            // input() - no prompt. The trim_end handles both "\n" and
            // "\r\n" line endings (Rust's str::trim_end matches any
            // trailing whitespace char, but for newline-only trimming
            // it's the right tool: \n, \r, and Unicode line ends alike).
            None => quote::quote! {{
                let mut __buff_prelude_line = String::new();
                std::io::stdin().read_line(&mut __buff_prelude_line).ok();
                __buff_prelude_line.trim_end().to_string()
            }},
            // input(prompt) - print prompt, flush stdout, then read.
            // The prompt is spliced via #prompt (quote!'s interpolation
            // handles any expression shape - String literal, ident,
            // interpolation result, ...).
            Some(prompt_expr) => {
                let prompt = self.lower_expr(prompt_expr)?;
                quote::quote! {{
                    print!(#prompt);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                    let mut __buff_prelude_line = String::new();
                    std::io::stdin().read_line(&mut __buff_prelude_line).ok();
                    __buff_prelude_line.trim_end().to_string()
                }}
            }
        };
        syn::parse2(tokens).map_err(|e| self.unsupported(&format!("input() codegen parse: {e}")))
    }

    /// T124g: lower `sleep(duration)` →
    /// `tokio::time::sleep(<duration>).await`. The `.await` is
    /// unconditional (Buff has no `await` keyword — the codegen inserts
    /// it transparently). The enclosing fn MUST be async (declared or
    /// propagated via the T31 walker); a sleep in a sync fn surfaces as
    /// a rustc diagnostic (`.await outside async`), not a Buff codegen
    /// error — matching the established "we generate the lowering; the
    /// borrow checker / rustc handles downstream errors" pattern.
    ///
    /// Duration arg shapes (canonical first, fallback last):
    /// - `sleep(Duration.seconds(N))` →
    ///   `tokio::time::sleep(std::time::Duration::from_secs(N)).await`.
    ///   Same for `.millis(M)`, `.micros(U)`, `.nanos(N)`. The
    ///   `Duration.<unit>(N)` AST shape is detected and rewritten to
    ///   `std::time::Duration::from_<unit>(N)` so the generated Rust
    ///   uses `std::time::Duration` (which `tokio::time::sleep` takes)
    ///   rather than `chrono::TimeDelta` (which T124b's Duration.seconds
    ///   would normally produce). This keeps the sleep path
    ///   chrono-independent (chrono's TimeDelta doesn't impl
    ///   `Into<std::time::Duration>` without an explicit conversion).
    /// - `sleep(N)` (plain Int literal) → treated as seconds:
    ///   `tokio::time::sleep(std::time::Duration::from_secs(N)).await`.
    /// - `sleep(other_expr)` → passthrough:
    ///   `tokio::time::sleep(other_expr).await`. The user is
    ///   responsible for passing a `std::time::Duration` value —
    ///   useful for `sleep(my_duration_var)` when the user constructs
    ///   the Duration themselves.
    ///
    /// Built via `quote!` + parse2 so the `.await` suffix slots cleanly
    /// onto the call (building the `Await` node by hand is awkward).
    pub(super) fn lower_sleep(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!(
                "sleep() expects exactly 1 arg (the duration), got {}",
                args.len()
            )));
        }
        let arg_expr = &args[0];
        // Detect `Duration.<unit>(N)` AST shape. Buff's parser produces
        // this as a MethodCall { receiver: Ident("Duration"),
        // method: Ident(<unit>), args: [N] }. The supported units are
        // the same as std::time::Duration::from_<unit> constructors:
        // secs / millis / micros / nanos. The T124b Duration assoc fns
        // are days / hours / minutes / seconds / millis — but for the
        // sleep path we only honour the std::time subset (a sleep
        // measured in days makes no sense; if a user really wants
        // that, they can pass `Duration.seconds(N * 86400)` or the
        // plain-int form).
        if let Expr::MethodCall {
            receiver,
            method,
            args: inner,
            ..
        } = arg_expr
        {
            if let Expr::Ident(recv_id, _) = receiver.as_ref() {
                if recv_id.name == "Duration" && inner.len() == 1 {
                    let unit = method.name.as_str();
                    // Map Buff's chrono-style names (seconds / millis)
                    // to std::time::Duration's constructor names (secs /
                    // millis). The mapping is intentionally narrow —
                    // only the constructors std::time::Duration
                    // actually exposes.
                    let std_unit = match unit {
                        "seconds" | "secs" => Some("secs"),
                        "millis" => Some("millis"),
                        "micros" => Some("micros"),
                        "nanos" => Some("nanos"),
                        _ => None,
                    };
                    if let Some(std_unit) = std_unit {
                        let n = self.lower_expr(&inner[0])?;
                        // Build the `std::time::Duration::from_<unit>`
                        // path. `quote!` doesn't support token-paste
                        // (`##`), so we splice the unit into the method
                        // name via `format_ident!` and emit a single
                        // Ident token. The result is
                        // `tokio::time::sleep(std::time::Duration::from_secs(N)).await`
                        // (or from_millis / from_micros / from_nanos).
                        let ctor_name = proc_macro2::Ident::new(
                            &format!("from_{std_unit}"),
                            proc_macro2::Span::call_site(),
                        );
                        let tokens: proc_macro2::TokenStream = quote::quote! {
                            tokio::time::sleep(
                                std::time::Duration::#ctor_name(#n)
                            ).await
                        };
                        return syn::parse2(tokens).map_err(|e| {
                            self.unsupported(&format!(
                                "sleep(Duration.{unit}(_)) codegen parse: {e}"
                            ))
                        });
                    }
                }
            }
        }
        // Plain Int literal: treat as seconds.
        if let Expr::Literal(Literal::Int(_), _) = arg_expr {
            let n = self.lower_expr(arg_expr)?;
            let tokens: proc_macro2::TokenStream = quote::quote! {
                tokio::time::sleep(std::time::Duration::from_secs(#n)).await
            };
            return syn::parse2(tokens)
                .map_err(|e| self.unsupported(&format!("sleep(<int>) codegen parse: {e}")));
        }
        // Passthrough: user-supplied duration expression.
        let arg = self.lower_expr(arg_expr)?;
        let tokens: proc_macro2::TokenStream = quote::quote! {
            tokio::time::sleep(#arg).await
        };
        syn::parse2(tokens)
            .map_err(|e| self.unsupported(&format!("sleep(<expr>) codegen parse: {e}")))
    }

    /// Lower exactly one argument, returning an error if the arg count is wrong.
    pub(super) fn lower_one_arg(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!("expected exactly 1 arg, got {}", args.len())));
        }
        self.lower_expr(&args[0])
    }

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
    pub(super) fn lower_method_call(
        &mut self,
        receiver: &Expr,
        method: &buff_lang_ast::common::Ident,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        // T124b: prelude-types registry — associated functions. A call of
        // the form `Type.method(args)` where the receiver is a bare Ident
        // naming a prelude type (DateTime, Date, Time, Duration, Instant)
        // is dispatched through the prelude-types table. This MUST run
        // before the T31 `result()` arm and the T26 zero-arg field-access
        // heuristic so that `DateTime.now()` (zero args) doesn't get
        // rewritten as a field access `DateTime.now`.
        //
        // This is the GENERAL entry point future v1.4 stdlib tasks extend
        // — see `crates/buff-lang-types/src/prelude_types.rs` for the
        // registry and the instructions for adding new types.
        if let Expr::Ident(id, _) = receiver {
            if let Some((ptype, pmethod)) =
                buff_lang_types::prelude_types::assoc_fn_lookup(&id.name, &method.name)
            {
                // The chrono / std::time lowering lives in a dedicated
                // helper so this arm stays a thin dispatch.
                return self.lower_prelude_type_assoc_fn(ptype, pmethod, args);
            }
        }

        // T124f: prelude-types registry - associated CONSTANTS. A
        // zero-arg `Type.NAME` access (parser produces MethodCall with
        // args == []) where the receiver is a bare Ident naming a
        // prelude type with a registered constant (currently only
        // `Math.PI` / `Math.E`). This MUST run before the T26
        // field-access heuristic below (which would rewrite
        // `Math.PI` as a Rust field access - meaningless because `Math`
        // is a namespace, not a Rust type with a `PI` field).
        //
        // The lowering lives in [`Self::lower_prelude_type_assoc_const`]
        // (dedicated helper so this arm stays a thin dispatch, mirroring
        // the assoc-fn dispatch above).
        if args.is_empty() {
            if let Expr::Ident(id, _) = receiver {
                if let Some((ptype, pconst)) =
                    buff_lang_types::prelude_types::assoc_const_lookup(&id.name, &method.name)
                {
                    return self.lower_prelude_type_assoc_const(ptype, pconst);
                }
            }
        }

        // T31: `task.result()` → Rust's `task.await`. This is the ONLY
        // `.await` form that originates from a method-call position; it's
        // the suspension-point API on Buff's `Task<T>` (a thin alias for
        // `tokio::task::JoinHandle<T>`). The check MUST run BEFORE the T26
        // field-access-vs-method-call heuristic below, because `result()`
        // is a zero-arg method call and would otherwise be rewritten as a
        // field access `task.result` (which is meaningless on a JoinHandle).
        // We accept both `task.result()` (with parens) and the postfix-
        // form `task.result` (no parens — same AST shape per the parser's
        // Dot arm) by NOT gating on `args.is_empty()` here.
        if method.name == "result" {
            let recv = self.lower_expr(receiver)?;
            return Ok(make_await(recv));
        }

        // T26 field-access-vs-method-call disambiguation.
        //
        // Buff parses `obj.field` and `obj.method()` through the SAME AST
        // shape (`Expr::MethodCall { receiver, method, args }`) — see the
        // parser's `parse_postfix` Dot arm: a `.` followed by an Ident
        // WITHOUT parens produces a zero-arg MethodCall. So `p.name` (a
        // field access on a user struct) and `v.len()` (a real method call)
        // are indistinguishable at the AST level.
        //
        // Heuristic (T26): when `args` is empty AND `method.name` is NOT in
        // the [`KNOWN_ZERO_ARG_METHODS`] allow-list, emit a Rust field
        // access `recv.field` instead of a method call `recv.field()`. This
        // is the additive-only approach: no AST migration required, no new
        // FieldAccess variant — just a codegen-time rewrite. A dedicated
        // `Expr::FieldAccess` AST node is the cleaner long-term shape (see
        // migration note in `decisions.md`), deferred to keep T26 additive.
        //
        // Trade-off: if a user defines a struct with a field literally named
        // `len` / `push` / etc., `obj.len` will emit `obj.len()` (wrong). The
        // allow-list is conservative (only names this codegen actually
        // handles + the universal `clone`/`to_string`/etc.); new builtins
        // added later must be added to the list to preserve the heuristic.
        //
        // T124b: this heuristic also needs to NOT fire for prelude-type
        // instance methods (`dt.format(...)`, `dt.year()`, ...). Those
        // receivers are NEVER a bare `Expr::Ident` naming a prelude TYPE
        // (handled by the assoc_fn_lookup arm above) — they're values. But
        // we must consult the registry to decide whether `dt.year()` (zero
        // args) is a real method call vs. a field access. The dedicated
        // prelude-instance arm runs AFTER this heuristic, so we extend
        // KNOWN_ZERO_ARG_METHODS to include the prelude instance methods
        // that take zero args (year/month/day/hour/minute/second/
        // timestamp). `format` takes one arg so it's never affected.
        //
        // T124m: we ALSO need the receiver's inferred type for the
        // prelude-instance-skip below, so we move the inference
        // ONCE here (before both the heuristic and the prelude
        // dispatch) and reuse the result. This is purely a reorder -
        // the semantic of `infer_expr(receiver).unwrap_or(Unknown)`
        // is unchanged from the original code that lived just below
        // the heuristic.
        let recv_for_prelude_check = self
            .type_inferencer
            .infer_expr(receiver)
            .unwrap_or(Type::Unknown);
        if args.is_empty()
            && !KNOWN_ZERO_ARG_METHODS.contains(&method.name.as_str())
            // T124m: skip the field-access heuristic when the
            // (recv_ty, method) pair is a REGISTERED prelude
            // instance method. Without this guard, `c.send()` (zero
            // args on a Type::Connection receiver) would be silently
            // rewritten as a Rust field access `c.send` - the
            // arity-validation arm in `lower_prelude_type_instance_fn`
            // (which rejects `send()` with 0 args, expecting exactly 1)
            // would never run, and the user would get a downstream
            // rustc "field `send` not found" error instead of a clear
            // Buff-side "send() expects exactly 1 arg" error. The same
            // gap applies to any future multi-arg prelude instance
            // method whose name is NOT in KNOWN_ZERO_ARG_METHODS
            // (send / send_to today; recv / close / recv_from already
            // pass through because they ARE in the table - they take
            // zero args legitimately).
            && buff_lang_types::prelude_types::instance_fn_lookup(
                &recv_for_prelude_check,
                &method.name,
            )
            .is_none()
        {
            let recv = self.lower_expr(receiver)?;
            return Ok(field_access(recv, &method.name));
        }

        // T124b: prelude-types registry — instance methods. A call of the
        // form `recv.method(args)` where the receiver INFERS to a prelude
        // datetime type. Runs AFTER the T26 field-access heuristic so the
        // zero-arg instance methods (year/month/day/...) — which are in
        // KNOWN_ZERO_ARG_METHODS — pass through to here. T124m also lets
        // multi-arg prelude instance methods (send / send_to) pass through
        // when called with zero args so their arity validation runs (see
        // the skip clause above).
        //
        // We consult the integrated TypeInferencer to get the receiver's
        // resolved Type (computed once above for both the heuristic and
        // this dispatch). Inference errors fall through to the default
        // `recv.method(args)` lowering (Rust will then diagnose the
        // receiver-type mismatch).
        if let Some(pmethod) = buff_lang_types::prelude_types::instance_fn_lookup(
            &recv_for_prelude_check,
            &method.name,
        ) {
            return self.lower_prelude_type_instance_fn(
                &recv_for_prelude_check,
                pmethod,
                receiver,
                args,
            );
        }

        // T24: `Matrix.new(rows, cols)` — the builtin Matrix constructor.
        // Buff's constructor convention is `Type.new()` / `Type.from()` (§7),
        // parsed as a MethodCall whose receiver is a bare Ident naming the
        // type. We special-case `Matrix.new(...)` here to lower it to Rust's
        // `Matrix::new(rows, cols)` associated-function call. The `Matrix<T>`
        // struct definition itself is emitted on-demand by
        // [`Self::generate`] when a program uses `Matrix.new(...)`.
        if method.name == "new" {
            if let Expr::Ident(id, _) = receiver {
                if id.name == "Matrix" {
                    return self.lower_matrix_new(args);
                }
            }
        }

        // T78: `recv.context("msg")` — error-context chaining.
        //
        // Attaches a human-readable context string to a `Result<T, E>`'s
        // `Err` variant, then (typically) propagates with `?`. The parser
        // already produces this as `Expr::MethodCall { method: "context",
        // args: [string_literal] }`, often wrapped in `Expr::Try` for the
        // trailing `?`. We special-case it HERE (before the field-access
        // heuristic and the default `recv.method(args)` arm) so the name
        // `context` NEVER falls through to a plain Rust method call.
        //
        // Desugar: `recv.context("msg")` →
        //   `recv.map_err(|e| format!("msg: {:?}", e))`
        //
        // The trailing `?` (if any) is added by the EXISTING `lower_try`
        // path — the wrapping `Expr::Try` lowers independently. So this
        // codegen is purely additive: NO new AST variant, NO change to
        // `lower_try`.
        //
        // Design choice — Debug (`{:?}`) over Display (`{}`):
        //   The std `Error: Debug` bound is universally implemented (every
        //   `T: Error` gets Debug via `#[derive(Debug)]` or manual impl),
        //   while `Display` is NOT automatically implemented for many error
        //   types. Using `{:?}` guarantees the generated Rust compiles for
        //   ANY error type the user's `Result<T, E>` might carry. The Debug
        //   rendering is also richer (shows variant names + fields), which
        //   is what a developer debugging a chained error wants.
        //
        // Design choice — `map_err` + `format!` over `anyhow::Context`:
        //   The codegen target is standalone `rustc` with NO external
        //   runtime crate (the generated Cargo project has no `anyhow` /
        //   `thiserror` dependency — confirmed by prior tasks where external
        //   crates were deferred). Emitting `anyhow::Context` would require
        //   adding `anyhow` to every generated project; the `map_err` +
        //   `format!` desugar keeps the generated Rust self-contained. The
        //   trade-off is loss of typed error context (we get a `String`
        //   error, not a structured error chain) — typed context objects
        //   are deferred (see decisions.md).
        if method.name == "context" {
            let recv = self.lower_expr(receiver)?;
            return self.lower_context_call(recv, args);
        }

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
            // T23: Vector iteration methods. `.map` / `.filter` take a single
            // closure and return a new `Vec`; `.reduce` takes a 2-arg closure
            // and returns `Option<T>`. We use `.into_iter()` so the closure
            // params are owned values (Buff hides references from users).
            // `.push(x)` / `.pop()` / `.len()` need no special mapping —
            // they fall through to the default `recv.method(args)` arm below.
            "map" if args.len() == 1 => {
                let f = self.lower_expr(&args[0])?;
                self.lower_into_iter_collect(recv, "map", f)?
            }
            "filter" if args.len() == 1 => {
                let f = self.lower_expr(&args[0])?;
                self.lower_into_iter_collect(recv, "filter", f)?
            }
            "reduce" if args.len() == 1 => {
                let f = self.lower_expr(&args[0])?;
                self.lower_into_iter_reduce(recv, f)?
            }
            // T124f: Sort instance methods on Buff's existing Vector type.
            // Rust's `Vec::<T>::sort()` / `sort_by(cmp)` mutate in-place
            // and return `()`, but Buff's surface treats them as
            // functional (returns a NEW sorted Vector). Mirrors the
            // `.map()` / `.filter()` "return a fresh Vec" stance so
            // `[3, 1, 2].sort()` evaluates to `[1, 2, 3]` per the
            // acceptance criterion (rather than requiring a `let mut`
            // dance the user has to write).
            //
            // Built via `quote!` + parse2 as a `{ let mut __v = recv;
            // __v.sort(); __v }` block (the in-place mutation happens
            // inside the block, the block evaluates to the owned Vec).
            // The `__v` name is underscore-prefixed to avoid colliding
            // with user vars in the surrounding scope (Buff's
            // identifier convention reserves `__`-prefixed names for
            // codegen-introduced temporaries - mirrors the
            // `splice_receiver_into_call` precedent).
            //
            // `.sort()` (no args) uses Rust's `Ord` impl on the
            // element type (so Int vectors sort ascending, String
            // vectors sort lexicographically, ...).
            // `.sort_by(cmp)` (1 arg) takes a 2-arg closure returning
            // `std::cmp::Ordering` (Buff's surface mirrors Rust's
            // exactly - a future task may add a more ergonomic
            // comparator-builder API like `Sort.by(field).asc()`).
            "sort" if args.is_empty() => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let mut __v = #recv;
                        __v.sort();
                        __v
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("sort codegen parse: {e}")))?
            }
            "sort_by" if args.len() == 1 => {
                let cmp = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let mut __v = #recv;
                        __v.sort_by(#cmp);
                        __v
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("sort_by codegen parse: {e}")))?
            }
            // T25: Map methods. The Buff names map to Rust's standard
            // HashMap methods, except `.contains(k)` → `.contains_key(k)`
            // (Buff hides the `_key` suffix for ergonomics). `.get(k)`
            // returns `Option<&V>` in Rust; we keep it as-is (`Option<&V>`)
            // for v0.5 — a future task may add `.cloned()` to recover an
            // owned `Option<V>` if the move-by-default analysis requires it.
            // `.insert(k, v)`, `.remove(k)`, and `.len()` pass through
            // unchanged because Buff and Rust share those names.
            "contains" if args.len() == 1 => {
                let arg = self.lower_expr(&args[0])?;
                method_call_one_arg(recv, "contains_key", arg)
            }
            // `.get`, `.insert`, `.remove`, `.len` all share Rust's name —
            // they fall through to the default arm below with no special
            // mapping. We keep this comment block to document the T25 design
            // (so a future change doesn't accidentally rewrite these).
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
    pub(super) fn method_chain(
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
    pub(super) fn lower_graphemes_call(&self, recv: SynExpr) -> Result<SynExpr, CodegenError> {
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

    /// T78: lower `recv.context("msg")` to
    /// `recv.map_err(|e| format!("msg: {:?}", e))`.
    ///
    /// Attaches a human-readable context string to a `Result<T, E>`'s `Err`
    /// variant by wrapping the inner error into a formatted `String`. The
    /// generated Rust compiles under standalone `rustc` (no `anyhow` /
    /// `thiserror` needed) because it uses only `Result::map_err` and the
    /// stdlib `format!` macro.
    ///
    /// Built via `quote!` + `syn::parse2::<SynExpr>` (the standard pattern
    /// in this module — the single string producer remains
    /// `prettyplease::unparse`). The message is spliced into a
    /// [`syn::LitStr`] that already carries the `: {:?}` format-spec suffix
    /// so the resulting `format!(...)` call has the right shape.
    ///
    /// Argument contract:
    /// - EXACTLY one argument.
    /// - The argument MUST be a `Expr::Literal(Literal::String(_), _)`. Any
    ///   other shape (non-string literal, identifier, call, ...) returns an
    ///   `unsupported` error — codegen does NOT do type checking, so this
    ///   guards against silent mis-compilation of `.context(42)` or similar.
    ///
    /// The trailing `?` (if the source had `recv.context("msg")?`) is added
    /// by the EXISTING [`Self::lower_try`] path: the parser produces
    /// `Expr::Try { expr: MethodCall{...} }`, and `lower_try` wraps the
    /// lowered MethodCall in Rust's native `?`. So NO change to `lower_try`
    /// is required — this method only produces the `map_err` expression.
    ///
    /// Debug (`{:?}`) is chosen over Display (`{}`) because the std
    /// `Error: Debug` bound is universally implemented while `Display` is
    /// not — using `{:?}` guarantees the generated Rust compiles for ANY
    /// error type the user's `Result<T, E>` might carry. See the design
    /// note on the `context` arm in [`Self::lower_method_call`].
    pub(super) fn lower_context_call(&self, recv: SynExpr, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!(
                "context() expects exactly 1 string-literal arg, got {}",
                args.len()
            )));
        }
        let msg: &str = match &args[0] {
            Expr::Literal(Literal::String(s), _) => s.as_str(),
            other => {
                return Err(self.unsupported(&format!(
                    "context() expects a string literal, got {:?}",
                    other
                )));
            }
        };
        // Build the format-string literal: `"<msg>: {:?}"`.
        //
        // The user's message is the literal prefix; the `: {:?}` suffix
        // renders the original error via Debug. If the message itself
        // contains `{` or `}`, those WILL be interpreted as `format!`
        // placeholders at runtime — this matches the documented behavior
        // (context is a human-readable label, not a format template).
        // Braces in context labels are rare; escaping them would silently
        // rewrite the user's text. Keeping the message verbatim preserves
        // the WYSIWYG property tested by `error_context_preserves_message_*`.
        let fmt = format!("{}: {{:?}}", msg);
        let fmt_lit = syn::LitStr::new(&fmt, ProcSpan::call_site());
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #recv.map_err(|e| format!(#fmt_lit, e))
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("context codegen parse: {e}")))
    }

    /// Lower `s.slice(a, b)` to a char-safe slice expression.
    ///
    /// Emits (conceptually) `s.chars().skip(a).take(b - a).collect::<String>()`.
    /// A single-arg form `s.slice(a)` becomes `s.chars().skip(a).collect::<String>()`.
    pub(super) fn lower_slice_call(&mut self, recv: SynExpr, args: &[Expr]) -> Result<SynExpr, CodegenError> {
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

    /// Lower a Vector iteration method that returns a new `Vec` (T23).
    ///
    /// `recv.<method>(closure)` → `recv.into_iter().<method>(closure).collect::<Vec<_>>()`.
    /// Used by `.map` and `.filter`. We use `.into_iter()` so the closure
    /// receives owned values (Buff hides references from users); this
    /// consumes the receiver, matching Buff's move-by-default semantics.
    /// The `.collect::<Vec<_>>()` rebuilds a Vec so the result can be indexed
    /// or chained further.
    pub(super) fn lower_into_iter_collect(
        &self,
        recv: SynExpr,
        method: &str,
        closure: SynExpr,
    ) -> Result<SynExpr, CodegenError> {
        let method_ident = Ident::new(method, ProcSpan::call_site());
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #recv.into_iter().#method_ident(#closure).collect::<Vec<_>>()
        };
        syn::parse2(tokens).map_err(|e| self.unsupported(&format!("{method} codegen parse: {e}")))
    }

    /// Lower `.reduce(closure)` → `recv.into_iter().reduce(closure)` (T23).
    ///
    /// Returns `Option<T>` (Rust parity). The closure is a 2-arg `|a, b| …`.
    pub(super) fn lower_into_iter_reduce(
        &self,
        recv: SynExpr,
        closure: SynExpr,
    ) -> Result<SynExpr, CodegenError> {
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #recv.into_iter().reduce(#closure)
        };
        syn::parse2(tokens).map_err(|e| self.unsupported(&format!("reduce codegen parse: {e}")))
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
    pub(super) fn lower_string_interp(&mut self, parts: &[InterpPart]) -> Result<SynExpr, CodegenError> {
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
                InterpPart::Expr(e, spec) => {
                    // T81: use `{spec}` when a format specifier is present,
                    // otherwise `{}`.
                    if let Some(s) = spec {
                        fmt_string.push('{');
                        fmt_string.push_str(s);
                        fmt_string.push('}');
                    } else {
                        fmt_string.push_str("{}");
                    }
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

    /// Lower a collection literal `[e1, e2, ...]` to Rust's `vec![...]` macro
    /// (T23).
    ///
    /// The element expressions are lowered and spliced into the macro token
    /// stream via `quote!`, comma-separated. An empty literal lowers to
    /// `vec![]` (Rust infers the element type from context — typically the
    /// `let`-binding's type annotation, which the type inferencer drove).
    pub(super) fn lower_array_lit(&mut self, elements: &[Expr]) -> Result<SynExpr, CodegenError> {
        // Lower each element, then build `vec![e0, e1, ...]`. The `[` / `]`
        // come from the `Bracket` delimiter; the `tokens` stream holds just
        // the comma-separated element expressions (so `vec![]` for empty).
        let mut lowered: Vec<SynExpr> = Vec::with_capacity(elements.len());
        for e in elements {
            lowered.push(self.lower_expr(e)?);
        }
        let mut tokens: proc_macro2::TokenStream = proc_macro2::TokenStream::new();
        for (i, e) in lowered.iter().enumerate() {
            let e = e.clone();
            if i > 0 {
                tokens.extend(quote::quote! { , });
            }
            tokens.extend(quote::quote! { #e });
        }
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: syn::Path::from(Ident::new("vec", ProcSpan::call_site())),
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Bracket(Default::default()),
                tokens,
            },
        }))
    }

    /// Lower a 2-D Matrix index `m[row, col]` to the flat-storage access
    /// `m.data[(row * m.cols + col) as usize]` (T24).
    ///
    /// The base expression `m` is lowered ONCE and the resulting `SynExpr` is
    /// spliced (via [`SynExpr::clone`]) into two positions:
    /// - `m.data` — the field holding the flat `Vec<T>`.
    /// - `m.cols` — the field carrying the column count.
    ///
    /// The flat index expression `row * m.cols + col` is built as a Rust
    /// binary tree (`Mul(row, Field(m, cols))` then `Add(.., col)`) and the
    /// whole thing is wrapped in a single `as usize` cast via [`cast_to`]
    /// (which parenthesises its operand, yielding exactly
    /// `(row * m.cols + col) as usize`). The outer `m.data[...]` is a Rust
    /// index expression.
    ///
    /// Both `row` and `col` are lowered as-is (no per-operand cast); the
    /// single trailing `as usize` covers the whole flat expression. This
    /// matches the T24 acceptance string `m.data[(1 * m.cols + 2) as usize]`.
    ///
    /// **GPU-readiness note**: because storage is one contiguous `Vec<T>`,
    /// the same flat-index expression is what a WGSL shader would compute to
    /// address a storage buffer — the REFACTOR goal of "share flat-storage
    /// pattern with GPU buffer codegen" lands naturally here.
    pub(super) fn lower_matrix_index(
        &mut self,
        base: &Expr,
        row: &Expr,
        col: &Expr,
    ) -> Result<SynExpr, CodegenError> {
        let base_e = self.lower_expr(base)?;
        let row_e = self.lower_expr(row)?;
        let col_e = self.lower_expr(col)?;
        // `m.data` — field access on the lowered base.
        let data_field = field_access(base_e.clone(), "data");
        // `m.cols` — field access on the lowered base (clone preserves the
        // move analyzer's clone decision, if any, that was baked into base_e).
        let cols_field = field_access(base_e, "cols");
        // `row * m.cols`
        let row_times_cols = SynExpr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(row_e),
            op: syn::BinOp::Mul(Default::default()),
            right: Box::new(cols_field),
        });
        // `(row * m.cols) + col`
        let flat_expr = SynExpr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(row_times_cols),
            op: syn::BinOp::Add(Default::default()),
            right: Box::new(col_e),
        });
        // `((row * m.cols) + col) as usize` — cast_to wraps in parens.
        let flat_index = cast_to(flat_expr, "usize");
        // `m.data[flat_index]`
        Ok(SynExpr::Index(syn::ExprIndex {
            attrs: Vec::new(),
            expr: Box::new(data_field),
            bracket_token: Default::default(),
            index: Box::new(flat_index),
        }))
    }

    /// Lower `Matrix.new(rows, cols)` to Rust's `Matrix::new(rows, cols)`
    /// associated-function call (T24).
    ///
    /// The receiver `Matrix` is NOT lowered as a value (it names a type, not
    /// a variable) — we build the path `Matrix::new` directly and splice the
    /// lowered arguments. The arity is checked: exactly 2 args (rows, cols)
    /// are required. The `Matrix<T>` struct + `new` impl are emitted by
    /// [`Self::generate`] when this constructor appears in the program.
    pub(super) fn lower_matrix_new(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 2 {
            return Err(self.unsupported(&format!(
                "Matrix.new expects exactly 2 args (rows, cols), got {}",
                args.len()
            )));
        }
        let mut lowered: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
        lowered.push(self.lower_expr(&args[0])?);
        lowered.push(self.lower_expr(&args[1])?);
        Ok(SynExpr::Call(syn::ExprCall {
            attrs: Vec::new(),
            func: Box::new(SynExpr::Path(syn::ExprPath {
                attrs: Vec::new(),
                qself: None,
                path: rust_path("Matrix::new"),
            })),
            paren_token: Default::default(),
            args: lowered,
        }))
    }

}

