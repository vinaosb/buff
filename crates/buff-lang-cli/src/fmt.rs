//! `buff fmt` — canonical Buff source pretty-printer (T54).
//!
//! Parses a `.buff` source string to AST, then emits canonical Buff source
//! that enforces the formatting-relevant subset of the 18 conventions (see
//! `.sisyphus/plans/buff-conventions.md`).
//!
//! # Enforced conventions
//!
//! | #  | Convention                                       | Status |
//! |----|--------------------------------------------------|--------|
//! |  2 | 4-space indentation (no tabs)                    | yes    |
//! |  2 | Line length ≤100 chars (best-effort)             | partial |
//! |  2 | No trailing whitespace                           | yes    |
//! |  2 | Max 2 consecutive blank lines                    | yes    |
//! |  2 | Trailing comma in multi-line collections         | yes    |
//! |  8 | Import ordering (alphabetical by `from` path)    | yes    |
//! | 14 | File organization (imports first)                | yes    |
//!
//! Conventions NOT mechanically enforced (formatter scope): naming (#1),
//! doc-comment structure (#3), error-message phrasing (#4), test naming
//! (#5), async naming (#6), constructor spelling (#7), deprecation tags
//! (#9), logging shape (#10), boolean named-args (#11 — preserved if
//! present, not inserted), iterator-method names (#12), Option/Result
//! methods (#13), visibility placement (#15), versioning (#16),
//! changelog format (#17), .gitignore (#18).
//!
//! # Idempotency
//!
//! [`format_source`] is idempotent: `format_source(format_source(x)) ==
//! format_source(x)` for any well-formed Buff source `x`. This is verified
//! by the test suite across the v0.1 + v0.5 example fixtures.
//!
//! # Limitations
//!
//! - **Comments are dropped**. The Buff lexer does not preserve `//` or
//!   `/* */` comments in the token stream, so an AST-level formatter
//!   cannot re-emit them. A lossless AST (T57) closes this gap. Until
//!   then, `buff fmt` is best suited for freshly-written Buff source or
//!   programs where comment-preservation isn't required.
//! - Operator-precedence parenthesization is conservative: nested binary
//!   expressions are always parenthesised (`(a + b) * c`), which
//!   guarantees idempotency at the cost of slightly more parens than a
//!   human would write.
//!
//! # Determinism
//!
//! The formatter uses ONLY `Vec` (source order) and `sort` (deterministic
//! ordering) — never `HashMap`. Same AST → byte-identical output. No RNG,
//! no system clock, no thread-id.

use std::fmt::Write;

use buff_lang_ast::{
    lossless::{parse_lossless, LosslessTree, Piece},
    Attribute, Block, Decl, EnumDecl, EnumVariant, ExportDecl, Expr, ExtendBlock, FuncDecl,
    GuardCondition, Ident, ImportDecl, InterpPart, Literal, MatchArm, MethodSig, Param, Pattern,
    ReexportDecl, Span, Stmt, StructDecl, TraitDecl, TypeRef,
};
use buff_lang_error::{ParseError, SourceId};
use buff_lang_lexer::{tokenize, LexerError};
use buff_lang_parser::parse;
use thiserror::Error;

/// Errors produced by [`format_source`].
#[derive(Debug, Error)]
pub enum FormatError {
    /// Source could not be lexed.
    #[error(transparent)]
    Lex(#[from] LexerError),
    /// Source could not be parsed.
    #[error(transparent)]
    Parse(#[from] ParseError),
}

/// Format a Buff source string into canonical form.
///
/// Lexes + parses the input, then emits canonical Buff source via the
/// [`Formatter`] pretty-printer. Idempotent on well-formed input.
///
/// # Comments (T57b)
///
/// Comments (`//` line + `/* */` block) are preserved through formatting
/// via the [`LosslessTree`] layer (T57). The lossless tree segments the
/// source into trivia + token pieces; the formatter drains comments in
/// source order, emitting each one at the appropriate position relative
/// to the surrounding AST node.
///
/// # Errors
///
/// Returns [`FormatError::Lex`] for any lex error (mixed tabs/spaces,
/// unterminated string, invalid number, …) or [`FormatError::Parse`] for
/// any syntax error.
pub fn format_source(src: &str) -> Result<String, FormatError> {
    let source_id = SourceId(0);
    let tokens = tokenize(src, source_id)?;
    let decls = parse(&tokens, source_id)?;
    let tree = parse_lossless(src);
    Ok(format_decls_with_comments(&decls, &tree))
}

/// Format a slice of parsed [`Decl`]s into canonical Buff source.
///
/// Public so that tests (and the future `buff check` linter) can format a
/// pre-parsed AST without re-running the parser. This legacy entry point
/// does NOT preserve comments — it operates on the lossy semantic AST
/// alone. Use [`format_source`] for comment-preserving formatting.
pub fn format_decls(decls: &[Decl]) -> String {
    let mut f = Formatter::new();
    f.write_decls(decls);
    f.finish()
}

/// Format a slice of parsed [`Decl`]s into canonical Buff source,
/// preserving comments via the [`LosslessTree`] (T57b).
///
/// This is the comment-preserving variant of [`format_decls`]. The tree
/// provides byte ranges of every comment in the source; the formatter
/// drains them in source order as it emits each AST node.
pub fn format_decls_with_comments(decls: &[Decl], tree: &LosslessTree) -> String {
    let mut f = Formatter::with_tree(tree);
    f.write_decls(decls);
    f.finish()
}

/// Test whether a Buff source string is already in canonical form.
///
/// Equivalent to `format_source(src) == Ok(src.to_string())` but returns
/// early on a mismatch.
pub fn is_already_formatted(src: &str) -> Result<bool, FormatError> {
    let canonical = format_source(src)?;
    Ok(canonical == src)
}

// ---------------------------------------------------------------------------
// Formatter (internal)
// ---------------------------------------------------------------------------

const INDENT_UNIT: &str = "    "; // 4 spaces
const MAX_LINE_LEN: usize = 100;

/// Internal pretty-printer state. Owns the output buffer + current indent.
///
/// The formatter is parameterised over a lifetime `'a` so it can borrow a
/// [`LosslessTree`] (T57b) for comment preservation. When constructed via
/// [`Formatter::new`], the formatter operates in legacy mode (no comments
/// preserved); [`Formatter::with_tree`] enables comment draining.
struct Formatter<'a> {
    buf: String,
    indent_level: usize,
    /// Original source bytes. Empty in legacy mode; borrowed from the
    /// [`LosslessTree`] in comment-preserving mode.
    src: &'a str,
    /// Comment pieces (line + block) sorted by source byte offset, cloned
    /// upfront from the lossless tree. Empty in legacy mode.
    comments: Vec<Piece>,
    /// Cursor into `comments` — points at the next comment not yet emitted.
    /// Monotonically increases; never resets.
    next_comment_idx: usize,
    /// Byte offset in `src` of the last byte the formatter "logically
    /// emitted" (either as an AST node or as a drained comment). Used to
    /// compute the separator between the previous emit and the next
    /// comment (counting newlines tells us trailing vs leading).
    last_emitted_byte: usize,
}

impl<'a> Formatter<'a> {
    fn new() -> Self {
        Self {
            buf: String::new(),
            indent_level: 0,
            src: "",
            comments: Vec::new(),
            next_comment_idx: 0,
            last_emitted_byte: 0,
        }
    }

