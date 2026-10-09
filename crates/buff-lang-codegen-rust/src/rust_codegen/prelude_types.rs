//! ITER-31 - prelude-type associated-function lowering:
//! lower_prelude_type_assoc_fn + its private helper lower_log_call
//! (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move of `impl RustCodegen` methods into this child module so the
//! parent file shrinks. lower_prelude_type_assoc_fn is pub(super) (called
//! from the parent and the method_call_lowering sibling); lower_log_call
//! stays private (its only call site is the Log arm inside this module). The
//! parent declares only `mod prelude_types;` (inherent methods resolve by
//! type, no `use` needed). Child inherits parent imports via use super::*
//! and may call the parent private methods (descendant privacy) and the
//! extracted helper modules.
//!
//! ITER-42: split along the natural internal seam - the ~90
//! framework-crate assoc-fn arms (DataFrame .. Decimal, everything
//! after the `(T::Channel, A::New)` arm) moved verbatim into the
//! `framework` grandchild module (`prelude_types/framework.rs`).
//! This file keeps the arity closures (one_arg/no_args/two_args/
//! n_args - they capture ptype/pmethod/args), the Log/time/text/
//! encoding/filesystem/network arms, and the lower_log_call helper;
//! the match delegates the remainder via a trailing wildcard arm
//! that passes the arity closures through, so every moved arm body
//! is byte-identical and the combined match order is unchanged.

use super::*;

