//! ITER-40 - T124 stdlib prelude lowering, remaining seam: the prelude-type
//! INSTANCE-method match (lower_prelude_type_instance_fn, ~170 arms over the
//! buff_lang_types::prelude_types registry's PreludeInstanceFn variants)
//! plus the associated-CONSTANT table (lower_prelude_type_assoc_const)
//! (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move of `impl RustCodegen` methods into this child module so the
//! parent file shrinks. Both entry points are pub(super) (their only call
//! sites are in the method_call_lowering sibling, after the registry
//! lookups). The prelude_types sibling (ITER-31) keeps the associated-
//! FUNCTION table (lower_prelude_type_assoc_fn); the prelude_fns sibling
//! (ITER-34) keeps the free-fn dispatch (lower_prelude_call). The parent
//! declares only `mod prelude_lowering;` (inherent methods resolve by type,
//! no `use` needed). Child inherits parent imports via use super::* and may
//! call the parent private methods (descendant privacy) and the extracted
//! helper modules.

use super::*;

impl RustCodegen {
    /// T124f: lower a prelude-type associated-CONSTANT access
    /// (`Type.NAME`) to the corresponding Rust path.
    ///
    /// Dispatched from [`Self::lower_method_call`] when the receiver is
    /// a bare Ident naming a prelude type with a registered associated
    /// constant (currently only `Math.PI` / `Math.E`). The lowering is
    /// a fully-qualified Rust path so generated code requires no `use`
    /// import (mirrors the chrono / regex / toml fully-qualified-path
    /// pattern).
    ///
    /// # Lowering table
    ///
    /// | Buff source  | Generated Rust              |
    /// |--------------|------------------------------|
    /// | `Math.PI`    | `std::f64::consts::PI`      |
    /// | `Math.E`     | `std::f64::consts::E`       |
    ///
    /// Both lower to `f64` consts from Rust's `std::f64::consts` module
    /// - NO extern crate needed (Math uses only Rust `std`).
    ///
    /// Built via `rust_call_expr`'s path machinery... wait, that
    /// produces a `path(args)` CALL. We need a bare PATH (no parens).
    /// The simplest path is `syn::parse_str` -> `syn::ExprPath` (any
    /// `::`-separated path string parses cleanly as a path expression).
    /// That's already the pattern used in [`lower_graphemes_call`] for
    /// the `unicode_segmentation::UnicodeSegmentation::graphemes` path
    /// fragment. We reuse it here for consistency.
    pub(super) fn lower_prelude_type_assoc_const(
        &mut self,
        ptype: buff_lang_types::PreludeType,
        pconst: buff_lang_types::PreludeAssocConst,
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::{PreludeAssocConst as C, PreludeType as T};
        let path: &str = match (ptype, pconst) {
            // `Math.PI` / `Math.E` -> `std::f64::consts::PI` / `E`.
            // Both Rust consts are `f64`; the codegen-lowered path is
            // fully-qualified so the generated crate needs no `use
            // std::f64::consts;` import.
            (T::Math, C::Pi) => "std::f64::consts::PI",
            (T::Math, C::E) => "std::f64::consts::E",
            // T47: `Platform.Discord` / `Platform.Telegram` ->
            // `buff_chat::Platform::Discord` / `::Telegram`. Both
            // variants are `Copy` enum units; the codegen-lowered path
            // is fully-qualified so the generated crate needs no `use
            // buff_chat::Platform;` import. Mirrors the Math const
            // lowering shape (zero-arg `Type.NAME` access).
            (T::Platform, C::Discord) => "buff_chat::Platform::Discord",
            (T::Platform, C::Telegram) => "buff_chat::Platform::Telegram",
            // Every other combination was already rejected by
            // `assoc_const_lookup` in the caller; this arm is
            // unreachable but required for exhaustiveness.
            _ => {
                return Err(self.unsupported(&format!(
                    "prelude type+const combination {:?}.{:?}",
                    ptype, pconst
                )));
            }
        };
        syn::parse_str::<SynExpr>(path)
            .map_err(|e| self.unsupported(&format!("Math const codegen parse ({path}): {e}")))
    }

