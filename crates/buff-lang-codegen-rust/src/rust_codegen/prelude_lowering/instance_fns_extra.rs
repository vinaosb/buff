//! ITER-43 - Second half of the T124 prelude-type INSTANCE-method match
//! (lower_prelude_type_instance_fn_arms_extra), moved verbatim from the
//! former monolithic lower_prelude_type_instance_fn in the parent: Faker,
//! Cache, I18n, Scrape Document/Element, Crawler, Xml/XmlElement, Audio,
//! Image/Audio save, Template render, String methods, HTTP Response*,
//! Terminal width, Reactive (Type::Unknown) get/set/update/invalidate,
//! Validator with_*, Email/Smtp, Web3 Provider/Wallet/Contract,
//! RsaKeypair families - plus the T31 gap-fill wildcard ("codegen not
//! yet implemented").
//!
//! This half also owns the `one_arg` arity-check closure and the
//! `pmethod_name` local: every `one_arg(self)` call site lives in these
//! arms (first at `M::Select if Type::Document`), so the closure moved
//! wholesale with no duplication.

use super::*;

impl RustCodegen {
    /// T124b (ITER-43 half 2 of 2): prelude-type instance-method call
    /// lowering, Faker..RsaKeypair arms + the T31 gap-fill wildcard.
    /// Reached only via the instance_fns sibling trailing wildcard
    /// (original arm order preserved end-to-end).
    pub(super) fn lower_prelude_type_instance_fn_arms_extra(
        &mut self,
        recv_ty: &Type,
        pmethod: buff_lang_types::PreludeInstanceFn,
        recv: SynExpr,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::PreludeInstanceFn as M;
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