mod framework;
impl RustCodegen {
    /// T124b: lower a prelude-type associated-function call (`Type.method(args)`)
    /// to the corresponding chrono / std::time Rust idiom.
    ///
    /// Dispatched from [`Self::lower_method_call`] when the receiver is a
    /// bare Ident naming a prelude type (DateTime, Date, Time, Duration,
    /// Instant). The method name is matched on the resolved
    /// `(PreludeType, PreludeAssocFn)` pair rather than raw strings so the
    /// prelude-types registry is the single source of truth.
    ///
    /// # Lowering table
    ///
    /// | Buff source                  | Generated Rust                                |
    /// |------------------------------|-----------------------------------------------|
    /// | `DateTime.now()`             | `chrono::Utc::now()`                          |
    /// | `DateTime.parse(s)`          | `chrono::DateTime::parse_from_rfc3339(s).unwrap_or(chrono::Utc::now())` |
    /// | `Date.today()`               | `chrono::Local::now().date_naive()`           |
    /// | `Date.parse(s)`              | `chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap_or(chrono::Local::now().date_naive())` |
    /// | `Instant.now()`              | `std::time::Instant::now()`                   |
    /// | `Duration.days(n)`           | `chrono::TimeDelta::days(n)`                  |
    /// | `Duration.hours(n)`          | `chrono::TimeDelta::hours(n)`                 |
    /// | `Duration.minutes(n)`        | `chrono::TimeDelta::minutes(n)`               |
    /// | `Duration.seconds(n)`        | `chrono::TimeDelta::seconds(n)`               |
    /// | `Duration.millis(n)`         | `chrono::TimeDelta::milliseconds(n)`          |
    ///
    /// The `parse` lowering uses `unwrap_or(<default>)` rather than `unwrap()`
    /// so generated user code never panics on a malformed input string —
    /// matching Buff's "no panicking generated code" stance where practical.
    /// (The user can still opt into panic-on-error by chaining `??` or
    /// `match`-ing once Result-shaped prelude values are added.)
    pub(super) fn lower_prelude_type_assoc_fn(
        &mut self,
        ptype: buff_lang_types::PreludeType,
        pmethod: buff_lang_types::PreludeAssocFn,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::{PreludeAssocFn as A, PreludeType as T};
        // Lower a single arg, erroring on arity mismatch.
        let one_arg = |c: &mut Self| -> Result<SynExpr, CodegenError> {
            if args.len() != 1 {
                return Err(c.unsupported(&format!(
                    "{}.{}() expects exactly 1 arg, got {}",
                    ptype.name(),
                    pmethod.name(),
                    args.len()
                )));
            }
            c.lower_expr(&args[0])
        };
        // Lower zero args, erroring if any were passed.
        let no_args = |c: &mut Self| -> Result<(), CodegenError> {
            if !args.is_empty() {
                return Err(c.unsupported(&format!(
                    "{}.{}() takes no arguments, got {}",
                    ptype.name(),
                    pmethod.name(),
                    args.len()
                )));
            }
            Ok(())
        };
        // Lower exactly two args, erroring on arity mismatch. Returns
        // a 2-tuple so T34 (buff-auth) call sites that destructure
        // `(token, secret)` work without refactor. Mirrors `one_arg`.
        let two_args = |c: &mut Self| -> Result<(SynExpr, SynExpr), CodegenError> {
            if args.len() != 2 {
                return Err(c.unsupported(&format!(
                    "{}.{}() expects exactly 2 args, got {}",
                    ptype.name(),
                    pmethod.name(),
                    args.len()
                )));
            }
            let a0 = c.lower_expr(&args[0])?;
            let a1 = c.lower_expr(&args[1])?;
            Ok((a0, a1))
        };
        // Lower exactly N args, erroring on arity mismatch. Returns the
        // lowered args as a Vec so multi-arg prelude calls (Math.pow,
        // Math.min/max, Random.int, Strings.split/join/replace/...)
        // can destructure them positionally.
        let n_args = |c: &mut Self, n: usize| -> Result<Vec<SynExpr>, CodegenError> {
            if args.len() != n {
                return Err(c.unsupported(&format!(
                    "{}.{}() expects exactly {} arg(s), got {}",
                    ptype.name(),
                    pmethod.name(),
                    n,
                    args.len()
                )));
            }
            args.iter().map(|a| c.lower_expr(a)).collect()
        };
        match (ptype, pmethod) {
            // T124c: Log module — Log.<level>(msg, key: val, ...) lowers
            // to the corresponding tracing macro. Dispatched to a
            // dedicated helper because the Log call signature is
            // variadic (positional msg + named fields) and the lowering
            // produces a MACRO invocation (not a function call), unlike
            // every other prelude-type assoc fn. Must run BEFORE the
            // chrono/std::time arms below — `(Log, _)` is not matched by
            // any of them, so the early-return is also a correctness
            // guard.
            (T::Log, _) => self.lower_log_call(pmethod, args),
            // ----- Time constructors ----------------------------------------
            (T::DateTime, A::Now) => {
                no_args(self)?;
                Ok(rust_call_expr("chrono::Utc::now", Vec::new()))
            }
            (T::Instant, A::Now) => {
                no_args(self)?;
                Ok(rust_call_expr("std::time::Instant::now", Vec::new()))
            }
            (T::Date, A::Today) => {
                no_args(self)?;
                // chrono::Local::now().date_naive() — the system's local
                // date "today". `date_naive()` strips the timezone to give
                // a `NaiveDate`.
                let inner = rust_call_expr("chrono::Local::now", Vec::new());
                Ok(method_call_no_args(inner, "date_naive"))
            }
            // ----- Parsing --------------------------------------------------
            (T::DateTime, A::Parse) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                // chrono::DateTime::parse_from_rfc3339(&s).unwrap_or(chrono::Utc::now())
                let parse_call = rust_call_expr("chrono::DateTime::parse_from_rfc3339", vec![arg]);
                let fallback = rust_call_expr("chrono::Utc::now", Vec::new());
                Ok(method_call_one_arg(parse_call, "unwrap_or", fallback))
            }
            (T::Date, A::Parse) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                // chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").unwrap_or(<today>)
                let fmt_lit = str_lit_expr("%Y-%m-%d");
                let parse_call =
                    rust_call_expr("chrono::NaiveDate::parse_from_str", vec![arg, fmt_lit]);
                let today_recv = rust_call_expr("chrono::Local::now", Vec::new());
                let today = method_call_no_args(today_recv, "date_naive");
                Ok(method_call_one_arg(parse_call, "unwrap_or", today))
            }
            // ----- Duration constructors ------------------------------------
            (T::Duration, A::Days) => {
                let arg = one_arg(self)?;
                Ok(rust_call_expr("chrono::TimeDelta::days", vec![arg]))
            }
            (T::Duration, A::Hours) => {
                let arg = one_arg(self)?;
                Ok(rust_call_expr("chrono::TimeDelta::hours", vec![arg]))
            }
            (T::Duration, A::Minutes) => {
                let arg = one_arg(self)?;
                Ok(rust_call_expr("chrono::TimeDelta::minutes", vec![arg]))
            }
            (T::Duration, A::Seconds) => {
                let arg = one_arg(self)?;
                Ok(rust_call_expr("chrono::TimeDelta::seconds", vec![arg]))
            }
            (T::Duration, A::Millis) => {
                let arg = one_arg(self)?;
                Ok(rust_call_expr("chrono::TimeDelta::milliseconds", vec![arg]))
            }
            // T124d: Regex.compile(pattern) -> Regex. Mirrors the
            // DateTime.parse "unwrap_or(<default>)" pattern (T124b):
            // `regex::Regex::new` is fallible (`Result<Regex, Error>`),
            // but Buff's prelude-type ctor surface is infallible (no
            // Result return). An invalid pattern yields a never-matching
            // fallback regex `r"a^"` (provably valid syntax: an `a`
            // followed by start-of-string anchor — syntactically valid,
            // semantically never matches anything). The inner `.unwrap()`
            // on this known-valid literal is the established Rust idiom
            // for infallible fallback from a fallible ctor when no
            // const-fn constructor exists (regex has no `Regex::empty()`
            // or const constructor; chrono's `Utc::now()` is the
            // equivalent trusted call in the T124b precedent).
            (T::Regex, A::Compile) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                // regex::Regex::new(pattern).unwrap_or_else(|_| regex::Regex::new(r"a^").unwrap())
                let new_call = rust_call_expr("regex::Regex::new", vec![arg]);
                let fallback = rust_call_expr("regex::Regex::new", vec![str_lit_expr(r"a^")]);
                // Build `.unwrap_or_else(|_| <fallback>.unwrap())` via
                // quote! + parse2 (the closure arg is awkward to build
                // directly via syn::ExprMethodCall).
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #new_call.unwrap_or_else(|_| #fallback.unwrap())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Regex.compile codegen parse: {e}")))
            }
            // T124e: Toml.parse(s) -> Map<String, Unknown>. Mirrors the
            // Regex.compile "unwrap_or_else(<default>)" panic-free
            // pattern: `toml::from_str` is fallible
            // (`Result<T, toml::de::Error>`), but Buff's prelude-type
            // surface is infallible. A parse failure yields an empty
            // Map via `.unwrap_or_default()` (HashMap impls Default) —
            // NO panic, matching the T124b/T124d precedent.
            //
            // The turbofish `::<std::collections::HashMap<String,
            // toml::Value>>` makes the concrete return type explicit in
            // the generated Rust. Buff's inferred return type
            // (Map<String, Unknown>) is the looser surface contract;
            // the turbofish pins the runtime representation so the
            // generated Rust is fully typed without requiring a let-
            // binding's type annotation to drive inference.
            //
            // The turbofish path can NOT be built via `rust_call_expr`
            // (which splits on `::` and creates Idents —
            // `<std::collections::...>` is a turbofish arg, not a path
            // segment). We build the whole call via `quote!` instead.
            //
            // String literals lower to `&'static str` already (no
            // borrow needed); non-literal String-typed args get an `&`
            // via `coerce_str_arg_to_ref` so Rust's Deref coercion
            // turns `&String` into `&str` (the type `toml::from_str`
            // requires).
            (T::Toml, A::Parse) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                // toml::from_str::<HashMap<String, toml::Value>>(s)
                //     .unwrap_or_default()
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    toml::from_str::<std::collections::HashMap<String, toml::Value>>(#arg)
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Toml.parse codegen parse: {e}")))
            }
            // T124e: Toml.stringify(v) -> String. Mirrors the
            // Toml.parse panic-free pattern: `toml::to_string` is
            // fallible (`Result<String, toml::ser::Error>`), but Buff
            // surfaces it as infallible. A serialization failure yields
            // the empty string via `.unwrap_or_default()` (String
            // impls Default) — NO panic.
            //
            // The arg is taken by `&v` because `toml::to_string`
            // requires `&impl Serialize`. Any value the user passes (a
            // Map<String, ?>, a struct, ...) is borrowed — the Rust
            // Serialize bound is checked at the rustc level (a value
            // that doesn't impl Serialize surfaces as a regular Rust
            // compile error, not a Buff codegen error).
            //
            // Built via `quote!` directly so the `& #arg` borrow is a
            // real syn::ExprRef (not a path-segment hack).
            (T::Toml, A::Stringify) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    toml::to_string(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Toml.stringify codegen parse: {e}")))
            }
            // T124f: Math module - all 11 assoc fns wrap Rust's `f64`
            // methods. The arg is cast to `f64` first (so an Int arg
            // like `Math.sqrt(16)` works as well as a Float arg like
            // `Math.sqrt(2.0)`) - this matches the spec's acceptance
            // criterion `Math.sqrt(16) -> 4.0`. The cast is wrapped in
            // parens via `cast_to` so compound expressions bind
            // correctly: `Math.sqrt(a + b)` -> `((a + b) as f64).sqrt()`.
            //
            // Math uses only Rust `std` (NO extern crate needed) -
            // every `f64` method is on the primitive type directly.
            //
            // UNARY MATH METHODS (1 arg): sqrt / sin / cos / tan / abs
            // / floor / ceil / round. Each lowers to `(<arg> as f64).<method>()`.
            // We use a single shared code path: build the method call
            // via `quote!` so the cast + method-chain is one
            // well-formed `syn::Expr`.
            (T::Math, A::Sqrt) => lower_math_unary(one_arg(self)?, "sqrt"),
            (T::Math, A::Sin) => lower_math_unary(one_arg(self)?, "sin"),
            (T::Math, A::Cos) => lower_math_unary(one_arg(self)?, "cos"),
            (T::Math, A::Tan) => lower_math_unary(one_arg(self)?, "tan"),
            (T::Math, A::Abs) => lower_math_unary(one_arg(self)?, "abs"),
            (T::Math, A::Floor) => lower_math_unary(one_arg(self)?, "floor"),
            (T::Math, A::Ceil) => lower_math_unary(one_arg(self)?, "ceil"),
            (T::Math, A::Round) => lower_math_unary(one_arg(self)?, "round"),
            // BINARY MATH METHODS (2 args): pow / min / max.
            // - `Math.pow(base, exp)` -> `((base as f64).powf(exp as f64))`.
            //   Both args cast to f64 because `f64::powf` takes `f64`.
            // - `Math.min(a, b)` / `Math.max(a, b)` -> `(a as f64).min(b as f64)`.
            //   Both args cast to f64 for symmetry with pow (Rust's
            //   `f64::min` / `f64::max` take `f64`).
            (T::Math, A::Pow) => {
                let args = n_args(self, 2)?;
                let (base, exp) = (args[0].clone(), args[1].clone());
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#base as f64).powf(#exp as f64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Math.pow codegen parse: {e}")))
            }
            (T::Math, A::Min) => lower_math_binary(n_args(self, 2)?, "min"),
            (T::Math, A::Max) => lower_math_binary(n_args(self, 2)?, "max"),
            // T124f: Random module - 4 assoc fns wrapping the `rand`
            // crate (0.9 API). All use `rand::rng()` to obtain
            // a thread-local RNG (NOT cryptographically secure - the
            // plan defers CSPRNG to a future Hash/Crypto module).
            //
            // `Random.int(min, max)` -> `rand::rng().random_range(min..=max)`.
            // The inclusive range `min..=max` matches the spec's
            // acceptance criterion `Random.int(1, 10)` returns int in
            // [1, 10] (NOT [1, 11)). `random_range` is the rand 0.9 API
            // (rand 0.8 called it `gen_range`).
            (T::Random, A::Int) => {
                let args = n_args(self, 2)?;
                let (lo, hi) = (args[0].clone(), args[1].clone());
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    rand::rng().random_range(#lo..=#hi)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Random.int codegen parse: {e}")))
            }
            // `Random.float()` -> `rand::rng().random::<f64>()`.
            // Returns f64 in `[0, 1)`. Zero args. Uses `random::<f64>()`
            // (rand 0.9 API; rand 0.8 called it `gen::<f64>()`).
            (T::Random, A::Float) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    rand::rng().random::<f64>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Random.float codegen parse: {e}")))
            }
            // `Random.choice(vec)` -> `rand::seq::IndexedRandom::choose(
            //   &vec, &mut rand::rng()).cloned()`.
            //
            // Returns `Option<T>` (None on empty input - NEVER panics,
            // matching Buff's "no panicking generated code" rule). The
            // fully-qualified `IndexedRandom::choose` path avoids needing
            // a `use rand::seq::IndexedRandom;` import in the generated
            // crate. The `.cloned()` lifts `Option<&T>` to `Option<T>`
            // so the user gets an owned value (Buff hides references).
            //
            // Acceptance: `Random.choice([1, 2, 3])` returns `Option<Int>`
            // (Some(1) / Some(2) / Some(3) at random; never None on a
            // non-empty input).
            (T::Random, A::Choice) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    rand::seq::IndexedRandom::choose(&#arg, &mut rand::rng()).cloned()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Random.choice codegen parse: {e}")))
            }
            // `Random.shuffle(vec)` -> `{ let mut __v = vec;
            //   rand::seq::IndexedRandom::shuffle(&mut __v, &mut
            //   rand::rng()); __v }`.
            //
            // Returns a NEW shuffled Vector (the input is consumed -
            // the codegen makes a `let mut` binding internally and
            // returns it; in Buff's move-by-default world this is the
            // natural ownership transfer). The fully-qualified
            // `IndexedRandom::shuffle` path avoids needing a `use` import.
            //
            // Built via `quote!` + parse2 because the block expression
            // (`{ let mut __v = ...; ...; __v }`) is awkward to build
            // via direct syn node construction. The result is a single
            // `syn::Expr::Block` that evaluates to the shuffled Vec.
            //
            // NOTE: the local binding name `__v` is deliberately
            // underscore-prefixed to avoid colliding with user vars in
            // the surrounding scope (Buff's identifier namespace
            // convention reserves `__`-prefixed names for codegen-
            // introduced temporaries - mirrors the `__recv` placeholder
            // pattern used in `splice_receiver_into_call`).
            (T::Random, A::Shuffle) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let mut __v = #arg;
                        rand::seq::IndexedRandom::shuffle(&mut __v, &mut rand::rng());
                        __v
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Random.shuffle codegen parse: {e}")))
            }
            // T124f: Strings module - 8 assoc fns wrapping Rust's `str`
            // / `String` methods as functional module calls. Strings
            // uses only Rust `std` (NO extern crate needed).
            //
            // Each arg of String type is borrowed via
            // `coerce_str_arg_to_ref` so Rust's Deref coercion turns
            // `&String` into `&str` (the type `str` methods take).
            // String literals lower to `&'static str` already (no
            // borrow needed).
            //
            // `Strings.split(text, sep)` ->
            //   `text.split(sep).map(|s| s.to_string()).collect::<Vec<String>>()`.
            //
            // The `.map(|s| s.to_string())` lifts `&str` to `String`
            // (Buff hides references from users). The turbofish
            // `::<Vec<String>>` pins the concrete return type so the
            // generated Rust is fully typed without a let-binding
            // annotation.
            (T::Strings, A::Split) => {
                let lowered = n_args(self, 2)?;
                let raw_text = lowered[0].clone();
                let raw_sep = lowered[1].clone();
                let text = coerce_str_arg_to_ref(raw_text, &args[0]);
                let sep = coerce_str_arg_to_ref(raw_sep, &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #text.split(#sep).map(|s| s.to_string()).collect::<Vec<String>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strings.split codegen parse: {e}")))
            }
            // `Strings.join(vec, sep)` -> `vec.join(&sep)`.
            // The sep is borrowed via `&` so both `'static str` and
            // `String` inputs satisfy `Vec::<String>::join`'s `&str`
            // bound. The vec itself is taken by value (`Vec::<String>::join`
            // takes `&self`, so the codegen auto-borrows via
            // `method_call_one_arg` which produces `vec.join(&sep)`).
            (T::Strings, A::Join) => {
                let lowered = n_args(self, 2)?;
                let vec_e = lowered[0].clone();
                let sep_e = lowered[1].clone();
                // Borrow the sep via `&` to satisfy `&str` bound on
                // `Vec::<String>::join(&self, sep: &str)`. The vec is
                // method-called directly (auto-borrowed by Rust's
                // method-call sugar).
                let sep_borrowed = syn::Expr::Reference(syn::ExprReference {
                    attrs: Vec::new(),
                    and_token: Default::default(),
                    expr: Box::new(sep_e),
                    mutability: None,
                });
                Ok(method_call_one_arg(vec_e, "join", sep_borrowed))
            }
            // `Strings.trim(text)` -> `text.trim().to_string()`.
            // `.trim()` returns `&str`; `.to_string()` lifts to `String`.
            (T::Strings, A::Trim) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #arg_ref.trim().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strings.trim codegen parse: {e}")))
            }
            // `Strings.replace(text, from, to)` -> `text.replace(from, to)`.
            // Rust's `str::replace` takes `&str, &str` (or a Pattern)
            // and returns a NEW `String`. We borrow each arg via
            // `coerce_str_arg_to_ref` so `&String` derefs to `&str`.
            (T::Strings, A::Replace) => {
                let lowered = n_args(self, 3)?;
                let raw_text = lowered[0].clone();
                let raw_from = lowered[1].clone();
                let raw_to = lowered[2].clone();
                let text = coerce_str_arg_to_ref(raw_text, &args[0]);
                let from = coerce_str_arg_to_ref(raw_from, &args[1]);
                let to = coerce_str_arg_to_ref(raw_to, &args[2]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #text.replace(#from, #to)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strings.replace codegen parse: {e}")))
            }
            // `Strings.contains(text, substr)` -> `text.contains(substr)`.
            // Returns Bool. The substr arg is borrowed via `&` so both
            // `'static str` and `String` inputs satisfy `str::contains`'s
            // Pattern bound (str: Pattern works directly).
            (T::Strings, A::Contains) => {
                let lowered = n_args(self, 2)?;
                let text_ref = coerce_str_arg_to_ref(lowered[0].clone(), &args[0]);
                let sep_ref = coerce_str_arg_to_ref(lowered[1].clone(), &args[1]);
                Ok(method_call_one_arg(text_ref, "contains", sep_ref))
            }
            // `Strings.starts_with(text, prefix)` -> `text.starts_with(prefix)`.
            // Same shape as `contains`.
            (T::Strings, A::StartsWith) => {
                let lowered = n_args(self, 2)?;
                let text_ref = coerce_str_arg_to_ref(lowered[0].clone(), &args[0]);
                let pre_ref = coerce_str_arg_to_ref(lowered[1].clone(), &args[1]);
                Ok(method_call_one_arg(text_ref, "starts_with", pre_ref))
            }
            // `Strings.to_uppercase(text)` -> `text.to_uppercase().to_string()`.
            // `.to_uppercase()` returns `String` already (Rust's
            // `str::to_uppercase` -> `String`), so the extra
            // `.to_string()` is a no-op for type but keeps the
            // generated code uniform with `trim` (both produce
            // `String`). Belt-and-suspenders: if a future Rust version
            // changes `to_uppercase` to return `Cow<str>` or similar,
            // the explicit `.to_string()` keeps Buff's surface stable.
            (T::Strings, A::ToUppercase) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #arg_ref.to_uppercase().to_string()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Strings.to_uppercase codegen parse: {e}"))
                })
            }
            // `Strings.to_lowercase(text)` -> `text.to_lowercase().to_string()`.
            // Same shape as `to_uppercase`.
            (T::Strings, A::ToLowercase) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #arg_ref.to_lowercase().to_string()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Strings.to_lowercase codegen parse: {e}"))
                })
            }
            // T124g: Args module - 2 assoc fns wrapping Rust's
            // `std::env::args` iterator. Args uses only Rust `std` (NO
            // extern crate needed).
            //
            // `Args.list()` -> `std::env::args().collect::<Vec<String>>()`.
            // Zero args. The turbofish `::<Vec<String>>` pins the
            // concrete return type so the generated Rust is fully typed
            // without a let-binding annotation (mirrors the Strings.split
            // turbofish pattern from T124f).
            (T::Args, A::List) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::args().collect::<Vec<String>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Args.list codegen parse: {e}")))
            }
            // `Args.get(i)` -> `std::env::args().nth(i).unwrap_or_default()`.
            // One Int arg. The `.unwrap_or_default()` yields the empty
            // String on out-of-bounds (NEVER panics - matching Buff's
            // "no panicking generated code" rule, mirroring the
            // Toml.parse / Regex.compile unwrap_or-panic-free pattern).
            (T::Args, A::Get) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::args().nth(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Args.get codegen parse: {e}")))
            }
            // T124g: Env module - 3 assoc fns wrapping Rust's
            // `std::env::var` / `set_var`. Env uses only Rust `std` (NO
            // extern crate needed).
            //
            // `Env.get("KEY")` -> `std::env::var(k).ok()`. One String
            // arg. Returns Option<String> (None when unset OR invalid
            // UTF-8 - both folded into None). Same `Get` variant as
            // Args.get but dispatched on (Env, Get); the codegen here
            // differs from (Args, Get) because the semantics differ
            // (var lookup vs positional arg). Mirrors the (DateTime,
            // Parse) / (Date, Parse) / (Toml, Parse) overload-by-type
            // pattern.
            //
            // String literals lower to `&'static str` already (no
            // borrow needed); non-literal String args get an `&` via
            // `coerce_str_arg_to_ref` so Rust's Deref coercion turns
            // `&String` into `&str` (the type `std::env::var` takes).
            (T::Env, A::Get) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::var(#arg).ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Env.get codegen parse: {e}")))
            }
            // `Env.set("KEY", "value")` -> `unsafe { std::env::set_var(k, v); }`.
            // Two String args. Returns Void. NOTE: `std::env::set_var`
            // is `unsafe` in Rust Edition 2024 (the edition the generated
            // code targets). The `unsafe { ... }` wrapper satisfies the
            // edition requirement.
            //
            // Both args are borrowed via `&` so Rust's Deref coercion
            // turns `&String` into `&str` (the type `set_var` takes).
            // The result is wrapped in a block `{ unsafe { ... }; }` so
            // the expression yields `()` (the call itself returns `()`
            // so the block is technically redundant, but uniform with
            // other Void-returning prelude calls avoids special-case
            // handling in expression-statement position).
            (T::Env, A::Set) => {
                let lowered = n_args(self, 2)?;
                let k = coerce_str_arg_to_ref(lowered[0].clone(), &args[0]);
                let v = coerce_str_arg_to_ref(lowered[1].clone(), &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    unsafe { std::env::set_var(#k, #v); }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Env.set codegen parse: {e}")))
            }
            // `Env.has("KEY")` -> `std::env::var(k).is_ok()`. One String
            // arg. Returns Bool. Same borrow coercion as Env.get.
            (T::Env, A::Has) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::var(#arg).is_ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Env.has codegen parse: {e}")))
            }
            // T114: Env.load(path) -> Map<String, String>. Load a .env
            // file (KEY=VALUE per line) into the process environment.
            // One optional arg (path, defaults to ".env"). Returns the
            // loaded key-value pairs. Does NOT override existing env vars
            // (only sets if absent). Simple parsing: one KEY=VALUE per
            // line, skip `#` comments, skip blank lines. No complex .env
            // syntax (multiline, quotes).
            //
            // Lowers to a helper block that:
            //   1. Reads the file via std::fs::read_to_string
            //   2. Parses KEY=VALUE lines (skip # comments, blank lines)
            //   3. Sets each var via unsafe { std::env::set_var } only
            //      when std::env::var(k).is_err() (not already set)
            //   4. Returns the loaded HashMap
            (T::Env, A::Load) => {
                let path = if args.is_empty() {
                    syn::parse2::<SynExpr>(quote::quote! { ".env" }).map_err(|e| {
                        self.unsupported(&format!("Env.load default path parse: {e}"))
                    })?
                } else if args.len() == 1 {
                    let p = self.lower_expr(&args[0])?;
                    coerce_str_arg_to_ref(p, &args[0])
                } else {
                    return Err(self.unsupported(&format!(
                        "Env.load() expects 0 or 1 arg, got {}",
                        args.len()
                    )));
                };
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let __buff_env_path: &str = #path;
                        let mut __buff_map = std::collections::HashMap::<String, String>::new();
                        if let Ok(__buff_contents) = std::fs::read_to_string(__buff_env_path) {
                            for __buff_line in __buff_contents.lines() {
                                let __buff_line = __buff_line.trim();
                                if __buff_line.is_empty() || __buff_line.starts_with('#') {
                                    continue;
                                }
                                if let Some((__buff_key, __buff_val)) = __buff_line.split_once('=') {
                                    let __buff_k = __buff_key.trim().to_string();
                                    let __buff_v = __buff_val.trim().to_string();
                                    if !__buff_k.is_empty() && std::env::var(&__buff_k).is_err() {
                                        unsafe { std::env::set_var(&__buff_k, &__buff_v); }
                                    }
                                    __buff_map.insert(__buff_k, __buff_v);
                                }
                            }
                        }
                        __buff_map
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Env.load codegen parse: {e}")))
            }
            // T124h: Base64 module - 2 assoc fns wrapping the `base64`
            // Rust crate (STANDARD engine via the `Engine` trait).
            //
            // `Base64.encode(bytes)` -> String. Wraps
            // `base64::Engine::encode(&base64::engine::general_purpose::STANDARD,
            // bytes)` (UFCS form so the `Engine` trait need not be in
            // scope at the call site - generated code requires NO `use
            // base64::Engine as _;` import). The arg is the byte source
            // (Vector<Byte> / &[u8] / anything `AsRef<[u8]>`).
            (T::Base64, A::Encode) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        #arg,
                    )
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Base64.encode codegen parse: {e}")))
            }
            // `Base64.decode(s)` -> Vector<Byte>. Wraps
            // `base64::Engine::decode(&general_purpose::STANDARD, s)
            // .unwrap_or_default()` (empty Vec on decode failure - NEVER
            // panics, matching Buff's "no panicking generated code" rule).
            // UFCS form so the `Engine` trait need not be in scope.
            //
            // The arg is borrowed via `&` so Rust's Deref coercion turns
            // `&String` into `&[u8]` (via `String`'s `AsRef<[u8]>` impl)
            // - the type `Engine::decode`'s generic bound accepts.
            (T::Base64, A::Decode) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        #arg_ref,
                    ).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Base64.decode codegen parse: {e}")))
            }
            // T124h: Hex module - 2 assoc fns wrapping the `hex` Rust
            // crate (free functions, no trait import needed).
            //
            // `Hex.encode(bytes)` -> String. Wraps `hex::encode(bytes)`.
            // The arg is the byte source (`AsRef<[u8]>`).
            (T::Hex, A::Encode) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    hex::encode(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Hex.encode codegen parse: {e}")))
            }
            // `Hex.decode(s)` -> Vector<Byte>. Wraps
            // `hex::decode(s).unwrap_or_default()` (empty Vec on decode
            // failure - NEVER panics). The arg is borrowed via `&` so
            // Rust's Deref coercion turns `&String` into `&str` (the
            // type `hex::decode` accepts via `AsRef<[u8]>` on `&str`).
            (T::Hex, A::Decode) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    hex::decode(#arg_ref).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Hex.decode codegen parse: {e}")))
            }
            // T124h: URLEncode module - 2 assoc fns wrapping the
            // `percent-encoding` Rust crate.
            //
            // `URLEncode.encode(s)` -> String. Wraps
            // `percent_encoding::utf8_percent_encode(s,
            // percent_encoding::NON_ALPHANUMERIC).to_string()`. The
            // `NON_ALPHANUMERIC` AsciiSet encodes everything that's not
            // an ASCII letter or digit (the canonical "encode special
            // characters" choice for safe URL embedding).
            //
            // The arg is borrowed via `&` so Rust's Deref coercion turns
            // `&String` into `&str` (the type `utf8_percent_encode`
            // takes).
            (T::URLEncode, A::Encode) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    percent_encoding::utf8_percent_encode(
                        #arg_ref,
                        percent_encoding::NON_ALPHANUMERIC,
                    ).to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("URLEncode.encode codegen parse: {e}")))
            }
            // `URLEncode.decode(s)` -> String. Wraps
            // `percent_encoding::percent_decode_str(s).decode_utf8_lossy()
            // .into_owned()`. Invalid UTF-8 sequences become U+FFFD
            // REPLACEMENT CHARACTER (lossy decode - NEVER panics).
            //
            // The arg is borrowed via `&` so Rust's Deref coercion turns
            // `&String` into `&str` (the type `percent_decode_str`
            // takes).
            (T::URLEncode, A::Decode) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    percent_encoding::percent_decode_str(#arg_ref)
                        .decode_utf8_lossy()
                        .into_owned()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("URLEncode.decode codegen parse: {e}")))
            }
            // T124h: UUID module - 3 assoc fns wrapping the `uuid` Rust
            // crate. Surface return types are String / String / Bool
            // (NOT a `Uuid` value type) - Buff surfaces UUIDs as their
            // canonical hyphen-separated String form.
            //
            // `UUID.v4()` -> String. Wraps
            // `uuid::Uuid::new_v4().to_string()` (requires the `v4`
            // feature on the `uuid` crate, configured at the workspace
            // level).
            (T::UUID, A::V4) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    uuid::Uuid::new_v4().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("UUID.v4 codegen parse: {e}")))
            }
            // `UUID.v7()` -> String. Wraps
            // `uuid::Uuid::now_v7().to_string()` (requires the `v7`
            // feature on the `uuid` crate). Distinct from v4 in
            // generation algorithm (v7 is timestamp-prefixed for sort
            // stability) but identical surface type.
            (T::UUID, A::V7) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    uuid::Uuid::now_v7().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("UUID.v7 codegen parse: {e}")))
            }
            // `UUID.parse(s)` -> Bool. Wraps
            // `uuid::Uuid::parse_str(s).is_ok()` (validation only).
            // Reuses the shared `Parse` variant (5th overload for Parse,
            // after DateTime / Date / Toml / URL). The arg is borrowed
            // via `&` so Rust's Deref coercion turns `&String` into
            // `&str` (the type `Uuid::parse_str` takes).
            (T::UUID, A::Parse) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    uuid::Uuid::parse_str(#arg_ref).is_ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("UUID.parse codegen parse: {e}")))
            }
            // T124h: URL module - 1 assoc fn (parse) wrapping the `url`
            // Rust crate. URL is a runtime-value type (NOT namespace-
            // only like Base64/Hex/URLEncode/UUID) - `URL.parse(s)`
            // returns a `URL` value carrying the four instance
            // accessors `.scheme` / `.host` / `.path` / `.query(key)`.
            //
            // `URL.parse(s)` -> URL. Wraps
            // `url::Url::parse(s).unwrap_or_else(|_| url::Url::parse("about:blank").unwrap())`.
            // The `about:blank` fallback is always parseable (it's a
            // reserved URL scheme per RFC 3986), so the inner `.unwrap()`
            // is infallible at runtime (matches Regex.compile's `r"a^"`
            // fallback stance from T124d). NEVER panics on malformed
            // input - falls back to a benign placeholder URL.
            //
            // The arg is borrowed via `&` so Rust's Deref coercion turns
            // `&String` into `&str` (the type `Url::parse` takes).
            (T::URL, A::Parse) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    url::Url::parse(#arg_ref)
                        .unwrap_or_else(|_| url::Url::parse("about:blank").unwrap())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("URL.parse codegen parse: {e}")))
            }
            // T124i: Yaml.parse(s) -> Map<String, Unknown>. Mirrors the
            // Toml.parse codegen arm (T124e) line-for-line, swapping
            // `toml::` paths for `serde_yml::` paths. The maintained
            // fork `serde_yml` is API-compatible with the deprecated
            // `serde_yaml` (`from_str::<T>(s) -> Result<T, Error>` and
            // `to_string(&v) -> Result<String, Error>`).
            //
            // The turbofish pins the concrete return type
            // `HashMap<String, serde_yml::Value>` so the generated Rust
            // is fully typed without requiring a let-binding annotation.
            // Buff's inferred surface return type is the looser
            // `Map<String, Unknown>` (the Unknown value type reflects
            // YAML's heterogeneous value space, mirroring the
            // Toml.parse Unknown-value stance from T124e).
            //
            // The turbofish path CANNOT be built via `rust_call_expr`
            // (which splits on `::` and creates Idents - the
            // `<std::collections::...>` turbofish arg is not a path
            // segment). Built via `quote!` exactly like Toml.parse.
            //
            // String literals lower to `&'static str` already; non-
            // literal String args get an `&` via `coerce_str_arg_to_ref`
            // so Rust's Deref coercion turns `&String` into `&str`
            // (the type `serde_yml::from_str` requires).
            //
            // `.unwrap_or_default()` (HashMap impls Default) is the
            // panic-free fallback: a parse failure yields an empty
            // Map, NEVER a panic (mirrors Toml.parse / Regex.compile).
            (T::Yaml, A::Parse) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    serde_yml::from_str::<std::collections::HashMap<String, serde_yml::Value>>(#arg)
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Yaml.parse codegen parse: {e}")))
            }
            // T124i: Yaml.stringify(v) -> String. Mirrors the
            // Toml.stringify codegen arm (T124e) line-for-line,
            // swapping `toml::to_string` for `serde_yml::to_string`.
            // Both APIs are structurally identical: take `&impl
            // Serialize`, return `Result<String, _>`. The arg is
            // borrowed via `&v` so Rust's serde-Serialize bound is
            // satisfied for any Map<String, ?> / Serialize-implementing
            // value.
            //
            // `.unwrap_or_default()` (String impls Default) is the
            // panic-free fallback: a serialization failure yields the
            // empty String, NEVER a panic.
            //
            // Built via `quote!` directly so the `& #arg` borrow is a
            // real syn::ExprRef (not a path-segment hack).
            (T::Yaml, A::Stringify) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    serde_yml::to_string(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Yaml.stringify codegen parse: {e}")))
            }
            // T23: Json.parse(s) -> Map<String, Unknown>. Mirrors the
            // Yaml.parse / Toml.parse codegen arms exactly, swapping
            // `serde_yml::from_str` / `toml::from_str` for
            // `serde_json::from_str`. The turbofish pins the concrete
            // `HashMap<String, serde_json::Value>` so the generated
            // Rust is fully typed. `.unwrap_or_default()` is the
            // panic-free fallback (empty Map on parse failure).
            (T::Json, A::Parse) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(#arg)
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Json.parse codegen parse: {e}")))
            }
            // T23: Json.stringify(v) -> String. Mirrors the
            // Yaml.stringify / Toml.stringify codegen arms exactly,
            // swapping `serde_yml::to_string` / `toml::to_string` for
            // `serde_json::to_string`. The arg is borrowed via `&v`
            // so Rust's serde-Serialize bound is satisfied.
            // `.unwrap_or_default()` is the panic-free fallback
            // (empty String on serialization failure).
            (T::Json, A::Stringify) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    serde_json::to_string(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Json.stringify codegen parse: {e}")))
            }
            // T124i: Csv.parse(s) -> Vector<Vector<String>>. Differs
            // from Yaml/Toml in surface type (uniform rows of Strings
            // vs heterogeneous Map) so the codegen is bespoke rather
            // than a 1:1 mirror. Wraps the `csv` crate's
            // `ReaderBuilder` + `records()` iterator.
            //
            // The generated block expression:
            //   {
            //     let mut __rdr = csv::ReaderBuilder::new()
            //         .has_headers(false)
            //         .from_reader(s.as_bytes());
            //     __rdr.records()
            //         .filter_map(|r| r.ok())
            //         .map(|r| r.iter().map(|f| f.to_string()).collect::<Vec<String>>())
            //         .collect::<Vec<Vec<String>>>()
            //   }
            //
            // Key design choices (mirror the Yaml/Toml panic-free
            // stance from T124e):
            // - `.has_headers(false)`: per spec, Csv.parse surfaces
            //   EVERY row uniformly (including the header row). CSV
            //   has no inherent type information - the header is just
            //   the first row of Strings. Disabling header handling
            //   means the reader doesn't consume the first row.
            // - `.filter_map(|r| r.ok())`: malformed rows are SKIPPED
            //   (not surfaced as panics or errors). Matches the
            //   "no panicking generated code" Buff rule. A CSV with
            //   a malformed row yields the well-formed rows before
            //   the bad one.
            // - `.map(|r| r.iter().map(|f| f.to_string()).collect::<
            //   Vec<String>>())`: each `csv::StringRecord` becomes a
            //   `Vec<String>` (Buff surfaces every cell as text -
            //   there is no CSV typing).
            // - The final `.collect::<Vec<Vec<String>>>()` pins the
            //   turbofish to Buff's `Vector<Vector<String>>` surface
            //   type so the generated Rust is fully typed.
            // - The whole block is wrapped in `{ ... }` so it
            //   evaluates to the collected Vec (a single syn::Expr::Block).
            // - The `__rdr` local binding name is `__`-prefixed to
            //   avoid colliding with user vars (mirrors the
            //   `__recv` / `__v` codegen-temporary convention from
            //   T124f Random.shuffle / splice_receiver_into_call).
            //
            // The arg is borrowed via `&` for `as_bytes()` so
            // non-literal String args get a `&String.as_bytes()` (via
            // Deref coercion) rather than `String.as_bytes()` (which
            // would be a no-op borrow on owned). String literals lower
            // to `&'static str` already.
            (T::Csv, A::Parse) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let mut __rdr = csv::ReaderBuilder::new()
                            .has_headers(false)
                            .from_reader(#arg.as_bytes());
                        __rdr.records()
                            .filter_map(|r| r.ok())
                            .map(|r| r.iter().map(|f| f.to_string()).collect::<Vec<String>>())
                            .collect::<Vec<Vec<String>>>()
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Csv.parse codegen parse: {e}")))
            }
            // T124i: Csv.stringify(rows) -> String. Wraps the `csv`
            // crate's `Writer` over an in-memory `Vec<u8>` buffer.
            // The generated block expression:
            //   {
            //     let mut __wtr = csv::Writer::from_writer(Vec::<u8>::new());
            //     for __row in &rows {
            //         __wtr.write_record(__row.clone()).ok();
            //     }
            //     String::from_utf8(__wtr.into_inner().unwrap_or_default())
            //         .unwrap_or_default()
            //   }
            //
            // Key design choices (mirror the Yaml/Toml panic-free
            // stance):
            // - `csv::Writer::from_writer(Vec::<u8>::new())`: write
            //   to an in-memory buffer (no file I/O). The turbofish
            //   `Vec::<u8>::new()` pins the writer's type parameter
            //   so Rust's inference doesn't need a let-binding
            //   annotation.
            // - `for __row in &rows`: iterate by reference so the
            //   input `Vec<Vec<String>>` is NOT consumed (Buff's
            //   move-by-default model would otherwise move the arg;
            //   borrowing lets the caller keep using the rows Vec).
            // - `__wtr.write_record(__row.clone()).ok();`: write
            //   each row. The `.clone()` lifts `&Vec<String>` to
            //   `Vec<String>` (write_record takes an owned iterator
            //   item via AsRef<[u8]> - the clone is the cheapest
            //   way to satisfy the bound without bespoke iterator
            //   plumbing). `.ok()` discards the Result - a single
            //   row write failure is NOT surfaced as a panic
            //   (matches the "no panicking generated code" rule);
            //   the row is simply omitted from the output.
            // - `__wtr.into_inner().unwrap_or_default()`: extract
            //   the underlying `Vec<u8>` writer. `into_inner` is
            //   fallible (`Result<W, csv::Error>`) only if a previous
            //   write was buffered and panicked; in practice it
            //   succeeds. `.unwrap_or_default()` yields an empty
            //   Vec<u8> on the (extremely unlikely) failure path -
            //   NEVER a panic.
            // - `String::from_utf8(...).unwrap_or_default()`: lift
            //   the byte buffer to String. Invalid UTF-8 yields the
            //   empty String (lossy - NEVER panics, mirrors the
            //   URLEncode.decode lossy stance from T124h).
            // - `__wtr` / `__row` are `__`-prefixed to avoid colliding
            //   with user vars (mirrors the `__recv` / `__v` / `__rdr`
            //   codegen-temporary convention).
            (T::Csv, A::Stringify) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let mut __wtr = csv::Writer::from_writer(Vec::<u8>::new());
                        for __row in &#arg {
                            __wtr.write_record(__row.clone()).ok();
                        }
                        String::from_utf8(__wtr.into_inner().unwrap_or_default())
                            .unwrap_or_default()
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Csv.stringify codegen parse: {e}")))
            }
            // T24: File I/O assoc fns. File is namespace-only (mirrors
            // Log / Toml / Math). All four fns lower to std::fs::* with
            // `.unwrap_or_default()` so the Buff surface is always
            // infallible — NEVER panics, matching Buff's "no panicking
            // generated code" rule. NO extern crate needed (std-only,
            // mirroring Math / Strings / Args / Env).
            //
            // `File.read(path)` -> String. Wraps
            // `std::fs::read_to_string(p).unwrap_or_default()`.
            (T::File, A::Read) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::read_to_string(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("File.read codegen parse: {e}")))
            }
            // `File.write(path, content)` -> Void. Wraps
            // `std::fs::write(p, c).unwrap_or_default()`.
            (T::File, A::Write) => {
                let path_arg = self.lower_expr(&args[0])?;
                let path_arg = coerce_str_arg_to_ref(path_arg, &args[0]);
                let content_arg = self.lower_expr(&args[1])?;
                let content_arg = coerce_str_arg_to_ref(content_arg, &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::write(#path_arg, #content_arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("File.write codegen parse: {e}")))
            }
            // `File.exists(path)` -> Bool. Wraps
            // `std::path::Path::new(p).exists()`.
            (T::File, A::Exists) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::path::Path::new(#arg).exists()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("File.exists codegen parse: {e}")))
            }
            // `File.append(path, content)` -> Void. Wraps
            // `std::fs::OpenOptions::new().append(true).open(p)
            // .and_then(|mut f| std::io::Write::write_all(&mut f, c.as_bytes()))
            // .unwrap_or_default()`.
            (T::File, A::Append) => {
                let path_arg = self.lower_expr(&args[0])?;
                let path_arg = coerce_str_arg_to_ref(path_arg, &args[0]);
                let content_arg = self.lower_expr(&args[1])?;
                let content_arg = coerce_str_arg_to_ref(content_arg, &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::OpenOptions::new()
                        .append(true)
                        .open(#path_arg)
                        .and_then(|mut f| std::io::Write::write_all(&mut f, #content_arg.as_bytes()))
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("File.append codegen parse: {e}")))
            }
            // T25/T80: Http assoc fns. Http is namespace-only (mirrors
            // File / Log / Toml / Math). All four fns lower to
            // reqwest::blocking::* with `.unwrap_or_default()` so the
            // Buff surface is always infallible — NEVER panics.
            // Returns `(i64, String)` — (status_code, body_text).
            // Records `reqwest` in extern_crates.
            //
            // `Http.get(url)` -> (Int, String). Wraps
            // `reqwest::blocking::get(u).map(|r| (r.status().as_u16() as i64, r.text().unwrap_or_default()))
            // .unwrap_or_default()`.
            (T::Http, A::HttpGet) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    reqwest::blocking::get(#arg)
                        .map(|r| (r.status().as_u16() as i64, r.text().unwrap_or_default()))
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Http.get codegen parse: {e}")))
            }
            // `Http.post(url, body)` -> (Int, String). Wraps
            // `reqwest::blocking::Client::new().post(u).body(b).send()
            // .map(|r| (r.status().as_u16() as i64, r.text().unwrap_or_default())).unwrap_or_default()`.
            (T::Http, A::HttpPost) => {
                let url_arg = self.lower_expr(&args[0])?;
                let url_arg = coerce_str_arg_to_ref(url_arg, &args[0]);
                let body_arg = self.lower_expr(&args[1])?;
                let body_arg = coerce_str_arg_to_ref(body_arg, &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    reqwest::blocking::Client::new()
                        .post(#url_arg)
                        .body(#body_arg.to_string())
                        .send()
                        .map(|r| (r.status().as_u16() as i64, r.text().unwrap_or_default()))
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Http.post codegen parse: {e}")))
            }
            // T80: `Http.put(url, body)` -> (Int, String). Wraps
            // `reqwest::blocking::Client::new().put(u).body(b).send()
            // .map(|r| (r.status().as_u16() as i64, r.text().unwrap_or_default())).unwrap_or_default()`.
            (T::Http, A::HttpPut) => {
                let url_arg = self.lower_expr(&args[0])?;
                let url_arg = coerce_str_arg_to_ref(url_arg, &args[0]);
                let body_arg = self.lower_expr(&args[1])?;
                let body_arg = coerce_str_arg_to_ref(body_arg, &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    reqwest::blocking::Client::new()
                        .put(#url_arg)
                        .body(#body_arg.to_string())
                        .send()
                        .map(|r| (r.status().as_u16() as i64, r.text().unwrap_or_default()))
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Http.put codegen parse: {e}")))
            }
            // T80: `Http.delete(url)` -> (Int, String). Wraps
            // `reqwest::blocking::Client::new().delete(u).send()
            // .map(|r| (r.status().as_u16() as i64, r.text().unwrap_or_default())).unwrap_or_default()`.
            (T::Http, A::HttpDelete) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    reqwest::blocking::Client::new()
                        .delete(#arg)
                        .send()
                        .map(|r| (r.status().as_u16() as i64, r.text().unwrap_or_default()))
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Http.delete codegen parse: {e}")))
            }
            // T26: Assert assoc fns. Assert is namespace-only (mirrors
            // File / Http / Log / Toml / Math). All five fns lower to
            // Rust's built-in `assert_eq!` / `assert!` macros. NO
            // extern crate needed (built-in macros).
            //
            // `Assert.equal(a, b)` -> Void. Lowers to `assert_eq!(a, b)`.
            (T::Assert, A::AssertEqual) => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    assert_eq!(#a0, #a1)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Assert.equal codegen parse: {e}")))
            }
            // `Assert.not_equal(a, b)` -> Void. Lowers to `assert_ne!(a, b)`.
            (T::Assert, A::AssertNotEqual) => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    assert_ne!(#a0, #a1)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Assert.not_equal codegen parse: {e}")))
            }
            // `Assert.true_(cond)` -> Void. Lowers to `assert!(cond)`.
            (T::Assert, A::AssertTrue) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    assert!(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Assert.true_ codegen parse: {e}")))
            }
            // `Assert.false_(cond)` -> Void. Lowers to `assert!(!cond)`.
            (T::Assert, A::AssertFalse) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    assert!(!#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Assert.false_ codegen parse: {e}")))
            }
            // `Assert.contains(haystack, needle)` -> Void. Lowers to
            // `assert!(haystack.contains(needle))`.
            (T::Assert, A::AssertContains) => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    assert!(#a0.contains(#a1))
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Assert.contains codegen parse: {e}")))
            }
            // T124j: Path module - 1 assoc fn (join) wrapping
            // `std::path::PathBuf` (std-only - NO extern crate
            // needed). Path is a runtime-value type (NOT namespace-
            // only like Dir/Tempfile) - `Path.join(a, b, ...)`
            // returns a `Path` value carrying the four instance
            // methods `.parent()` / `.extension()` / `.basename()` /
            // `.exists()`.
            //
            // `Path.join(a, b, ...)` -> Path. Wraps a chained
            // `std::path::PathBuf::from(a).join(b).join(c)...` for
            // any number of args >= 1. A single-arg `Path.join(a)`
            // returns `PathBuf::from(a)` (the no-op join). The
            // `PathBuf::from` constructor accepts any `AsRef<Path>`
            // - String / &str / PathBuf values all satisfy the bound
            // via Rust's std blanket impls.
            //
            // The arg sequence is lowered once and chained into a
            // single expression tree via folding (the first arg
            // becomes the PathBuf root, each subsequent arg becomes
            // a `.join(...)` call wrapping the accumulator). This
            // shape is required because quote! can't splice a Vec
            // into a chain of method calls without an explicit
            // fold (the `#(#args).*` repetition would emit them as
            // a flat sequence, not a nested call chain).
            //
            // Same shared `Join` variant as Strings.join (T124f).
            (T::Path, A::Join) => {
                if args.is_empty() {
                    return Err(self
                        .unsupported("Path.join() expects at least 1 arg (the path head), got 0"));
                }
                // Lower each arg once (avoids re-evaluating side
                // effects). The first arg becomes the PathBuf root;
                // each subsequent arg becomes a `.join(...)` call.
                let lowered: Vec<SynExpr> = args
                    .iter()
                    .map(|a| self.lower_expr(a))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut iter = lowered.into_iter();
                let head = iter
                    .next()
                    .ok_or_else(|| self.unsupported("Path.join requires at least one argument"))?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::path::PathBuf::from(#head)
                };
                let mut acc = syn::parse2::<SynExpr>(tokens)
                    .map_err(|e| self.unsupported(&format!("Path.join head codegen parse: {e}")))?;
                for next in iter {
                    acc = method_call_one_arg(acc, "join", next);
                }
                Ok(acc)
            }
            // T124j: Dir module - 4 assoc fns (list/create/remove/walk)
            // wrapping `std::fs::*` (std-only - NO extern crate needed
            // for list/create/remove) and the `walkdir` Rust crate (for
            // walk - the walkdir crate is recorded in `extern_crates`).
            // Dir is namespace-only (mirrors Log/Toml/Yaml/Csv) - every
            // call returns a value, NEVER a Dir value type.
            //
            // `Dir.list(path)` -> Vector<String>. Wraps
            // `std::fs::read_dir(p).filter_map(|e| e.ok()).map(|e|
            // e.file_name().to_string_lossy().into_owned())
            // .collect::<Vec<String>>()`. Skips inaccessible entries
            // via `.filter_map(|e| e.ok())` - NEVER panics (mirrors
            // the Csv.parse panic-free stance from T124i). Returns
            // entry NAMES (NOT paths) - the surface mirrors shell
            // `ls` / Python `os.listdir`.
            //
            // Same shared `List` variant as Args.list (T124g).
            (T::Dir, A::List) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::read_dir(#arg)
                        .map(|entries| {
                            entries
                                .filter_map(|e| e.ok())
                                .map(|e| e.file_name().to_string_lossy().into_owned())
                                .collect::<Vec<String>>()
                        })
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Dir.list codegen parse: {e}")))
            }
            // `Dir.create(path)` -> Void. Wraps
            // `std::fs::create_dir_all(p).ok()` (creates the
            // directory + any missing parents - mirrors `mkdir -p`;
            // discards errors via `.ok()` - NEVER panics). Same
            // shared `Create` variant as Tempfile.create.
            (T::Dir, A::Create) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::create_dir_all(#arg).ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Dir.create codegen parse: {e}")))
            }
            // `Dir.remove(path)` -> Void. Wraps
            // `std::fs::remove_dir_all(p).ok()` (removes the
            // directory tree recursively; discards errors via
            // `.ok()` - NEVER panics, mirroring the Dir.create
            // stance).
            (T::Dir, A::Remove) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::remove_dir_all(#arg).ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Dir.remove codegen parse: {e}")))
            }
            // `Dir.walk(path)` -> Vector<Path>. Wraps
            // `walkdir::WalkDir::new(p).into_iter().filter_map(|e|
            // e.ok()).map(|e| e.path().to_path_buf())
            // .collect::<Vec<std::path::PathBuf>>()`. Skips
            // inaccessible entries via `.filter_map(|e| e.ok())` -
            // NEVER panics (mirrors the Csv.parse panic-free stance
            // from T124i). The walkdir crate is recorded in
            // `extern_crates` when a Buff program uses Dir.walk.
            (T::Dir, A::Walk) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    walkdir::WalkDir::new(#arg)
                        .into_iter()
                        .filter_map(|e| e.ok())
                        .map(|e| e.path().to_path_buf())
                        .collect::<Vec<std::path::PathBuf>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Dir.walk codegen parse: {e}")))
            }
            // T124j: Tempfile module - 2 assoc fns (create/dir)
            // wrapping the `tempfile` Rust crate + `std::env::temp_dir`.
            // Tempfile is namespace-only (mirrors Log/Toml/Yaml/Csv/
            // Dir). The `tempfile` crate is recorded in `extern_crates`
            // when a Buff program uses Tempfile.create/dir.
            //
            // `Tempfile.create()` -> Path. Wraps
            // `tempfile::NamedTempFile::new().map(|f|
            // f.into_temp_path().keep().unwrap_or_default())
            // .unwrap_or_default()`. The `into_temp_path().keep()`
            // chain persists the temp file's path beyond the
            // NamedTempFile's drop (the file becomes a regular file
            // the user can write/read/delete like any other). Both
            // inner `.unwrap_or_default()` calls handle the
            // potential PathPersistError / io::Error paths -
            // panic-free (empty PathBuf on failure - NEVER panics,
            // mirrors the Regex.compile / URL.parse infallible-ctor
            // stance from T124d/T124h).
            //
            // Same shared `Create` variant as Dir.create.
            (T::Tempfile, A::Create) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "Tempfile.create() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    tempfile::NamedTempFile::new()
                        .map(|f| f.into_temp_path().keep().unwrap_or_default())
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Tempfile.create codegen parse: {e}")))
            }
            // `Tempfile.dir()` -> Path. Wraps `std::env::temp_dir()`
            // (the `tempfile::env::temp_dir()` is a re-export of the
            // std fn; we splice the std path directly so NO extern
            // crate is needed for this call alone - but the narrow
            // walker records `tempfile` for symmetry with
            // Tempfile.create).
            (T::Tempfile, A::Dir) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "Tempfile.dir() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::temp_dir()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Tempfile.dir codegen parse: {e}")))
            }
            // T124k: Hash module - 3 assoc fns wrapping the `sha2`
            // (SHA-256 / SHA-512) + `md5` RustCrypto crates. Hash is
            // namespace-only (mirrors Log/Toml/Base64/Hex/Yaml/Csv/
            // Dir/Tempfile). The `sha2` / `md5` crates are recorded
            // in `extern_crates` when a Buff program uses Hash.*
            // (the narrow walkers flag the specific method names -
            // sha256/sha512 -> sha2, md5 -> md5); the `hex` crate is
            // recorded alongside (shared with T124h Hex module's
            // walker).
            //
            // `Hash.sha256(data)` -> String. Wraps
            // `{ use sha2::Digest; hex::encode(sha2::Sha256::digest
            // (<d>.as_bytes())) }`. The block-scoped `use` brings
            // the `Digest` trait's `digest` method into scope WITHOUT
            // polluting the caller's namespace (`digest` is a trait
            // method, NOT an inherent method on `Sha256` - so the
            // `use` is required for the path-syntax call
            // `sha2::Sha256::digest(...)` to resolve).
            //
            // The arg accepts String OR Vector<Byte> (anything
            // `AsRef<[u8]>` at the codegen layer); `.as_bytes()`
            // gives `&[u8]` for both String (str::as_bytes) and
            // Vec<u8> (slice::as_bytes via [u8] identity). The
            // returned `GenericArray<u8, U32>` is accepted by
            // `hex::encode` via its `AsRef<[u8]>` bound.
            //
            // Sanity check: `Hash.sha256("hello")` =
            // `2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824`.
            (T::Hash, A::Sha256) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use sha2::Digest;
                        hex::encode(sha2::Sha256::digest(#arg.as_bytes()))
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Hash.sha256 codegen parse: {e}")))
            }
            // `Hash.sha512(data)` -> String. Same shape as sha256
            // but `Sha512`. Returns the 128-char lowercase hex
            // String. Block-scoped `use sha2::Digest;` for the trait
            // method (same rationale as sha256).
            (T::Hash, A::Sha512) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use sha2::Digest;
                        hex::encode(sha2::Sha512::digest(#arg.as_bytes()))
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Hash.sha512 codegen parse: {e}")))
            }
            // `Hash.md5(data)` -> String. Wraps
            // `hex::encode(md5::compute(<d>.as_bytes()).0)` (the
            // `.0` accesses the inner `[u8; 16]` of the
            // `md5::Digest` tuple struct; `hex::encode` accepts it
            // via `AsRef<[u8]>` on `[u8; N]` arrays). NO `use`
            // needed - `md5::compute` is a free function (not a
            // trait method) and `.0` is a public field access (not
            // a trait method either). Returns the 32-char lowercase
            // hex String. **MD5 is CRYPTOGRAPHICALLY BROKEN** -
            // exposed for checksum compatibility only; NEVER use
            // for security.
            (T::Hash, A::Md5) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    hex::encode(md5::compute(#arg.as_bytes()).0)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Hash.md5 codegen parse: {e}")))
            }
            // T124k: HMAC module - 1 assoc fn wrapping the `hmac` +
            // `sha2` RustCrypto crates. HMAC is namespace-only
            // (mirrors Hash / Log / Toml / Base64 / Hex / ...). The
            // `hmac` + `sha2` crates are recorded in `extern_crates`
            // when a Buff program uses `HMAC.sha256` (the
            // `hmac::Hmac<sha2::Sha256>` path needs BOTH); the
            // `hex` crate is recorded alongside.
            //
            // `HMAC.sha256(key, data)` -> String. Wraps
            // `{ use hmac::Mac; hmac::Hmac::<sha2::Sha256>
            // ::new_from_slice(<k>.as_bytes()).map(|mut mac| {
            // mac.update(<d>.as_bytes()); hex::encode(mac.finalize()
            // .into_bytes()) }).unwrap_or_default() }`. Block-scoped
            // `use hmac::Mac;` brings the `Mac` trait's `update` /
            // `finalize` methods into scope (they're trait methods,
            // NOT inherent on `Hmac`).
            //
            // `new_from_slice` returns `Result<Hmac<Sha256>,
            // MacError>` and accepts ANY key length (HMAC has no
            // fixed key size); the `.map(...).unwrap_or_default()`
            // collapses the Err branch to an empty String - NEVER
            // panics, matching Buff's "no panicking generated code"
            // rule (mirrors Base64.decode / Hex.decode / Csv.parse's
            // panic-free stance).
            //
            // Both args accept String OR Vector<Byte> (anything
            // `AsRef<[u8]>`); `.as_bytes()` gives `&[u8]` for both.
            // The `mac.finalize().into_bytes()` returns a
            // `GenericArray<u8, U32>` (32 bytes for SHA-256) that
            // `hex::encode` accepts via `AsRef<[u8]>`.
            //
            // Same shared `Sha256` variant as `Hash.sha256`;
            // dispatched on the (HMAC, Sha256) pair.
            (T::HMAC, A::Sha256) => {
                let mut lowered = n_args(self, 2)?;
                let key = lowered.remove(0);
                let data = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use hmac::Mac;
                        hmac::Hmac::<sha2::Sha256>::new_from_slice(#key.as_bytes())
                            .map(|mut mac| {
                                mac.update(#data.as_bytes());
                                hex::encode(mac.finalize().into_bytes())
                            })
                            .unwrap_or_default()
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("HMAC.sha256 codegen parse: {e}")))
            }
            // T124l: Process module - 2 assoc fns wrapping
            // `std::process::Command` (spawn) + `std::process::exit`
            // (the side-effecting terminal call). Process is a
            // runtime-value type (mirrors Regex T124d / URL T124h /
            // Path T124j). `Process.*` uses ONLY `std::process` - NO
            // extern crate recorded (mirrors Path/Dir.list stance
            // from T124j).
            //
            // `Process.spawn(command, args)` -> Process. Wraps
            // `std::process::Command::new(<cmd>).args(<args>).spawn()
            // .ok()` (the `.ok()` collapses a spawn failure to None
            // - NEVER panics, matching Buff's "no panicking generated
            // code" rule). The command + args are passed SEPARATELY
            // (NOT through a shell) so there's NO shell-injection
            // vector (the spec's safety stance). The returned value
            // is `Option<std::process::Child>` - the codegen
            // instance-method lowerings (.wait / .id) chain `.map()
            // .unwrap_or_default()` through the Option so spawn-
            // failure is observable as default Int (0) without
            // panicking.
            //
            // The generated Rust type is `Option<std::process::
            // Child>`, surfaced in Buff as `Process` (see the
            // `buff_type_to_syn` arm + the [`Type::Process`] doc).
            (T::Process, A::Spawn) => {
                let mut lowered = n_args(self, 2)?;
                let cmd = lowered.remove(0);
                let args_expr = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::process::Command::new(#cmd)
                        .args(#args_expr)
                        .spawn()
                        .ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Process.spawn codegen parse: {e}")))
            }
            // `Process.exit(code)` -> Void. Wraps
            // `std::process::exit(<code> as i32)`. The call NEVER
            // returns (it terminates the program immediately). NOTE:
            // Rust's `std::process::exit` does NOT run destructors;
            // the Buff surface inherits that behavior (the spec
            // calls this out as the "exit yourself" primitive,
            // distinct from signal-based shutdown which is explicitly
            // out-of-scope). The `as i32` cast narrows Buff's
            // default `Int<64>` to the OS's `i32` exit-code width.
            (T::Process, A::Exit) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::process::exit(#arg as i32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Process.exit codegen parse: {e}")))
            }
            // T124l: OS module - 4 assoc fns wrapping
            // `std::env::consts::{OS,ARCH}` + env-var hostname +
            // `num_cpus::get`. OS is namespace-only (mirrors Log /
            // Toml / Math / Strings / Args / Env / Hash / HMAC).
            //
            // `OS.name()` -> String. Wraps
            // `std::env::consts::OS.to_string()` (compile-time
            // const - one of `linux` / `macos` / `windows` /
            // `freebsd` / ...). Zero args.
            (T::OS, A::Name) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::consts::OS.to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("OS.name codegen parse: {e}")))
            }
            // `OS.arch()` -> String. Wraps
            // `std::env::consts::ARCH.to_string()` (compile-time
            // const - one of `x86_64` / `aarch64` / `x86` / ...).
            // Zero args.
            (T::OS, A::Arch) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::consts::ARCH.to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("OS.arch codegen parse: {e}")))
            }
            // `OS.hostname()` -> String. Wraps
            // `std::env::var("COMPUTERNAME").or_else(|_|
            // std::env::var("HOSTNAME")).unwrap_or_default()` (empty
            // String when neither env var is set - NEVER panics).
            // The bare-minimum env-var approach covering Windows
            // (COMPUTERNAME) + Unix (HOSTNAME). NO `hostname` crate
            // added (the spec explicitly forbids it - the codegen-
            // only linking boundary limit is observed: this call
            // alone needs NO extern crate). Zero args.
            (T::OS, A::Hostname) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::var("COMPUTERNAME")
                        .or_else(|_| std::env::var("HOSTNAME"))
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("OS.hostname codegen parse: {e}")))
            }
            // `OS.cpus()` -> Int. Wraps `num_cpus::get() as i64`.
            // The `num_cpus` crate is recorded in codegen
            // `extern_crates` when a Buff program uses `OS.cpus`
            // (the narrow `program_uses_num_cpus` walker flags ONLY
            // the `cpus` method name - `name` / `arch` / `hostname`
            // use std only and record NO extern crate). Zero args.
            (T::OS, A::Cpus) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    num_cpus::get() as i64
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("OS.cpus codegen parse: {e}")))
            }
            // T124m: TCP / UDP / WebSocket networking assoc fns.
            // Each wraps an async tokio / tokio-tungstenite connect
            // / bind call via `.await.ok()` (panic-free - a connect
            // or bind failure collapses to `None`). The returned
            // value (Connection / Socket / WsConnection) is the
            // receiver for the corresponding instance methods.
            //
            // Same codegen-only-linking-boundary stance as the
            // other tokio / tokio-tungstenite lowerings: single-
            // file `buff run` rustc path does NOT link tokio (the
            // `.await` calls surface a rustc-level error if the
            // enclosing function is not async - the T31 walker
            // propagates async-ness ONLY through bare-Ident free-
            // function calls, NOT method-call / namespace-assoc-fn
            // calls, so the enclosing-fn-async transformation is a
            // deferral; see issues.md).
            //
            // `TCP.connect(host, port) -> Connection`. Wraps
            // `tokio::net::TcpStream::connect(format!("{}:{}",
            // h, p)).await.ok()` (two args: String host, Int port;
            // the format! builds the `"host:port"` SocketAddr
            // string tokio's connect accepts). The `.ok()`
            // collapses a connect failure to `None` - NEVER panics.
            // The `tokio` crate is recorded in codegen
            // `extern_crates` (idempotent with the existing tokio
            // walker from T124g - any sleep() call OR TCP.* /
            // UDP.* call flags `tokio`).
            (T::TCP, A::Connect) => {
                let mut lowered = n_args(self, 2)?;
                let host = lowered.remove(0);
                let port = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    tokio::net::TcpStream::connect(format!("{}:{}", #host, #port))
                        .await
                        .ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("TCP.connect codegen parse: {e}")))
            }
            // `UDP.bind(host, port) -> Socket`. Wraps
            // `tokio::net::UdpSocket::bind(format!("{}:{}", h,
            // p)).await.ok()` (two args: String host, Int port).
            // The `.ok()` collapses a bind failure to `None` -
            // NEVER panics.
            (T::UDP, A::Bind) => {
                let mut lowered = n_args(self, 2)?;
                let host = lowered.remove(0);
                let port = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    tokio::net::UdpSocket::bind(format!("{}:{}", #host, #port))
                        .await
                        .ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("UDP.bind codegen parse: {e}")))
            }
            // `WebSocket.connect(url) -> WsConnection`. Wraps
            // `tokio_tungstenite::connect_async(url).await.ok()
            // .map(|(ws, _)| ws)` (one arg: String url). The
            // `.ok().map(...)` chain collapses a connect failure to
            // `None` and unwraps the `(WebSocketStream, Response)`
            // tuple tokio-tungstenite's connect_async returns -
            // NEVER panics. The `tokio-tungstenite` + `futures-
            // util` crates are recorded in codegen `extern_crates`
            // via the narrow `program_uses_tokio_tungstenite`
            // walker. Same shared `Connect` variant as
            // `TCP.connect(host, port)`; dispatched on the
            // (WebSocket, Connect) pair (mirrors `Parse` shared
            // between DateTime / Date / Toml / URL / UUID).
            (T::WebSocket, A::Connect) => {
                let url = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    tokio_tungstenite::connect_async(#url)
                        .await
                        .ok()
                        .map(|(ws, _)| ws)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("WebSocket.connect codegen parse: {e}")))
            }
            // T2: Channel.new(buf_size) -> (Sender<T>, Receiver<T>).
            // Wraps `buff_lang_runtime::Channel::new(buf_size)` which
            // internally calls `tokio::sync::mpsc::channel(buf_size)`
            // and returns the `(Sender<T>, Receiver<T>)` tuple directly.
            // The runtime hides tokio behind the abstraction per Metis G6.
            // NO turbofish at the call site - Rust's type inference
            // derives T from subsequent `sender.send(value)` /
            // `receiver.recv()` usage (the user never writes the
            // turbofish in Buff source; Buff does not expose explicit
            // generic-type-argument syntax on method calls).
            //
            // One arg (Int buf_size). The codegen does NOT cast the
            // arg to usize (Rust's type inference derives usize from
            // the `tokio::sync::mpsc::channel(buffer: usize)` signature;
            // an untyped Int literal in Buff lowers to `i64`, which
            // Rust's `Into<usize>` would not satisfy without a cast).
            // For the literal case the user typically writes a small
            // positive integer (`Channel.new(10)`, `Channel.new(100)`)
            // which `i64` -> `usize` infers cleanly on most hosts. If
            // a user passes a negative or huge value, Rust surfaces a
            // normal overflow diagnostic at compile time (we do NOT
            // silently coerce).
            (T::Channel, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_lang_runtime::Channel::new(#arg as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Channel.new codegen parse: {e}")))
            }
            // ITER-42: the framework-crate assoc-fn arms (DataFrame ..
            // Decimal - the families backed by the buff-* framework
            // crates) live in the `framework` child module (pure move).
            // Wildcard delegation preserves the original match order:
            // no arm in this match overlaps any child pattern, and the
            // child's own trailing wildcard reproduces the original
            // unreachable-combination error verbatim. The arity closures
            // travel as one 4-tuple (clippy too_many_arguments counts
            // self, so four separate closure params would trip it).
            _ => self.lower_prelude_type_assoc_fn_framework(
                ptype,
                pmethod,
                args,
                (&one_arg, &no_args, &two_args, &n_args),
            ),
        }
    }

    /// T124c: lower a prelude `Log` module call (`Log.<level>(msg, ...)`) to
    /// the corresponding `tracing::<level>!(...)` macro invocation.
    ///
    /// Call shape (mirrors `tracing`'s macro surface):
    ///
    /// ```text
    /// Log.info("msg")                       -> tracing::info!("msg")
    /// Log.info("msg", k1: v1, k2: v2)       -> tracing::info!(k1 = v1, k2 = v2, "msg")
    /// ```
    ///
    /// The first positional arg is the **message** (typically a string
    /// literal, but any `Display`-able expression works at the Rust level).
    /// All SUBSEQUENT args MUST be `Expr::NamedArg` (`key: value`) — they
    /// become the tracing macro's structured fields. Mixed positional-after-
    /// named args are rejected (tracing's macro syntax requires the message
    /// literal LAST, after all field assignments).
    ///
    /// # Field ordering (determinism)
    ///
    /// Fields are emitted in **source order** (the order the user wrote
    /// them). This is the simplest deterministic choice — `tracing` itself
    /// preserves insertion order in its event record, and insta snapshots
    /// prove byte-identical output across runs. The alternative (alphabetical
    /// sort) would reorder fields away from the user's intent; we keep
    /// source order.
    ///
    /// # Lowering mechanism
    ///
    /// The macro is built as a [`syn::ExprMacro`] whose `mac.path` is
    /// `tracing::<level>` and whose `mac.tokens` carries the comma-separated
    /// field assignments + the trailing message. Token construction goes
    /// through `quote!` so NO raw-string Rust is emitted — the single
    /// string producer remains `prettyplease::unparse`. The resulting
    /// `ExprMacro` re-parses cleanly because every spliced fragment is
    /// already a `syn` node (Ident, SynExpr).
    ///
    /// # Errors
    ///
    /// - Empty arg list → `unsupported` (caller must supply at least the
    ///   message).
    /// - Any arg after the first that is NOT an `Expr::NamedArg` →
    ///   `unsupported` (mixed positional/named is rejected — the message
    ///   must be the LAST positional and the only one).
    /// - An unknown `PreludeAssocFn` for `Log` (i.e. not one of
    ///   Debug/Info/Warn/Error) → `unsupported` (defensive — the registry
    ///   already rejects the combo at the lookup layer).
    fn lower_log_call(
        &mut self,
        level: buff_lang_types::PreludeAssocFn,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::PreludeAssocFn as A;
        // Resolve the tracing macro name from the level variant.
        // `PreludeAssocFn::name()` already returns the lowercase Rust
        // spelling ("debug" / "info" / "warn" / "error"), so we can
        // splice it directly into `tracing::<name>!`.
        let level_name = match level {
            A::Debug | A::Info | A::Warn | A::Error => level.name(),
            other => {
                return Err(self.unsupported(&format!(
                    "Log.{:?}() is not a recognised Log level (expected debug/info/warn/error)",
                    other
                )));
            }
        };
        if args.is_empty() {
            return Err(self.unsupported(&format!(
                "Log.{level_name}() requires at least the message argument"
            )));
        }
        // First positional arg is the message; the rest must be NamedArgs.
        let msg_expr = &args[0];
        for (i, a) in args[1..].iter().enumerate() {
            if !matches!(a, Expr::NamedArg { .. }) {
                return Err(self.unsupported(&format!(
                    "Log.{level_name}(): argument {} (after the message) must be a named field \
                     (e.g. `key: value`); positional args after the message are not allowed",
                    i + 2
                )));
            }
        }
        // Lower the message. For a string literal, this produces a
        // `SynExpr::Lit(Lit::Str(...))` that quote! splices as the bare
        // string literal token (so `tracing::info!("msg")` not
        // `tracing::info!({ "msg" })`).
        let msg = self.lower_expr(msg_expr)?;
        // Lower field name + value pairs in SOURCE ORDER (deterministic).
        // We collect into Vecs so the `#(#names = #values,)*` repetition
        // in quote! produces a comma-separated list with a trailing comma
        // after every entry (so the message is unambiguously separated).
        let mut field_names: Vec<Ident> = Vec::with_capacity(args.len().saturating_sub(1));
        let mut field_values: Vec<SynExpr> = Vec::with_capacity(field_names.len());
        for f in &args[1..] {
            if let Expr::NamedArg { name, value, .. } = f {
                field_names.push(ast_ident_to_syn(name));
                field_values.push(self.lower_expr(value)?);
            }
        }
        // Build the macro path: `tracing::info`, `tracing::debug`, ...
        let macro_path = rust_path(&format!("tracing::{level_name}"));
        // Build the macro token body. The `#(#names = #values,)*` repetition
        // emits `k1 = v1, k2 = v2,` (each entry followed by a comma), then
        // the message splices in last. tracing accepts the trailing comma
        // after the last field (Rust macro_rules $() sep behavior).
        let tokens: proc_macro2::TokenStream = if field_names.is_empty() {
            quote::quote! { #msg }
        } else {
            quote::quote! { #(#field_names = #field_values,)* #msg }
        };
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: macro_path,
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Paren(Default::default()),
                tokens,
            },
        }))
    }
}
