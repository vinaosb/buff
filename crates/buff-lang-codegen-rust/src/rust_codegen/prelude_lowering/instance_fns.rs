//! ITER-43 - First half of the T124 prelude-type INSTANCE-method match
//! (lower_prelude_type_instance_fn_arms), moved verbatim from the former
//! monolithic lower_prelude_type_instance_fn in the parent: DateTime
//! accessors, Regex match/find/replace/captures, URL/Path, Process,
//! TCP/UDP/WebSocket/Channel send/recv/close, DataFrame, Image, Geo
//! (Point/LineString/Polygon), Decimal, Simd, Language/Message
//! (protobuf), Bot/ChatMessage/Platform families.
//!
//! Arms keep their ORIGINAL relative order; the trailing wildcard
//! delegates to the instance_fns_extra sibling (which owns the final
//! "codegen not yet implemented" fallback), making the chain semantically
//! identical to the former single match. `recv` is lowered ONCE in the
//! parent entry and passed down by value (lower_expr allocates temps;
//! per-half lowering would change generated temp names).

use super::*;

impl RustCodegen {
    /// T124b (ITER-43 half 1 of 2): prelude-type instance-method call
    /// lowering, DateTime..Platform arms. See the parent entry point
    /// [`Self::lower_prelude_type_instance_fn`] for the lowering-table
    /// overview; ordered wildcard-delegation continues in the
    /// instance_fns_extra sibling.
    pub(super) fn lower_prelude_type_instance_fn_arms(
        &mut self,
        recv_ty: &Type,
        pmethod: buff_lang_types::PreludeInstanceFn,
        recv: SynExpr,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::PreludeInstanceFn as M;
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
                if !matches!(recv_ty, Type::SmtpClient | Type::ContractMethod) =>
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
                    return Err(
                        self.unsupported(&format!("len() takes no arguments, got {}", args.len()))
                    );
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
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("DataFrame.group_by codegen parse: {e}"))
                })
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
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("DataFrame.to_table_string codegen parse: {e}"))
                })
            }
            // Non-DataFrame receiver with a DataFrame-only method
            // (Select/Filter/Sort/Head/GroupBy/Agg/ToTableString) falls
            // through to a clear error (mirrors the Send/Recv/Close
            // safety net). T43 excludes Type::Document / Type::Element
            // from the M::Select catch-all so the buff-scrape arms
            // below fire first (mirrors how the `M::Len` arm excludes
            // Type::Cache).
            M::Select if !matches!(recv_ty, Type::Document | Type::Element) => Err(self
                .unsupported(&format!(
                    "{recv_ty}.select() is not a recognised prelude instance method",
                ))),
            M::Filter | M::Sort | M::Head | M::GroupBy | M::Agg | M::ToTableString => Err(self
                .unsupported(&format!(
                    "{recv_ty}.{:?}() is not a recognised prelude instance method",
                    pmethod
                ))),
            // `Len` is shared between DataFrame.len (above) and
            // future Vector.len / Map.len / Series.len / Cache.len —
            // dispatched on receiver type. Non-DataFrame / non-Cache
            // receivers fall through to the existing method-resolution
            // path. T31 (Cache) arm lives below; the guard on this
            // arm skips Cache so the Cache-specific arm fires first.
            M::Len if !matches!(recv_ty, Type::Cache | Type::String) => Err(self.unsupported(
                &format!("{recv_ty}.len() is not a recognised prelude instance method",),
            )),
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
                    return Err(self
                        .unsupported(&format!("height() takes no arguments, got {}", args.len())));
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
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Image.pixel_format codegen parse: {e}"))
                })
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
                    return Err(self
                        .unsupported(&format!("invert() takes no arguments, got {}", args.len())));
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
                    return Err(
                        self.unsupported(&format!("x() takes no arguments, got {}", args.len()))
                    );
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
                    return Err(
                        self.unsupported(&format!("y() takes no arguments, got {}", args.len()))
                    );
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
                    return Err(self
                        .unsupported(&format!("length() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.length()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("LineString.length codegen parse: {e}")))
            }
            // `polygon.area()` -> Float. Zero args. Wraps `recv.area()`.
            M::Area if matches!(recv_ty, Type::Polygon) => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("area() takes no arguments, got {}", args.len()))
                    );
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
                    return Err(
                        self.unsupported(&format!("sum() takes no arguments, got {}", args.len()))
                    );
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
                    return Err(
                        self.unsupported(&format!("min() takes no arguments, got {}", args.len()))
                    );
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
                    return Err(
                        self.unsupported(&format!("max() takes no arguments, got {}", args.len()))
                    );
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
                    return Err(self
                        .unsupported(&format!("to_vec() takes no arguments, got {}", args.len())));
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
                    return Err(
                        self.unsupported(&format!("code() takes no arguments, got {}", args.len()))
                    );
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
                    return Err(
                        self.unsupported(&format!("name() takes no arguments, got {}", args.len()))
                    );
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
                    return Err(self
                        .unsupported(&format!("encode() takes no arguments, got {}", args.len())));
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
                    return Err(self
                        .unsupported(&format!("start() takes no arguments, got {}", args.len())));
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
                    return Err(
                        self.unsupported(&format!("stop() takes no arguments, got {}", args.len()))
                    );
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
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.command_count codegen parse: {e}")))
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
                    return Err(
                        self.unsupported(&format!("text() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.text().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("ChatMessage.text codegen parse: {e}")))
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
                    return Err(self
                        .unsupported(&format!("author() takes no arguments, got {}", args.len())));
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
                    return Err(self
                        .unsupported(&format!("is_dm() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.is_dm()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("ChatMessage.is_dm codegen parse: {e}")))
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
            // ITER-43 ordered-delegation seam: no arm above matched
            // (all guards above are pure matches! on recv_ty), so
            // continue the original arm order in the extra sibling.
            _ => self.lower_prelude_type_instance_fn_arms_extra(recv_ty, pmethod, recv, args),
        }
    }
}