    /// T124b: lower a prelude-type instance-method call (`recv.method(args)`)
    /// to the corresponding chrono / std::time Rust idiom.
    ///
    /// Dispatched from [`Self::lower_method_call`] when the receiver's
    /// inferred type is one of the prelude datetime family AND the method
    /// name is a recognised instance method on that type.
    ///
    /// # Lowering table
    ///
    /// | Buff source           | Generated Rust                                  |
    /// |-----------------------|-------------------------------------------------|
    /// | `dt.format("%Y-%m-%d")` | `dt.format("%Y-%m-%d").to_string()`           |
    /// | `dt.year()`           | `dt.year()`  (i32 → promoted to i64 by annotation) |
    /// | `dt.month()`          | `dt.month()`                                    |
    /// | `dt.day()`            | `dt.day()`                                      |
    /// | `dt.hour()`           | `dt.hour()`                                     |
    /// | `dt.minute()`         | `dt.minute()`                                   |
    /// | `dt.second()`         | `dt.second()`                                   |
    /// | `dt.timestamp()`      | `dt.timestamp()`                                |
    ///
    /// `format` returns `chrono::DelayedFormat<...>`, which doesn't impl
    /// `Into<String>` directly — we chain `.to_string()` so the result is
    /// a real Rust `String` (Display-able via `.to_string()`). The other
    /// accessors return `i32` / `i64` which Rust coerces implicitly when
    /// the surrounding context expects `i64`.
    pub(super) fn lower_prelude_type_instance_fn(
        &mut self,
        recv_ty: &Type,
        pmethod: buff_lang_types::PreludeInstanceFn,
        receiver: &Expr,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::PreludeInstanceFn as M;
        let recv = self.lower_expr(receiver)?;
        // Defensive `one_arg` closure for instance methods that take a
        // single positional arg (added by T44 buff-i18n backfill —
        // also unblocks T42/T43 sibling code that already used the
        // name without defining it locally). Mirrors the
        // `lower_prelude_type_assoc_fn::one_arg` closure shape.
        let pmethod_name = pmethod.name();
        let one_arg = |c: &mut Self| -> Result<SynExpr, CodegenError> {
            if args.len() != 1 {
                return Err(c.unsupported(&format!(
                    "{}() expects exactly 1 arg, got {}",
                    pmethod_name,
                    args.len()
                )));
            }
            c.lower_expr(&args[0])
        };
        // All current instance methods are either 0-arg or 1-arg (format).
        // We validate arity once here so the dispatch below doesn't repeat
        // the check.
        match pmethod {
            M::Format => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "format() expects exactly 1 arg (the strftime format string), got {}",
                        args.len()
                    )));
                }
                let fmt = self.lower_expr(&args[0])?;
                let fmt = coerce_str_arg_to_ref(fmt, &args[0]);
                // recv.format(fmt).to_string() — the chain returns a String.
                let format_call = method_call_one_arg(recv, "format", fmt);
                Ok(method_call_no_args(format_call, "to_string"))
            }
            M::Year | M::Month | M::Day | M::Hour | M::Minute | M::Second | M::Timestamp => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "{:?}() takes no arguments, got {}",
                        pmethod,
                        args.len()
                    )));
                }
                // The chrono methods have the same names as Buff's surface,
                // so we just emit recv.method().
                let method_name = pmethod.name();
                // Defensive: confirm the receiver type actually supports
                // this method. This was already checked by
                // `instance_fn_lookup` in the caller, but we re-check here
                // so the helper stays self-contained.
                if buff_lang_types::instance_fn_return_type(recv_ty, pmethod, &[]).is_none() {
                    return Err(self.unsupported(&format!(
                        "{recv_ty}.{method_name}() is not a recognised prelude instance method"
                    )));
                }
                Ok(method_call_no_args(recv, method_name))
            }
            // T124d: Regex instance methods.
            //
            // `regex.match(text)` -> Option<String>. Wraps the bool
            // result of `regex::Regex::is_match` into an Option that
            // carries the original text on match (so it composes with
            // Buff's Option-handling surface identically to `find`).
            M::Match => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "match() expects exactly 1 arg (the text to search), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let text_ref = coerce_str_arg_to_ref(text.clone(), &args[0]);
                // if recv.is_match(text) { Some(text.to_string()) } else { None }
                // Built via quote! so the if/else shape is a real syn::ExprIf.
                let is_match_call = method_call_one_arg(recv, "is_match", text_ref);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    if #is_match_call {
                        Some(#text.to_string())
                    } else {
                        None
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Regex.match codegen parse: {e}")))
            }
            // `regex.find(text)` -> Option<String>. Lowers to
            // `recv.find(text).map(|m| m.as_str().to_string())`.
            //
            // T50: the `!matches!(recv_ty, Type::Xml)` guard excludes
            // XmlDocument — its `.find()` returns `Result<&XmlElement,
            // XmlError>` (not `Option<regex::Match>`), so the
            // `.map(|m| m.as_str().to_string())` shape is wrong for
            // it. The Xml-specific arm (`M::Find if matches!(recv_ty,
            // Type::Xml)` below) handles XmlDocument via
            // `XmlDocument::find(&recv, &arg).ok().cloned()`. Without
            // this guard the Xml arm would be shadowed (unreachable).
            M::Find if !matches!(recv_ty, Type::Xml) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "find() expects exactly 1 arg (the text to search), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let text_ref = coerce_str_arg_to_ref(text, &args[0]);
                let find_call = method_call_one_arg(recv, "find", text_ref);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #find_call.map(|m| m.as_str().to_string())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Regex.find codegen parse: {e}")))
            }
            // `regex.replace(text, repl)` -> String. Lowers to
            // `recv.replace_all(text, repl).to_string()`.
            // `replace_all` (not `replace`) gives the "replace ALL
            // matches" semantics the task spec requires:
            // `regex.replace("a1b2","\\d","X") == "aXbX"`.
            // Guard on Type::Regex so the String.Replace arm below
            // (dispatched on Type::String) is reachable (T73).
            M::Replace if matches!(recv_ty, Type::Regex) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "replace() expects exactly 2 args (text, replacement), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let repl = self.lower_expr(&args[1])?;
                let text_ref = coerce_str_arg_to_ref(text, &args[0]);
                let repl_ref = coerce_str_arg_to_ref(repl, &args[1]);
                // recv.replace_all(text, repl).to_string()
                let mut call_args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                call_args.push(text_ref);
                call_args.push(repl_ref);
                let replace_call = SynExpr::MethodCall(syn::ExprMethodCall {
                    attrs: Vec::new(),
                    receiver: Box::new(recv),
                    dot_token: Default::default(),
                    method: Ident::new("replace_all", ProcSpan::call_site()),
                    turbofish: None,
                    paren_token: Default::default(),
                    args: call_args,
                });
                Ok(method_call_no_args(replace_call, "to_string"))
            }
            // `regex.captures(text)` -> Map<String, String>. Lowers to a
            // block expression that:
            //   1. Calls `recv.captures(text)` (returns Option<Captures>).
            //   2. Builds a `std::collections::HashMap<String, String>`.
            //   3. Iterates `caps.iter()` in INDEX order (numbered
            //      groups: "0" = full match, "1" = first group, ...).
            //   4. Iterates `recv.capture_names().flatten()` for NAMED
            //      groups (source-declaration order).
            //   5. Returns the populated map (or empty map on no match).
            //
            // DETERMINISTIC codegen: the generated Rust source is the
            // same for every Buff source with the same shape (the
            // closure + iteration structure is fixed; only the receiver
            // and text args vary). Runtime iteration order of the
            // resulting HashMap is NOT deterministic — but that's a
            // Rust HashMap property, not a codegen concern (lookups by
            // key still work regardless of iteration order). The
            // group-index iteration order IS deterministic at runtime,
            // so populating from numbered-first then named preserves
            // source-declaration order if the user dumps the map.
            M::Captures => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "captures() expects exactly 1 arg (the text to search), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let text_ref = coerce_str_arg_to_ref(text.clone(), &args[0]);
                // Bind the captures result so we can iterate it twice
                // (once for numbered, once for named). Use an explicit
                // `let __buff_caps = recv.captures(text);` to avoid
                // re-evaluating the receiver (which may have side effects).
                let caps_call = method_call_one_arg(recv.clone(), "captures", text_ref);
                // `recv.capture_names()` is needed for named-group
                // iteration. We call it on the receiver, NOT on a
                // borrow — `capture_names` takes `&self` so this works.
                let capture_names_call = method_call_no_args(recv, "capture_names");
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let __buff_caps = #caps_call;
                        let mut __buff_map: std::collections::HashMap<String, String> =
                            std::collections::HashMap::new();
                        if let Some(__buff_c) = __buff_caps {
                            for (__buff_i, __buff_opt) in __buff_c.iter().enumerate() {
                                if let Some(__buff_m) = __buff_opt {
                                    __buff_map.insert(
                                        __buff_i.to_string(),
                                        __buff_m.as_str().to_string(),
                                    );
                                }
                            }
                            for __buff_name in #capture_names_call.flatten() {
                                if let Some(__buff_m) = __buff_c.name(__buff_name) {
                                    __buff_map.insert(
                                        __buff_name.to_string(),
                                        __buff_m.as_str().to_string(),
                                    );
                                }
                            }
                        }
                        __buff_map
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Regex.captures codegen parse: {e}")))
            }
            // T124h: URL instance accessors.
            //
            // `url.scheme` -> String. Wraps `url::Url::scheme().to_string()`.
            // The `.to_string()` lifts `&str` to `String` (Buff hides
            // references from users). Zero args.
            M::Scheme => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("scheme takes no arguments, got {}", args.len()))
                    );
                }
                let scheme_call = method_call_no_args(recv, "scheme");
                Ok(method_call_no_args(scheme_call, "to_string"))
            }
            // `url.host` -> String (empty when the URL has no host - NEVER
            // panics). Wraps
            // `url::Url::host_str().unwrap_or_default().to_string()`.
            // `host_str()` returns `Option<&str>` (None when the URL has
            // no host - e.g. `mailto:` URLs); `.unwrap_or_default()`
            // yields `&str` (the `""` when None); `.to_string()` lifts to
            // owned `String`.
            M::Host => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("host takes no arguments, got {}", args.len()))
                    );
                }
                let host_call = method_call_no_args(recv, "host_str");
                let default = method_call_no_args(host_call, "unwrap_or_default");
                Ok(method_call_no_args(default, "to_string"))
            }
            // `url.path` -> String. Wraps `url::Url::path().to_string()`.
            // The `.to_string()` lifts `&str` to `String`.
            M::Path => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("path takes no arguments, got {}", args.len()))
                    );
                }
                let path_call = method_call_no_args(recv, "path");
                Ok(method_call_no_args(path_call, "to_string"))
            }
            // `url.query(key)` -> Option<String>. Wraps a block:
            // ```
            // {
            //     let __buff_key = (key).to_string();
            //     recv.query_pairs()
            //         .find(|(k, _)| *k == __buff_key)
            //         .map(|(_, v)| v.into_owned())
            // }
            // ```
            // The `to_string()` on the key normalises both `&str`
            // literals and `String` idents to owned `String` so the
            // closure's `*k == __buff_key` comparison (where `*k:
            // Cow<str>` and `__buff_key: String`) type-checks via
            // `impl PartialEq<String> for Cow<'_, str>`. `.into_owned()`
            // lifts the matched `Cow<str>` value to owned `String`.
            // Returns `None` when the key is absent (find returns None) -
            // NEVER panics.
            //
            // The block-bind is required because `key` may have side
            // effects (function call) or move semantics (variable) that
            // would otherwise be re-evaluated on every closure call.
            // Binding once to `__buff_key` makes the lookup O(1) in
            // key-construction cost.
            M::Query => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "query(key) expects exactly 1 arg (the key), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let __buff_key = (#key).to_string();
                        #recv.query_pairs()
                            .find(|(k, _)| *k == __buff_key)
                            .map(|(_, v)| v.into_owned())
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("URL.query codegen parse: {e}")))
            }
            // T124j: Path instance methods. Each lowers to a
            // fully-qualified `std::path::Path` method (Buff hides
            // references from users; the underlying Rust accessors
            // return `Option<&Path>` / `Option<&OsStr>` / `Option<&OsStr>`
            // / `bool`).
            //
            // `path.parent()` -> Option<Path>. Wraps
            // `recv.parent().map(|p| p.to_path_buf())`. The
            // `.to_path_buf()` lifts `&Path` to owned `PathBuf`
            // (Buff surfaces owned values). Zero args. Returns None
            // when the path has no parent (e.g. `/` or a bare
            // filename) - NEVER panics.
            M::Parent => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("parent() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.parent().map(|p| p.to_path_buf())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Path.parent codegen parse: {e}")))
            }
            // `path.extension()` -> Option<String>. Wraps
            // `recv.extension().map(|e| e.to_string())`. The
            // `.to_string()` lifts `&OsStr` to owned `String` (may
            // panic if the OsStr is non-UTF-8 - but std's
            // `OsStr::to_string` (via Display) is lossy-panic-free,
            // it returns the replacement char for non-UTF-8 bytes,
            // matching Buff's "no panicking generated code" rule).
            // Zero args. Returns None when there's no extension.
            M::Extension => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "extension() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.extension().map(|e| e.to_string())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Path.extension codegen parse: {e}")))
            }
            // `path.basename()` -> String. Wraps `recv.file_name()
            // .and_then(|n| n.to_str()).unwrap_or_default().to_string()`.
            // The `.and_then(|n| n.to_str())` handles non-UTF-8
            // filenames lossy-ly (returns None - which falls through
            // to the empty String default - rather than panicking).
            // Zero args. Empty String when the path terminates in
            // `..` or `/` (file_name returns None for those).
            M::Basename => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "basename() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default()
                        .to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Path.basename codegen parse: {e}")))
            }
            // `path.exists()` -> Bool. Wraps `recv.exists()` (the
            // underlying std method is infallible - returns `false`
            // on permission errors, never panics). Zero args.
            M::Exists => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("exists() takes no arguments, got {}", args.len())));
                }
                Ok(method_call_no_args(recv, "exists"))
            }
            // T124l: Process instance methods. Each lowers to a
            // fully-qualified `std::process::Child` method chained
            // through the `Option<Child>` wrapper the codegen adds
            // at spawn time (`Process.spawn` -> `Command::spawn().ok()`).
            // The Option-wrapper layer keeps the calls panic-free
            // even when spawn failed - the Option collapses to a
            // default Int (0) via `.map(...).unwrap_or_default()`.
            //
            // `process.wait() -> Int`. Wraps
            // `recv.map(|mut c| c.wait().map(|s| s.code()
            // .unwrap_or_default()).unwrap_or_default())
            // .unwrap_or_default()` (the outer Option handles the
            // spawn-failed case; the middle Result handles wait()
            // failure; the inner Option handles signal-terminated
            // processes that have no exit code - all collapse to
            // `0` via `unwrap_or_default()`, NEVER panics). Zero
            // args. The `mut c` binding is required because
            // `Child::wait` takes `&mut self`.
            M::Wait => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("wait() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv
                        .map(|mut c| {
                            c.wait()
                                .map(|s| s.code().unwrap_or_default())
                                .unwrap_or_default()
                        })
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Process.wait codegen parse: {e}")))
            }
            // `process.id() -> Int`. Wraps
            // `recv.map(|c| c.id() as i64).unwrap_or_default()`
            // (0 when the spawn failed or the process has already
            // exited and been reaped - NEVER panics). Zero args.
            // The `as i64` cast widens Rust's `u32` pid to Buff's
            // default `Int<64>` width.
            M::Id => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("id() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv
                        .map(|c| c.id() as i64)
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Process.id codegen parse: {e}")))
            }
            // T124m: TCP-Connection / UDP-Socket / WebSocket-
            // WsConnection instance methods. Each lowers to a
            // fully-qualified tokio / futures-util async method
            // chained through the `Option<...>` wrapper the
            // codegen adds at connect / bind time. The Option-
            // wrapper layer keeps the calls panic-free even when
            // connect / bind failed - the Option's None branch is
            // a no-op (send / close), an empty Vec (recv), or an
            // empty String (ws.recv).
            //
            // All networking instance methods emit `.await` per
            // the tokio / futures-util async API. Buff has NO
            // `await` keyword - the `.await` is purely a codegen
            // concern, snapshot-verified only (single-file `buff
            // run` rustc path does NOT link tokio; the T31 async
            // walker propagates async-ness ONLY through bare-Ident
            // free-fn calls, NOT method-call / namespace-assoc-fn
            // calls, so the enclosing-fn-async transformation is
            // a deferral; see issues.md).
            //
            // Connection.send(data) -> Void. Wraps
            // `{ use tokio::io::AsyncWriteExt; if let Some(mut s)
            // = recv { s.write_all(d.as_bytes()).await.ok(); } }`
            // (block-scoped trait import; `.ok()` discards the
            // write result; Option None branch is a no-op - NEVER
            // panics). One arg (String).
            M::Send if matches!(recv_ty, Type::Connection) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "send() expects exactly 1 arg (the data String), got {}",
                        args.len()
                    )));
                }
                let data = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use tokio::io::AsyncWriteExt;
                        if let Some(mut s) = #recv {
                            s.write_all(#data.as_bytes()).await.ok();
                        }
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Connection.send codegen parse: {e}")))
            }
            // Connection.recv() -> Vector<Byte>. Wraps
            // `{ use tokio::io::AsyncReadExt; let mut buf =
            // Vec::new(); if let Some(mut s) = recv { let _ =
            // s.read(&mut buf).await; } buf }` (returns empty Vec
            // on EOF / error / connect-failed - NEVER panics).
            // Zero args. Returns Vec<u8>.
            M::Recv if matches!(recv_ty, Type::Connection) => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("recv() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use tokio::io::AsyncReadExt;
                        let mut buf: Vec<u8> = Vec::new();
                        if let Some(mut s) = #recv {
                            let _ = s.read(&mut buf).await;
                        }
                        buf
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Connection.recv codegen parse: {e}")))
            }
            // Connection.close() -> Void. Wraps
            // `{ use tokio::io::AsyncWriteExt; if let Some(mut s)
            // = recv { s.shutdown().await.ok(); } }` (graceful
            // shutdown of the write side; Option None branch is a
            // no-op - NEVER panics). Zero args. Same `Close`
            // variant dispatched on WsConnection (different
            // lowering - SinkExt::close).
            M::Close if matches!(recv_ty, Type::Connection) => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("close() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use tokio::io::AsyncWriteExt;
                        if let Some(mut s) = #recv {
                            s.shutdown().await.ok();
                        }
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Connection.close codegen parse: {e}")))
            }
            // Socket.send_to(data, addr) -> Void. Wraps
            // `{ if let Some(s) = recv { s.send_to(d.as_bytes(),
            // a).await.ok(); } }` (Option None branch is a no-op -
            // NEVER panics). Two args (String data, String addr).
            M::SendTo => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "send_to() expects exactly 2 args (data, addr), got {}",
                        args.len()
                    )));
                }
                let data = self.lower_expr(&args[0])?;
                let addr = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        if let Some(s) = #recv {
                            s.send_to(#data.as_bytes(), #addr).await.ok();
                        }
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Socket.send_to codegen parse: {e}")))
            }
            // Socket.recv_from() -> Tuple. Returns
            // `(Vector<Byte>, String)` (datagram bytes + sender
            // addr). Wraps `{ let mut buf = vec![0u8; 65535]; if
            // let Some(s) = recv { return s.recv_from(&mut buf)
            // .await.ok().map(|(n, addr)| (buf[..n].to_vec(),
            // addr.to_string())); } (Vec::new(), String::new()) }`
            // (returns empty tuple on connect-failed / recv error
            // - NEVER panics). Zero args. The 65535 buffer size is
            // the max UDP datagram payload.
            M::RecvFrom => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "recv_from() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let mut buf = vec![0u8; 65535];
                        if let Some(s) = #recv {
                            return s
                                .recv_from(&mut buf)
                                .await
                                .ok()
                                .map(|(n, addr)| (buf[..n].to_vec(), addr.to_string()));
                        }
                        (Vec::new(), String::new())
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Socket.recv_from codegen parse: {e}")))
            }
            // WsConnection.send(text) -> Void. Wraps
            // `{ use futures_util::SinkExt; if let Some(mut s) =
            // recv { s.send(tokio_tungstenite::tungstenite::
            // Message::Text(t)).await.ok(); } }` (block-scoped
            // trait import; `.ok()` discards the send result;
            // Option None branch is a no-op - NEVER panics). One
            // arg (String text). Same `Send` variant as
            // Connection.send (TCP); dispatched on the
            // (WsConnection, Send) pair.
            M::Send if matches!(recv_ty, Type::WsConnection) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "send() expects exactly 1 arg (the text String), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use futures_util::SinkExt;
                        if let Some(mut s) = #recv {
                            s.send(tokio_tungstenite::tungstenite::Message::Text(#text))
                                .await
                                .ok();
                        }
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("WsConnection.send codegen parse: {e}")))
            }
            // WsConnection.recv() -> String. Wraps
            // `{ use futures_util::StreamExt; if let Some(mut s)
            // = recv { while let Some(Ok(msg)) = s.next().await {
            // if let tokio_tungstenite::tungstenite::Message::
            // Text(t) = msg { return t; } } } String::new() }`
            // (returns empty String on connect-failed / closed /
            // non-text message - NEVER panics). Zero args. The
            // while loop drains non-text frames (Binary / Ping /
            // Pong) until a Text frame arrives or the stream
            // closes; the explicit `return` exits the block on
            // the first Text frame. Distinct from Connection.recv
            // (TCP) which returns Vector<Byte>.
            M::Recv if matches!(recv_ty, Type::WsConnection) => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("recv() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use futures_util::StreamExt;
                        if let Some(mut s) = #recv {
                            while let Some(Ok(msg)) = s.next().await {
                                if let tokio_tungstenite::tungstenite::Message::Text(t) = msg {
                                    return t;
                                }
                            }
                        }
                        String::new()
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("WsConnection.recv codegen parse: {e}")))
            }
            // WsConnection.close() -> Void. Wraps
            // `{ use futures_util::SinkExt; if let Some(mut s) =
            // recv { s.close(None).await.ok(); } }` (sends a Close
            // frame; Option None branch is a no-op - NEVER
            // panics). Zero args. Same `Close` variant as
            // Connection.close (TCP); dispatched on the
            // (WsConnection, Close) pair (different lowering -
            // SinkExt::close vs AsyncWriteExt::shutdown).
            M::Close if matches!(recv_ty, Type::WsConnection) => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("close() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use futures_util::SinkExt;
                        if let Some(mut s) = #recv {
                            s.close(None).await.ok();
                        }
                    }
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("WsConnection.close codegen parse: {e}"))
                })
            }
            // T2: Channel-Sender.send(value) -> Void. Wraps
            // `runtime_sender.send(value).await.ok()` (the .ok()
            // discards the Result<(), RuntimeError>; the user-facing
            // surface is Void in MVP - v1.18+ may surface the
            // Result). One arg (the value to send). Auto-await per
            // T31 - the codegen emits .await at the call site.
            M::Send if matches!(recv_ty, Type::Sender) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "send() expects exactly 1 arg (the value to send), got {}",
                        args.len()
                    )));
                }
                let value = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        #recv.send(#value).await.ok();
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Sender.send codegen parse: {e}")))
            }
            // T2: Channel-Receiver.recv() -> Option<T>. Wraps
            // `runtime_receiver.recv().await` (returns Option<T>:
            // Some(value) when a value arrives; None when all senders
            // were dropped - the canonical channel-closed semantic).
            // Zero args. Auto-await per T31.
            M::Recv if matches!(recv_ty, Type::Receiver) => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("recv() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.recv().await
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Receiver.recv codegen parse: {e}")))
            }
            // T2: Channel-Receiver.close() -> Void. Wraps
            // `runtime_receiver.close()` (sync - NOT async; returns
            // immediately after marking the receiver closed). Zero
            // args. Idempotent (mirrors tokio mpsc::Receiver::close).
            M::Close if matches!(recv_ty, Type::Receiver) => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("close() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        #recv.close();
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Receiver.close codegen parse: {e}")))
            }
            // T124m: Send / Recv / Close on a non-(Connection /
            // WsConnection / Sender / Receiver / SmtpClient /
            // ContractMethod) receiver type fall through to a clear
            // error (mirrors the unreachable defensive arm in
            // lower_prelude_type_assoc_fn). The registry's
            // instance_fn_lookup already rejected the (type,
            // method) pair before reaching this point; this arm
            // is the safety net for future runtime-value types
            // that might also expose `send` / `recv` / `close`
            // methods. T42 (SmtpClient.send) + T48
            // (ContractMethod.send) have their own dedicated arms
            // LATER in this match — the guard below excludes them
            // from this wildcard so they reach their dedicated
            // arms (without the guard, this wildcard would catch
            // them first as dead code).
            M::Send | M::Recv | M::Close
                if !matches!(
                    recv_ty,
                    Type::SmtpClient | Type::ContractMethod
                ) =>
            {
                Err(self.unsupported(&format!(
                    "{recv_ty}.{:?}() is not a recognised prelude instance method",
                    pmethod
                )))
            }
            // T7: DataFrame instance methods. Each chainable method
            // returns `buff_dataframe::DataFrame` so the user can
            // chain `df.select(cols).filter(pred).head(10)`. The
            // codegen records `buff-dataframe` in extern_crates via
            // the narrow `program_uses_namespace("DataFrame")` walker
            // (matches the buff-image / buff-audio precedent). All
            // DataFrame methods panic-free at the codegen layer via
            // `unwrap_or_default()` (DataFrame impls Default as the
            // empty frame).
            //
            // `df.select(cols)` -> DataFrame. One arg (Vector<String>
            // of column names). The codegen splat-converts the Vec
            // to a `&[&str]` slice via `.iter().map(|s| s.as_str())
            // .collect::<Vec<&str>>()` so the buff_dataframe API
            // takes the slice directly.
            M::Select if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "select() expects exactly 1 arg (Vector<String> of column names), got {}",
                        args.len()
                    )));
                }
                let cols = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let __cols: Vec<String> = #cols;
                        let __slice: Vec<&str> = __cols.iter().map(|s| s.as_str()).collect();
                        #recv.select(&__slice).unwrap_or_default()
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.select codegen parse: {e}")))
            }
            // `df.filter(predicate)` -> DataFrame. One arg (a lambda
            // `|row| -> Bool`). The codegen passes the user closure
            // directly to `buff_dataframe::DataFrame::filter`, which
            // expects `Fn(&RowView<'_>) -> bool`. The user closure's
            // param is a RowView — Buff's codegen surfaces RowView as
            // an opaque value (the user writes `row.get_int("age")`
            // etc.; the codegen passes &RowView by reference).
            M::Filter if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "filter() expects exactly 1 arg (a closure predicate), got {}",
                        args.len()
                    )));
                }
                let pred = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.filter(|__row| (#pred)(__row)).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.filter codegen parse: {e}")))
            }
            // `df.sort(col)` -> DataFrame. One arg (String column
            // name). Ascending lexicographic sort by the column's
            // cells.
            M::Sort if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "sort() expects exactly 1 arg (the column name), got {}",
                        args.len()
                    )));
                }
                let col = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.sort(#col.as_str()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.sort codegen parse: {e}")))
            }
            // `df.head(n)` -> DataFrame. One arg (Int). First n rows.
            M::Head if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "head() expects exactly 1 arg (the row count), got {}",
                        args.len()
                    )));
                }
                let n = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.head((#n).max(0) as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.head codegen parse: {e}")))
            }
            // `df.len()` -> Int. Zero args. Row count.
            M::Len if matches!(recv_ty, Type::DataFrame) => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("len() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.len() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.len codegen parse: {e}")))
            }
            // `df.join(other, on)` -> DataFrame. Two args (DataFrame
            // other, String on-column). Inner equi-join.
            M::Join if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "join() expects exactly 2 args (other DataFrame, on-column), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let on = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.join(&#other, #on.as_str()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.join codegen parse: {e}")))
            }
            // `df.group_by(col)` -> DataFrame. One arg (String column
            // name). Returns a DataFrame (the GroupBy intermediate is
            // collapsed into a DataFrame via `into_inner()` so
            // subsequent `.agg(...)` calls dispatch on the DataFrame
            // receiver — a true GroupBy intermediate type would
            // require a second Type variant + display arm + codegen
            // path; deferred to v1.18+).
            M::GroupBy if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "group_by() expects exactly 1 arg (the column name), got {}",
                        args.len()
                    )));
                }
                let col = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.group_by(#col.as_str())
                        .map(|__gb| __gb.into_df())
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.group_by codegen parse: {e}")))
            }
            // `df.agg(col, op)` -> DataFrame. Two args (String column
            // name, String aggregation op — "sum"/"mean"/"min"/"max"/
            // "count"). Returns a per-group aggregate DataFrame.
            M::Agg if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "agg() expects exactly 2 args (column name, op string), got {}",
                        args.len()
                    )));
                }
                let col = self.lower_expr(&args[0])?;
                let op = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let __col: String = #col;
                        let __op_str: String = #op;
                        let __op = buff_dataframe::AggOp::parse(__op_str.as_str())
                            .unwrap_or(buff_dataframe::AggOp::Count);
                        #recv.agg(__col.as_str(), __op)
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.agg codegen parse: {e}")))
            }
            // `df.to_table_string()` -> String. Zero args. Fixed-width
            // pretty-printer. Infallible (no `unwrap_or_default()` wrap
            // needed — `to_table_string` returns String directly).
            M::ToTableString if matches!(recv_ty, Type::DataFrame) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_table_string() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.to_table_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.to_table_string codegen parse: {e}")))
            }
            // Non-DataFrame receiver with a DataFrame-only method
            // (Select/Filter/Sort/Head/GroupBy/Agg/ToTableString) falls
            // through to a clear error (mirrors the Send/Recv/Close
            // safety net). T43 excludes Type::Document / Type::Element
            // from the M::Select catch-all so the buff-scrape arms
            // below fire first (mirrors how the `M::Len` arm excludes
            // Type::Cache).
            M::Select if !matches!(recv_ty, Type::Document | Type::Element)
                => Err(self.unsupported(&format!(
                    "{recv_ty}.select() is not a recognised prelude instance method",
                ))),
            M::Filter | M::Sort | M::Head | M::GroupBy | M::Agg | M::ToTableString => {
                Err(self.unsupported(&format!(
                    "{recv_ty}.{:?}() is not a recognised prelude instance method",
                    pmethod
                )))
            }
            // `Len` is shared between DataFrame.len (above) and
            // future Vector.len / Map.len / Series.len / Cache.len —
            // dispatched on receiver type. Non-DataFrame / non-Cache
            // receivers fall through to the existing method-resolution
            // path. T31 (Cache) arm lives below; the guard on this
            // arm skips Cache so the Cache-specific arm fires first.
            M::Len if !matches!(recv_ty, Type::Cache | Type::String) => Err(self.unsupported(&format!(
                "{recv_ty}.len() is not a recognised prelude instance method",
            ))),
            // T9: Image instance methods. Each filter returning a new
            // Image (grayscale / resize / crop / blur) lowers to
            // `buff_image::Image::<method>` and is panic-free via
            // `unwrap_or_default()` (Image impls Default as a 1x1
            // transparent pixel — added in the same T9 finish commit
            // as this codegen arm). The codegen records `buff-image`
            // + `image` in extern_crates via the
            // `program_uses_namespace("Image")` walker.
            //
            // `img.width()` -> Int. Zero args. Wraps `recv.width() as
            // i64` (the `as i64` lifts u32 to Buff's Int width).
            M::Width if matches!(recv_ty, Type::Image) => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("width() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.width() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.width codegen parse: {e}")))
            }
            // `img.height()` -> Int. Zero args. Wraps `recv.height()
            // as i64`.
            M::Height if matches!(recv_ty, Type::Image) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "height() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.height() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.height codegen parse: {e}")))
            }
            // `img.pixel_format()` -> PixelFormat. Zero args. Wraps
            // `recv.format()` (renamed on the Buff surface to avoid a
            // clash with DateTime.format — distinct variant, distinct
            // semantics). Returns Type::Unknown at the type-checker
            // layer; codegen emits the bare call and Rust infers
            // `buff_image::PixelFormat`.
            M::PixelFormat if matches!(recv_ty, Type::Image) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "pixel_format() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.format()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.pixel_format codegen parse: {e}")))
            }
            // `img.get_pixel(x, y)` -> Color. Two args. Bounds-
            // checked; the codegen lowers to `recv.get_pixel(x as u32,
            // y as u32).unwrap_or_default()` (Color impls Default as
            // black — panic-free on out-of-bounds coords).
            M::GetPixel if matches!(recv_ty, Type::Image) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "get_pixel() expects exactly 2 args (x, y), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let y = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.get_pixel(#x as u32, #y as u32).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.get_pixel codegen parse: {e}")))
            }
            // `img.set_pixel(x, y, color)` -> Void. Three args. In-
            // place mutation. Panic-free via `unwrap_or_default()` (()
            // impls Default — out-of-bounds coords are a no-op).
            M::SetPixel if matches!(recv_ty, Type::Image) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "set_pixel() expects exactly 3 args (x, y, color), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let y = self.lower_expr(&args[1])?;
                let color = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.set_pixel(#x as u32, #y as u32, #color).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.set_pixel codegen parse: {e}")))
            }
            // `img.grayscale()` -> Image. Zero args. Consumes self,
            // returns a new Image. Infallible (no `unwrap_or_default`
            // wrap needed — `grayscale` returns Image directly).
            M::Grayscale if matches!(recv_ty, Type::Image) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "grayscale() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.grayscale()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.grayscale codegen parse: {e}")))
            }
            // `img.invert()` -> Void. Zero args. In-place channel
            // inversion. Infallible.
            M::Invert if matches!(recv_ty, Type::Image) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "invert() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.invert()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.invert codegen parse: {e}")))
            }
            // `img.resize(w, h)` -> Image. Two args (Int w, Int h).
            // Lanczos3 resize. Panic-free via `unwrap_or_default()`
            // (Image impls Default — invalid dims collapse to 1x1).
            M::Resize if matches!(recv_ty, Type::Image) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "resize() expects exactly 2 args (w, h), got {}",
                        args.len()
                    )));
                }
                let w = self.lower_expr(&args[0])?;
                let h = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.resize(#w as u32, #h as u32).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.resize codegen parse: {e}")))
            }
            // `img.crop(x, y, w, h)` -> Image. Four args. Bounds-
            // checked. Panic-free via `unwrap_or_default()`.
            M::Crop if matches!(recv_ty, Type::Image) => {
                if args.len() != 4 {
                    return Err(self.unsupported(&format!(
                        "crop() expects exactly 4 args (x, y, w, h), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let y = self.lower_expr(&args[1])?;
                let w = self.lower_expr(&args[2])?;
                let h = self.lower_expr(&args[3])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.crop(#x as u32, #y as u32, #w as u32, #h as u32).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.crop codegen parse: {e}")))
            }
            // `img.blur(sigma)` -> Image. One arg (Float). Gaussian
            // blur. Infallible (returns Image directly).
            M::Blur if matches!(recv_ty, Type::Image) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "blur() expects exactly 1 arg (sigma), got {}",
                        args.len()
                    )));
                }
                let sigma = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.blur(#sigma as f32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.blur codegen parse: {e}")))
            }
            // T45: buff-geo instance methods. Each lowers to the
            // matching `buff_geo::{Point, LineString, Polygon}` method.
            // All infallible at the codegen layer (the wrapper crate's
            // methods return f64 / bool directly — no unwrap_or_default
            // needed). Records `buff-geo` + `geo` + `geo-types` in
            // extern_crates via the `program_uses_namespace("Point")` /
            // ("LineString") / ("Polygon") walkers.
            //
            // `point.x()` -> Float. Zero args. Wraps `recv.x()`.
            M::X if matches!(recv_ty, Type::Point) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "x() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.x()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Point.x codegen parse: {e}")))
            }
            // `point.y()` -> Float. Zero args. Wraps `recv.y()`.
            M::Y if matches!(recv_ty, Type::Point) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "y() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.y()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Point.y codegen parse: {e}")))
            }
            // `point.distance_to(other)` -> Float. One arg (Point).
            // Wraps `recv.distance_to(other)`.
            M::DistanceTo if matches!(recv_ty, Type::Point) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "distance_to() expects exactly 1 arg (other Point), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.distance_to(#other)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Point.distance_to codegen parse: {e}")))
            }
            // `line_string.length()` -> Float. Zero args. Wraps
            // `recv.length()`.
            M::Length if matches!(recv_ty, Type::LineString) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "length() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.length()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("LineString.length codegen parse: {e}"))
                })
            }
            // `polygon.area()` -> Float. Zero args. Wraps `recv.area()`.
            M::Area if matches!(recv_ty, Type::Polygon) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "area() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.area()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Polygon.area codegen parse: {e}")))
            }
            // `polygon.contains(point)` -> Bool. One arg (Point).
            // Wraps `recv.contains(point)`. Shared `Contains` variant —
            // dispatched on (Polygon, Contains) pair (same variant as
            // Cache.contains, different receiver type).
            M::Contains if matches!(recv_ty, Type::Polygon) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "contains() expects exactly 1 arg (Point), got {}",
                        args.len()
                    )));
                }
                let pt = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.contains(#pt)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Polygon.contains codegen parse: {e}")))
            }
            // `polygon.intersects(other)` -> Bool. One arg (Polygon).
            // Wraps `recv.intersects(&other)` (panic-free via
            // catch_unwind inside the wrapper per FFI guide R6).
            M::Intersects if matches!(recv_ty, Type::Polygon) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "intersects() expects exactly 1 arg (other Polygon), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.intersects(&#other)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Polygon.intersects codegen parse: {e}"))
                })
            }
            // T54: buff-simd instance methods. Each lowers to the
            // matching `buff_simd::Simd` method. The 4 lane-wise binary
            // ops (add/sub/mul/div) each take one Simd arg and return
            // Simd. The 3 horizontal reductions (sum/min/max) take no
            // args and return f32. The extract (to_vec) takes no args
            // and returns Vec<f32>. All infallible at the codegen layer
            // (the wrapper crate's methods return Simd / f32 / Vec<f32>
            // directly — no unwrap_or_default needed). Records
            // `buff-simd` + `wide` in extern_crates via the
            // `program_uses_namespace("Simd")` walker.
            //
            // T89: Decimal instance methods. Dispatched on
            // (Type::Decimal, variant) pairs. Reuses the existing
            // Add / Mul / ToString variants (shared with Simd / Xml).
            //
            // `d.add(other)` -> Decimal. One arg (Decimal). Wraps
            // `recv + other` (Rust's `Add` trait on `rust_decimal::Decimal`).
            M::Add if matches!(recv_ty, Type::Decimal) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "add() expects exactly 1 arg (other Decimal), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv + #other
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Decimal.add codegen parse: {e}")))
            }
            // `d.mul(other)` -> Decimal. One arg (Decimal). Wraps
            // `recv * other` (Rust's `Mul` trait on `rust_decimal::Decimal`).
            M::Mul if matches!(recv_ty, Type::Decimal) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "mul() expects exactly 1 arg (other Decimal), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv * #other
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Decimal.mul codegen parse: {e}")))
            }
            // `d.to_string()` -> String. Zero args. Wraps
            // `recv.to_string()` (rust_decimal::Decimal implements Display).
            M::ToString if matches!(recv_ty, Type::Decimal) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_string() takes no arguments, got {}",
                        args.len()
                    )));
                }
                Ok(method_call_no_args(recv, "to_string"))
            }
            // `simd.add(other)` -> Simd. One arg (Simd). Wraps
            // `recv.add(other)`.
            M::Add if matches!(recv_ty, Type::Simd) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "add() expects exactly 1 arg (other Simd), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.add(#other)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.add codegen parse: {e}")))
            }
            // `simd.sub(other)` -> Simd. One arg (Simd). Wraps
            // `recv.sub(other)`.
            M::Sub if matches!(recv_ty, Type::Simd) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "sub() expects exactly 1 arg (other Simd), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.sub(#other)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.sub codegen parse: {e}")))
            }
            // `simd.mul(other)` -> Simd. One arg (Simd). Wraps
            // `recv.mul(other)`.
            M::Mul if matches!(recv_ty, Type::Simd) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "mul() expects exactly 1 arg (other Simd), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.mul(#other)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.mul codegen parse: {e}")))
            }
            // `simd.div(other)` -> Simd. One arg (Simd). Wraps
            // `recv.div(other)`.
            M::Div if matches!(recv_ty, Type::Simd) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "div() expects exactly 1 arg (other Simd), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.div(#other)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.div codegen parse: {e}")))
            }
            // `simd.sum()` -> Float. Zero args. Wraps `recv.sum()`.
            M::Sum if matches!(recv_ty, Type::Simd) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "sum() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.sum()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.sum codegen parse: {e}")))
            }
            // `simd.min()` -> Float. Zero args. Wraps `recv.min()`.
            M::Min if matches!(recv_ty, Type::Simd) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "min() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.min()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.min codegen parse: {e}")))
            }
            // `simd.max()` -> Float. Zero args. Wraps `recv.max()`.
            M::Max if matches!(recv_ty, Type::Simd) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "max() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.max()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.max codegen parse: {e}")))
            }
            // `simd.to_vec()` -> Vector<Float>. Zero args. Wraps
            // `recv.to_vec()`.
            M::ToVec if matches!(recv_ty, Type::Simd) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_vec() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.to_vec()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.to_vec codegen parse: {e}")))
            }
            // T46: buff-nlp Language instance methods. Both infallible
            // — the `buff_nlp::Language::code` / `name` methods return
            // owned String directly (cloning the inner `&'static str`
            // per FFI guide R5). Records `buff-nlp` + `whatlang` +
            // `rust-stemmers` + `unicode-segmentation` in extern_crates
            // via the `program_uses_namespace("Text")` walker (the
            // walker fires on Text.* calls; Language values arise only
            // as Text.detect_language return values, so a program that
            // uses lang.code() always also uses Text.* — the walker
            // registers buff-nlp correctly either way).
            //
            // `language.code()` -> String (ISO 639-3). Zero args.
            M::Code if matches!(recv_ty, Type::Language) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "code() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.code()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Language.code codegen parse: {e}")))
            }
            // `language.name()` -> String (English name). Zero args.
            // `Name` is shared with `faker.name()` (Faker arm below) —
            // dispatched on receiver type.
            M::Name if matches!(recv_ty, Type::Language) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "name() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.name()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Language.name codegen parse: {e}")))
            }
            // T52: buff-protobuf Message instance methods. All
            // infallible — `buff_protobuf::Message::byte_size` /
            // `type_url` / `encode` return `usize` / `&str` / `&[u8]`
            // directly (no Result wrapper). `payload` is fallible in
            // Rust (returns `Result<Value, ProtobufError>`) so the
            // codegen wraps with `.unwrap_or_default()` (Value::Null
            // on decode failure — panic-free via
            // `.unwrap_or_default()`, NOT bare `.unwrap()`). Records
            // `buff-protobuf` + `prost` + `prost-types` + `serde_json`
            // in extern_crates via the
            // `program_uses_namespace("Message")` walker.
            //
            // `message.byte_size()` -> Int. Zero args. Wraps
            // `recv.byte_size() as i64` (the underlying Rust method
            // returns `usize`; the cast lifts to Buff's `Int<64>`).
            M::ByteSize if matches!(recv_ty, Type::Message) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "byte_size() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.byte_size() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.byte_size codegen parse: {e}")))
            }
            // `message.type_url()` -> String. Zero args. Wraps
            // `recv.type_url().to_string()` (the underlying Rust
            // method returns `&str`; the `.to_string()` lifts to
            // owned String per FFI guide R2 — Buff surfaces owned
            // values).
            M::TypeUrl if matches!(recv_ty, Type::Message) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "type_url() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.type_url().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.type_url codegen parse: {e}")))
            }
            // `message.payload()` -> Value. Zero args. Wraps
            // `recv.payload().unwrap_or_default()` (Value::Null on
            // decode failure — panic-free via `.unwrap_or_default()`,
            // NOT bare `.unwrap()`).
            M::Payload if matches!(recv_ty, Type::Message) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "payload() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.payload().unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.payload codegen parse: {e}")))
            }
            // `message.encode()` -> Vector<Byte>. Zero args. Wraps
            // `recv.encode().to_vec()` (the underlying Rust method
            // returns `&[u8]`; the `.to_vec()` lifts to owned `Vec<u8>`
            // per FFI guide R2 — Buff surfaces owned values). Distinct
            // from `PreludeAssocFn::Encode` (the Base64.encode /
            // Hex.encode *associated-function* shape) — this Encode is
            // an *instance method* on a Message value (different enum,
            // different dispatch table).
            M::Encode if matches!(recv_ty, Type::Message) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "encode() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.encode().to_vec()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.encode codegen parse: {e}")))
            }
            // T47: buff-chat Bot instance methods. The Bot wrapper's
            // methods all panic-free via `.unwrap_or(())` (command /
            // on_message / start / stop / dispatch — return Result in
            // Rust but collapse to Void at the Buff surface per FFI
            // guide R3) or infallible directly (is_running /
            // command_count / has_message_handler / platform — return
            // bool / usize / Platform directly). Records `buff-chat` +
            // `serenity` + `teloxide` + `async-trait` + `tokio` in
            // extern_crates via the `program_uses_namespace("Bot")`
            // walker.
            //
            // `bot.command(name, handler)` -> Void. Two args (String
            // name, closure handler). The closure is spliced directly
            // — Rust coerces `|msg| ...` to the `F: Fn(Message) +
            // Send + Sync + 'static` bound on `Bot::command` (no
            // Arc::new / Box::new wrap needed; the wrapper ctor does
            // the Arc-sharing internally). `.unwrap_or(())` collapses
            // ChatError::EmptyCommandName / DuplicateCommand to Void
            // (silently swallowed at the Buff surface).
            M::Command if matches!(recv_ty, Type::Bot) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "command() expects exactly 2 args (name, handler), got {}",
                        args.len()
                    )));
                }
                let name = self.lower_expr(&args[0])?;
                let handler = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.command(&(#name).to_string(), move |msg| #handler).unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.command codegen parse: {e}")))
            }
            // `bot.on_message(handler)` -> Void. One arg (closure
            // handler). Same closure-splice shape as `command` minus
            // the name arg. `.unwrap_or(())` collapses registration
            // failure to Void.
            M::OnMessage if matches!(recv_ty, Type::Bot) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "on_message() expects exactly 1 arg (handler), got {}",
                        args.len()
                    )));
                }
                let handler = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.on_message(move |msg| #handler).unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.on_message codegen parse: {e}")))
            }
            // `bot.start()` -> Void. Zero args. Blocks on the platform
            // event loop. `.unwrap_or(())` collapses ChatError to Void
            // (AlreadyRunning / AlreadyInRuntime / Connect / Runtime).
            M::Start if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "start() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.start().unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.start codegen parse: {e}")))
            }
            // `bot.stop()` -> Void. Zero args. Cooperative shutdown
            // (AtomicBool flag). `.unwrap_or(())` collapses
            // ChatError::NotRunning to Void.
            M::Stop if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "stop() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.stop().unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.stop codegen parse: {e}")))
            }
            // `bot.dispatch(msg)` -> Void. One arg (ChatMessage). The
            // public testing entry — exercises the handler routing
            // without a live network connection (T47 "mock API").
            M::Dispatch if matches!(recv_ty, Type::Bot) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "dispatch() expects exactly 1 arg (Message), got {}",
                        args.len()
                    )));
                }
                let msg = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.dispatch(#msg).unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.dispatch codegen parse: {e}")))
            }
            // `bot.is_running()` -> Bool. Zero args. Infallible (the
            // wrapper returns false on poisoned lock, never panics).
            M::IsRunning if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "is_running() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.is_running()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.is_running codegen parse: {e}")))
            }
            // `bot.command_count()` -> Int. Zero args. The underlying
            // Rust method returns `usize`; the `as i64` lifts to
            // Buff's `Int<64>`.
            M::CommandCount if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "command_count() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.command_count() as i64
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Bot.command_count codegen parse: {e}"))
                })
            }
            // `bot.has_message_handler()` -> Bool. Zero args.
            M::HasMessageHandler if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "has_message_handler() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.has_message_handler()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Bot.has_message_handler codegen parse: {e}"))
                })
            }
            // `bot.platform()` -> Platform. Zero args. Infallible (Copy
            // value, never panics). Shared `Platform` variant —
            // dispatched on the (Bot, Platform) pair (same variant as
            // ChatMessage.platform, different receiver type).
            M::Platform if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "platform() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.platform()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.platform codegen parse: {e}")))
            }
            // T47: buff-chat ChatMessage instance methods. All
            // infallible — the `buff_chat::Message` methods return
            // `&str` / Platform / bool directly (the codegen wraps
            // &str returns in `.to_string()` so Buff surfaces owned
            // String values per FFI guide R2). Records `buff-chat` +
            // `serenity` + `teloxide` + `async-trait` + `tokio` in
            // extern_crates via the `program_uses_namespace
            // ("ChatMessage")` walker.
            //
            // `msg.text()` -> String. Zero args. Shared `Text`
            // variant — dispatched on the (ChatMessage, Text) pair
            // (same variant as Document / Element / XmlElement.text,
            // different receiver type).
            M::Text if matches!(recv_ty, Type::ChatMessage) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "text() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.text().to_string()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ChatMessage.text codegen parse: {e}"))
                })
            }
            // `msg.channel()` -> String. Zero args.
            M::Channel if matches!(recv_ty, Type::ChatMessage) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "channel() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.channel().to_string()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ChatMessage.channel codegen parse: {e}"))
                })
            }
            // `msg.author()` -> String. Zero args.
            M::Author if matches!(recv_ty, Type::ChatMessage) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "author() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.author().to_string()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ChatMessage.author codegen parse: {e}"))
                })
            }
            // `msg.platform()` -> Platform. Zero args. Shared
            // `Platform` variant — dispatched on the
            // (ChatMessage, Platform) pair (same variant as
            // Bot.platform, different receiver type).
            M::Platform if matches!(recv_ty, Type::ChatMessage) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "platform() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.platform()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ChatMessage.platform codegen parse: {e}"))
                })
            }
            // `msg.is_dm()` -> Bool. Zero args.
            M::IsDm if matches!(recv_ty, Type::ChatMessage) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "is_dm() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.is_dm()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ChatMessage.is_dm codegen parse: {e}"))
                })
            }
            // T47: buff-chat Platform instance methods. Both
            // infallible — the `buff_chat::Platform::is_discord` /
            // `is_telegram` methods return `bool` directly (Copy
            // value). Records `buff-chat` + `serenity` + `teloxide` +
            // `async-trait` + `tokio` in extern_crates via the
            // `program_uses_namespace("Platform")` walker.
            //
            // `platform.is_discord()` -> Bool. Zero args.
            M::IsDiscord if matches!(recv_ty, Type::Platform) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "is_discord() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.is_discord()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Platform.is_discord codegen parse: {e}"))
                })
            }
            // `platform.is_telegram()` -> Bool. Zero args.
            M::IsTelegram if matches!(recv_ty, Type::Platform) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "is_telegram() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.is_telegram()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Platform.is_telegram codegen parse: {e}"))
                })
            }
            // T37: Faker instance methods. All infallible — the
            // `buff_fake::Faker` methods return owned String / i64
            // directly (no unwrap_or_default needed). Records
            // `buff-fake` + `fake` in extern_crates via the
            // `program_uses_namespace("Faker")` walker.
            //
            // `faker.name()` -> String. Zero args.
            M::Name if matches!(recv_ty, Type::Faker) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "name() takes no arguments, got {}", args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.name()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.name codegen parse: {e}")))
            }
            // `faker.email()` -> String. Zero args.
            M::Email if matches!(recv_ty, Type::Faker) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "email() takes no arguments, got {}", args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.email()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.email codegen parse: {e}")))
            }
            // `faker.address()` -> String. Zero args.
            M::Address if matches!(recv_ty, Type::Faker) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "address() takes no arguments, got {}", args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.address()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.address codegen parse: {e}")))
            }
            // `faker.phone()` -> String. Zero args.
            M::Phone if matches!(recv_ty, Type::Faker) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "phone() takes no arguments, got {}", args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.phone()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.phone codegen parse: {e}")))
            }
            // `faker.uuid()` -> String. Zero args.
            M::Uuid if matches!(recv_ty, Type::Faker) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "uuid() takes no arguments, got {}", args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.uuid()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.uuid codegen parse: {e}")))
            }
            // `faker.lorem(words)` -> String. One arg (Int word_count).
            M::Lorem if matches!(recv_ty, Type::Faker) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "lorem() expects exactly 1 arg (word_count), got {}", args.len()
                    )));
                }
                let word_count = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.lorem(#word_count as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.lorem codegen parse: {e}")))
            }
            // `faker.int(min, max)` -> Int. Two args (Int min, Int max).
            M::FakerInt if matches!(recv_ty, Type::Faker) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "int() expects exactly 2 args (min, max), got {}", args.len()
                    )));
                }
                let min = self.lower_expr(&args[0])?;
                let max = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.int(#min, #max)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.int codegen parse: {e}")))
            }
            // `faker.datetime(start, end)` -> String. Two args (String
            // start, String end). Wraps `recv.datetime(&start, &end)
            // .unwrap_or_default()` (panic-free — empty string on error).
            M::FakerDatetime if matches!(recv_ty, Type::Faker) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "datetime() expects exactly 2 args (start, end), got {}", args.len()
                    )));
                }
                let start = self.lower_expr(&args[0])?;
                let end = self.lower_expr(&args[1])?;
                let start_ref = coerce_str_arg_to_ref(start, &args[0]);
                let end_ref = coerce_str_arg_to_ref(end, &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.datetime(#start_ref, #end_ref).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.datetime codegen parse: {e}")))
            }
            // T31: Cache instance methods. Each method lowers to
            // `buff_cache::Cache::<method>`. The codegen records
            // `buff-cache` + `moka` in extern_crates via the
            // `program_uses_namespace("Cache")` walker.
            //
            // `cache.get(key)` -> String?. One arg (String). Wraps
            // `recv.get(&key)` (returns Option<String> natively).
            M::Get if matches!(recv_ty, Type::Cache) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "get() expects exactly 1 arg (key), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.get(&#key)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.get codegen parse: {e}")))
            }
            // `cache.set(key, value)` -> Void. Two args. Wraps
            // `recv.set(key, value)` (the no-TTL overload).
            M::Set if matches!(recv_ty, Type::Cache) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "set() expects exactly 2 args (key, value), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let value = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.set(#key, #value)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.set codegen parse: {e}")))
            }
            // `cache.set(key, value, ttl)` -> Void. Three args. Wraps
            // `recv.set_with_ttl(key, value, ttl)`. The Buff surface
            // uses the same `set` method name (arity-based dispatch
            // via the codegen's arg-count check); the underlying
            // Rust method is `set_with_ttl` to avoid overload
            // ambiguity.
            M::SetTtl if matches!(recv_ty, Type::Cache) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "set(key, value, ttl) expects exactly 3 args, got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let value = self.lower_expr(&args[1])?;
                let ttl = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.set_with_ttl(#key, #value, #ttl)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.set(ttl) codegen parse: {e}")))
            }
            // `cache.delete(key)` -> Void. One arg. Wraps
            // `recv.delete(&key)`.
            M::Delete if matches!(recv_ty, Type::Cache) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "delete() expects exactly 1 arg (key), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.delete(&#key)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.delete codegen parse: {e}")))
            }
            // `cache.contains(key)` -> Bool. One arg. Wraps
            // `recv.contains(&key)`.
            M::Contains if matches!(recv_ty, Type::Cache) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "contains() expects exactly 1 arg (key), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.contains(&#key)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.contains codegen parse: {e}")))
            }
            // T84: `(0..10).contains(5)` -> Bool. One arg. Wraps
            // `recv.contains(&item)` — Rust's `std::ops::Range` /
            // `RangeInclusive` both implement `RangeBounds<T>` whose
            // `contains` takes `&T`. The receiver type is `Type::Range`
            // (the lazy integer range produced by `..` / `..=`). O(1)
            // — never materialises the range.
            M::Contains if matches!(recv_ty, Type::Range(_)) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "contains() expects exactly 1 arg (item), got {}",
                        args.len()
                    )));
                }
                let item = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.contains(&#item)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Range.contains codegen parse: {e}")))
            }
            // `cache.clear()` -> Void. Zero args. Wraps
            // `recv.clear()`.
            M::Clear if matches!(recv_ty, Type::Cache) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "clear() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.clear()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.clear codegen parse: {e}")))
            }
            // `cache.len()` -> Int. Zero args. Wraps `recv.len() as
            // i64` (the `as i64` lifts u64 to Buff's Int width).
            M::Len if matches!(recv_ty, Type::Cache) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "len() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.len() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.len codegen parse: {e}")))
            }
            // T44 MVP: I18n instance methods. AddResource / Load are
            // 2-arg / 1-arg Void methods wrapped in `.unwrap_or(())`
            // for panic-free codegen (mirrors the Cache.set / SetTtl
            // stance). Translate is a 1-arg String method. The 7
            // deferred methods (SetFallback / AvailableLocales /
            // CurrentLocale / FallbackLocale / TranslateWithArgs /
            // HasMessage / Warnings) are available on the
            // `buff_i18n::I18n` Rust type but codegen-wiring is
            // deferred to a follow-up.
            M::AddResource if matches!(recv_ty, Type::I18n) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "add_resource() expects exactly 2 args (locale, ftl), got {}",
                        args.len()
                    )));
                }
                let locale = self.lower_expr(&args[0])?;
                let ftl = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.add_resource(&#locale, &#ftl).unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("I18n.add_resource codegen parse: {e}")))
            }
            // `i18n.load(locale)` -> Void. One arg (String). Wraps
            // `recv.load(&locale).unwrap_or(())` (panic-free on
            // LocaleNotLoaded — no-op).
            M::Load if matches!(recv_ty, Type::I18n) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "load() expects exactly 1 arg (locale), got {}",
                        args.len()
                    )));
                }
                let locale = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.load(&#locale).unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("I18n.load codegen parse: {e}")))
            }
            // `i18n.translate(key)` -> String. One arg (String).
            // Wraps `recv.translate(&key)` (current → fallback → key
            // string contract — NEVER panics).
            M::Translate if matches!(recv_ty, Type::I18n) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "translate() expects exactly 1 arg (key), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.translate(&#key)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("I18n.translate codegen parse: {e}")))
            }
            // T43: buff-scrape instance methods. Each method lowers to
            // `buff_scrape::{Document, Element, Crawler}::<method>`. The
            // codegen records `buff-scrape` + `scraper` (for Document /
            // Element) or `reqwest` (for Crawler) in extern_crates via
            // the `program_uses_namespace("Document" / "Element" /
            // "Crawler")` walker. All methods are panic-free at the
            // codegen layer (Document/Element/Crawler all impl Default;
            // fallible ops lower to `unwrap_or_default()` or `?`
            // depending on whether the receiver itself is consumed).
            //
            // ---- Document instance methods (4) -----------------
            // `doc.select(css)` -> Vector<Element>. One arg (String).
            // Wraps `buff_scrape::Document::select(&recv, &css)
            // .unwrap_or_default()` (panic-free on invalid CSS — empty
            // Vec returned; mirrors the Image.from_path panic-free
            // pattern).
            M::Select if matches!(recv_ty, Type::Document) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Document::select(&#recv, &#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Document.select codegen parse: {e}")))
            }
            // `doc.text()` -> String. Zero args. Wraps `recv.text()`.
            M::Text if matches!(recv_ty, Type::Document) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "text() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.text()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Document.text codegen parse: {e}")))
            }
            // `doc.html()` -> String. Zero args. Wraps `recv.html()`.
            // Shared `Html` variant — distinct from (Email, Html)
            // (two-arg template builder).
            M::Html if matches!(recv_ty, Type::Document) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "html() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.html()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Document.html codegen parse: {e}")))
            }
            // `doc.title()` -> String?. Zero args. Wraps `recv.title()`.
            M::Title if matches!(recv_ty, Type::Document) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "title() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.title()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Document.title codegen parse: {e}")))
            }
            // ---- Element instance methods (5) -----------------
            // `el.select(css)` -> Vector<Element>. One arg (String).
            // Wraps `Element::select(&recv, &css).unwrap_or_default()`.
            M::Select if matches!(recv_ty, Type::Element) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Element::select(&#recv, &#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Element.select codegen parse: {e}")))
            }
            // `el.text()` -> String. Zero args. Wraps `recv.text()`.
            M::Text if matches!(recv_ty, Type::Element) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "text() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.text()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Element.text codegen parse: {e}")))
            }
            // `el.attr(name)` -> String?. One arg (String). Wraps
            // `recv.attr(&name)`.
            M::Attr if matches!(recv_ty, Type::Element) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.attr(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Element.attr codegen parse: {e}")))
            }
            // `el.html()` -> String. Zero args. Wraps `recv.html()`.
            M::Html if matches!(recv_ty, Type::Element) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "html() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.html()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Element.html codegen parse: {e}")))
            }
            // `el.inner_html()` -> String. Zero args. Wraps
            // `recv.inner_html()`.
            M::InnerHtml if matches!(recv_ty, Type::Element) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "inner_html() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.inner_html()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Element.inner_html codegen parse: {e}")))
            }
            // ---- Crawler instance methods (4) -----------------
            // `crawler.seed()` -> String. Zero args. Wraps `recv.seed()`.
            M::Seed if matches!(recv_ty, Type::Crawler) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "seed() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.seed()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.seed codegen parse: {e}")))
            }
            // `crawler.fetch(url)` -> Document. One arg (String URL).
            // Wraps `Crawler::fetch(&recv, &url).unwrap_or_default()`
            // (panic-free on network / HTTP error — Document impls
            // Default as `<html></html>`; matches the Image.from_path
            // `unwrap_or_default()` panic-free pattern).
            M::Fetch if matches!(recv_ty, Type::Crawler) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Crawler::fetch(&#recv, &#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.fetch codegen parse: {e}")))
            }
            // `crawler.crawl(max_pages)` -> Vector<String>. One arg
            // (Int). Wraps `Crawler::crawl(&recv, max_pages as i64)
            // .unwrap_or_default()` (panic-free on network error —
            // returns whatever was crawled before the failure).
            M::Crawl if matches!(recv_ty, Type::Crawler) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Crawler::crawl(&#recv, #arg as i64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.crawl codegen parse: {e}")))
            }
            // `crawler.robots_allows(url)` -> Bool. One arg (String URL).
            // Wraps `Crawler::robots_allows(&recv, &url)` (infallible —
            // returns `true` on robots.txt fetch failure per the Robots
            // Exclusion Protocol fail-open guidance).
            M::RobotsAllows if matches!(recv_ty, Type::Crawler) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Crawler::robots_allows(&#recv, &#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.robots_allows codegen parse: {e}")))
            }
            // T50: Xml instance methods. Each method lowers to
            // `buff_xml::XmlDocument::<method>`. The codegen records
            // `buff-xml` + `quick-xml` in extern_crates via the
            // `program_uses_namespace("Xml")` walker. All methods
            // panic-free at the codegen layer.
            //
            // `doc.root()` -> XmlDocument (opaque). Zero args.
            M::Root if matches!(recv_ty, Type::Xml) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "root() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlDocument::root(&#recv).clone()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Xml.root codegen parse: {e}")))
            }
            // `doc.find(xpath)` -> Option<XmlDocument>. One arg (String).
            M::Find if matches!(recv_ty, Type::Xml) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlDocument::find(&#recv, &#arg).ok().cloned()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Xml.find codegen parse: {e}")))
            }
            // `doc.to_string()` -> String. Zero args.
            M::ToString if matches!(recv_ty, Type::Xml) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_string() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlDocument::to_string(&#recv).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Xml.to_string codegen parse: {e}")))
            }
            // T50: XmlElement instance methods. Each method lowers to
            // the matching `buff_xml::XmlElement` method. The wrapper
            // crate's methods return `&str` / `Option<&str>` /
            // `&[XmlElement]` — the codegen lifts these to owned Buff
            // values (`String` / `Option<String>` / `Vec<XmlElement>`)
            // per FFI guide R2 (Buff surfaces only owned values).
            // Records `buff-xml` + `quick-xml` in extern_crates via
            // the `program_uses_namespace("XmlElement")` walker.
            //
            // `el.name()` -> String. Zero args. Wraps
            // `recv.name().to_string()` (lifts `&str` -> `String`).
            M::Name if matches!(recv_ty, Type::XmlElement) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "name() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.name().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.name codegen parse: {e}")))
            }
            // `el.text()` -> Option<String>. Zero args. Wraps
            // `recv.text().map(|s| s.to_string())` (lifts
            // `Option<&str>` -> `Option<String>`).
            M::Text if matches!(recv_ty, Type::XmlElement) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "text() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.text().map(|s| s.to_string())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.text codegen parse: {e}")))
            }
            // `el.attr(name)` -> Option<String>. One arg (String).
            // Wraps `recv.attr(&name).map(|s| s.to_string())` (lifts
            // `Option<&str>` -> `Option<String>`).
            M::Attr if matches!(recv_ty, Type::XmlElement) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.attr(&#arg).map(|s| s.to_string())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.attr codegen parse: {e}")))
            }
            // `el.children()` -> Vector<XmlElement>. Zero args. Wraps
            // `recv.children().to_vec()` (lifts `&[XmlElement]` ->
            // `Vec<XmlElement>`).
            M::Children if matches!(recv_ty, Type::XmlElement) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "children() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.children().to_vec()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.children codegen parse: {e}")))
            }
            // T10: AudioBuffer instance methods. Each method lowers
            // to `buff_audio::AudioBuffer::<method>`. The codegen
            // records `buff-audio` + `hound` + `symphonia` in
            // extern_crates via the `program_uses_namespace
            // ("AudioBuffer")` walker. All methods panic-free at the
            // codegen layer (slice via `unwrap_or_default()`;
            // AudioBuffer impls Default — added in the same T10
            // finish commit as this codegen arm).
            //
            // `buf.samples()` -> Vector<Float>. Zero args. Wraps
            // `recv.samples().to_vec()` (the `.to_vec()` lifts `&[f32]`
            // to `Vec<f32>` — Buff surfaces owned values).
            M::Samples if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "samples() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.samples().to_vec()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.samples codegen parse: {e}")))
            }
            // `buf.sample_rate()` -> Int. Zero args. Wraps
            // `recv.sample_rate() as i64`.
            M::SampleRate if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "sample_rate() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.sample_rate() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.sample_rate codegen parse: {e}")))
            }
            // `buf.channels()` -> Int. Zero args. Wraps
            // `recv.channels() as i64`.
            M::Channels if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "channels() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.channels() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.channels codegen parse: {e}")))
            }
            // `buf.frames()` -> Int. Zero args. Wraps `recv.frames()
            // as i64`.
            M::Frames if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "frames() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.frames() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.frames codegen parse: {e}")))
            }
            // `buf.duration_secs()` -> Float. Zero args. Wraps
            // `recv.duration_secs()` (already f64).
            M::DurationSecs if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "duration_secs() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.duration_secs()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.duration_secs codegen parse: {e}")))
            }
            // `buf.amplify(factor)` -> Void. One arg (Float). In-place
            // scale. Infallible.
            M::Amplify if matches!(recv_ty, Type::Audio) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "amplify() expects exactly 1 arg (factor), got {}",
                        args.len()
                    )));
                }
                let factor = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.amplify(#factor as f32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.amplify codegen parse: {e}")))
            }
            // `buf.normalize(target)` -> Void. One arg (Float). In-
            // place peak-normalize. Infallible (zero-sample buffer is
            // a no-op).
            M::Normalize if matches!(recv_ty, Type::Audio) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "normalize() expects exactly 1 arg (target), got {}",
                        args.len()
                    )));
                }
                let target = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.normalize(#target as f32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.normalize codegen parse: {e}")))
            }
            // `buf.mix(other)` -> Void. One arg (AudioBuffer). Sample-
            // wise add. Panic-free via `unwrap_or_default()` (rate /
            // channel mismatch is a no-op).
            M::Mix if matches!(recv_ty, Type::Audio) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "mix() expects exactly 1 arg (other AudioBuffer), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.mix(&#other).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.mix codegen parse: {e}")))
            }
            // `buf.slice(start_sec, end_sec)` -> AudioBuffer. Two args
            // (Float, Float). Returns a new AudioBuffer for the time
            // window. Panic-free via `unwrap_or_default()`
            // (AudioBuffer impls Default — invalid endpoints collapse
            // to empty 44100Hz mono).
            M::Slice if matches!(recv_ty, Type::Audio) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "slice() expects exactly 2 args (start_sec, end_sec), got {}",
                        args.len()
                    )));
                }
                let start = self.lower_expr(&args[0])?;
                let end = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.slice(#start as f64, #end as f64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.slice codegen parse: {e}")))
            }
            // `buf.summarize()` -> AudioSummary. Zero args. Returns a
            // statistics snapshot. Infallible (returns AudioSummary
            // directly — no `unwrap_or_default()` needed). The return
            // type is Type::Unknown at the type-checker layer; codegen
            // emits the bare call and Rust infers
            // `buff_audio::AudioSummary`.
            M::Summarize if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "summarize() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.summarize()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.summarize codegen parse: {e}")))
            }
            // `Save` is shared between Image.save (above) and
            // AudioBuffer.save (below) — dispatched on receiver type
            // (mirrors `Send` shared between Connection / WsConnection
            // and `Format` shared between DateTime / Date / Time).
            //
            // `img.save(path)` -> Void. One arg (String / Path).
            // Writes to disk. Panic-free via `unwrap_or_default()` (()
            // impls Default — I/O failure is a no-op).
            M::Save if matches!(recv_ty, Type::Image) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "save() expects exactly 1 arg (path), got {}",
                        args.len()
                    )));
                }
                let path = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.save(#path).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.save codegen parse: {e}")))
            }
            // `buf.save(path)` -> Void. One arg (String / Path). WAV
            // encode. Panic-free via `unwrap_or_default()`.
            M::Save if matches!(recv_ty, Type::Audio) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "save() expects exactly 1 arg (path), got {}",
                        args.len()
                    )));
                }
                let path = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.save(#path).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.save codegen parse: {e}")))
            }
            // T19: Template.render(context_json) -> String. One arg
            // (String — a JSON object). Wraps
            // `buff_template::Template::render(&self, &ctx)
            // .unwrap_or_default()` (panic-free on render failure —
            // missing variable / partial error collapses to empty
            // string, matching Buff's "no panicking generated code"
            // rule).
            M::Render if matches!(recv_ty, Type::Template) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "render() expects exactly 1 arg (context_json), got {}",
                        args.len()
                    )));
                }
                let ctx = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.render(#ctx).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Template.render codegen parse: {e}")))
            }
            // T73: String instance methods — dispatched on (Type::String,
            // variant) pairs. These lower to Rust's str methods directly.
            // Methods returning `&str` chain `.to_string()` to produce an
            // owned String (Buff hides references from users).
            //
            // `s.split(sep)` -> Vector<String>. Wraps
            // `s.split(sep).map(|s| s.to_string()).collect::<Vec<String>>()`.
            M::Split if matches!(recv_ty, Type::String) => {
                let sep = one_arg(self)?;
                let sep_ref = coerce_str_arg_to_ref(sep, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.split(#sep_ref).map(|s| s.to_string()).collect::<Vec<String>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("String.split codegen parse: {e}")))
            }
            // `s.trim()` -> String. Wraps `s.trim().to_string()`.
            M::Trim if matches!(recv_ty, Type::String) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "trim() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.trim().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("String.trim codegen parse: {e}")))
            }
            // `s.starts_with(prefix)` -> Bool. Wraps `s.starts_with(prefix)`.
            M::StartsWith if matches!(recv_ty, Type::String) => {
                let prefix = one_arg(self)?;
                let prefix_ref = coerce_str_arg_to_ref(prefix, &args[0]);
                Ok(method_call_one_arg(recv, "starts_with", prefix_ref))
            }
            // `s.ends_with(suffix)` -> Bool. Wraps `s.ends_with(suffix)`.
            M::EndsWith if matches!(recv_ty, Type::String) => {
                let suffix = one_arg(self)?;
                let suffix_ref = coerce_str_arg_to_ref(suffix, &args[0]);
                Ok(method_call_one_arg(recv, "ends_with", suffix_ref))
            }
            // `s.to_upper()` -> String. Wraps `s.to_uppercase().to_string()`.
            // `.to_uppercase()` returns a `String` in Rust, but we chain
            // `.to_string()` for consistency (the source `&str` method
            // signature returns `String` directly; chaining `.to_string()`
            // on a `String` is a no-op clone, but the generated code is
            // simple and the optimizer removes the redundant `to_string`).
            M::ToUppercase if matches!(recv_ty, Type::String) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_upper() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.to_uppercase().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("String.to_uppercase codegen parse: {e}")))
            }
            // `s.to_lower()` -> String. Wraps `s.to_lowercase().to_string()`.
            M::ToLowercase if matches!(recv_ty, Type::String) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_lower() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.to_lowercase().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("String.to_lowercase codegen parse: {e}")))
            }
            // `s.replace(from, to)` -> String (reuses existing Replace
            // variant — dispatched on Type::String). Wraps
            // `s.replace(from, to)`. Two args (String, String).
            M::Replace if matches!(recv_ty, Type::String) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "replace() expects exactly 2 args (from, to), got {}",
                        args.len()
                    )));
                }
                let from = self.lower_expr(&args[0])?;
                let to = self.lower_expr(&args[1])?;
                let from_ref = coerce_str_arg_to_ref(from, &args[0]);
                let to_ref = coerce_str_arg_to_ref(to, &args[1]);
                let mut call_args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                call_args.push(from_ref);
                call_args.push(to_ref);
                let replace_call = SynExpr::MethodCall(syn::ExprMethodCall {
                    attrs: Vec::new(),
                    receiver: Box::new(recv),
                    dot_token: Default::default(),
                    method: Ident::new("replace", ProcSpan::call_site()),
                    turbofish: None,
                    paren_token: Default::default(),
                    args: call_args,
                });
                Ok(replace_call)
            }
            // `s.contains(sub)` -> Bool (reuses existing Contains variant
            // — dispatched on Type::String). Wraps `s.contains(sub)`.
            M::Contains if matches!(recv_ty, Type::String) => {
                let sub = one_arg(self)?;
                let sub_ref = coerce_str_arg_to_ref(sub, &args[0]);
                Ok(method_call_one_arg(recv, "contains", sub_ref))
            }
            // `s.len()` -> Int (reuses existing Len variant — dispatched on
            // Type::String). Zero args. Wraps `recv.len() as i64`.
            M::Len if matches!(recv_ty, Type::String) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "len() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.len() as i64
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("String.len codegen parse: {e}")))
            }
            // T80: Http response instance methods. Dispatch on
            // `(Int, String)` tuple (the return type of Http.get / post / put / delete).
            //
            // `response.status()` -> Int. Wraps `recv.0` (tuple field access).
            M::ResponseStatus => {
                if !matches!(recv_ty, Type::Tuple(_)) {
                    return Err(self.unsupported(&format!(
                        "status() requires a response tuple, got {recv_ty}"
                    )));
                }
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "status() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.0
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Response.status codegen parse: {e}")))
            }
            // `response.body()` -> String. Wraps `recv.1.clone()`.
            M::ResponseBody => {
                if !matches!(recv_ty, Type::Tuple(_)) {
                    return Err(self.unsupported(&format!(
                        "body() requires a response tuple, got {recv_ty}"
                    )));
                }
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "body() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.1.clone()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Response.body codegen parse: {e}")))
            }
            // `response.json()` -> Map<String, Unknown>. Wraps
            // `serde_json::from_str::<HashMap<String, serde_json::Value>>(&recv.1).unwrap_or_default()`.
            M::ResponseJson => {
                if !matches!(recv_ty, Type::Tuple(_)) {
                    return Err(self.unsupported(&format!(
                        "json() requires a response tuple, got {recv_ty}"
                    )));
                }
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "json() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(&#recv.1)
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Response.json codegen parse: {e}")))
            }
            // `response.headers()` -> Map<String, String>. Returns empty Map
            // (headers not available from body-only response).
            M::ResponseHeaders => {
                if !matches!(recv_ty, Type::Tuple(_)) {
                    return Err(self.unsupported(&format!(
                        "headers() requires a response tuple, got {recv_ty}"
                    )));
                }
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "headers() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::collections::HashMap::<String, String>::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Response.headers codegen parse: {e}")))
            }
            // Non-Image / Non-Audio receiver with an Image-only /
            // Audio-only method (Width / Height / PixelFormat /
            // GetPixel / SetPixel / Grayscale / Invert / Resize /
            // Crop / Blur / Samples / SampleRate / Channels / Frames
            // / DurationSecs / Amplify / Normalize / Mix / Slice /
            // Summarize / Save) falls through to a clear error
            // (mirrors the Select / Filter / Sort / Head / GroupBy /
            // Agg / ToTableString safety net above).
            M::Width
            | M::Height
            | M::PixelFormat
            | M::GetPixel
            | M::SetPixel
            | M::Grayscale
            | M::Invert
            | M::Resize
            | M::Crop
            | M::Blur
            | M::Samples
            | M::SampleRate
            | M::Channels
            | M::Frames
            | M::DurationSecs
            | M::Amplify
            | M::Normalize
            | M::Mix
            | M::Slice
            | M::Summarize
            | M::Render
            | M::Save
            // T31: Cache-only methods. Non-Cache receiver with one of
            // these methods falls through to a clear error (mirrors
            // the Image / Audio / Template safety nets above).
            | M::SetTtl
            | M::Delete
            | M::Contains
            | M::Clear => Err(self.unsupported(&format!(
                "{recv_ty}.{:?}() is not a recognised prelude instance method",
                pmethod
            ))),
            // T20: Reactive instance methods on Type::Unknown receivers.
            // Dispatched when type inference resolves the receiver to
            // Type::Unknown (the forward-declaration contract for
            // ReactiveSignal.new / ReactiveComputed.new /
            // ReactiveEffect.new return values — coordinated
            // Type::ReactiveSignal / ReactiveComputed / ReactiveEffect
            // variants in ty.rs are follow-up sibling tasks OUTSIDE
            // the T20 shared zone). Each arm emits the method call
            // directly; Rust's method resolution finds the matching
            // `buff_reactive::Signal::get` / `set` / `update` /
            // `Computed::get` / `Effect::run` method.
            M::Get if matches!(recv_ty, Type::Unknown) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "get() takes no arguments, got {}",
                        args.len()
                    )));
                }
                Ok(method_call_no_args(recv, "get"))
            }
            M::Set if matches!(recv_ty, Type::Unknown) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "set() expects exactly 1 arg (the new value), got {}",
                        args.len()
                    )));
                }
                let value = self.lower_expr(&args[0])?;
                Ok(method_call_one_arg(recv, "set", value))
            }
            M::Update if matches!(recv_ty, Type::Unknown) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "update() expects exactly 1 arg (the mutating closure), got {}",
                        args.len()
                    )));
                }
                let closure = self.lower_expr(&args[0])?;
                // `Signal::update` returns `Result<(), ReactiveError>`;
                // discard via `.ok()` so generated code is panic-free
                // and infallible at the Buff surface (mirrors the
                // DataFrame / Image unwrap_or_default stance).
                let call = method_call_one_arg(recv, "update", closure);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #call.ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signal.update codegen parse: {e}")))
            }
            M::Invalidate if matches!(recv_ty, Type::Unknown) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "invalidate() takes no arguments, got {}",
                        args.len()
                    )));
                }
                Ok(method_call_no_args(recv, "invalidate"))
            }
            // T29: Validator instance methods. Records `buff-validate`
            // + `validator` + `serde_json` + `regex` in extern_crates
            // via the `program_uses_namespace("Validator")` walker.
            //
            // The five builder methods (with_*) consume self and
            // return Self — Buff's "no visible references" stance
            // mirrors the axum `Router::route` pattern. Each call
            // lowers to `recv.with_xxx(arg)` (the buff-validate
            // surface takes the args by value).
            //
            // `validator.with_email(field)` -> Validator. One arg.
            M::WithEmail if matches!(recv_ty, Type::Validator) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "with_email() expects exactly 1 arg (field), got {}",
                        args.len()
                    )));
                }
                let field = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.with_email(#field)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.with_email codegen parse: {e}")))
            }
            // `validator.with_url(field)` -> Validator. One arg.
            M::WithUrl if matches!(recv_ty, Type::Validator) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "with_url() expects exactly 1 arg (field), got {}",
                        args.len()
                    )));
                }
                let field = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.with_url(#field)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.with_url codegen parse: {e}")))
            }
            // `validator.with_length(field, min, max)` -> Validator.
            // Three args. Panic-free via `unwrap_or_default()`
            // (Validator impls Default as an empty rule set —
            // InvalidRuleConfig surfaces as no-op clone).
            M::WithLength if matches!(recv_ty, Type::Validator) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "with_length() expects exactly 3 args (field, min, max), got {}",
                        args.len()
                    )));
                }
                let field = self.lower_expr(&args[0])?;
                let min = self.lower_expr(&args[1])?;
                let max = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.with_length(#field, #min as u64, #max as u64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.with_length codegen parse: {e}")))
            }
            // `validator.with_range(field, min, max)` -> Validator.
            // Three args. Panic-free via `unwrap_or_default()`.
            M::WithRange if matches!(recv_ty, Type::Validator) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "with_range() expects exactly 3 args (field, min, max), got {}",
                        args.len()
                    )));
                }
                let field = self.lower_expr(&args[0])?;
                let min = self.lower_expr(&args[1])?;
                let max = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.with_range(#field, #min as i64, #max as i64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.with_range codegen parse: {e}")))
            }
            // `validator.with_regex(field, pattern)` -> Validator.
            // Two args. Panic-free via `unwrap_or_default()` (a
            // malformed pattern surfaces as no-op clone — the
            // underlying buff-validate surface compiles the regex
            // eagerly at registration).
            M::WithRegex if matches!(recv_ty, Type::Validator) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "with_regex() expects exactly 2 args (field, pattern), got {}",
                        args.len()
                    )));
                }
                let field = self.lower_expr(&args[0])?;
                let pattern = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.with_regex(#field, #pattern).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.with_regex codegen parse: {e}")))
            }
            // `validator.validate(input)` -> Result<Void, String>.
            // One arg (Map<String, String>). Wraps
            // `recv.validate(&input).map_err(|e| e.to_string())` so
            // the Buff `?` operator propagates a string error.
            M::Validate if matches!(recv_ty, Type::Validator) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "validate() expects exactly 1 arg (input), got {}",
                        args.len()
                    )));
                }
                let input = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.validate(#input).map_err(|e| e.to_string())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.validate codegen parse: {e}")))
            }
            // `validator.to_json_schema()` -> String. Zero args.
            // Wraps `recv.to_json_schema()`.
            M::ToJsonSchema if matches!(recv_ty, Type::Validator) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_json_schema() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.to_json_schema()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.to_json_schema codegen parse: {e}")))
            }
            // T42: Email builder methods. Each consumes self and
            // returns a new Email (Buff "no visible references"
            // stance — mirrors Validator with_* + HttpClient.new).
            // `email.body(text)` -> Email. One arg (String plain).
            // Wraps `recv.body(&text)?` (the `?` propagates
            // EmailError::Panic — only failure mode for a string
            // setter per the catch_unwind contract).
            M::Body if matches!(recv_ty, Type::Email) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "body() expects exactly 1 arg (text), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.body(&#text)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Email.body codegen parse: {e}")))
            }
            // `email.html(template, context_json)` -> Email. Two args
            // (String handlebars template, String JSON context).
            // Wraps `recv.html(&template, &ctx)?` (the `?` propagates
            // EmailError::TemplateParse / TemplateRender).
            M::Html if matches!(recv_ty, Type::Email) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "html() expects exactly 2 args (template, context_json), got {}",
                        args.len()
                    )));
                }
                let template = self.lower_expr(&args[0])?;
                let context = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.html(&#template, &#context)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Email.html codegen parse: {e}")))
            }
            // `email.attach(path)` -> Email. One arg (String path).
            // Wraps `recv.attach(&path)?` (panic-free — file is NOT
            // read at builder time; EmailError surfaces at send time
            // via the build_message MIME-assembly path).
            M::Attach if matches!(recv_ty, Type::Email) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "attach() expects exactly 1 arg (path), got {}",
                        args.len()
                    )));
                }
                let path = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.attach(&#path)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Email.attach codegen parse: {e}")))
            }
            // T42: SmtpClient action method. The single send method
            // is dispatched on (Type::SmtpClient, Send) — shares the
            // Send variant with TCP / WebSocket / Sender. Returns
            // Void (the codegen discards the Result via
            // unwrap_or_default panic-free — invalid email / SMTP
            // failure is a no-op at the Buff surface, matching the
            // Image save / Cache set precedent).
            // `client.send(email)` -> Void. One arg (Email). Wraps
            // `recv.send(&email).unwrap_or_default()`.
            M::Send if matches!(recv_ty, Type::SmtpClient) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "send() expects exactly 1 arg (email), got {}",
                        args.len()
                    )));
                }
                let email = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.send(&#email).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("SmtpClient.send codegen parse: {e}")))
            }
            // T48: buff-web3 instance methods. Each lowers to a
            // fully-qualified `buff_web3::*` method chained with
            // `.unwrap_or_default()` / `as i64` (panic-free — mirrors
            // T9 Image / T45 Point / T47 Bot / T52 Message). The
            // shared `Address` variant covers Wallet.address /
            // ConnectedWallet.address / Contract.address; the shared
            // `Connect` variant covers Wallet.connect; the shared
            // `Send` variant covers ContractMethod.send.
            //
            // `provider.chain_id()` -> Int. Zero args. Wraps
            // `recv.chain_id().unwrap_or_default() as i64`.
            M::ChainId if matches!(recv_ty, Type::Provider) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "chain_id() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.chain_id().unwrap_or_default() as i64
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Provider.chain_id codegen parse: {e}")))
            }
            // `provider.block_number()` -> Int. Zero args.
            M::BlockNumber if matches!(recv_ty, Type::Provider) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "block_number() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.block_number().unwrap_or_default() as i64
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Provider.block_number codegen parse: {e}"))
                })
            }
            // `provider.get_balance(address)` -> Int. One arg (String).
            M::GetBalance if matches!(recv_ty, Type::Provider) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "get_balance() expects exactly 1 arg (address), got {}",
                        args.len()
                    )));
                }
                let address = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.get_balance(&#address).unwrap_or_default() as i64
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Provider.get_balance codegen parse: {e}"))
                })
            }
            // `provider.get_nonce(address)` -> Int. One arg (String).
            M::GetNonce if matches!(recv_ty, Type::Provider) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "get_nonce() expects exactly 1 arg (address), got {}",
                        args.len()
                    )));
                }
                let address = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.get_nonce(&#address).unwrap_or_default() as i64
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Provider.get_nonce codegen parse: {e}")))
            }
            // `provider.wait_for_tx(tx_hash)` -> String. One arg (String).
            M::WaitForTx if matches!(recv_ty, Type::Provider) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "wait_for_tx() expects exactly 1 arg (tx_hash), got {}",
                        args.len()
                    )));
                }
                let hash = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.wait_for_tx(&#hash).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Provider.wait_for_tx codegen parse: {e}"))
                })
            }
            // `wallet.address()` -> String. Zero args. Shared `Address`
            // variant dispatched on (Wallet, Address) / (ConnectedWallet,
            // Address) / (Contract, Address) pairs. Infallible (the
            // underlying buff_web3 methods return String directly).
            M::Address if matches!(recv_ty, Type::Wallet | Type::ConnectedWallet | Type::Contract) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "address() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.address()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("web3 .address codegen parse: {e}")))
            }
            // `wallet.connect(provider)` -> ConnectedWallet. One arg
            // (Provider). Infallible (returns ConnectedWallet directly —
            // no failure mode). Wraps `recv.connect(#provider)` (move
            // semantics — consumes self). Shared `Connect` variant
            // dispatched on (Wallet, Connect) — distinct lowering from
            // TCP.connect / WebSocket.connect.
            M::Connect if matches!(recv_ty, Type::Wallet) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "connect() expects exactly 1 arg (provider), got {}",
                        args.len()
                    )));
                }
                let provider = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.connect(#provider)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Wallet.connect codegen parse: {e}")))
            }
            // `wallet.sign_message(message)` -> String. One arg (String).
            // Wraps `recv.sign_message(&msg).unwrap_or_default()` (panic-
            // free — Web3Error::Rpc collapses to String::default()).
            M::SignMessage if matches!(recv_ty, Type::Wallet) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "sign_message() expects exactly 1 arg (message), got {}",
                        args.len()
                    )));
                }
                let message = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.sign_message(&#message).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Wallet.sign_message codegen parse: {e}"))
                })
            }
            // `contract.method(name)` -> ContractMethod. One arg (String).
            // Wraps `recv.method(&name).unwrap_or_default()` (panic-free —
            // MethodNotFound / InvalidAbi collapses to a default
            // ContractMethod whose .call() / .send() return Default).
            M::Method if matches!(recv_ty, Type::Contract) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "method() expects exactly 1 arg (name), got {}",
                        args.len()
                    )));
                }
                let name = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.method(&#name).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Contract.method codegen parse: {e}")))
            }
            // `m.arg(name, value)` -> ContractMethod. Two args (String
            // name, String value). The name is currently IGNORED at the
            // wire layer (ethers::abi::Token doesn't carry names for
            // non-tuple inputs); future tuple support may consume it.
            // The value is spliced as `ethers::abi::Token::String`.
            // Chainable — consumes self, returns Self (mirrors
            // Validator.with_* / Email.body builder pattern).
            M::Arg if matches!(recv_ty, Type::ContractMethod) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "arg() expects exactly 2 args (name, value), got {}",
                        args.len()
                    )));
                }
                let value = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.arg(ethers::abi::Token::String((#value).to_string()))
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("ContractMethod.arg codegen parse: {e}")))
            }
            // `m.args(values)` -> ContractMethod. One arg (Vector<String>).
            // Each value spliced as `ethers::abi::Token::String`.
            // Chainable — consumes self, returns Self.
            M::Args if matches!(recv_ty, Type::ContractMethod) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "args() expects exactly 1 arg (values), got {}",
                        args.len()
                    )));
                }
                let values = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.args((#values).into_iter().map(|v| ethers::abi::Token::String(v)))
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("ContractMethod.args codegen parse: {e}")))
            }
            // `m.call()` -> String. Zero args. Wraps `recv.call()
            // .unwrap_or_default()` (panic-free — Web3Error::Rpc /
            // AbiDecode collapses to String::default()).
            M::Call if matches!(recv_ty, Type::ContractMethod) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "call() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.call().unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ContractMethod.call codegen parse: {e}"))
                })
            }
            // `m.send()` -> String. Zero args. Wraps `recv.send()
            // .unwrap_or_default()` (panic-free — Web3Error::Rpc /
            // WalletNotConnected collapses to String::default()). Shared
            // `Send` variant dispatched on (ContractMethod, Send) —
            // distinct lowering from Connection.send /
            // WsConnection.send / Sender.send / SmtpClient.send.
            M::Send if matches!(recv_ty, Type::ContractMethod) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "send() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.send().unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ContractMethod.send codegen parse: {e}"))
                })
            }
            // T49: RsaKeypair.public_pem() -> String. Zero args.
            // Wraps `recv.public_pem.clone()` (the underlying field
            // is `String`; `.clone()` lifts `&String` to owned
            // `String` per Buff's "hide references from users" rule).
            // Infallible (no failure mode — the field is always
            // populated when constructed via RSA.generate_keypair).
            // Shared `PublicPem` variant dispatched on
            // (RsaKeypair, PublicPem).
            M::PublicPem if matches!(recv_ty, Type::RsaKeypair) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "public_pem() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.public_pem.clone()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("RsaKeypair.public_pem codegen parse: {e}"))
                })
            }
            // T49: RsaKeypair.private_pem() -> String. Zero args.
            // Wraps `recv.private_pem.clone()`. Same shape as
            // public_pem above.
            M::PrivatePem if matches!(recv_ty, Type::RsaKeypair) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "private_pem() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.private_pem.clone()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("RsaKeypair.private_pem codegen parse: {e}"))
                })
            }
            // T31 (gap-fill): wildcard for instance-method variants
            // whose codegen arms haven't been written yet (T11 Signal
            // / Spectrum / Window, T12 ECS, T17 Web route_*, T20
            // Reactive Update/Invalidate, T26 Audit, T29 Validator
            // with_*). The variant exists in the PreludeInstanceFn
            // enum but no `lower_*` arm handles it — surfaces as a
            // clear "unsupported" error instead of a compile break.
            // As sibling tasks complete their codegen wiring, they
            // add explicit arms ABOVE this wildcard.
            _ => Err(self.unsupported(&format!(
                "{recv_ty}.{:?}() codegen not yet implemented",
                pmethod
            ))),
        }
    }
}