    /// Construct a comment-aware formatter borrowing the given tree. The
    /// tree's comment pieces are cloned upfront (the tree itself is not
    /// retained beyond this call) so the formatter can iterate them
    /// without re-borrowing on every drain.
    fn with_tree(tree: &'a LosslessTree) -> Self {
        let comments: Vec<Piece> = tree.comments().cloned().collect();
        Self {
            buf: String::new(),
            indent_level: 0,
            src: tree.src(),
            comments,
            next_comment_idx: 0,
            last_emitted_byte: 0,
        }
    }

    /// Whether this formatter is in comment-preserving mode.
    fn has_comments(&self) -> bool {
        !self.comments.is_empty()
    }

    /// Peek the next un-emitted comment whose start offset falls in
    /// `[lo, hi)`. Returns `None` when no comment matches or the
    /// formatter is in legacy mode. Does NOT advance the cursor — the
    /// caller must call [`Self::advance_comment`] after emitting.
    fn peek_comment_in(&self, lo: usize, hi: usize) -> Option<&Piece> {
        if self.comments.is_empty() {
            return None;
        }
        let i = self.next_comment_idx;
        if i >= self.comments.len() {
            return None;
        }
        let c = &self.comments[i];
        if c.start() >= lo && c.start() < hi {
            Some(c)
        } else {
            None
        }
    }

    /// Advance the comment cursor past the just-emitted piece. The
    /// caller passes the piece's `end` byte so the formatter can update
    /// [`Self::last_emitted_byte`].
    fn advance_comment(&mut self, end_byte: usize) {
        self.next_comment_idx += 1;
        self.last_emitted_byte = end_byte;
    }

    /// Count newlines in `src[a..b]` (clamped; 0 if `a >= b`). Used to
    /// decide whether a comment is trailing (no newline between it and
    /// the previous content) or leading (newline(s) above it).
    fn newlines_between(&self, a: usize, b: usize) -> usize {
        if a >= b || b > self.src.len() {
            return 0;
        }
        let lo = a.min(self.src.len());
        let hi = b.min(self.src.len());
        if lo >= hi {
            return 0;
        }
        self.src[lo..hi].matches('\n').count()
    }

    /// Whether the output buffer currently ends at the START of a fresh
    /// line (i.e. nothing has been written on the current line yet, only
    /// indent whitespace). Used to distinguish "trailing comment" (must
    /// append ` // …` to existing content) from "leading comment on its
    /// own line" (must just write the comment at the current indent).
    fn at_line_start(&self) -> bool {
        // Walk back from the end, skipping indent whitespace (spaces).
        // If we hit a `\n` (or the buffer is empty), we're at line start.
        let bytes = self.buf.as_bytes();
        if bytes.is_empty() {
            return true;
        }
        for &b in bytes.iter().rev() {
            if b == b'\n' {
                return true;
            }
            if b != b' ' {
                return false;
            }
        }
        true
    }

    /// Emit a single comment piece at the current cursor position. For
    /// multi-line block comments, re-indent each continuation line by
    /// stripping any leading whitespace from the source and applying the
    /// current canonical indent.
    fn write_comment_text(&mut self, piece: &Piece) {
        let text = piece.text();
        // Block comments may span multiple source lines. Strip leading
        // whitespace from each continuation line and re-apply canonical
        // indent so the output is canonical regardless of input indent.
        let is_multiline = text.contains('\n');
        if !is_multiline {
            self.buf.push_str(text);
            return;
        }
        let mut first = true;
        for line in text.lines() {
            if !first {
                self.buf.push('\n');
                self.write_indent();
            }
            // Strip leading whitespace on continuation lines so output
            // uses canonical indent exclusively.
            let trimmed = line.trim_start();
            self.buf.push_str(trimmed);
            first = false;
        }
    }

    /// Drain all comments whose start byte falls in `[lo, hi)`, emitting
    /// each one at the appropriate position relative to the previous
    /// emitted content.
    ///
    /// For each comment:
    /// - Count newlines in the source between the previous anchor (the
    ///   `lo` parameter for the FIRST comment, the previous comment's end
    ///   for subsequent comments) and this comment's start.
    /// - If 0 newlines AND the cursor is NOT at line start → "trailing"
    ///   comment on the current content line. Emit ` ` + comment text.
    /// - If 0 newlines AND cursor IS at line start → leading comment on
    ///   its own line (rare; happens when a comment immediately follows
    ///   a newline but the previous emit was on a previous line).
    /// - If ≥1 newlines → leading comment. Emit `newlines_before`
    ///   newlines + indent + comment.
    ///
    /// After draining, the cursor sits at the END of the last comment
    /// (no trailing newline written). Callers handle the transition to
    /// the next AST node. [`Self::last_emitted_byte`] is updated to the
    /// last drained comment's end.
    fn drain_comments_in(&mut self, lo: usize, hi: usize) {
        // Local anchor tracks the previous emit WITHIN this drain call.
        // The first comment is anchored against `lo` (NOT
        // self.last_emitted_byte, which may reflect an emit in a
        // different scope — e.g. an outer write_decls call). This
        // isolates each drain's newline-counting from outer context.
        let mut anchor = lo;
        // Clone-and-peek to sidestep borrow-checker issues with mutating
        // self while holding a &Piece borrow. Piece is small (start/end/
        // text/kind) so cloning is cheap.
        while let Some(c) = self.peek_comment_in(lo, hi).cloned() {
            let c_start = c.start();
            let c_end = c.end();
            let newlines_before = self.newlines_between(anchor, c_start);
            if newlines_before == 0 {
                if self.at_line_start() {
                    // Cursor is on a fresh (indent-only) line. Write the
                    // comment at the current indent.
                    self.write_comment_text(&c);
                } else {
                    // Trailing: same line as previous content.
                    self.buf.push(' ');
                    self.write_comment_text(&c);
                }
            } else {
                // Leading comment on a new line. Emit `newlines_before`
                // newlines (preserving source blank-line shape) + indent
                // + comment text.
                for _ in 0..newlines_before {
                    self.buf.push('\n');
                }
                self.write_indent();
                self.write_comment_text(&c);
            }
            anchor = c_end;
            self.advance_comment(c_end);
        }
    }

