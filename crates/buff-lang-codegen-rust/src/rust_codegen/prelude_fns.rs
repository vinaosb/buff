//! ITER-34 - prelude FREE-FUNCTION lowering: lower_prelude_call
//! (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move of the `impl RustCodegen` method into this child module
//! so the parent file shrinks. lower_prelude_call is pub(super) (its only
//! call site is the parent FuncCall lowering, after the prelude lookup).
//! The per-arm helper methods it dispatches to (lower_print, lower_input,
//! lower_sleep, lower_convert, lower_min_max, ...) already live in the
//! method_call_lowering sibling (shared with the method-call path) and
//! stay there; ConvKind lives in the conv_helpers sibling. The parent
//! declares only `mod prelude_fns;` (inherent methods resolve by type,
//! no `use` needed). Child inherits parent imports via use super::* and
//! may call the parent private methods (descendant privacy).

use super::*;

impl RustCodegen {
    /// Lower a Buff **prelude** call to the corresponding Rust idiom (T96).
    ///
    /// The prelude is the implicit standard library — these functions are
    /// available in every Buff program without an `import`. The mappings are
    /// grouped by category (matching [`buff_lang_types::prelude`]):
    ///
    /// # Math
    ///
    /// | Buff            | Rust                              | Notes             |
    /// |-----------------|-----------------------------------|-------------------|
    /// | `abs(x)`        | `(x).abs()`                       | works for any numeric (i64 / f32 / f64) |
    /// | `min(a, b)`     | `(a).min(b)`                      | `Ord::min` for ints, inherent `min` for floats |
    /// | `max(a, b)`     | `(a).max(b)`                      | analogous         |
    /// | `sqrt(x)`       | `((x) as f64).sqrt()`             | always returns `f64`; coerce arg up so int args work |
    /// | `floor(x)`      | `((x) as f64).floor()`            | always returns `f64` |
    /// | `ceil(x)`       | `((x) as f64).ceil()`             | always returns `f64` |
    /// | `round(x)`      | `((x) as f64).round()`            | always returns `f64` |
    /// | `pow(b, e)`     | `(b).powf((e) as f64)` if `b` is float-like; else `(b).pow((e) as u32)` | the inferencer picks the arm |
    ///
    /// # Type conversions
    ///
    /// The arg's inferred type drives the Rust idiom:
    ///
    /// | Buff            | Rust (arg is String)               | Rust (arg is numeric)         |
    /// |-----------------|------------------------------------|-------------------------------|
    /// | `Int(x)`        | `x.parse::<i64>().unwrap_or(0)`    | `(x) as i64`                  |
    /// | `Float(x)`      | `x.parse::<f32>().unwrap_or(0.0)`  | `(x) as f32`                  |
    /// | `Bool(x)`       | `x.parse::<bool>().unwrap_or(false)` | `(x) != 0`                  |
    /// | `String(x)`     | `x.to_string()` (any `Display`)    | `x.to_string()`              |
    ///
    /// **Parse-failure policy (v0.5):** `Int("bad")` returns `0`,
    /// `Float("bad")` returns `0.0`, `Bool("bad")` returns `false`. We use
    /// `unwrap_or` (not `expect`/`unwrap`) so generated code never panics
    /// on malformed runtime input. A proper `Result`-returning conversion
    /// API is deferred — see T96 notes in `decisions.md`.
    ///
    /// # I/O
    ///
    /// | Buff                  | Rust                                                        |
    /// |-----------------------|-------------------------------------------------------------|
    /// | `print("lit")`        | `println!("lit")` (a bare string literal drops the `{}`)    |
    /// | `print(x)` / `println(x)` (non-literal) | `println!("{}", x)`                          |
    /// | `read_line()`         | `{ let mut s = String::new(); std::io::stdin().read_line(&mut s).ok(); s }` |
    ///
    /// `read_line()` swallows the trailing newline (matches Rust's
    /// `read_line` semantics — callers can `.trim_end()` if they want it
    /// gone). The `.ok()` discards the `io::Result` error as `Some(())`/
    /// `None` (we don't panic on I/O failure).
    pub(super) fn lower_prelude_call(
        &mut self,
        fn_: PreludeFn,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        match fn_ {
            // ----- Math ---------------------------------------------------
            PreludeFn::Abs => {
                self.lower_one_arg_method(args, "abs", /*wrap_parens*/ true)
            }
            PreludeFn::Min => self.lower_min_max(args, "min"),
            PreludeFn::Max => self.lower_min_max(args, "max"),
            PreludeFn::Sqrt => self.lower_float_unary(args, "sqrt"),
            PreludeFn::Floor => self.lower_float_unary(args, "floor"),
            PreludeFn::Ceil => self.lower_float_unary(args, "ceil"),
            PreludeFn::Round => self.lower_float_unary(args, "round"),
            PreludeFn::Pow => self.lower_pow(args),

            // ----- Conversions -------------------------------------------
            PreludeFn::Int => self.lower_convert(args, "i64", ConvKind::Numeric),
            PreludeFn::Float => self.lower_convert(args, "f32", ConvKind::Numeric),
            PreludeFn::Bool => self.lower_convert(args, "bool", ConvKind::Bool),
            PreludeFn::String => self.lower_to_string(args),

            // ----- I/O ---------------------------------------------------
            PreludeFn::Print => self.lower_print(args),
            PreludeFn::Println => self.lower_print(args),
            PreludeFn::ReadLine => Ok(self.lower_read_line()),
            // T124g: input() / input(prompt) - read one line from stdin
            // (optionally printing a prompt first). The prompt is print!
            // (no newline) so the user's input appears on the same line.
            // Trailing newline from read_line is trimmed. Wraps
            // std::io::stdin + std::io::Write::flush (for the prompt).
            PreludeFn::Input => self.lower_input(args),

            // ----- System / environment (T99) ---------------------------
            // args() → std::env::args().collect::<Vec<String>>()
            PreludeFn::Args => {
                if !args.is_empty() {
                    return Err(self.unsupported("args() takes no arguments"));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::args().collect::<Vec<String>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("args() codegen parse: {e}")))
            }
            // env("NAME") → std::env::var("NAME").ok()
            PreludeFn::Env => {
                if args.len() != 1 {
                    return Err(
                        self.unsupported("env() expects exactly 1 argument (the variable name)")
                    );
                }
                let arg = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::var(#arg).ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("env() codegen parse: {e}")))
            }
            // exit(code) → std::process::exit(code)
            PreludeFn::Exit => {
                if args.len() != 1 {
                    return Err(
                        self.unsupported("exit() expects exactly 1 argument (the exit code)")
                    );
                }
                let arg = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::process::exit(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("exit() codegen parse: {e}")))
            }
            // T124g: sleep(duration) - async-transparent sleep. Lowers to
            // `tokio::time::sleep(<duration>).await`. The `.await` is
            // unconditional (Buff has no `await` keyword - the codegen
            // inserts it). The enclosing fn MUST be async; the T31
            // propagation walker doesn't YET know about `sleep` (a
            // future task can teach it), so for now the user must
            // declare an async fn that calls sleep (or call sleep from
            // main, which is auto-stamped `#[tokio::main]` when
            // propagated). The codegen boundary is the established
            // "codegen-only linking" pattern (single-file rustc link of
            // tokio is deferred - same as chrono/regex/toml/rand).
            //
            // Duration arg: the canonical Buff form is
            // `sleep(Duration.seconds(N))` which would lower to
            // `tokio::time::sleep(chrono::TimeDelta::seconds(N)).await` -
            // BUT chrono::TimeDelta is NOT a std::time::Duration (which
            // tokio::time::sleep requires). To keep the surface
            // ergonomic AND the generated code self-contained, we
            // detect the `Duration.<unit>(N)` AST shape and lower it
            // directly to `std::time::Duration::from_<unit>(N)` (no
            // chrono dependency in the sleep path). Plain Int args are
            // treated as seconds (`from_secs`). Other arg shapes pass
            // through unchanged (user responsibility - useful for
            // `std::time::Duration::from_millis(100)` directly if the
            // user constructs it).
            PreludeFn::Sleep => self.lower_sleep(args),
            // T35: assert_eq(a, b) → assert_eq!(a, b)
            // The Rust `assert_eq!` macro panics when the two args are not
            // equal, which is exactly Buff's semantics. Used inside `@test`
            // functions (where the test runner catches the panic).
            PreludeFn::AssertEq => {
                if args.len() != 2 {
                    return Err(self.unsupported(
                        "assert_eq() expects exactly 2 arguments (actual, expected)",
                    ));
                }
                let lhs = self.lower_expr(&args[0])?;
                let rhs = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    assert_eq!(#lhs, #rhs)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("assert_eq() codegen parse: {e}")))
            }
            // T38: assertThat(value) → buff_assertions::assertThat(value)
            // The fluent assertion entry point. Lowers to the
            // buff_assertions crate's assertThat function, which returns
            // an AssertThat<T> wrapper with chainable methods.
            PreludeFn::AssertThat => {
                if args.len() != 1 {
                    return Err(self.unsupported(
                        "assertThat() expects exactly 1 argument (the value to assert on)",
                    ));
                }
                let value = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_assertions::assertThat(#value)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("assertThat() codegen parse: {e}")))
            }
        }
    }
}
