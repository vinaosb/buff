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

use super::*;

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
            // T7: DataFrame.from_csv(path) -> DataFrame. Wraps
            // `buff_dataframe::DataFrame::from_csv(path)
            // .unwrap_or_default()` (panic-free on file-not-found /
            // parse failure — DataFrame impls Default as the empty
            // frame, matching Buff's "no panicking generated code"
            // rule). Records `buff-dataframe` in extern_crates via
            // the narrow `program_uses_namespace("DataFrame")` walker.
            (T::DataFrame, A::FromCsv) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_dataframe::DataFrame::from_csv(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("DataFrame.from_csv codegen parse: {e}"))
                })
            }
            // T7: DataFrame.from_json(path) -> DataFrame. Same shape
            // as FromCsv — panic-free via `unwrap_or_default()`.
            // Reads JSON-lines (one JSON object per line).
            (T::DataFrame, A::FromJson) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_dataframe::DataFrame::from_json(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("DataFrame.from_json codegen parse: {e}"))
                })
            }
            // T9: Image.from_path(path) -> Image. Wraps
            // `buff_image::Image::from_path(arg).unwrap_or_default()`
            // (panic-free on file-not-found / decode failure / codec
            // panic — Image impls Default as a 1x1 transparent pixel,
            // matching Buff's "no panicking generated code" rule).
            // Records `buff-image` + `image` in extern_crates via the
            // `program_uses_namespace("Image")` walker.
            (T::Image, A::FromPath) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_image::Image::from_path(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.from_path codegen parse: {e}")))
            }
            // T9: Image.from_bytes(bytes) -> Image. Same shape as
            // FromPath — panic-free via `unwrap_or_default()`. The
            // arg is a `Vector<Byte>` on the Buff surface (Vec<u8>
            // after codegen lowering); the codegen passes it by ref
            // to `buff_image::Image::from_bytes(&bytes)`.
            (T::Image, A::FromBytes) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_image::Image::from_bytes(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.from_bytes codegen parse: {e}")))
            }
            // T37: Faker.new() -> Faker. Wraps
            // `buff_fake::Faker::new()` (default locale en-US, random
            // seed). Infallible — no unwrap_or_default needed. Records
            // `buff-fake` + `fake` in extern_crates via the
            // `program_uses_namespace("Faker")` walker.
            (T::Faker, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fake::Faker::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.new codegen parse: {e}")))
            }
            // T37: Faker.with_locale(locale) -> Faker. One arg (String
            // locale, either "en-US" or "pt-BR"). Wraps
            // `buff_fake::Faker::with_locale(locale)`.
            (T::Faker, A::WithLocale) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fake::Faker::with_locale(match #arg.as_str() {
                        "pt-BR" => buff_fake::FakerLocale::PtBr,
                        _ => buff_fake::FakerLocale::EnUs,
                    })
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.with_locale codegen parse: {e}")))
            }
            // T37: Faker.with_seed(locale, seed) -> Faker. Two args
            // (String locale, Int seed). Wraps
            // `buff_fake::Faker::with_seed(locale, seed)`.
            (T::Faker, A::WithSeed) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Faker.with_seed expects exactly 2 args (locale, seed), got {}",
                        args.len()
                    )));
                }
                let locale = self.lower_expr(&args[0])?;
                let seed = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fake::Faker::with_seed(match #locale.as_str() {
                        "pt-BR" => buff_fake::FakerLocale::PtBr,
                        _ => buff_fake::FakerLocale::EnUs,
                    }, #seed as u64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.with_seed codegen parse: {e}")))
            }
            // T10: AudioBuffer.from_path(path) -> AudioBuffer. Wraps
            // `buff_audio::AudioBuffer::from_path(arg)
            // .unwrap_or_default()` (panic-free on file-not-found /
            // decode failure / codec panic — AudioBuffer impls Default
            // as an empty 44100Hz mono buffer, matching Buff's "no
            // panicking generated code" rule). Records `buff-audio` +
            // `hound` + `symphonia` in extern_crates via the
            // `program_uses_namespace("AudioBuffer")` walker.
            (T::Audio, A::FromPath) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audio::AudioBuffer::from_path(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("AudioBuffer.from_path codegen parse: {e}"))
                })
            }
            // T10: AudioBuffer.from_samples(samples, sample_rate,
            // channels) -> AudioBuffer. Three args (Vec<f32>, u32,
            // u16). The Buff surface passes (Vector<Float>, Int,
            // Int); codegen casts Int -> u32 / u16 at the call site
            // (mirrors the i64 -> usize cast in Channel.new).
            // Panic-free via `unwrap_or_default()` (AudioBuffer
            // impls Default — invalid params collapse to empty).
            (T::Audio, A::FromSamples) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "from_samples() expects exactly 3 args (samples, sample_rate, channels), got {}",
                        args.len()
                    )));
                }
                let samples = self.lower_expr(&args[0])?;
                let sample_rate = self.lower_expr(&args[1])?;
                let channels = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audio::AudioBuffer::from_samples(#samples, #sample_rate as u32, #channels as u16)
                        .unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("AudioBuffer.from_samples codegen parse: {e}"))
                })
            }
            // T26: Audit.scan(path) -> Vector<String>. One arg (String
            // / Path). Wraps `buff_audit::scan(&arg)
            // .unwrap_or_default()` (panic-free on io / advisory-DB
            // failure - empty Vec, matching Buff's "no panicking
            // generated code" rule). Records `buff-audit` +
            // `ed25519-dalek` + `sha2` + `hex` + `rand` in
            // extern_crates via the narrow
            // `program_uses_namespace("Audit")` walker.
            (T::Audit, A::Scan) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::scan(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Audit.scan codegen parse: {e}")))
            }
            // T26: Audit.list() -> Vector<String>. Zero args. Wraps
            // `buff_audit::known_advisories()` (infallible - returns
            // the static `advisory_db::ALL` ID list). Records the
            // same extern_crates set as Audit.scan. Reuses the
            // existing T124g `List` variant (shared between Args.list
            // / Env.list / Audit.list - same shared-variant pattern
            // as Parse / Get / Encode).
            (T::Audit, A::List) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::known_advisories()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Audit.list codegen parse: {e}")))
            }
            // T26: Signature.sign(data, secret_hex) -> String. Two
            // args (Vector<Byte>, String). Wraps `buff_audit::sign
            // (&data, &secret_hex).unwrap_or_default()` (panic-free
            // on bad-key / bad-hex - empty String). The `&#data` is
            // `&Vec<u8>` which Rust auto-derefs to `&[u8]` at the
            // call site. Records the same extern_crates set as
            // Audit.* via the `program_uses_namespace("Signature")`
            // walker.
            (T::Signature, A::Sign) => {
                let mut lowered = n_args(self, 2)?;
                let data = lowered.remove(0);
                let secret_hex = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::sign(&#data, &#secret_hex).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signature.sign codegen parse: {e}")))
            }
            // T26: Signature.verify(data, sig_hex, public_hex) ->
            // Bool. Three args. Wraps `buff_audit::verify(...).unwrap_
            // or(false)` (the unwrap_or(false) is the contract: bad
            // signature, bad key, OR bad hex all collapse to false -
            // NEVER panics, NEVER errors. The T26 task spec mandates
            // the bool return so a future `buff add --no-verify`
            // bypass layers cleanly).
            (T::Signature, A::Verify) => {
                let mut lowered = n_args(self, 3)?;
                let data = lowered.remove(0);
                let sig_hex = lowered.remove(0);
                let public_hex = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::verify(&#data, &#sig_hex, &#public_hex).unwrap_or(false)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signature.verify codegen parse: {e}")))
            }
            // T26: Signature.keypair() -> (String, String). Zero
            // args. Wraps `buff_audit::keypair()
            // .unwrap_or_default()` (the unwrap_or_default collapses
            // a Panic error to two empty Strings - NEVER panics).
            // Used by `buff publish --sign` to mint a fresh signing
            // identity per package release.
            (T::Signature, A::Keypair) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::keypair().unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signature.keypair codegen parse: {e}")))
            }
            // T27: Fuzz.run(strategy, iterations, closure) -> Void.
            // Three args (Strategy, Int, closure). Wraps
            // `buff_fuzz::run(&strategy, iterations as u32, |n: i64| closure_body(n))`.
            // The lowered call returns FuzzSummary; the codegen
            // discards it via `let _ = buff_fuzz::run(...)` so the
            // Buff surface stays Void-only. Records `buff-fuzz` +
            // `proptest` in extern_crates via the narrow
            // `program_uses_namespace("Fuzz")` walker.
            //
            // The `Run` variant is Fuzz-only - no other prelude type
            // exposes a method named `run` today.
            (T::Fuzz, A::Run) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "Fuzz.run() expects exactly 3 args (strategy, iterations, closure), got {}",
                        args.len()
                    )));
                }
                let strategy = self.lower_expr(&args[0])?;
                let iterations = self.lower_expr(&args[1])?;
                let closure = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    let _ = buff_fuzz::run(&#strategy, #iterations as u32, #closure);
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Fuzz.run codegen parse: {e}")))
            }
            // T27: Strategy.int(min, max) -> Strategy. Two args (Int, Int).
            // Wraps `buff_fuzz::Strategy::int(min, max)`. The `Int`
            // variant is shared with `Random.int(min, max)` (T124f),
            // dispatched on the (Strategy, Int) pair.
            (T::Strategy, A::Int) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Strategy.int() expects exactly 2 args (min, max), got {}",
                        args.len()
                    )));
                }
                let min = self.lower_expr(&args[0])?;
                let max = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::int(#min, #max)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.int codegen parse: {e}")))
            }
            // T27: Strategy.float(min, max) -> Strategy. Two args (Float, Float).
            // Wraps `buff_fuzz::Strategy::float(min, max)`. The `Float`
            // variant is shared with `Random.float()` (T124f), dispatched
            // on the (Strategy, Float) pair.
            (T::Strategy, A::Float) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Strategy.float() expects exactly 2 args (min, max), got {}",
                        args.len()
                    )));
                }
                let min = self.lower_expr(&args[0])?;
                let max = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::float(#min as f64, #max as f64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.float codegen parse: {e}")))
            }
            // T27: Strategy.bool() -> Strategy. Zero args. Wraps
            // `buff_fuzz::Strategy::bool()`.
            (T::Strategy, A::Bool) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::bool()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.bool codegen parse: {e}")))
            }
            // T27: Strategy.string(max_len) -> Strategy. One arg (Int).
            // Wraps `buff_fuzz::Strategy::string(max_len)`.
            (T::Strategy, A::String) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::string(#arg as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.string codegen parse: {e}")))
            }
            // T27: Strategy.bytes(max_len) -> Strategy. One arg (Int).
            // Wraps `buff_fuzz::Strategy::bytes(max_len)`.
            (T::Strategy, A::Bytes) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::bytes(#arg as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.bytes codegen parse: {e}")))
            }
            // T20: ReactiveSignal.new(value) -> Signal<T>. One arg (T).
            // Wraps `buff_reactive::Signal::new(value)` (infallible).
            (T::ReactiveSignal, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_reactive::Signal::new(#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ReactiveSignal.new codegen parse: {e}"))
                })
            }
            // T20: ReactiveComputed.new(fn) -> Computed<T>. One arg
            // (closure `Fn() -> T`). Wraps `buff_reactive::Computed::new(fn)`.
            (T::ReactiveComputed, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_reactive::Computed::new(#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ReactiveComputed.new codegen parse: {e}"))
                })
            }
            // T20: ReactiveEffect.new(fn) -> Effect. One arg (closure
            // `Fn() -> Void`). Wraps `buff_reactive::Effect::new(fn)`.
            (T::ReactiveEffect, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_reactive::Effect::new(#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ReactiveEffect.new codegen parse: {e}"))
                })
            }
            // T18: Database.connect(url) -> Pool (forward-declared as
            // `Type::Unknown` in prelude_types.rs; the buff-db crate's
            // `Pool` type IS the runtime value). Wraps
            // `buff_db::Pool::connect(&url).await?` (the `?` propagates
            // `DbError` per Buff's R3 error-mapping contract — the
            // Buff user's surrounding fn must return
            // `Result<T, buff_db::DbError>` so the `?` splices
            // cleanly; the Buff `?` operator is the standard error-
            // propagation idiom, mirroring `regex::Regex::new(p)?`
            // from T124d and `buff_image::Image::from_path(p)?` from
            // T9). The `.await` is auto-inserted by the T31 async-
            // propagation pass when the surrounding fn is async
            // (Buff has no `await` keyword). Records `buff-db` +
            // `sqlx` + `tokio` in extern_crates via the narrow
            // `program_uses_namespace("Database")` walker. Same
            // shared `Connect` variant as `TCP.connect(host, port)`
            // / `WebSocket.connect(url)`; dispatched on the
            // (Database, Connect) pair (mirrors `Parse` shared
            // between DateTime / Date / Toml / URL / UUID).
            //
            // One arg (String url — e.g. `"sqlite::memory:"` or
            // `"postgres://user:pass@host/db"`). The codegen does NOT
            // cast the arg — it passes the owned `String` directly to
            // `Pool::connect(url: &str)` (Rust's deref coercion lifts
            // `String` to `&str` automatically). The returned `Pool`
            // value is the receiver for the deferred `.query()` /
            // `.execute()` / `.begin()` instance methods (a sibling
            // task adds `Type::Pool` + instance-method lowering arms).
            (T::Database, A::Connect) => {
                let url = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_db::Pool::connect(&#url).await?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Database.connect codegen parse: {e}")))
            }
            // T17: Web.new() -> Web. Zero args. Wraps
            // `buff_web::Web::new()` (infallible - returns an empty
            // Web with no routes / no middleware / no bind addr).
            // Records `buff-web` + `axum` + `tokio` + `serde_json` in
            // extern_crates via the `program_uses_namespace("Web")`
            // walker. Dispatch on (PreludeType::Web, New) - mirrors
            // the (Channel, New) precedent (Channel.new also returns
            // a runtime value via a zero-arg ctor).
            (T::Web, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web::Web::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Web.new codegen parse: {e}")))
            }
            // T17: Web.bind(addr) -> Web. One arg (String). Wraps
            // `buff_web::Web::bind(addr)` (infallible - returns an
            // empty Web with the bind addr preset; the user adds
            // routes via web.get / web.post / ... and starts serving
            // via web.run()).
            //
            // Same shared `Bind` variant as `UDP.bind(host, port)`
            // (T124m) - dispatched on (Web, Bind) pair (mirrors
            // `Parse` shared between DateTime / Date / Toml / URL /
            // UUID, `Connect` shared between TCP / WebSocket).
            (T::Web, A::Bind) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web::Web::bind(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Web.bind codegen parse: {e}")))
            }
            // T33: HttpClient.new() -> HttpClient. Zero args. Wraps
            // `buff_http_client::HttpClient::new()` (infallible -
            // returns a new client with default settings). Records
            // `buff-http-client` + `reqwest` in extern_crates via the
            // `program_uses_namespace("HttpClient")` walker. Dispatch
            // on (PreludeType::HttpClient, New) - mirrors the (Web,
            // New) / (Channel, New) precedent.
            (T::HttpClient, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_http_client::HttpClient::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("HttpClient.new codegen parse: {e}")))
            }
            // T29: Validator.new() -> Validator. Zero args. Wraps
            // `buff_validate::Validator::new()` (infallible - returns
            // an empty rule set). Records `buff-validate` +
            // `validator` + `serde_json` + `regex` in extern_crates
            // via the `program_uses_namespace("Validator")` walker.
            // Dispatch on (PreludeType::Validator, New) - mirrors the
            // (HttpClient, New) / (Channel, New) precedent.
            (T::Validator, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_validate::Validator::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.new codegen parse: {e}")))
            }
            // T42: Email.new(from, to, subject) -> Email. Three args
            // (String from, String to, String subject). Wraps
            // `buff_email::Email::new(&from, &to, &subject)?` (the
            // `?` propagates EmailError::InvalidAddress per Buff's
            // R3 error-mapping contract). Records `buff-email` +
            // `lettre` + `handlebars` in extern_crates via the
            // `program_uses_namespace("Email")` walker. Dispatch on
            // (PreludeType::Email, New) - mirrors the (Validator,
            // New) / (HttpClient, New) / (Cache, New) precedent.
            (T::Email, A::New) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "Email.new() expects exactly 3 args (from, to, subject), got {}",
                        args.len()
                    )));
                }
                let from = self.lower_expr(&args[0])?;
                let to = self.lower_expr(&args[1])?;
                let subject = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_email::Email::new(&#from, &#to, &#subject)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Email.new codegen parse: {e}")))
            }
            // T42: SmtpClient.new(host, port, username, password) ->
            // SmtpClient. Four args (String host, Int port, String
            // username, String password). Wraps
            // `buff_email::SmtpClient::new(&host, port as u16, &user,
            // &pass)?` (the `?` propagates EmailError::InvalidRelay).
            // Records `buff-email` + `lettre` in extern_crates
            // (shared walker with Email). Dispatch on
            // (PreludeType::SmtpClient, New).
            (T::SmtpClient, A::New) => {
                if args.len() != 4 {
                    return Err(self.unsupported(&format!(
                        "SmtpClient.new() expects exactly 4 args (host, port, username, password), got {}",
                        args.len()
                    )));
                }
                let host = self.lower_expr(&args[0])?;
                let port = self.lower_expr(&args[1])?;
                let username = self.lower_expr(&args[2])?;
                let password = self.lower_expr(&args[3])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_email::SmtpClient::new(&#host, #port as u16, &#username, &#password)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("SmtpClient.new codegen parse: {e}")))
            }
            // T31: Cache.new(max_capacity) -> Cache. One arg (Int).
            // Wraps `buff_cache::Cache::new(max_capacity as u64)
            // .unwrap_or_default()` (panic-free on zero-capacity —
            // Cache impls Default as a 1024-capacity empty cache,
            // matching Buff's "no panicking generated code" rule).
            // Records `buff-cache` + `moka` in extern_crates via the
            // `program_uses_namespace("Cache")` walker.
            (T::Cache, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_cache::Cache::new(#arg as u64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.new codegen parse: {e}")))
            }
            // T44: I18n.new(locale) -> I18n. One arg (String). Wraps
            // `buff_i18n::I18n::new(&locale).unwrap_or_default()`
            // (panic-free on invalid locale — I18n impls Default as
            // an empty English catalog, matching Buff's "no
            // panicking generated code" rule + the Image / Cache /
            // Document precedent). Records `buff-i18n` +
            // `fluent-bundle` + `unic-langid` in extern_crates via
            // the `program_uses_namespace("I18n")` walker.
            (T::I18n, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_i18n::I18n::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("I18n.new codegen parse: {e}")))
            }
            // T44: I18n.with_fallback(locale, fallback) -> I18n. Two
            // args (String locale, String fallback). Wraps
            // `buff_i18n::I18n::with_fallback(&locale, &fallback)
            // .unwrap_or_default()` (panic-free).
            (T::I18n, A::WithFallback) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "with_fallback() expects exactly 2 args (locale, fallback), got {}",
                        args.len()
                    )));
                }
                let locale = self.lower_expr(&args[0])?;
                let fallback = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_i18n::I18n::with_fallback(&#locale, &#fallback).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("I18n.with_fallback codegen parse: {e}"))
                })
            }
            // T43: Document.from_html(html) -> Document. One arg
            // (String). Wraps `buff_scrape::Document::from_html(&html)
            // .unwrap_or_default()` (panic-free on empty input —
            // Document impls Default as `<html></html>`, matching
            // Buff's "no panicking generated code" rule; mirrors the
            // Image.from_path `unwrap_or_default()` precedent). Records
            // `buff-scrape` + `scraper` in extern_crates via the
            // `program_uses_namespace("Document")` walker.
            (T::Document, A::FromHtml) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Document::from_html(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Document.from_html codegen parse: {e}"))
                })
            }
            // T43: Crawler.new(seed_url) -> Crawler. One arg (String).
            // Wraps `buff_scrape::Crawler::new(&seed)
            // .unwrap_or_default()` (panic-free on empty seed —
            // Crawler impls Default as an about:blank-seeded client).
            // Records `buff-scrape` + `reqwest` in extern_crates via
            // the `program_uses_namespace("Crawler")` walker.
            (T::Crawler, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Crawler::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.new codegen parse: {e}")))
            }
            // T30: Config module — namespace-only (no runtime value).
            // `Config.new()` creates a `buff_config::Config` and stores
            // it in a thread-local static so subsequent method calls
            // (`cfg.set_default`, `cfg.load_file`, etc.) operate on the
            // same instance. The codegen emits a lazy-static pattern
            // (one Config per thread — no Mutex contention for the
            // common single-threaded case). Mirrors the Log / Toml /
            // Math namespace-only pattern.
            (T::Config, A::New) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {{
                    static CONFIG: std::sync::LazyLock<buff_config::Config> =
                        std::sync::LazyLock::new(buff_config::Config::new);
                    &*CONFIG
                }};
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.new codegen parse: {e}")))
            }
            // `Config.set_default(key, val)` -> Void. Two args.
            (T::Config, A::SetDefault) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "set_default() expects exactly 2 args (key, value), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let val = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.set_default(#key, #val)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Config.set_default codegen parse: {e}"))
                })
            }
            // `Config.load_file(path)` -> Void. One arg.
            (T::Config, A::LoadFile) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.load_file(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.load_file codegen parse: {e}")))
            }
            // `Config.load_env(prefix)` -> Void. One arg.
            (T::Config, A::LoadEnv) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.load_env(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.load_env codegen parse: {e}")))
            }
            // `Config.load_args(args)` -> Void. One arg (Vector<String>).
            (T::Config, A::LoadArgs) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.load_args(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.load_args codegen parse: {e}")))
            }
            // `Config.get(key)` -> Option<String>. One arg. Reuses the
            // shared `Get` variant (also used by Args.get / Env.get).
            (T::Config, A::Get) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get codegen parse: {e}")))
            }
            // `Config.get_int(key)` -> Option<Int>. One arg.
            (T::Config, A::GetInt) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get_int(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get_int codegen parse: {e}")))
            }
            // `Config.get_float(key)` -> Option<Float>. One arg.
            (T::Config, A::GetFloat) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get_float(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get_float codegen parse: {e}")))
            }
            // `Config.get_bool(key)` -> Option<Bool>. One arg.
            (T::Config, A::GetBool) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get_bool(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get_bool codegen parse: {e}")))
            }
            // `Config.watch(path, callback)` -> Void. Two args.
            (T::Config, A::Watch) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "watch() expects exactly 2 args (path, callback), got {}",
                        args.len()
                    )));
                }
                let path = self.lower_expr(&args[0])?;
                let cb = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.watch(#path, #cb).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.watch codegen parse: {e}")))
            }
            // T34: buff-auth assoc fns. The 5 (type, method) pairs
            // below cover the MVP surface: JWT.encode / JWT.decode /
            // Password.hash / Password.verify / OAuth2Client.new /
            // Rbac.new. The instance-method forms
            // (client.authorization_url / client.exchange_code /
            // policy.add / policy.enforce) are deferred to the sibling
            // task that adds Type::OAuth2Client / Type::Rbac — mirrors
            // the T17 Web (web.get / web.listen) + T18 Database
            // (pool.query / pool.execute) forward-declaration
            // precedent. Records `buff-auth` + `jsonwebtoken` +
            // `argon2` + `oauth2` + `reqwest` in extern_crates via the
            // shared `program_uses_namespace("JWT"|"OAuth2Client"|
            // "Password"|"Rbac")` walker.
            //
            // JWT.encode(claims, secret) -> String. Two args
            // (Map<String, Unknown>, String). Wraps
            // `buff_auth::jwt_encode(&claims_obj, &secret)
            // .unwrap_or_default()` (panic-free — empty String on
            // encode failure, NEVER panics). The codegen serialises
            // the Buff Map<String, Unknown> arg to a serde_json Value
            // and extracts the inner Map<String, Value> via
            // `.as_object().cloned()` (the Buff Map<String, Unknown>
            // lowers to a `std::collections::HashMap<String, ?>`
            // whose serde_json serialisation round-trips through
            // Map<String, Value>).
            (T::Jwt, A::Encode) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "JWT.encode() expects exactly 2 args (claims, secret), got {}",
                        args.len()
                    )));
                }
                let claims = self.lower_expr(&args[0])?;
                let secret = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::jwt_encode(
                        &serde_json::to_value(&#claims)
                            .ok()
                            .and_then(|v| v.as_object().cloned())
                            .unwrap_or_default(),
                        &#secret,
                    ).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("JWT.encode codegen parse: {e}")))
            }
            // JWT.decode(token, secret) -> Map<String, Unknown>. Two
            // args (String, String). Wraps
            // `buff_auth::jwt_decode(&token, &secret).unwrap_or_default()`
            // (panic-free — invalid signature / malformed token /
            // expired all collapse to an empty Map, NEVER panics).
            (T::Jwt, A::Decode) => {
                let (token, secret) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::jwt_decode(#token, #secret)
                        .unwrap_or_default()
                        .into_iter().collect()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("JWT.decode codegen parse: {e}")))
            }
            // Password.hash(plain) -> String. One arg (String). Wraps
            // `buff_auth::password_hash(plain).unwrap_or_default()`
            // (panic-free — empty String on hash failure, NEVER
            // panics).
            (T::Password, A::PasswordHash) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::password_hash(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Password.hash codegen parse: {e}")))
            }
            // Password.verify(plain, phc_hash) -> Bool. Two args
            // (String, String). Wraps
            // `buff_auth::password_verify(plain, hash).unwrap_or(false)`
            // (panic-free — false on mismatch or hash-format failure,
            // NEVER panics). Mirrors the T26 Signature.verify lowering
            // stance: verification failure is Ok(false), NOT an error.
            (T::Password, A::PasswordVerify) => {
                let (plain, hash) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::password_verify(#plain, #hash).unwrap_or(false)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Password.verify codegen parse: {e}")))
            }
            // T39: Archive.compress_dir(input_dir, output_path) -> Void.
            // Two args. Wraps `buff_archive::Archive::compress_dir(
            // input_dir, output_path, buff_archive::Format::from_path(
            // std::path::Path::new(&output_path)).unwrap_or(
            // buff_archive::Format::Zip))?` (the `?` propagates
            // `ArchiveError` per Buff's R3 error-mapping contract; the
            // format is auto-detected from the output_path extension —
            // `.zip` → Zip, `.tar.gz` → Gz, `.tar.zst` → Zstd, etc.,
            // matching the cross-language convention of `tar -czf
            // x.tar.gz src/`). Records `buff-archive` + `zip` + `tar`
            // + `flate2` + `ruzstd` in extern_crates via the
            // `program_uses_namespace("Archive")` walker.
            (T::Archive, A::CompressDir) => {
                let (input_dir, output_path) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_archive::Archive::compress_dir(
                        #input_dir,
                        #output_path,
                        buff_archive::Format::from_path(std::path::Path::new(&#output_path))
                            .unwrap_or(buff_archive::Format::Zip),
                    )?
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Archive.compress_dir codegen parse: {e}"))
                })
            }
            // T39: Archive.extract(archive_path, output_dir) -> Void.
            // Two args. Wraps `buff_archive::Archive::extract(
            // archive_path, output_dir)?` (the format is auto-detected
            // from the file's extension inside the wrapper). Records
            // `buff-archive` + `zip` + `tar` + `flate2` + `ruzstd` in
            // extern_crates via the `program_uses_namespace("Archive")`
            // walker.
            (T::Archive, A::Extract) => {
                let (archive_path, output_dir) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_archive::Archive::extract(#archive_path, #output_dir)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Archive.extract codegen parse: {e}")))
            }
            // T51: MsgPack.serialize(value) -> Vector<Byte>. Wraps
            // `buff_msgpack::serialize(&value).unwrap_or_default()`
            // (empty Vec on serialize failure — NEVER panics, matching
            // Buff's "no panicking generated code" rule). Records
            // `buff-msgpack` + `rmp-serde` + `serde_json` in
            // extern_crates via the
            // `program_uses_namespace("MsgPack")` walker.
            (T::MsgPack, A::Serialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_msgpack::serialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("MsgPack.serialize codegen parse: {e}")))
            }
            // T51: MsgPack.deserialize(bytes) -> Value. Wraps
            // `buff_msgpack::deserialize(&bytes).unwrap_or_default()`
            // (returns `serde_json::Value::Null` on deserialize failure
            // — `Value` impls `Default`; NEVER panics). The arg is a
            // `Vector<Byte>` on the Buff surface (Vec<u8> after
            // codegen lowering); the codegen passes it by ref.
            (T::MsgPack, A::Deserialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_msgpack::deserialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("MsgPack.deserialize codegen parse: {e}"))
                })
            }
            // T51: MsgPack.roundtrip(value) -> Option<Value>. Wraps
            // `buff_msgpack::roundtrip(&value)` directly — the
            // runtime fn already returns `Option<serde_json::Value>`
            // (None on either step failing). NEVER panics.
            (T::MsgPack, A::Roundtrip) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_msgpack::roundtrip(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("MsgPack.roundtrip codegen parse: {e}")))
            }
            // T52: Protobuf.serialize(value) -> Vector<Byte>. Wraps
            // `buff_protobuf::serialize(&value).unwrap_or_default()`
            // (empty Vec on serialize failure — NEVER panics, matching
            // Buff's "no panicking generated code" rule). Records
            // `buff-protobuf` + `prost` + `prost-types` + `serde_json`
            // in extern_crates via the
            // `program_uses_namespace("Protobuf")` /
            // `program_uses_namespace("Message")` walker. Mirrors T51
            // MsgPack.serialize 1:1.
            (T::Protobuf, A::Serialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::serialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Protobuf.serialize codegen parse: {e}"))
                })
            }
            // T52: Protobuf.deserialize(bytes) -> Value. Wraps
            // `buff_protobuf::deserialize(&bytes).unwrap_or_default()`
            // (returns `serde_json::Value::Null` on deserialize failure
            // — `Value` impls `Default`; NEVER panics). The arg is a
            // `Vector<Byte>` on the Buff surface (Vec<u8> after
            // codegen lowering); the codegen passes it by ref. Mirrors
            // T51 MsgPack.deserialize 1:1.
            (T::Protobuf, A::Deserialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::deserialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Protobuf.deserialize codegen parse: {e}"))
                })
            }
            // T52: Protobuf.roundtrip(value) -> Option<Value>. Wraps
            // `buff_protobuf::roundtrip(&value)` directly — the
            // runtime fn already returns `Option<serde_json::Value>`
            // (None on either step failing). NEVER panics. Mirrors T51
            // MsgPack.roundtrip 1:1.
            (T::Protobuf, A::Roundtrip) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::roundtrip(&#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Protobuf.roundtrip codegen parse: {e}"))
                })
            }
            // T52: Message.new(value) -> Message. One arg (the value
            // to encode). Wraps `buff_protobuf::Message::new(&value)
            // .unwrap_or_default()` (panic-free on encode failure —
            // Message impls Default as an empty-payload message).
            // `New` is shared with Channel.new / Faker.new /
            // Crawler.new / Point.new / XmlElement.new — dispatched
            // on the (Message, New) pair. Records `buff-protobuf` +
            // `prost` + `prost-types` + `serde_json` in extern_crates
            // via the `program_uses_namespace("Message")` walker.
            (T::Message, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::Message::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.new codegen parse: {e}")))
            }
            // T52: Message.from_bytes(bytes) -> Message. One arg
            // (Vector<Byte>). Wraps
            // `buff_protobuf::Message::from_bytes(bytes)
            // .unwrap_or_default()` (panic-free on decode failure /
            // empty buffer — Message impls Default). `FromBytes` is
            // shared with Image.from_bytes — dispatched on the
            // (Message, FromBytes) pair.
            (T::Message, A::FromBytes) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::Message::from_bytes(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Message.from_bytes codegen parse: {e}"))
                })
            }
            // T52: Message.decode(bytes) -> Message. Class-method
            // alias for Message.from_bytes — same lowering shape but
            // takes a `&[u8]` (Bytes ref) instead of `Vec<u8>` (owned
            // bytes). The codegen splices the arg directly (Buff's
            // `Vector<Byte>` lowers to `Vec<u8>` which deref-coerces
            // to `&[u8]` for the underlying `Message::decode(&[u8])`
            // Rust signature). `Decode` is shared with Base64.decode /
            // Hex.decode / URLEncode.decode — dispatched on the
            // (Message, Decode) pair.
            (T::Message, A::Decode) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::Message::decode(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.decode codegen parse: {e}")))
            }
            // T47: Bot.new(platform, token) -> Bot. Two args (Platform,
            // String). Wraps `buff_chat::Bot::new(platform, token)
            // .unwrap_or_default()` (panic-free on construction
            // failure — Bot impls Default as an empty Discord bot,
            // added in the T47 MVP commit). `New` is shared with
            // Channel.new / Faker.new / Point.new / XmlElement.new /
            // Message.new (T52) — dispatched on the (Bot, New) pair.
            // Records `buff-chat` + `serenity` + `teloxide` +
            // `async-trait` + `tokio` in extern_crates via the
            // `program_uses_namespace("Bot")` walker.
            (T::Bot, A::New) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Bot.new expects exactly 2 args (platform, token), got {}",
                        args.len()
                    )));
                }
                let platform = self.lower_expr(&args[0])?;
                let token = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_chat::Bot::new(#platform, (#token).to_string()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.new codegen parse: {e}")))
            }
            // T47: ChatMessage.new(text, channel, author, platform,
            // is_dm) -> ChatMessage. Five args (String, String,
            // String, Platform, Bool). Wraps
            // `buff_chat::Message::new(text, channel, author,
            // platform, is_dm)` directly (infallible — the underlying
            // Message::new ctor has no failure mode). The token-
            // coercion `(#x).to_string()` on each String arg handles
            // Buff's `&str`-from-literal lowering (the codegen inserts
            // the coercion so the user can pass either String or &str
            // — the wrapper ctor takes owned `String` per FFI guide
            // R5). Records `buff-chat` + `serenity` + `teloxide` +
            // `async-trait` + `tokio` in extern_crates via the
            // `program_uses_namespace("ChatMessage")` walker.
            (T::ChatMessage, A::New) => {
                if args.len() != 5 {
                    return Err(self.unsupported(&format!(
                        "ChatMessage.new expects exactly 5 args (text, channel, author, platform, is_dm), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let channel = self.lower_expr(&args[1])?;
                let author = self.lower_expr(&args[2])?;
                let platform = self.lower_expr(&args[3])?;
                let is_dm = self.lower_expr(&args[4])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_chat::Message::new(
                        (#text).to_string(),
                        (#channel).to_string(),
                        (#author).to_string(),
                        #platform,
                        #is_dm,
                    )
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("ChatMessage.new codegen parse: {e}")))
            }
            // T50: Xml.from_str(xml) -> XmlDocument. One arg (String).
            // Wraps `buff_xml::XmlDocument::from_str(&xml)
            // .unwrap_or_default()` (panic-free on empty/parse failure —
            // XmlDocument impls Default as a root-only document). Records
            // `buff-xml` + `quick-xml` in extern_crates via the
            // `program_uses_namespace("Xml")` walker.
            (T::Xml, A::FromStr) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlDocument::from_str(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Xml.from_str codegen parse: {e}")))
            }
            // T50: XmlElement.new(name, text, attrs) -> XmlElement.
            // Three args (String, String, Map<String,String>). Wraps
            // `buff_xml::XmlElement::new(&name, &text, attrs_vec)`
            // where `attrs_vec` is built by `.into_iter().map(|(k, v)|
            // (k.to_string(), v.to_string())).collect::<Vec<(String,
            // String)>>()` — the conversion accepts any IntoIterator
            // yielding string-like tuples (Buff Map literal codegens
            // to `HashMap<&str, &str>`; user-passed `HashMap<String,
            // String>` / `Vec<(String, String)>` also work). Infallible
            // (the wrapper ctor never fails — no `?` / unwrap_or_default
            // needed). Records `buff-xml` + `quick-xml` in extern_crates
            // via the `program_uses_namespace("XmlElement")` walker.
            (T::XmlElement, A::New) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "XmlElement.new expects exactly 3 args (name, text, attrs), got {}",
                        args.len()
                    )));
                }
                let name = self.lower_expr(&args[0])?;
                let text = self.lower_expr(&args[1])?;
                let attrs = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlElement::new(
                        &(#name).to_string(),
                        &(#text).to_string(),
                        (#attrs)
                            .into_iter()
                            .map(|(k, v)| (
                                std::string::ToString::to_string(&k),
                                std::string::ToString::to_string(&v)
                            ))
                            .collect::<Vec<(String, String)>>(),
                    )
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.new codegen parse: {e}")))
            }
            // T45: Point.new(x, y) -> Point. Two args (Float, Float).
            // Wraps `buff_geo::Point::new(x, y)` (infallible — the
            // underlying geo_types::Point::new never fails). Records
            // `buff-geo` + `geo` + `geo-types` in extern_crates via the
            // `program_uses_namespace("Point")` walker.
            (T::Point, A::New) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Point.new expects exactly 2 args (x, y), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let y = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::Point::new(#x, #y)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Point.new codegen parse: {e}")))
            }
            // T45: LineString.new(points) -> LineString. One arg
            // (Vector<Point>). Wraps
            // `buff_geo::LineString::new(points).unwrap_or_default()`
            // (panic-free on empty input — LineString impls Default).
            (T::LineString, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::LineString::new(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("LineString.new codegen parse: {e}")))
            }
            // T45: LineString.from_coords(flat) -> LineString. One arg
            // (Vector<Float>). Wraps
            // `buff_geo::LineString::from_coords(coords).unwrap_or_default()`
            // (panic-free on empty / odd-length input).
            (T::LineString, A::FromCoords) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::LineString::from_coords(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("LineString.from_coords codegen parse: {e}"))
                })
            }
            // T45: Polygon.new(ring) -> Polygon. One arg (Vector<Point>).
            // Wraps `buff_geo::Polygon::new(ring).unwrap_or_default()`
            // (panic-free on degenerate input — Polygon impls Default).
            (T::Polygon, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::Polygon::new(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Polygon.new codegen parse: {e}")))
            }
            // T45: Polygon.from_coords(flat) -> Polygon. One arg
            // (Vector<Float>). Wraps
            // `buff_geo::Polygon::from_coords(coords).unwrap_or_default()`
            // (panic-free on empty / odd-length / degenerate input).
            (T::Polygon, A::FromCoords) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::Polygon::from_coords(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Polygon.from_coords codegen parse: {e}"))
                })
            }
            // T54: buff-simd constructors. Each lowers to the matching
            // `buff_simd::Simd::*` associated function. `Simd.splat(x)`
            // and `Simd.from_array(arr)` are infallible (wrap the
            // underlying `wide::f32x4` ctors directly). `Simd.from_slice
            // (slice)` is fallible in Rust (returns
            // `Result<_, SimdError>`) but surfaces as infallible on the
            // Buff side via `.unwrap_or_default()` (Simd impls Default
            // as `splat(0.0)`). Records `buff-simd` + `wide` in
            // extern_crates via the `program_uses_namespace("Simd")`
            // walker.
            //
            // `Simd.splat(x)` -> Simd. One arg (Float). Wraps
            // `buff_simd::Simd::splat(x)`.
            (T::Simd, A::Splat) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "Simd.splat expects exactly 1 arg (x: Float), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_simd::Simd::splat(#x as f32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.splat codegen parse: {e}")))
            }
            // `Simd.from_slice(slice)` -> Simd. One arg (Vector<Float>).
            // Wraps
            // `buff_simd::Simd::from_slice(&slice).unwrap_or_default()`
            // (panic-free on too-short / non-finite input).
            (T::Simd, A::FromSlice) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_simd::Simd::from_slice(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.from_slice codegen parse: {e}")))
            }
            // `Simd.from_array(arr)` -> Simd. One arg (Vector<Float>).
            // Wraps `buff_simd::Simd::from_array(...)` via the slice
            // path (the wrapper accepts a slice; codegen passes the
            // array as a slice reference). Infallible via
            // `.unwrap_or_default()`.
            (T::Simd, A::FromArray) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_simd::Simd::from_slice(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.from_array codegen parse: {e}")))
            }
            // T46: Text.detect_language(text) -> Option<Language>. One
            // arg (String). Wraps `buff_nlp::Text::detect_language(&text)`
            // directly — the wrapper already returns Option<Language>
            // (None on empty input / detection failure). NEVER panics
            // (the wrapper uses catch_unwind per FFI guide R6). Records
            // `buff-nlp` + `whatlang` + `rust-stemmers` +
            // `unicode-segmentation` in extern_crates via the
            // `program_uses_namespace("Text")` walker.
            (T::Text, A::DetectLanguage) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::detect_language(&#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Text.detect_language codegen parse: {e}"))
                })
            }
            // T46: Text.stem(word, algorithm) -> String. Two args
            // (String word, String algorithm — lowercase Snowball name
            // like "english" / "portuguese"). Wraps
            // `buff_nlp::Text::stem(&word,
            // buff_nlp::StemAlgorithm::from_code(&algorithm)
            // .unwrap_or(buff_nlp::StemAlgorithm::English))?` — the
            // `?` propagates `NlpError` per Buff's R3 error-mapping
            // contract; unknown algorithm names fall back to English
            // (defensive, never silently corrupts). The wrapper uses
            // catch_unwind per FFI guide R6.
            (T::Text, A::Stem) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Text.stem expects exactly 2 args (word, algorithm), got {}",
                        args.len()
                    )));
                }
                let word = self.lower_expr(&args[0])?;
                let algorithm = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::stem(
                        &#word,
                        buff_nlp::StemAlgorithm::from_code(&#algorithm)
                            .unwrap_or(buff_nlp::StemAlgorithm::English),
                    )?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Text.stem codegen parse: {e}")))
            }
            // T46: Text.tokenize(text) -> Vector<String>. One arg
            // (String). Wraps `buff_nlp::Text::tokenize(&text)` —
            // pure iterator over UAX #29 word boundaries (no panic
            // vectors; catch_unwind omitted per the lib.rs note).
            (T::Text, A::Tokenize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::tokenize(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Text.tokenize codegen parse: {e}")))
            }
            // T46: Text.sentences(text) -> Vector<String>. One arg
            // (String). Wraps `buff_nlp::Text::sentences(&text)` —
            // pure iterator over UAX #29 sentence boundaries.
            (T::Text, A::Sentences) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::sentences(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Text.sentences codegen parse: {e}")))
            }
            // T48: Provider.new(rpc_url) -> Provider. One arg (String).
            // Wraps `buff_web3::Provider::new(&url).unwrap_or_default()`
            // (panic-free — Provider impls Default as a localhost-pointed
            // no-op provider; the codegen-lowered `.unwrap_or_default()`
            // collapses `Web3Error::InvalidUrl` / `Web3Error::Panic` to
            // the default Provider per Buff's "no panicking generated
            // code" rule). Records `buff-web3` + `ethers` + `tokio` +
            // `reqwest` + `serde_json` + `hex` in extern_crates via the
            // `program_uses_namespace("Provider")` walker.
            (T::Provider, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web3::Provider::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Provider.new codegen parse: {e}")))
            }
            // T48: Wallet.from_private_key(key) -> Wallet. One arg
            // (String — accepts `0x`-prefixed or bare 64-char hex).
            // Wraps `buff_web3::Wallet::from_private_key(&key)
            // .unwrap_or_default()` (panic-free — Wallet impls Default
            // as a "burner" wallet derived from a fixed test key,
            // NEVER use on mainnet; the codegen-lowered
            // `.unwrap_or_default()` collapses
            // `Web3Error::InvalidPrivateKey` / `Web3Error::Panic` to
            // the default Wallet). Records `buff-web3` + `ethers` +
            // `tokio` + `reqwest` + `serde_json` + `hex` in
            // extern_crates via the `program_uses_namespace("Wallet")`
            // walker.
            (T::Wallet, A::FromPrivateKey) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web3::Wallet::from_private_key(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Wallet.from_private_key codegen parse: {e}"))
                })
            }
            // T48: Contract.new(address, abi_json, client) -> Contract.
            // Three args (String address, String abi JSON,
            // Provider|ConnectedWallet client). Wraps
            // `buff_web3::Contract::new(&addr, &abi, #client)
            // .unwrap_or_default()` (panic-free — Contract impls
            // Default as a zero-address + empty-ABI + read-only
            // contract; the codegen-lowered `.unwrap_or_default()`
            // collapses `Web3Error::InvalidAddress` /
            // `Web3Error::InvalidAbi` / `Web3Error::Panic` to the
            // default Contract). The `client` arg is spliced directly
            // — the buff_web3 `IntoClient` trait accepts both Provider
            // (read-only) and ConnectedWallet (signing). Records
            // `buff-web3` + `ethers` + `tokio` + `reqwest` +
            // `serde_json` + `hex` in extern_crates via the
            // `program_uses_namespace("Contract")` walker.
            (T::Contract, A::New) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "Contract.new expects exactly 3 args (address, abi_json, client), got {}",
                        args.len()
                    )));
                }
                let address = self.lower_expr(&args[0])?;
                let abi = self.lower_expr(&args[1])?;
                let client = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web3::Contract::new(&#address, &#abi, #client).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Contract.new codegen parse: {e}")))
            }
            // T49: AES.generate_key() -> Vector<Byte>. Zero args.
            // Wraps `buff_crypto_extras::aes_gcm_api::generate_key()`
            // (infallible — returns Vec<u8> directly via
            // `Aes256Gcm::generate_key(&mut OsRng)`; OsRng::fill_bytes
            // is infallible on all platforms Buff supports). Records
            // `buff-crypto-extras` + 8 RustCrypto crates in
            // extern_crates via the
            // `program_uses_namespace("AES")` walker.
            (T::AES, A::GenerateKey) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::generate_key()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AES.generate_key codegen parse: {e}")))
            }
            // T49: AES.generate_nonce() -> Vector<Byte>. Zero args.
            // Wraps `buff_crypto_extras::aes_gcm_api::generate_nonce()`
            // (infallible — returns the 12-byte GCM nonce via
            // `Aes256Gcm::generate_nonce(&mut OsRng)`).
            (T::AES, A::GenerateNonce) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::generate_nonce()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("AES.generate_nonce codegen parse: {e}"))
                })
            }
            // T49: AES.encrypt(key, nonce, plaintext) -> Vector<Byte>.
            // Three args. Wraps `buff_crypto_extras::aes_gcm_api::
            // encrypt(&key, &nonce, &plaintext).unwrap_or_default()`
            // (empty Vec on any failure — wrong key/nonce length,
            // AES engine error, panic — NEVER panics, matching
            // Buff's "no panicking generated code" rule). The args
            // are spliced by reference so the underlying `&[u8]`
            // bounds are satisfied for both Vec<u8> and slices.
            (T::AES, A::Encrypt) => {
                let mut lowered = n_args(self, 3)?;
                let key = lowered.remove(0);
                let nonce = lowered.remove(0);
                let plaintext = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::encrypt(#key.as_slice(), #nonce.as_slice(), #plaintext.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AES.encrypt codegen parse: {e}")))
            }
            // T49: AES.decrypt(key, nonce, ciphertext) -> Vector<Byte>.
            // Three args. Wraps `buff_crypto_extras::aes_gcm_api::
            // decrypt(&key, &nonce, &ciphertext).unwrap_or_default()`
            // (empty Vec on auth-tag mismatch / wrong key / wrong
            // nonce length / panic — NEVER panics). Same shape as
            // AES.encrypt.
            (T::AES, A::Decrypt) => {
                let mut lowered = n_args(self, 3)?;
                let key = lowered.remove(0);
                let nonce = lowered.remove(0);
                let ciphertext = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::decrypt(#key.as_slice(), #nonce.as_slice(), #ciphertext.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AES.decrypt codegen parse: {e}")))
            }
            // T49: RSA.generate_keypair(bits) -> RsaKeypair. One arg
            // (Int). Wraps `buff_crypto_extras::rsa_api::generate_keypair
            // (bits as usize).unwrap_or_default()` (panic-free — the
            // wrapper crate's RsaKeypair impls Default as the
            // empty-PEM-string fallback; the codegen-lowered
            // `.unwrap_or_default()` collapses CryptoError::
            // InvalidLength / Panic to the default RsaKeypair per
            // Buff's "no panicking generated code" rule). The `as
            // usize` lifts Buff's Int<64> to the usize Rust expects.
            // Computationally expensive (~100ms for 2048-bit, ~1s
            // for 4096-bit).
            (T::RSA, A::GenerateKeypair) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::rsa_api::generate_keypair(#arg as usize).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("RSA.generate_keypair codegen parse: {e}"))
                })
            }
            // T49: RSA.sign(private_pem, data) -> Vector<Byte>. Two
            // args (String, Vector<Byte>). Wraps
            // `buff_crypto_extras::rsa_api::sign(&private_pem,
            // data.as_slice()).unwrap_or_default()` (empty Vec on
            // malformed PEM / sign engine failure / panic — NEVER
            // panics; the empty-Vec fallback is the correct
            // user-facing behavior since RSA.verify will return
            // false for any non-matching signature, including an
            // empty one).
            (T::RSA, A::Sign) => {
                let (private_pem, data) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::rsa_api::sign(&#private_pem, #data.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("RSA.sign codegen parse: {e}")))
            }
            // T49: RSA.verify(public_pem, data, signature) -> Bool.
            // Three args. Wraps `buff_crypto_extras::rsa_api::verify(
            // &public_pem, data.as_slice(), signature.as_slice())`
            // (the wrapper already returns `bool` — false on any
            // failure: signature mismatch, malformed PEM, invalid
            // signature bytes, or panic — mirrors T26 Signature.
            // verify + T34 Password.verify stance so a future
            // verify_allow policy can layer cleanly). NO
            // `.unwrap_or_default()` needed (the wrapper collapses
            // all failures to `false` itself).
            (T::RSA, A::Verify) => {
                let mut lowered = n_args(self, 3)?;
                let public_pem = lowered.remove(0);
                let data = lowered.remove(0);
                let signature = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::rsa_api::verify(&#public_pem, #data.as_slice(), #signature.as_slice())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("RSA.verify codegen parse: {e}")))
            }
            // T49: ECDH.generate_private() -> Vector<Byte>. Zero
            // args. Wraps `buff_crypto_extras::ecdh_api::
            // p256_generate_private()` (infallible — returns the
            // 32-byte P-256 scalar via `P256Secret::random(&mut
            // OsRng)`).
            (T::ECDH, A::GeneratePrivate) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::ecdh_api::p256_generate_private()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ECDH.generate_private codegen parse: {e}"))
                })
            }
            // T49: ECDH.public_from_private(private) -> Vector<Byte>.
            // One arg (Vector<Byte>). Wraps
            // `buff_crypto_extras::ecdh_api::p256_public_from_private
            // (private.as_slice()).unwrap_or_default()` (empty Vec
            // on wrong length / invalid scalar / panic — NEVER
            // panics).
            (T::ECDH, A::PublicFromPrivate) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::ecdh_api::p256_public_from_private(#arg.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ECDH.public_from_private codegen parse: {e}"))
                })
            }
            // T49: ECDH.derive_shared(private, public) ->
            // Vector<Byte>. Two args. Wraps
            // `buff_crypto_extras::ecdh_api::p256_derive_shared(
            // private.as_slice(), public.as_slice())
            // .unwrap_or_default()` (empty Vec on wrong length /
            // invalid point / cofactor edge case / panic — NEVER
            // panics).
            (T::ECDH, A::DeriveShared) => {
                let (private, public) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::ecdh_api::p256_derive_shared(#private.as_slice(), #public.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ECDH.derive_shared codegen parse: {e}"))
                })
            }
            // T49: Argon2.generate_salt() -> Vector<Byte>. Zero
            // args. Wraps `buff_crypto_extras::argon2_api::
            // generate_salt()` (infallible — fills a 16-byte Vec
            // via `rand::rng().fill_bytes`).
            (T::Argon2, A::GenerateSalt) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::argon2_api::generate_salt()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Argon2.generate_salt codegen parse: {e}"))
                })
            }
            // T49: Argon2.derive_key(password, salt) -> Vector<Byte>.
            // Two args (String, Vector<Byte>). Wraps
            // `buff_crypto_extras::argon2_api::derive_key(&password,
            // salt.as_slice()).unwrap_or_default()` (empty Vec on
            // wrong salt length / Argon2 engine failure / panic —
            // NEVER panics).
            (T::Argon2, A::DeriveKey) => {
                let (password, salt) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::argon2_api::derive_key(&#password, #salt.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Argon2.derive_key codegen parse: {e}")))
            }
            // T89: Decimal constructors.
            // `Decimal.new(str)` -> Decimal. One arg (String). Wraps
            // `rust_decimal::Decimal::from_str(&s).unwrap_or_default()`
            // (panic-free — invalid input collapses to Decimal::ZERO).
            (T::Decimal, A::New) => {
                let s = one_arg(self)?;
                let s_ref = coerce_str_arg_to_ref(s, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    rust_decimal::Decimal::from_str(#s_ref).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Decimal.new codegen parse: {e}")))
            }
            // `Decimal.from_float(f)` -> Decimal. One arg (Float). Wraps
            // `rust_decimal::Decimal::from_f64(f).unwrap_or_default()`
            // (panic-free — NaN/Inf collapse to Decimal::ZERO).
            (T::Decimal, A::FromFloat) => {
                let f = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    rust_decimal::Decimal::from_f64(#f).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Decimal.from_float codegen parse: {e}"))
                })
            }
            // Every other combination was already rejected by
            // `assoc_fn_lookup` in the caller; this arm is unreachable but
            // required for exhaustiveness.
            _ => Err(self.unsupported(&format!(
                "prelude type+method combination {:?}.{:?}",
                ptype, pmethod
            ))),
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