    /// Drain any "orphan" comments whose byte offset is `>= lo` (no upper
    /// bound). Used at end-of-file to catch trailing comments after the
    /// last AST node. Same emission logic as [`Self::drain_comments_in`].
    fn drain_trailing_comments(&mut self, lo: usize) {
        if !self.has_comments() {
            return;
        }
        let hi = self.src.len();
        self.drain_comments_in(lo, hi);
    }

    /// Mark the formatter as having "logically emitted" up to `end_byte`
    /// — used after writing an AST node so the next drain knows where to
    /// start counting newlines from.
    fn mark_emitted_end(&mut self, end_byte: usize) {
        self.last_emitted_byte = end_byte;
    }

    /// Finalize: ensure a single trailing newline (unless the output is
    /// empty — an empty input stays empty, no trailing newline).
    fn finish(mut self) -> String {
        if self.buf.is_empty() {
            return self.buf;
        }
        while self.buf.ends_with("\n\n") {
            self.buf.pop();
        }
        if !self.buf.ends_with('\n') {
            self.buf.push('\n');
        }
        self.buf
    }

    fn indent(&mut self) {
        self.indent_level += 1;
    }

    fn dedent(&mut self) {
        if self.indent_level > 0 {
            self.indent_level -= 1;
        }
    }

    /// Write the current indentation (4 spaces × level).
    fn write_indent(&mut self) {
        for _ in 0..self.indent_level {
            self.buf.push_str(INDENT_UNIT);
        }
    }

    /// Write a literal string at the current cursor.
    fn raw(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    /// Start a new line at the current indent level.
    fn nl(&mut self) {
        self.buf.push('\n');
        self.write_indent();
    }

    /// Top-level decls entry point. Splits imports (sorted, top-of-file)
    /// from other decls (source order).
    ///
    /// # Comment preservation (T57b)
    ///
    /// When the formatter is constructed via [`Self::with_tree`], comments
    /// between top-level decls are drained in source order and emitted
    /// at the appropriate byte position. The number of newlines between
    /// consecutive decls is preserved from the source (so a blank line
    /// around an orphan comment survives). When in legacy mode (no tree),
    /// the formatter always emits `\n\n` between non-import decls and
    /// `\n` between imports — matching the pre-T57b canonical output.
    fn write_decls(&mut self, decls: &[Decl]) {
        // Partition into imports (for sorting) + others (source order).
        // Convention #14 ("imports first") justifies pulling imports to
        // the top; non-import decls preserve user order (reordering
        // functions is too aggressive for a formatter).
        let mut imports: Vec<&ImportDecl> = Vec::new();
        let mut others: Vec<&Decl> = Vec::new();
        for d in decls {
            if let Decl::ImportDecl(imp) = d {
                imports.push(imp);
            } else {
                others.push(d);
            }
        }

        // Sort imports alphabetically by canonical key. Stable sort
        // preserves source order within a tied group.
        imports.sort_by_key(|imp| sort_key(imp));

        let mut wrote_anything = false;
        for imp in &imports {
            if wrote_anything {
                self.raw("\n");
            }
            wrote_anything = true;
            self.write_indent();
            self.write_import(imp);
            if self.has_comments() {
                self.mark_emitted_end(imp.span.end);
            }
        }
        for d in &others {
            let d_span = decl_span(d);
            if wrote_anything {
                if self.has_comments() {
                    // Drain inter-decl comments in [last_byte, d_span.start).
                    self.drain_comments_in(self.last_emitted_byte, d_span.start);
                    // Preserve source newlines between last emit and this decl.
                    let n = self
                        .newlines_between(self.last_emitted_byte, d_span.start)
                        .max(1);
                    for _ in 0..n {
                        self.raw("\n");
                    }
                } else {
                    self.raw("\n\n");
                }
            } else if self.has_comments() {
                // First decl: drain file-header comments at [0, d_span.start).
                self.drain_comments_in(0, d_span.start);
                let n = self.newlines_between(self.last_emitted_byte, d_span.start);
                for _ in 0..n {
                    self.raw("\n");
                }
            }
            wrote_anything = true;
            self.write_indent();
            self.write_decl(d);
            if self.has_comments() {
                self.mark_emitted_end(d_span.end);
            }
        }

        // Trailing comments after the last decl (orphan EOF comments).
        if self.has_comments() {
            let prev = self.last_emitted_byte;
            // Compute source newlines between last decl/comment and EOF
            // so an orphan trailing comment is preceded by the same blank
            // line shape it had in the source.
            let n = self.newlines_between(prev, self.src.len());
            for _ in 0..n {
                self.raw("\n");
            }
            self.drain_trailing_comments(prev);
        }
    }

    /// Whether any un-emitted comment falls in `[lo, hi)`. Used by
    /// debug assertions and external callers to check whether a drain
    /// would emit anything without actually emitting.
    #[allow(dead_code)]
    fn has_undrained_in(&self, lo: usize, hi: usize) -> bool {
        self.peek_comment_in(lo, hi).is_some()
    }

    // ------- declarations -------

    fn write_decl(&mut self, decl: &Decl) {
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
            Decl::ExtendBlock(ext) => self.write_extend(ext),
        }
    }

