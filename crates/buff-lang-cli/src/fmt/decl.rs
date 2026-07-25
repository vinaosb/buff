//! Declaration formatting - extracted from `fmt.rs` (T106 mechanical split).
//!
//! impl block for the declaration-writing methods on [`Formatter`].

use std::fmt::Write;

use buff_lang_ast::{
    Attribute, Decl, EnumDecl, EnumVariant, ExportDecl, ExtendBlock, FuncDecl, ImplBlock,
    ImportDecl, MethodSig, Param, ReexportDecl, StructDecl, TraitDecl,
};

use super::Formatter;

impl<'a> Formatter<'a> {
    pub(super) fn write_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::FuncDecl(f) => self.write_func(f),
            Decl::StructDecl(s) => self.write_struct(s),
            Decl::EnumDecl(e) => self.write_enum(e),
            Decl::ImportDecl(i) => {
                self.write_indent();
                self.write_import(i);
            }
            Decl::ModuleDecl(m) => {
                self.write_indent();
                let _ = write!(self.buf, "module {};", m.name);
            }
            Decl::TraitDecl(t) => self.write_trait(t),
            Decl::ExportDecl(e) => self.write_export(e),
            Decl::ReexportDecl(r) => {
                self.write_indent();
                self.write_reexport(r);
            }
            Decl::ExternCrateDecl(c) => {
                self.write_indent();
                let _ = write!(self.buf, "extern crate {:?}", c.name);
            }
            // T119: `extern "ABI" [from "crate"] func name(...) -> Ret`.
            // Render the surface syntax the user wrote.
            Decl::ExternFuncDecl(d) => {
                self.write_indent();
                let _ = write!(self.buf, "extern {:?} ", d.abi);
                if let Some(c) = &d.crate_name {
                    let _ = write!(self.buf, "from {:?} ", c);
                }
                let _ = write!(self.buf, "func {}(", d.name);
                for (i, p) in d.params.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    let _ = write!(self.buf, "{p}");
                }
                self.raw(")");
                if let Some(rt) = &d.return_type {
                    let _ = write!(self.buf, " -> {rt}");
                }
            }
            Decl::ExtendBlock(ext) => self.write_extend(ext),
            Decl::ImplBlock(imp) => self.write_impl_block(imp),
        }
    }

    pub(super) fn write_attributes(&mut self, attrs: &[Attribute]) {
        for a in attrs {
            self.write_indent();
            self.raw("@");
            self.raw(&a.name.name);
            if !a.args.is_empty() {
                self.raw("(");
                for (i, arg) in a.args.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    let _ = write!(self.buf, "{:?}", arg);
                }
                self.raw(")");
            }
            self.nl();
        }
    }

    pub(super) fn write_func(&mut self, f: &FuncDecl) {
        self.write_attributes(&f.attributes);
        self.write_indent();
        self.write_func_signature(f);
        if f.is_extern {
            // extern funcs are bodyless (signature only).
            self.raw(";");
            return;
        }
        self.raw(":");
        self.write_block_body(&f.body);
    }

    /// Emit the `func name(params) -> Ret` signature (everything before
    /// `:` or `;`).
    pub(super) fn write_func_signature(&mut self, f: &FuncDecl) {
        if f.is_extern {
            self.raw("extern ");
        }
        if f.is_async {
            self.raw("async ");
        }
        if f.is_unsafe {
            self.raw("unsafe ");
        }
        let _ = write!(self.buf, "func {}(", f.name);
        self.write_params(&f.params);
        self.raw(")");
        if let Some(rt) = &f.return_type {
            self.raw(" -> ");
            self.write_typeref(rt);
        }
    }

    pub(super) fn write_params(&mut self, params: &[Param]) {
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                self.raw(", ");
            }
            let _ = write!(self.buf, "{}: ", p.name);
            self.write_typeref(&p.ty);
            if let Some(def) = &p.default_value {
                self.raw(" = ");
                self.write_expr(def);
            }
        }
    }

    pub(super) fn write_struct(&mut self, s: &StructDecl) {
        self.write_indent();
        let _ = write!(self.buf, "struct {}", s.name);
        if !s.traits.is_empty() {
            self.raw(": ");
            for (i, t) in s.traits.iter().enumerate() {
                if i > 0 {
                    self.raw(" + ");
                }
                let _ = write!(self.buf, "{t}");
            }
        }
        if s.fields.is_empty() {
            // Empty struct: `struct Foo {}`.
            self.raw(" {}");
            return;
        }
        self.raw(" {");
        self.indent();
        for (name, ty) in &s.fields {
            self.nl();
            let _ = write!(self.buf, "{}: ", name);
            self.write_typeref(ty);
            self.raw(",");
        }
        self.dedent();
        self.nl();
        self.raw("}");
    }

    pub(super) fn write_enum(&mut self, e: &EnumDecl) {
        self.write_indent();
        let _ = write!(self.buf, "enum {}", e.name);
        if !e.type_params.is_empty() {
            self.raw("<");
            for (i, tp) in e.type_params.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                let _ = write!(self.buf, "{}", tp.name);
            }
            self.raw(">");
        }
        if e.variants.is_empty() {
            self.raw(" {}");
            return;
        }
        self.raw(" {");
        // Anchor for the first variant's leading-comment drain: the byte
        // offset of the `{` in source (so newlines_between counts the
        // newline after `{` correctly).
        let lbrace_byte = self.find_byte_after(e.name.span.end, b'{');
        self.indent();
        let mut prev_end = lbrace_byte + 1;
        for v in &e.variants {
            if self.has_comments() {
                self.drain_comments_in(prev_end, v.span.start);
            }
            self.nl();
            self.write_enum_variant(v);
            self.raw(",");
            if self.has_comments() {
                self.mark_emitted_end(v.span.end);
                self.drain_trailing_after();
                prev_end = self.last_emitted_byte;
            }
        }
        // Drain trailing comments inside the enum body (before `}`).
        if self.has_comments() {
            let hi = self.find_byte_after(lbrace_byte, b'}');
            self.drain_comments_in(prev_end, hi);
        }
        self.dedent();
        self.nl();
        self.raw("}");
    }

    /// Find the first occurrence of `needle` in `self.src` at or after
    /// `start`. Used to locate the `{` / `}` byte offsets inside
    /// struct/enum bodies for the comment-drain anchor. Returns `start`
    /// if not found (defensive — should not happen for well-formed input).
    pub(super) fn find_byte_after(&self, start: usize, needle: u8) -> usize {
        let start = start.min(self.src.len());
        let bytes = self.src.as_bytes();
        for (i, &b) in bytes[start..].iter().enumerate() {
            if b == needle {
                return start + i;
            }
        }
        start
    }

    pub(super) fn write_enum_variant(&mut self, v: &EnumVariant) {
        let _ = write!(self.buf, "{}", v.name);
        if let Some(tys) = &v.data {
            self.raw("(");
            for (i, t) in tys.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                self.write_typeref(t);
            }
            self.raw(")");
        }
    }

    /// Emit an import declaration. Does NOT write the leading indent —
    /// callers handle that. Used by both `write_decl` (single-decl path)
    /// and `write_decls` (batch path with sorting).
    pub(super) fn write_import(&mut self, imp: &ImportDecl) {
        self.raw("import ");
        if let Some(from) = &imp.from_path {
            // ES6 form.
            if imp.wildcard {
                self.raw("*");
            } else {
                self.raw("{ ");
                for (i, n) in imp.imports.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    let _ = write!(self.buf, "{n}");
                }
                self.raw(" }");
            }
            let _ = write!(self.buf, " from {:?}", from);
        } else {
            // Legacy form: a.b.c [as alias].
            for (i, p) in imp.path.iter().enumerate() {
                if i > 0 {
                    self.raw(".");
                }
                let _ = write!(self.buf, "{p}");
            }
            if let Some(alias) = &imp.alias {
                let _ = write!(self.buf, " as {alias}");
            }
        }
    }

    pub(super) fn write_export(&mut self, exp: &ExportDecl) {
        match exp.inner.as_ref() {
            Decl::FuncDecl(f) => {
                self.write_attributes(&f.attributes);
                self.write_indent();
                self.raw("export ");
                self.write_func_signature(f);
                if f.is_extern {
                    self.raw(";");
                    return;
                }
                self.raw(":");
                self.write_block_body(&f.body);
            }
            Decl::StructDecl(s) => {
                self.write_indent();
                self.raw("export ");
                self.write_struct(s);
            }
            Decl::EnumDecl(e) => {
                self.write_indent();
                self.raw("export ");
                self.write_enum(e);
            }
            other => {
                // Defensive: the parser rejects other inner decls.
                self.write_indent();
                let _ = write!(self.buf, "export {}", other);
            }
        }
    }

    /// Emit a re-export declaration without leading indent.
    pub(super) fn write_reexport(&mut self, r: &ReexportDecl) {
        self.raw("export ");
        if r.wildcard {
            self.raw("*");
        } else {
            self.raw("{ ");
            for (i, n) in r.names.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                let _ = write!(self.buf, "{n}");
            }
            self.raw(" }");
        }
        let _ = write!(self.buf, " from {:?}", r.from);
    }

    pub(super) fn write_trait(&mut self, t: &TraitDecl) {
        self.write_indent();
        let _ = write!(self.buf, "trait {}", t.name);
        if !t.supertraits.is_empty() {
            self.raw(": ");
            for (i, st) in t.supertraits.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                self.write_typeref(st);
            }
        }
        if t.required.is_empty() && t.defaults.is_empty() {
            self.raw(" {}");
            return;
        }
        self.raw(" {");
        self.indent();
        for sig in &t.required {
            self.nl();
            self.write_method_sig(sig);
            self.raw(";");
        }
        for def in &t.defaults {
            self.nl();
            // Trait default-method form: `fn name(params) -> Ret:` + body.
            self.write_fn_method(def);
        }
        self.dedent();
        self.nl();
        self.raw("}");
    }

    pub(super) fn write_method_sig(&mut self, sig: &MethodSig) {
        let _ = write!(self.buf, "fn {}(", sig.name);
        self.write_params(&sig.params);
        self.raw(")");
        if let Some(rt) = &sig.return_type {
            self.raw(" -> ");
            self.write_typeref(rt);
        }
    }

    pub(super) fn write_extend(&mut self, ext: &ExtendBlock) {
        self.write_indent();
        self.raw("extend ");
        self.write_typeref(&ext.target);
        self.raw(" {");
        if ext.methods.is_empty() {
            self.raw(" }");
            return;
        }
        self.indent();
        for m in &ext.methods {
            self.nl();
            self.write_fn_method(m);
        }
        self.dedent();
        self.nl();
        self.raw("}");
    }

    /// An `impl Trait for Target { ... }` block (T75c — associated types +
    /// method bodies). Mirrors [`write_extend`] but emits the leading
    /// `impl <trait> for <target>` header + optional `type Item = T;`
    /// bindings before the method bodies. Method bodies use the same
    /// `write_fn_method` shape as `extend` (the surface `fn` keyword + body).
    pub(super) fn write_impl_block(&mut self, imp: &ImplBlock) {
        self.write_indent();
        self.raw("impl ");
        self.write_typeref(&imp.trait_name);
        self.raw(" for ");
        self.write_typeref(&imp.target);
        self.raw(" {");
        if imp.type_bindings.is_empty() && imp.methods.is_empty() {
            self.raw(" }");
            return;
        }
        self.indent();
        for b in &imp.type_bindings {
            self.nl();
            let _ = write!(self.buf, "type {} = ", b.name);
            self.write_typeref(&b.target);
            self.raw(";");
        }
        for m in &imp.methods {
            self.nl();
            self.write_fn_method(m);
        }
        self.dedent();
        self.nl();
        self.raw("}");
    }

    /// An `extend`-block / `trait`-body method uses `fn` (not `func`) and
    /// the same body form as a regular function.
    pub(super) fn write_fn_method(&mut self, f: &FuncDecl) {
        if f.is_async {
            self.raw("async ");
        }
        if f.is_unsafe {
            self.raw("unsafe ");
        }
        let _ = write!(self.buf, "fn {}(", f.name);
        self.write_params(&f.params);
        self.raw(")");
        if let Some(rt) = &f.return_type {
            self.raw(" -> ");
            self.write_typeref(rt);
        }
        self.raw(":");
        self.write_block_body(&f.body);
    }
}