    fn write_attributes(&mut self, attrs: &[Attribute]) {
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

    fn write_func(&mut self, f: &FuncDecl) {
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
    fn write_func_signature(&mut self, f: &FuncDecl) {
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

    fn write_params(&mut self, params: &[Param]) {
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

    fn write_struct(&mut self, s: &StructDecl) {
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

    fn write_enum(&mut self, e: &EnumDecl) {
        self.write_indent();
        let _ = write!(self.buf, "enum {}", e.name);
        if !e.generics.is_empty() {
            self.raw("<");
            for (i, g) in e.generics.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                let _ = write!(self.buf, "{g}");
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
    fn find_byte_after(&self, start: usize, needle: u8) -> usize {
        let start = start.min(self.src.len());
        let bytes = self.src.as_bytes();
        for (i, &b) in bytes[start..].iter().enumerate() {
            if b == needle {
                return start + i;
            }
        }
        start
    }

    fn write_enum_variant(&mut self, v: &EnumVariant) {
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
    fn write_import(&mut self, imp: &ImportDecl) {
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

    fn write_export(&mut self, exp: &ExportDecl) {
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
    fn write_reexport(&mut self, r: &ReexportDecl) {
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

    fn write_trait(&mut self, t: &TraitDecl) {
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

    fn write_method_sig(&mut self, sig: &MethodSig) {
        let _ = write!(self.buf, "fn {}(", sig.name);
        self.write_params(&sig.params);
        self.raw(")");
        if let Some(rt) = &sig.return_type {
            self.raw(" -> ");
            self.write_typeref(rt);
        }
    }

    fn write_extend(&mut self, ext: &ExtendBlock) {
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

    /// An `extend`-block / `trait`-body method uses `fn` (not `func`) and
    /// the same body form as a regular function.
    fn write_fn_method(&mut self, f: &FuncDecl) {
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

    // ------- types -------

    fn write_typeref(&mut self, ty: &TypeRef) {
        match ty {
            TypeRef::Named { name, .. } => {
                let _ = write!(self.buf, "{name}");
            }
            TypeRef::Generic { base, args, .. } => {
                self.write_typeref(base);
                self.raw("<");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    self.write_typeref(a);
                }
                self.raw(">");
            }
            TypeRef::Option(inner, _) => {
                self.raw("Option<");
                self.write_typeref(inner);
                self.raw(">");
            }
            TypeRef::Function {
                params,
                return_type,
                is_async,
                ..
            } => {
                if *is_async {
                    self.raw("async ");
                }
                self.raw("(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    self.write_typeref(p);
                }
                self.raw(") -> ");
                self.write_typeref(return_type);
            }
            TypeRef::Union(members, _) => {
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        self.raw(" | ");
                    }
                    self.write_typeref(m);
                }
            }
            TypeRef::Tuple(members, _) => {
                self.raw("(");
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    self.write_typeref(m);
                }
                self.raw(")");
            }
        }
    }

    // ------- blocks + statements -------

    /// Write a layout-block body (`: NEWLINE INDENT ... DEDENT`).
    ///
    /// Assumes the leading `:` has already been written. Bumps indent for
    /// the body, writes one statement per line, then dedents.
    ///
    /// # Comment preservation (T57b)
    ///
    /// When in comment-aware mode, drains comments inside the block's byte
    /// range before each stmt (leading comments), after each stmt
    /// (trailing same-line comments), and at end-of-block (after the last
    /// stmt). Source-order byte ranges ensure each comment is emitted
    /// exactly once at its correct position.
    fn write_block_body(&mut self, block: &Block) {
        if block.stmts.is_empty() {
            // No stmts, but there might be comments inside the empty
            // body. Drain them at the current (post-indent) level.
            if self.has_comments() {
                self.indent();
                self.drain_block_trailing(block);
                self.dedent();
            }
            return;
        }
        self.indent();
        let mut prev_end = block.span.start;
        for s in &block.stmts {
            let s_span = stmt_span(s);
            // Drain leading comments before this stmt (in [prev_end, s_start)).
            // Stops at the first comment that requires a newline (leading).
            if self.has_comments() {
                self.drain_comments_in(prev_end, s_span.start);
            }
            self.nl();
            self.write_stmt(s);
            if self.has_comments() {
                // Mark this stmt's end so drain's newline counting is
                // anchored correctly.
                self.mark_emitted_end(s_span.end);
                // Drain trailing same-line comments only (e.g. `let x = 5 // foo`).
                self.drain_trailing_after();
                // prev_end updated to wherever we ended (may be past a
                // trailing comment) so the next iteration doesn't re-drain.
                prev_end = self.last_emitted_byte;
            }
        }
        // Drain any comments inside the block but after the last stmt
        // (e.g. `// end of body` inside a function).
        if self.has_comments() {
            self.drain_block_trailing(block);
        }
        self.dedent();
    }

    /// Drain trailing same-line comments — i.e. comments whose start has
    /// NO newline between it and [`Self::last_emitted_byte`]. Stops at the
    /// first comment that requires a newline (those are leading comments
    /// for the NEXT stmt and will be drained by the next iteration).
    ///
    /// Emits each trailing comment as ` ` + comment text (appended to the
    /// current line, no newline introduced).
    fn drain_trailing_after(&mut self) {
        if !self.has_comments() {
            return;
        }
        let hi = self.src.len();
        loop {
            let next = self.peek_comment_in(0, hi).cloned();
            let Some(c) = next else { break };
            let nlines = self.newlines_between(self.last_emitted_byte, c.start());
            if nlines > 0 {
                // Leading comment — not trailing. Leave for next iter.
                break;
            }
            // Trailing: append space + comment text on the current line.
            if !self.at_line_start() {
                self.buf.push(' ');
            }
            self.write_comment_text(&c);
            self.advance_comment(c.end());
        }
    }

    /// Drain comments inside a block's byte range that fall AFTER the last
    /// stmt. Used to emit "end-of-body" comments like:
    ///
    /// ```text
    /// func foo():
    ///     print("a")
    ///     // end of body
    /// ```
    ///
    /// The drain function preserves the source's blank-line shape. Cursor
    /// ends at the last drained comment's end (no trailing newline).
    ///
    /// # Body scope extension
    ///
    /// The parser's `block.span.end` only covers up to the last STMT's
    /// end (comments aren't tokens). A trailing body comment that comes
    /// AFTER the last stmt but BEFORE the source's indent drops back to
    /// the parent level is logically still "inside" the body. We extend
    /// the drain upper bound to [`Self::body_scope_end`] to catch such
    /// comments.
    fn drain_block_trailing(&mut self, block: &Block) {
        let lo = if let Some(last) = block.stmts.last() {
            stmt_span(last).end
        } else {
            block.span.start
        };
        // Extend hi to include trailing body-level comments past the
        // parser's block.span.end. The body scope ends where the source's
        // indentation drops below the body's expected indent.
        let body_indent_spaces = self.indent_level * INDENT_UNIT.len();
        let hi = self.body_scope_end(block.span.end, body_indent_spaces);
        // Drain any comments in (last_stmt_end, body_scope_end).
        self.drain_comments_in(lo, hi);
    }

    /// Walk source forward from `from`, finding the byte offset where the
    /// source's per-line indentation drops below `body_indent_spaces`.
    /// Used to determine the effective end of a layout-block body when
    /// trailing comments inside the body extend past the parser's
    /// `block.span.end`.
    ///
    /// Returns `from` immediately if no newline follows (the body ends
    /// at EOF without trailing content). Otherwise scans each subsequent
    /// line: blank lines and lines indented ≥ `body_indent_spaces`
    /// extend the body scope; the first line indented `<
    /// body_indent_spaces` ends it (the byte offset of that line's
    /// start is returned).
    fn body_scope_end(&self, from: usize, body_indent_spaces: usize) -> usize {
        let bytes = self.src.as_bytes();
        let mut pos = from;
        while pos < bytes.len() {
            // Find next newline at or after pos.
            let nl_idx = match bytes[pos..].iter().position(|&b| b == b'\n') {
                Some(i) => i,
                None => return bytes.len(),
            };
            let line_start = pos + nl_idx + 1;
            if line_start >= bytes.len() {
                return bytes.len();
            }
            // Count leading spaces on this line.
            let mut indent = 0;
            let mut p = line_start;
            while p < bytes.len() && bytes[p] == b' ' {
                indent += 1;
                p += 1;
            }
            // Blank line (only spaces, then newline) OR line indented ≥
            // body level → still inside body scope.
            let is_blank = p >= bytes.len() || (bytes[p] == b'\n') || (p == bytes.len());
            if is_blank || indent >= body_indent_spaces {
                // Move past this line; continue scanning.
                pos = p;
                while pos < bytes.len() && bytes[pos] != b'\n' {
                    pos += 1;
                }
                // Loop will re-find the next newline starting at pos.
                continue;
            }
            // Less-indented line → body scope ends here.
            return line_start;
        }
        bytes.len()
    }

    fn write_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::LetDecl {
                name,
                value,
                mutable,
                ty,
                ..
            } => {
                self.raw("let ");
                if *mutable {
                    self.raw("mut ");
                }
                let _ = write!(self.buf, "{name}");
                if let Some(t) = ty {
                    self.raw(": ");
                    self.write_typeref(t);
                }
                self.raw(" = ");
                self.write_expr(value);
            }
            Stmt::LetPattern {
                pattern,
                value,
                mutable,
                ty,
                ..
            } => {
                self.raw("let ");
                if *mutable {
                    self.raw("mut ");
                }
                self.write_pattern(pattern);
                if let Some(t) = ty {
                    self.raw(": ");
                    self.write_typeref(t);
                }
                self.raw(" = ");
                self.write_expr(value);
            }
            Stmt::Assignment {
                target, op, value, ..
            } => {
                self.write_expr(target);
                let _ = write!(self.buf, " {op} ");
                self.write_expr(value);
            }
            Stmt::ExprStmt(e, _) => self.write_expr(e),
            Stmt::Return(Some(e), _) => {
                self.raw("return ");
                self.write_expr(e);
            }
            Stmt::Return(None, _) => {
                self.raw("return");
            }
            Stmt::Break(_) => self.raw("break"),
            Stmt::Continue(_) => self.raw("continue"),
            Stmt::ForIn {
                var, iter, body, ..
            } => {
                let _ = write!(self.buf, "for {var} in ");
                self.write_expr(iter);
                self.raw(":");
                self.write_block_body(body);
            }
            Stmt::ForWhile { cond, body, .. } => {
                self.raw("for ");
                self.write_expr(cond);
                self.raw(":");
                self.write_block_body(body);
            }
            Stmt::ForLet {
                pattern,
                value,
                body,
                ..
            } => {
                self.raw("for let ");
                self.write_pattern(pattern);
                self.raw(" = ");
                self.write_expr(value);
                self.raw(":");
                self.write_block_body(body);
            }
            Stmt::Guard {
                conditions,
                else_block,
                ..
            } => {
                self.raw("guard ");
                for (i, c) in conditions.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    match c {
                        GuardCondition::Let { pattern, value, .. } => {
                            self.raw("let ");
                            self.write_pattern(pattern);
                            self.raw(" = ");
                            self.write_expr(value);
                        }
                        GuardCondition::Bool(e) => self.write_expr(e),
                    }
                }
                self.raw(" else:");
                self.write_block_body(else_block);
            }
            Stmt::Defer { expr, .. } => {
                self.raw("defer ");
                self.write_expr(expr);
            }
        }
    }

    // ------- expressions -------

    fn write_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal(lit, _) => self.write_literal(lit),
            Expr::Ident(name, _) => {
                let _ = write!(self.buf, "{name}");
            }
            Expr::BinaryOp { op, lhs, rhs, .. } => {
                self.write_expr_operand(lhs);
                let _ = write!(self.buf, " {op} ");
                self.write_expr_operand(rhs);
            }
            Expr::UnaryOp { op, operand, .. } => {
                let s = match op {
                    buff_lang_ast::UnaryOp::Neg => "-",
                    buff_lang_ast::UnaryOp::Not => "!",
                    buff_lang_ast::UnaryOp::BitNot => "~",
                };
                self.raw(s);
                self.write_expr_operand(operand);
            }
            Expr::IfExpr {
                cond,
                then_block,
                else_block,
                ..
            } => self.write_if_expr(cond, then_block, else_block.as_ref()),
            Expr::FuncCall { callee, args, .. } => {
                self.write_expr_operand(callee);
                self.write_call_args(args);
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                self.write_expr_operand(receiver);
                let _ = write!(self.buf, ".{method}");
                self.write_call_args(args);
            }
            Expr::Lambda {
                params,
                body,
                return_type,
                ..
            } => self.write_lambda(params, body, return_type.as_ref()),
            Expr::StructInit {
                type_name, fields, ..
            } => self.write_struct_init(type_name, fields),
            Expr::MatchExpr {
                scrutinee, arms, ..
            } => {
                self.write_match(scrutinee, arms);
            }
            Expr::SuspendExpr { inner, .. } => {
                self.raw("suspend ");
                self.write_expr_operand(inner);
            }
            Expr::ArrayLit { elements, .. } => self.write_array_lit(elements),
            Expr::Index { base, indices, .. } => {
                self.write_expr_operand(base);
                self.raw("[");
                for (i, idx) in indices.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    self.write_expr(idx);
                }
                self.raw("]");
            }
            Expr::StringInterp { parts, .. } => self.write_string_interp(parts),
            Expr::MapLit { entries, .. } => self.write_map_lit(entries),
            Expr::Try { expr, .. } => {
                self.write_expr_operand(expr);
                self.raw("?");
            }
            Expr::Spawn { task, .. } => {
                self.raw("spawn ");
                self.write_expr_operand(task);
            }
            Expr::Range {
                start,
                end,
                inclusive,
                ..
            } => {
                self.write_expr_operand(start);
                if *inclusive {
                    self.raw("..=");
                } else {
                    self.raw("..");
                }
                self.write_expr_operand(end);
            }
            Expr::IfLet {
                pattern,
                value,
                then_block,
                else_block,
                ..
            } => self.write_if_let(pattern, value, then_block, else_block.as_ref()),
            Expr::TupleLit(members, _) => {
                self.raw("(");
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    self.write_expr(m);
                }
                self.raw(")");
            }
            Expr::NamedArg { name, value, .. } => {
                let _ = write!(self.buf, "{name}: ");
                self.write_expr(value);
            }
        }
    }

    /// Write an expression in operand position — parenthesises anything
    /// that could change precedence (BinaryOp, UnaryOp, IfExpr, Lambda,
    /// MatchExpr, Range, IfLet, StructInit, MapLit). Simple primaries
    /// (literals, idents, calls) are written as-is.
    fn write_expr_operand(&mut self, expr: &Expr) {
        let needs_paren = matches!(
            expr,
            Expr::BinaryOp { .. }
                | Expr::UnaryOp { .. }
                | Expr::IfExpr { .. }
                | Expr::Lambda { .. }
                | Expr::MatchExpr { .. }
                | Expr::Range { .. }
                | Expr::IfLet { .. }
                | Expr::StructInit { .. }
                | Expr::MapLit { .. }
                | Expr::SuspendExpr { .. }
                | Expr::Spawn { .. }
        );
        if needs_paren {
            self.raw("(");
            self.write_expr(expr);
            self.raw(")");
        } else {
            self.write_expr(expr);
        }
    }

    fn write_literal(&mut self, lit: &Literal) {
        match lit {
            Literal::Int(v) => {
                let _ = write!(self.buf, "{v}");
            }
            Literal::Float(v) => {
                // Rust's Debug form for f32 round-trips losslessly through
                // the Buff lexer's `parse::<f32>()` path. Plain Display is
                // not enough (it can drop trailing digits).
                let _ = write!(self.buf, "{v:?}");
            }
            Literal::Double(v) => {
                let _ = write!(self.buf, "{v:?}");
                self.raw("d");
            }
            Literal::Bool(v) => {
                let _ = write!(self.buf, "{v}");
            }
            Literal::String(s) => {
                self.write_string_literal(s);
            }
            Literal::Byte(v) => {
                let _ = write!(self.buf, "0x{v:02X}");
            }
            Literal::Char(c) => {
                let _ = write!(self.buf, "{c:?}");
            }
            Literal::Decimal(text) => {
                let _ = write!(self.buf, "{text}m");
            }
            Literal::Regex(pattern) => {
                let _ = write!(self.buf, "/{pattern}/");
            }
        }
    }

    /// Emit a double-quoted string literal with escapes. Round-trips with
    /// the lexer's plain-string scan (no interpolation).
    fn write_string_literal(&mut self, s: &str) {
        self.buf.push('"');
        for c in s.chars() {
            match c {
                '"' => self.buf.push_str("\\\""),
                '\\' => self.buf.push_str("\\\\"),
                '\n' => self.buf.push_str("\\n"),
                '\r' => self.buf.push_str("\\r"),
                '\t' => self.buf.push_str("\\t"),
                _ => self.buf.push(c),
            }
        }
        self.buf.push('"');
    }

    /// Emit a string-interpolation expression `"…{expr}…"` using braces.
    fn write_string_interp(&mut self, parts: &[InterpPart]) {
        self.buf.push('"');
        for part in parts {
            match part {
                InterpPart::Literal(s) => {
                    for c in s.chars() {
                        match c {
                            '"' => self.buf.push_str("\\\""),
                            '\\' => self.buf.push_str("\\\\"),
                            '\n' => self.buf.push_str("\\n"),
                            '\r' => self.buf.push_str("\\r"),
                            '\t' => self.buf.push_str("\\t"),
                            _ => self.buf.push(c),
                        }
                    }
                }
                InterpPart::Expr(e) => {
                    self.buf.push('{');
                    self.write_expr(e);
                    self.buf.push('}');
                }
            }
        }
        self.buf.push('"');
    }

    fn write_call_args(&mut self, args: &[Expr]) {
        self.raw("(");
        // Multi-line when the inline form would exceed MAX_LINE_LEN.
        if self.would_overflow(args) {
            // Multi-line: each arg on its own line with trailing comma.
            self.indent();
            for a in args {
                self.nl();
                self.write_expr(a);
                self.raw(",");
            }
            self.dedent();
            self.nl();
        } else {
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                self.write_expr(a);
            }
        }
        self.raw(")");
    }

    /// Heuristic: would writing `args` inline push the current line past
    /// [`MAX_LINE_LEN`]? Uses a rough byte estimate.
    fn would_overflow(&self, args: &[Expr]) -> bool {
        if args.len() < 4 {
            return false;
        }
        let column = self.current_column();
        let est_inline_len =
            column + args.iter().map(est_expr_len).sum::<usize>() + (args.len() - 1) * 2;
        est_inline_len > MAX_LINE_LEN
    }

    fn current_column(&self) -> usize {
        match self.buf.rfind('\n') {
            Some(i) => self.buf.len() - i - 1,
            None => self.buf.len(),
        }
    }

    fn write_struct_init(&mut self, name: &Ident, fields: &[(Ident, Expr)]) {
        let _ = write!(self.buf, "{name} {{");
        if fields.is_empty() {
            self.raw(" }");
            return;
        }
        let multiline = fields.len() >= 3 || fields.iter().any(|(_, v)| est_expr_len(v) > 20);
        if multiline {
            self.indent();
            for (n, v) in fields {
                self.nl();
                let _ = write!(self.buf, "{n}: ");
                self.write_expr(v);
                self.raw(",");
            }
            self.dedent();
            self.nl();
            self.raw("}");
        } else {
            for (i, (n, v)) in fields.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                let _ = write!(self.buf, "{n}: ");
                self.write_expr(v);
            }
            self.raw(" }");
        }
    }

    fn write_array_lit(&mut self, elements: &[Expr]) {
        self.raw("[");
        if elements.is_empty() {
            self.raw("]");
            return;
        }
        let multiline = elements.len() >= 4 || elements.iter().any(|e| est_expr_len(e) > 20);
        if multiline {
            self.indent();
            for e in elements {
                self.nl();
                self.write_expr(e);
                self.raw(",");
            }
            self.dedent();
            self.nl();
            self.raw("]");
        } else {
            for (i, e) in elements.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                self.write_expr(e);
            }
            self.raw("]");
        }
    }

    fn write_map_lit(&mut self, entries: &[(Expr, Expr)]) {
        if entries.is_empty() {
            self.raw("{:}");
            return;
        }
        self.raw("{");
        let multiline = entries.len() >= 3
            || entries
                .iter()
                .any(|(k, v)| est_expr_len(k) + est_expr_len(v) > 20);
        if multiline {
            self.indent();
            for (k, v) in entries {
                self.nl();
                self.write_expr(k);
                self.raw(": ");
                self.write_expr(v);
                self.raw(",");
            }
            self.dedent();
            self.nl();
            self.raw("}");
        } else {
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                self.write_expr(k);
                self.raw(": ");
                self.write_expr(v);
            }
            self.raw("}");
        }
    }

    fn write_if_expr(&mut self, cond: &Expr, then_block: &Block, else_block: Option<&Block>) {
        self.raw("if ");
        self.write_expr(cond);
        self.raw(":");
        self.write_block_body(then_block);
        if let Some(els) = else_block {
            // After `write_block_body` the cursor sits at the end of the
            // last body statement; indent_level is back to the original
            // value. `nl()` writes newline + current-indent, which is
            // exactly what `else:` needs to align with the `if`.
            self.nl();
            self.raw("else:");
            self.write_block_body(els);
        }
    }

    fn write_if_let(
        &mut self,
        pattern: &Pattern,
        value: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) {
        self.raw("if let ");
        self.write_pattern(pattern);
        self.raw(" = ");
        self.write_expr(value);
        self.raw(":");
        self.write_block_body(then_block);
        if let Some(els) = else_block {
            self.nl();
            self.raw("else:");
            self.write_block_body(els);
        }
    }

    fn write_lambda(&mut self, params: &[Param], body: &Block, _return_type: Option<&TypeRef>) {
        // Buff lambda canonical form: `{ params => body }` (single-line).
        // Multi-statement bodies use `; ` separators inside the braces.
        self.raw("{ ");
        if params.is_empty() {
            // No params — bare `=>`.
        } else {
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                let _ = write!(self.buf, "{}", p.name);
            }
        }
        self.raw(" => ");
        if body.stmts.is_empty() {
            self.raw("()");
        } else {
            for (i, s) in body.stmts.iter().enumerate() {
                if i > 0 {
                    self.raw("; ");
                }
                self.write_stmt(s);
            }
        }
        self.raw(" }");
    }

    fn write_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) {
        // Single-line form when there are ≤2 simple arms.
        let inline_ok = arms.len() <= 2
            && arms
                .iter()
                .all(|a| a.body.stmts.len() == 1 && est_block_len(&a.body) < 30);
        if inline_ok {
            self.raw("match ");
            self.write_expr(scrutinee);
            self.raw(" { ");
            for (i, arm) in arms.iter().enumerate() {
                if i > 0 {
                    self.raw(", ");
                }
                self.write_pattern(&arm.pattern);
                self.raw(" => ");
                self.write_stmt(&arm.body.stmts[0]);
            }
            self.raw(" }");
        } else {
            self.raw("match ");
            self.write_expr(scrutinee);
            self.raw(":");
            self.indent();
            for arm in arms {
                self.nl();
                self.write_pattern(&arm.pattern);
                self.raw(" => ");
                if arm.body.stmts.len() == 1 {
                    self.write_stmt(&arm.body.stmts[0]);
                } else {
                    for (i, s) in arm.body.stmts.iter().enumerate() {
                        if i > 0 {
                            self.raw("; ");
                        }
                        self.write_stmt(s);
                    }
                }
            }
            self.dedent();
        }
    }

    fn write_pattern(&mut self, pat: &Pattern) {
        match pat {
            Pattern::Wildcard(_) => self.raw("_"),
            Pattern::Literal(lit, _) => self.write_literal(lit),
            Pattern::Ident(name, _) => {
                let _ = write!(self.buf, "{name}");
            }
            Pattern::Variant {
                enum_name,
                variant,
                subpatterns,
                ..
            } => {
                // The parser stores an empty `enum_name` when the user
                // wrote a bare variant name like `Some(x)` (no `Option::`
                // prefix). Re-emit the bare form in that case so the
                // round-trip re-parses cleanly.
                if !enum_name.name.is_empty() {
                    let _ = write!(self.buf, "{enum_name}::");
                }
                let _ = write!(self.buf, "{variant}");
                if !subpatterns.is_empty() {
                    self.raw("(");
                    for (i, p) in subpatterns.iter().enumerate() {
                        if i > 0 {
                            self.raw(", ");
                        }
                        self.write_pattern(p);
                    }
                    self.raw(")");
                }
            }
            Pattern::Tuple(subs, _) => {
                self.raw("(");
                for (i, p) in subs.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    self.write_pattern(p);
                }
                self.raw(")");
            }
            Pattern::Struct { name, fields, .. } => {
                let _ = write!(self.buf, "{name} {{ ");
                for (i, (fname, p)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.raw(", ");
                    }
                    let _ = write!(self.buf, "{fname}: ");
                    self.write_pattern(p);
                }
                self.raw(" }");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Byte span of a top-level [`Decl`]. Used by the comment-draining logic
/// to compute byte ranges between adjacent decls (e.g. inter-decl
/// comments). Matches every variant of [`Decl`].
fn decl_span(d: &Decl) -> Span {
    match d {
        Decl::FuncDecl(f) => f.span,
        Decl::StructDecl(s) => s.span,
        Decl::EnumDecl(e) => e.span,
        Decl::ImportDecl(i) => i.span,
        Decl::ModuleDecl(m) => m.span,
        Decl::TraitDecl(t) => t.span,
        Decl::ExportDecl(e) => e.span,
        Decl::ReexportDecl(r) => r.span,
        Decl::ExternCrateDecl(c) => c.span,
        Decl::ExtendBlock(ext) => ext.span,
    }
}

/// Byte span of a [`Stmt`]. Used by the comment-draining logic inside
/// function bodies (drains between adjacent stmts).
fn stmt_span(s: &Stmt) -> Span {
    match s {
        Stmt::LetDecl { span, .. }
        | Stmt::Assignment { span, .. }
        | Stmt::ForIn { span, .. }
        | Stmt::ForWhile { span, .. }
        | Stmt::LetPattern { span, .. }
        | Stmt::ForLet { span, .. }
        | Stmt::Guard { span, .. }
        | Stmt::Defer { span, .. } => *span,
        Stmt::ExprStmt(_, sp) | Stmt::Return(_, sp) | Stmt::Break(sp) | Stmt::Continue(sp) => *sp,
    }
}

/// Sort key for an ImportDecl. ES6 form uses `from_path`; legacy form uses
/// the dotted path. Empty fallback ensures stable ordering.
fn sort_key(imp: &ImportDecl) -> String {
    if let Some(from) = &imp.from_path {
        from.clone()
    } else if imp.path.is_empty() {
        String::new()
    } else {
        imp.path
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }
}

/// Rough byte-cost estimate of an expression for line-wrapping decisions.
fn est_expr_len(expr: &Expr) -> usize {
    match expr {
        Expr::Literal(lit, _) => match lit {
            Literal::Int(v) => v.to_string().len(),
            Literal::Float(v) => format!("{v:?}").len(),
            Literal::Double(v) => format!("{v:?}").len() + 1,
            Literal::Bool(v) => v.to_string().len(),
            Literal::String(s) => s.len() + 2,
            Literal::Byte(v) => format!("0x{v:02X}").len(),
            Literal::Char(c) => c.len_utf8() + 2,
            Literal::Decimal(s) => s.len() + 1,
            Literal::Regex(s) => s.len() + 2,
        },
        Expr::Ident(i, _) => i.name.len(),
        Expr::NamedArg { name, value, .. } => name.name.len() + 2 + est_expr_len(value),
        Expr::BinaryOp { lhs, rhs, .. } => est_expr_len(lhs) + est_expr_len(rhs) + 3,
        Expr::UnaryOp { operand, .. } => est_expr_len(operand) + 1,
        Expr::FuncCall { callee, args, .. } => {
            est_expr_len(callee) + 2 + args.iter().map(est_expr_len).sum::<usize>()
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            est_expr_len(receiver)
                + method.name.len()
                + 2
                + args.iter().map(est_expr_len).sum::<usize>()
        }
        _ => 16,
    }
}

fn est_block_len(block: &Block) -> usize {
    block.stmts.iter().map(est_stmt_len).sum()
}

fn est_stmt_len(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::ExprStmt(e, _) => est_expr_len(e),
        Stmt::Return(Some(e), _) => 7 + est_expr_len(e),
        Stmt::Return(None, _) => 6,
        Stmt::Break(_) => 5,
        Stmt::Continue(_) => 8,
        Stmt::LetDecl { value, .. } => 8 + est_expr_len(value),
        Stmt::Assignment { target, value, .. } => est_expr_len(target) + est_expr_len(value) + 3,
        _ => 24,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_empty() {
        assert_eq!(format_source("").unwrap(), "");
    }

    #[test]
    fn basic_func_round_trips() {
        let src = "func main():\n    print(\"hello\")\n";
        let out = format_source(src).unwrap();
        assert_eq!(out, src);
    }

    #[test]
    fn idempotent_on_simple_func() {
        let src = "func main():\n    print(\"hello\")\n";
        let once = format_source(src).unwrap();
        let twice = format_source(&once).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn two_space_indent_normalized_to_four() {
        let src = "func main():\n  print(\"hello\")\n";
        let out = format_source(src).unwrap();
        assert!(
            out.contains("    print("),
            "expected 4-space indent, got: {out}"
        );
        assert!(!out.contains("\n  print"), "found 2-space indent in output");
    }

    #[test]
    fn imports_sorted_alphabetically() {
        let src = "import { zebra } from \"./z\"\nimport { alpha } from \"./a\"\nfunc main():\n    print(\"hi\")\n";
        let out = format_source(src).unwrap();
        let alpha_pos = out.find("alpha").unwrap_or(usize::MAX);
        let zebra_pos = out.find("zebra").unwrap_or(usize::MAX);
        assert!(
            alpha_pos < zebra_pos,
            "alpha should come before zebra. Output:\n{out}"
        );
    }
}
