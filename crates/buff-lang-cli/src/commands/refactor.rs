//! `buff refactor` — non-interactive refactoring tools (T66).
//!
//! Three subcommands operate on `.buff` source by parsing it to AST,
//! applying a transformation, and writing the canonical formatted
//! output back via the T54 formatter
//! ([`crate::fmt::format_decls`]).
//!
//! - [`RefactorCmd::Rename`] — walk every identifier-bearing node in
//!   the AST and replace name matches. Scope-unaware MVP.
//! - [`RefactorCmd::ExtractFunction`] — lift a line range inside the
//!   first function into a new top-level function.
//! - [`RefactorCmd::InlineVariable`] — find the first `let NAME = expr`
//!   and replace every subsequent reference in the same function body
//!   with `expr`.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use buff_lang_ast::{Block, Decl, Expr, FuncDecl, Ident, Literal, Param, Pattern, Stmt, TypeRef};
use buff_lang_error::{SourceId, Span};
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

use crate::cli::RefactorCmd;
use crate::fmt;

pub fn run(cmd: RefactorCmd) -> Result<()> {
    match cmd {
        RefactorCmd::Rename { old, new, files } => run_rename(&old, &new, files.as_deref()),
        RefactorCmd::ExtractFunction { file, range, name } => {
            run_extract_function(&file, &range, &name)
        }
        RefactorCmd::InlineVariable { file, name } => run_inline_variable(&file, &name),
    }
}

fn run_rename(old: &str, new: &str, files: Option<&Path>) -> Result<()> {
    validate_identifier(old, "old")?;
    validate_identifier(new, "new")?;
    if old == new {
        bail!("old and new names are identical (`{old}`)");
    }

    let targets = resolve_targets(files)?;
    if targets.is_empty() {
        eprintln!("refactor rename: no .buff files found");
        return Ok(());
    }

    let mut changed = 0usize;
    for path in &targets {
        let src = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read `{}`", path.display()))?;
        let out = apply_rename_to_source(&src, old, new)?;
        if out == src {
            continue;
        }
        std::fs::write(path, &out)
            .with_context(|| format!("failed to write `{}`", path.display()))?;
        changed += 1;
    }
    eprintln!(
        "refactor rename: `{old}` → `{new}` applied to {changed} of {} file(s)",
        targets.len()
    );
    Ok(())
}

fn run_extract_function(file: &Path, range: &str, name: &str) -> Result<()> {
    validate_identifier(name, "name")?;

    let (start_line, end_line) = parse_line_range(range)?;
    if start_line == 0 || end_line == 0 {
        bail!("line numbers are 1-indexed; got `{range}`");
    }
    if end_line < start_line {
        bail!("range end ({end_line}) is before start ({start_line}) in `{range}`");
    }

    let src = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read `{}`", file.display()))?;
    let out = apply_extract_to_source(&src, start_line, end_line, name)?;
    std::fs::write(file, &out).with_context(|| format!("failed to write `{}`", file.display()))?;
    eprintln!(
        "refactor extract-function: extracted lines [{start_line}, {end_line}] \
         into new function `{name}` in `{}`",
        file.display()
    );
    Ok(())
}

fn run_inline_variable(file: &Path, name: &str) -> Result<()> {
    validate_identifier(name, "name")?;

    let src = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read `{}`", file.display()))?;
    let out = apply_inline_to_source(&src, name)?;
    std::fs::write(file, &out).with_context(|| format!("failed to write `{}`", file.display()))?;
    eprintln!(
        "refactor inline-variable: inlined `let {name}` in `{}`",
        file.display()
    );
    Ok(())
}

struct RenameWalker {
    old: String,
    new: String,
    changed: bool,
}

impl RenameWalker {
    fn new(old: &str, new: &str) -> Self {
        Self {
            old: old.to_string(),
            new: new.to_string(),
            changed: false,
        }
    }

    fn maybe_rename(&mut self, ident: &mut Ident) {
        if ident.name == self.old {
            ident.name = self.new.clone();
            self.changed = true;
        }
    }

    fn rename_decl(&mut self, decl: &mut Decl) {
        match decl {
            Decl::FuncDecl(f) => self.rename_func(f),
            Decl::StructDecl(s) => {
                self.maybe_rename(&mut s.name);
                for (fname, _) in &mut s.fields {
                    self.maybe_rename(fname);
                }
                for t in &mut s.traits {
                    self.maybe_rename(t);
                }
            }
            Decl::EnumDecl(e) => {
                self.maybe_rename(&mut e.name);
                for tp in &mut e.type_params {
                    self.maybe_rename(&mut tp.name);
                }
                for v in &mut e.variants {
                    self.maybe_rename(&mut v.name);
                }
            }
            Decl::ImportDecl(i) => {
                for p in &mut i.path {
                    self.maybe_rename(p);
                }
                for n in &mut i.imports {
                    self.maybe_rename(n);
                }
                if let Some(a) = &mut i.alias {
                    self.maybe_rename(a);
                }
            }
            Decl::ModuleDecl(m) => self.maybe_rename(&mut m.name),
            Decl::TraitDecl(t) => {
                self.maybe_rename(&mut t.name);
                for req in &mut t.required {
                    self.rename_method_sig(req);
                }
                for def in &mut t.defaults {
                    self.rename_func(def);
                }
            }
            Decl::ExportDecl(e) => self.rename_decl(&mut e.inner),
            Decl::ReexportDecl(r) => {
                for n in &mut r.names {
                    self.maybe_rename(n);
                }
            }
            Decl::ExternCrateDecl(_) => {}
            Decl::ExternFuncDecl(e) => {
                self.maybe_rename(&mut e.name);
                for p in &mut e.params {
                    self.rename_param(p);
                }
            }
            Decl::ExtendBlock(ext) => {
                self.rename_typeref(&mut ext.target);
                for m in &mut ext.methods {
                    self.rename_func(m);
                }
            }
            Decl::ImplBlock(imp) => {
                self.rename_typeref(&mut imp.trait_name);
                self.rename_typeref(&mut imp.target);
                for m in &mut imp.methods {
                    self.rename_func(m);
                }
            }
        }
    }

    fn rename_func(&mut self, f: &mut FuncDecl) {
        self.maybe_rename(&mut f.name);
        for p in &mut f.params {
            self.rename_param(p);
        }
        for a in &mut f.attributes {
            self.maybe_rename(&mut a.name);
        }
        self.rename_block(&mut f.body);
    }

    fn rename_method_sig(&mut self, sig: &mut buff_lang_ast::MethodSig) {
        self.maybe_rename(&mut sig.name);
        for p in &mut sig.params {
            self.rename_param(p);
        }
        if let Some(rt) = &mut sig.return_type {
            self.rename_typeref(rt);
        }
    }

    fn rename_param(&mut self, p: &mut Param) {
        self.maybe_rename(&mut p.name);
        self.rename_typeref(&mut p.ty);
        if let Some(dv) = &mut p.default_value {
            self.rename_expr(dv);
        }
    }

    fn rename_typeref(&mut self, t: &mut TypeRef) {
        match t {
            TypeRef::Named { name, span: _ } => self.maybe_rename(name),
            TypeRef::Generic { base, args, .. } => {
                self.rename_typeref(base);
                for a in args {
                    self.rename_typeref(a);
                }
            }
            TypeRef::Function {
                params,
                return_type,
                ..
            } => {
                for p in params {
                    self.rename_typeref(p);
                }
                self.rename_typeref(return_type);
            }
            _ => {}
        }
    }

    fn rename_block(&mut self, b: &mut Block) {
        for s in &mut b.stmts {
            self.rename_stmt(s);
        }
    }

    fn rename_stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::LetDecl { name, value, .. } => {
                self.maybe_rename(name);
                self.rename_expr(value);
            }
            Stmt::Assignment { target, value, .. } => {
                self.rename_expr(target);
                self.rename_expr(value);
            }
            Stmt::ExprStmt(e, _) => self.rename_expr(e),
            Stmt::Return(e, _) => {
                if let Some(e) = e {
                    self.rename_expr(e);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::ForIn {
                var, iter, body, ..
            } => {
                self.maybe_rename(var);
                self.rename_expr(iter);
                self.rename_block(body);
            }
            Stmt::ForWhile { cond, body, .. } => {
                self.rename_expr(cond);
                self.rename_block(body);
            }
            Stmt::LetPattern { pattern, value, .. } => {
                self.rename_pattern(pattern);
                self.rename_expr(value);
            }
            Stmt::ForLet { pattern, value, .. } => {
                self.rename_pattern(pattern);
                self.rename_expr(value);
            }
            Stmt::Guard {
                conditions,
                else_block,
                ..
            } => {
                for c in conditions {
                    match c {
                        buff_lang_ast::GuardCondition::Let { pattern, value, .. } => {
                            self.rename_pattern(pattern);
                            self.rename_expr(value);
                        }
                        buff_lang_ast::GuardCondition::Bool(e) => self.rename_expr(e),
                    }
                }
                self.rename_block(else_block);
            }
            Stmt::Defer { expr, .. } => self.rename_expr(expr),
            Stmt::ComptimeBlock { body, .. } => self.rename_block(body),
        }
    }

    fn rename_pattern(&mut self, p: &mut Pattern) {
        match p {
            Pattern::Ident(name, _) => self.maybe_rename(name),
            Pattern::Variant {
                enum_name,
                variant,
                subpatterns,
                ..
            } => {
                self.maybe_rename(enum_name);
                self.maybe_rename(variant);
                for sp in subpatterns {
                    self.rename_pattern(sp);
                }
            }
            Pattern::Tuple(subs, _) => {
                for sp in subs {
                    self.rename_pattern(sp);
                }
            }
            Pattern::Struct { name, fields, .. } => {
                self.maybe_rename(name);
                for (fname, sub) in fields {
                    self.maybe_rename(fname);
                    self.rename_pattern(sub);
                }
            }
            // T39: recurse into each or-pattern alternative so a rename
            // inside `Red | Some(x)` reaches the `x` binding.
            Pattern::Or(alts, _) => {
                for alt in alts {
                    self.rename_pattern(alt);
                }
            }
            _ => {}
        }
    }

    fn rename_expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Ident(ident, _) => self.maybe_rename(ident),
            Expr::Literal(_, _) => {}
            Expr::BinaryOp { lhs, rhs, .. } => {
                self.rename_expr(lhs);
                self.rename_expr(rhs);
            }
            Expr::UnaryOp { operand, .. } => self.rename_expr(operand),
            Expr::IfExpr {
                cond,
                then_block,
                else_block,
                ..
            } => {
                self.rename_expr(cond);
                self.rename_block(then_block);
                if let Some(eb) = else_block {
                    self.rename_block(eb);
                }
            }
            Expr::FuncCall { callee, args, .. } => {
                self.rename_expr(callee);
                for a in args {
                    self.rename_expr(a);
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                self.rename_expr(receiver);
                self.maybe_rename(method);
                for a in args {
                    self.rename_expr(a);
                }
            }
            Expr::Lambda { params, body, .. } => {
                for p in params {
                    self.rename_param(p);
                }
                self.rename_block(body);
            }
            Expr::StructInit {
                type_name, fields, ..
            } => {
                self.maybe_rename(type_name);
                for (fname, v) in fields {
                    self.maybe_rename(fname);
                    self.rename_expr(v);
                }
            }
            Expr::MatchExpr {
                scrutinee, arms, ..
            } => {
                self.rename_expr(scrutinee);
                for arm in arms {
                    self.rename_pattern(&mut arm.pattern);
                    self.rename_block(&mut arm.body);
                }
            }
            Expr::SuspendExpr { inner, .. } => self.rename_expr(inner),
            Expr::ArrayLit { elements, .. } => {
                for e in elements {
                    self.rename_expr(e);
                }
            }
            Expr::Index { base, indices, .. } => {
                self.rename_expr(base);
                for i in indices {
                    self.rename_expr(i);
                }
            }
            Expr::StringInterp { parts, .. } => {
                for p in parts {
                    if let buff_lang_ast::InterpPart::Expr(e, _) = p {
                        self.rename_expr(e);
                    }
                }
            }
            Expr::MapLit { entries, .. } => {
                for (k, v) in entries {
                    self.rename_expr(k);
                    self.rename_expr(v);
                }
            }
            Expr::Try { expr, .. } => self.rename_expr(expr),
            Expr::Spawn { task, .. } => self.rename_expr(task),
            Expr::Range { start, end, .. } => {
                self.rename_expr(start);
                self.rename_expr(end);
            }
            Expr::IfLet {
                pattern,
                value,
                then_block,
                else_block,
                ..
            } => {
                self.rename_pattern(pattern);
                self.rename_expr(value);
                self.rename_block(then_block);
                if let Some(eb) = else_block {
                    self.rename_block(eb);
                }
            }
            Expr::TupleLit(members, _) => {
                for m in members {
                    self.rename_expr(m);
                }
            }
            Expr::NamedArg { name, value, .. } => {
                self.maybe_rename(name);
                self.rename_expr(value);
            }
        }
    }
}

fn stmt_span(s: &Stmt) -> Span {
    match s {
        Stmt::LetDecl { span, .. }
        | Stmt::Assignment { span, .. }
        | Stmt::ExprStmt(_, span)
        | Stmt::Return(_, span)
        | Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::ForIn { span, .. }
        | Stmt::ForWhile { span, .. }
        | Stmt::LetPattern { span, .. }
        | Stmt::ForLet { span, .. }
        | Stmt::Guard { span, .. }
        | Stmt::Defer { span, .. }
        | Stmt::ComptimeBlock { span, .. } => *span,
    }
}

struct LineTable {
    line_starts: Vec<usize>,
}

impl LineTable {
    fn new(src: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
    }

    fn byte_to_line(&self, byte_offset: usize) -> usize {
        match self.line_starts.binary_search(&byte_offset) {
            Ok(idx) => idx + 1,
            Err(idx) => idx,
        }
    }
}

fn parse_line_range(range: &str) -> Result<(usize, usize)> {
    let parts: Vec<&str> = range.splitn(2, '-').collect();
    if parts.len() != 2 {
        bail!("invalid range `{range}` — expected `<START>-<END>` (e.g. `5-8`)");
    }
    let start: usize = parts[0]
        .parse()
        .with_context(|| format!("invalid start line `{}` in range `{range}`", parts[0]))?;
    let end: usize = parts[1]
        .parse()
        .with_context(|| format!("invalid end line `{}` in range `{range}`", parts[1]))?;
    Ok((start, end))
}

fn span_for_name(src: &str, name: &str) -> Option<Span> {
    let needle = name;
    let mut search_start = 0;
    while let Some(rel) = src[search_start..].find(needle) {
        let abs = search_start + rel;
        let before_ok = abs == 0
            || !src
                .as_bytes()
                .get(abs - 1)
                .copied()
                .is_some_and(is_ident_byte);
        let after_pos = abs + needle.len();
        let after_ok = after_pos >= src.len()
            || !src
                .as_bytes()
                .get(after_pos)
                .copied()
                .is_some_and(is_ident_byte);
        if before_ok && after_ok {
            return Some(Span::new(abs, after_pos, SourceId(0)));
        }
        search_start = abs + needle.len();
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_side_effect_free(e: &Expr) -> bool {
    matches!(e, Expr::Literal(_, _) | Expr::Ident(_, _))
}

fn replace_ident_in_stmt(s: &mut Stmt, name: &str, initializer: &Expr) {
    match s {
        Stmt::LetDecl { value, .. } => replace_ident_in_expr(value, name, initializer),
        Stmt::Assignment { target, value, .. } => {
            replace_ident_in_expr(target, name, initializer);
            replace_ident_in_expr(value, name, initializer);
        }
        Stmt::ExprStmt(e, _) => replace_ident_in_expr(e, name, initializer),
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                replace_ident_in_expr(e, name, initializer);
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            replace_ident_in_expr(iter, name, initializer);
            for s in &mut body.stmts {
                replace_ident_in_stmt(s, name, initializer);
            }
        }
        Stmt::ForWhile { cond, body, .. } => {
            replace_ident_in_expr(cond, name, initializer);
            for s in &mut body.stmts {
                replace_ident_in_stmt(s, name, initializer);
            }
        }
        Stmt::LetPattern { value, .. } => replace_ident_in_expr(value, name, initializer),
        Stmt::ForLet { value, body, .. } => {
            replace_ident_in_expr(value, name, initializer);
            for s in &mut body.stmts {
                replace_ident_in_stmt(s, name, initializer);
            }
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for c in conditions {
                match c {
                    buff_lang_ast::GuardCondition::Let { value, .. } => {
                        replace_ident_in_expr(value, name, initializer);
                    }
                    buff_lang_ast::GuardCondition::Bool(e) => {
                        replace_ident_in_expr(e, name, initializer);
                    }
                }
            }
            for s in &mut else_block.stmts {
                replace_ident_in_stmt(s, name, initializer);
            }
        }
        Stmt::Defer { expr, .. } => replace_ident_in_expr(expr, name, initializer),
        Stmt::ComptimeBlock { body, .. } => {
            for s in &mut body.stmts {
                replace_ident_in_stmt(s, name, initializer);
            }
        }
    }
}

fn replace_ident_in_expr(e: &mut Expr, name: &str, initializer: &Expr) {
    match e {
        Expr::Ident(ident, _) if ident.name == name => {
            *e = initializer.clone();
        }
        Expr::Ident(_, _) | Expr::Literal(_, _) => {}
        Expr::BinaryOp { lhs, rhs, .. } => {
            replace_ident_in_expr(lhs, name, initializer);
            replace_ident_in_expr(rhs, name, initializer);
        }
        Expr::UnaryOp { operand, .. } => replace_ident_in_expr(operand, name, initializer),
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            replace_ident_in_expr(cond, name, initializer);
            for s in &mut then_block.stmts {
                replace_ident_in_stmt(s, name, initializer);
            }
            if let Some(eb) = else_block {
                for s in &mut eb.stmts {
                    replace_ident_in_stmt(s, name, initializer);
                }
            }
        }
        Expr::FuncCall { callee, args, .. } => {
            replace_ident_in_expr(callee, name, initializer);
            for a in args {
                replace_ident_in_expr(a, name, initializer);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            replace_ident_in_expr(receiver, name, initializer);
            for a in args {
                replace_ident_in_expr(a, name, initializer);
            }
        }
        Expr::Lambda { body, .. } => {
            for s in &mut body.stmts {
                replace_ident_in_stmt(s, name, initializer);
            }
        }
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                replace_ident_in_expr(v, name, initializer);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            replace_ident_in_expr(scrutinee, name, initializer);
            for arm in arms {
                for s in &mut arm.body.stmts {
                    replace_ident_in_stmt(s, name, initializer);
                }
            }
        }
        Expr::SuspendExpr { inner, .. } => replace_ident_in_expr(inner, name, initializer),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                replace_ident_in_expr(e, name, initializer);
            }
        }
        Expr::Index { base, indices, .. } => {
            replace_ident_in_expr(base, name, initializer);
            for i in indices {
                replace_ident_in_expr(i, name, initializer);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for p in parts {
                if let buff_lang_ast::InterpPart::Expr(e, _) = p {
                    replace_ident_in_expr(e, name, initializer);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                replace_ident_in_expr(k, name, initializer);
                replace_ident_in_expr(v, name, initializer);
            }
        }
        Expr::Try { expr, .. } => replace_ident_in_expr(expr, name, initializer),
        Expr::Spawn { task, .. } => replace_ident_in_expr(task, name, initializer),
        Expr::Range { start, end, .. } => {
            replace_ident_in_expr(start, name, initializer);
            replace_ident_in_expr(end, name, initializer);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            replace_ident_in_expr(value, name, initializer);
            for s in &mut then_block.stmts {
                replace_ident_in_stmt(s, name, initializer);
            }
            if let Some(eb) = else_block {
                for s in &mut eb.stmts {
                    replace_ident_in_stmt(s, name, initializer);
                }
            }
        }
        Expr::TupleLit(members, _) => {
            for m in members {
                replace_ident_in_expr(m, name, initializer);
            }
        }
        Expr::NamedArg { value, .. } => replace_ident_in_expr(value, name, initializer),
    }
}

const BUFF_KEYWORDS: &[&str] = &[
    "func", "let", "mut", "struct", "enum", "trait", "type", "if", "else", "for", "return",
    "break", "continue", "in", "match", "async", "spawn", "import", "export", "from", "as", "true",
    "false", "extern", "unsafe",
];

fn validate_identifier(name: &str, label: &str) -> Result<()> {
    if name.is_empty() {
        bail!("refactor: {label} name is empty");
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap_or(' ');
    if !(first.is_ascii_alphabetic() || first == '_') {
        bail!("refactor: {label} name `{name}` does not start with a letter or underscore");
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!(
            "refactor: {label} name `{name}` contains non-identifier characters \
             (only a-z, A-Z, 0-9, _ allowed)"
        );
    }
    if BUFF_KEYWORDS.contains(&name) {
        bail!("refactor: {label} name `{name}` is a reserved Buff keyword");
    }
    Ok(())
}

fn parse_source(src: &str, file: &Path) -> Result<Vec<Decl>> {
    let source_id = SourceId(0);
    let tokens =
        tokenize(src, source_id).with_context(|| format!("lex error in `{}`", file.display()))?;
    let decls = parse(&tokens, source_id)
        .with_context(|| format!("parse error in `{}`", file.display()))?;
    Ok(decls)
}

fn resolve_targets(path: Option<&Path>) -> Result<Vec<PathBuf>> {
    let root = path.unwrap_or_else(|| Path::new("."));
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    if !root.is_dir() {
        bail!(
            "refactor rename: path `{}` is neither a file nor a directory",
            root.display()
        );
    }
    let mut out = Vec::new();
    walk_buff_files(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_buff_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(root)
        .with_context(|| format!("failed to read dir `{}`", root.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading dir `{}`", root.display()))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_buff_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("buff") {
            out.push(path);
        }
    }
    Ok(())
}

pub fn apply_rename_to_source(src: &str, old: &str, new: &str) -> Result<String> {
    validate_identifier(old, "old")?;
    validate_identifier(new, "new")?;
    let mut decls = parse_source(src, Path::new("<inline>"))?;
    let mut walker = RenameWalker::new(old, new);
    for decl in &mut decls {
        walker.rename_decl(decl);
    }
    Ok(fmt::format_decls(&decls))
}

pub fn apply_extract_to_source(
    src: &str,
    start_line: usize,
    end_line: usize,
    name: &str,
) -> Result<String> {
    validate_identifier(name, "name")?;
    let mut decls = parse_source(src, Path::new("<inline>"))?;
    let func_idx = decls
        .iter()
        .position(|d| matches!(d, Decl::FuncDecl(_)))
        .ok_or_else(|| anyhow!("no top-level function found"))?;

    let host_func = match &mut decls[func_idx] {
        Decl::FuncDecl(f) => f,
        _ => bail!("internal: not a FuncDecl"),
    };

    let body_span = host_func.body.span;
    let line_of = LineTable::new(src);

    let mut lift_indices: Vec<usize> = Vec::new();
    for (i, s) in host_func.body.stmts.iter().enumerate() {
        let stmt_line = line_of.byte_to_line(stmt_span(s).start);
        if stmt_line >= start_line && stmt_line <= end_line {
            lift_indices.push(i);
        }
    }
    if lift_indices.is_empty() {
        bail!(
            "no statements in source fall on lines [{start_line}, {end_line}] \
             inside the first function"
        );
    }
    let first = lift_indices[0];
    let last = *lift_indices.last().unwrap_or(&first);
    if lift_indices != (first..=last).collect::<Vec<_>>() {
        bail!(
            "lines [{start_line}, {end_line}] select non-contiguous statements; \
             extract-function requires a single contiguous range"
        );
    }

    let lifted: Vec<Stmt> = host_func.body.stmts.drain(first..=last).collect();
    let call_span = body_span;
    let call = Expr::FuncCall {
        callee: Box::new(Expr::Ident(Ident::new(name, call_span), call_span)),
        args: Vec::new(),
        span: call_span,
    };
    let call_stmt = Stmt::ExprStmt(call, call_span);

    let placeholder_span = span_for_name(src, name).unwrap_or_else(Span::dummy);
    let new_func = FuncDecl {
        name: Ident::new(name, placeholder_span),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts: lifted,
            span: placeholder_span,
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: placeholder_span,
    };
    decls.insert(func_idx + 1, Decl::FuncDecl(new_func));

    if let Decl::FuncDecl(f) = &mut decls[func_idx] {
        let pos = std::cmp::min(first, f.body.stmts.len());
        f.body.stmts.insert(pos, call_stmt);
    }

    Ok(fmt::format_decls(&decls))
}

pub fn apply_inline_to_source(src: &str, name: &str) -> Result<String> {
    validate_identifier(name, "name")?;
    let mut decls = parse_source(src, Path::new("<inline>"))?;
    let host_idx = decls
        .iter()
        .position(|d| {
            if let Decl::FuncDecl(f) = d {
                f.body
                    .stmts
                    .iter()
                    .any(|s| matches!(s, Stmt::LetDecl { name: n, .. } if n.name == name))
            } else {
                false
            }
        })
        .ok_or_else(|| anyhow!("no `let {name}` binding found"))?;

    let host_func = match &mut decls[host_idx] {
        Decl::FuncDecl(f) => f,
        _ => bail!("internal: not a FuncDecl"),
    };

    let mut binding_idx: Option<usize> = None;
    let mut initializer: Option<Expr> = None;
    for (i, s) in host_func.body.stmts.iter().enumerate() {
        if let Stmt::LetDecl { name: n, value, .. } = s {
            if n.name == name {
                binding_idx = Some(i);
                initializer = Some(value.clone());
                break;
            }
        }
    }
    let binding_idx = binding_idx.ok_or_else(|| anyhow!("`let {name}` not found"))?;
    let initializer = initializer.ok_or_else(|| anyhow!("`let {name}` not found"))?;

    if !is_side_effect_free(&initializer) {
        bail!(
            "refactor inline-variable: initializer for `{name}` is not a \
             side-effect-free expression (Literal or Ident) — refusing to \
             duplicate it; this is the MVP scope"
        );
    }

    let body = &mut host_func.body;
    for (i, s) in body.stmts.iter_mut().enumerate() {
        if i == binding_idx {
            continue;
        }
        replace_ident_in_stmt(s, name, &initializer);
    }
    body.stmts.remove(binding_idx);

    Ok(fmt::format_decls(&decls))
}

#[doc(hidden)]
pub fn test_literal_int(v: i64) -> Literal {
    Literal::Int(v)
}

#[doc(hidden)]
pub fn test_literal_string(s: &str) -> Literal {
    Literal::String(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_list_includes_func_and_let() {
        assert!(BUFF_KEYWORDS.contains(&"func"));
        assert!(BUFF_KEYWORDS.contains(&"let"));
        assert!(!BUFF_KEYWORDS.contains(&"my_var"));
    }

    #[test]
    fn validate_rejects_empty_name() {
        assert!(validate_identifier("", "x").is_err());
    }

    #[test]
    fn validate_rejects_digit_start() {
        assert!(validate_identifier("9lives", "x").is_err());
    }

    #[test]
    fn validate_accepts_underscore_start() {
        assert!(validate_identifier("_internal", "x").is_ok());
    }

    #[test]
    fn validate_rejects_keyword_func() {
        assert!(validate_identifier("func", "x").is_err());
    }

    #[test]
    fn parse_line_range_basic() {
        let (s, e) = parse_line_range("3-7").unwrap();
        assert_eq!(s, 3);
        assert_eq!(e, 7);
    }

    #[test]
    fn parse_line_range_missing_dash() {
        assert!(parse_line_range("5").is_err());
    }

    #[test]
    fn parse_line_range_non_numeric() {
        assert!(parse_line_range("a-b").is_err());
    }

    #[test]
    fn line_table_byte_to_line_for_simple_source() {
        let src = "a\nb\nc\n";
        let t = LineTable::new(src);
        assert_eq!(t.byte_to_line(0), 1);
        assert_eq!(t.byte_to_line(2), 2);
        assert_eq!(t.byte_to_line(4), 3);
    }

    #[test]
    fn is_side_effect_free_for_int_literal() {
        let e = Expr::Literal(Literal::Int(42), Span::dummy());
        assert!(is_side_effect_free(&e));
    }

    #[test]
    fn is_side_effect_free_for_ident() {
        let e = Expr::Ident(Ident::new("x", Span::dummy()), Span::dummy());
        assert!(is_side_effect_free(&e));
    }

    #[test]
    fn is_side_effect_free_rejects_call() {
        let e = Expr::FuncCall {
            callee: Box::new(Expr::Ident(Ident::new("f", Span::dummy()), Span::dummy())),
            args: Vec::new(),
            span: Span::dummy(),
        };
        assert!(!is_side_effect_free(&e));
    }

    #[test]
    fn span_for_name_finds_word_boundary() {
        let src = "func foo():\n    print(foo)\n";
        let s = span_for_name(src, "foo").unwrap();
        assert_eq!(s.start, 5);
        assert_eq!(s.end, 8);
    }

    #[test]
    fn span_for_name_returns_none_for_missing() {
        let src = "func main():\n    print(\"x\")\n";
        assert!(span_for_name(src, "missing").is_none());
    }

    #[test]
    fn apply_rename_simple_program() {
        let src = "func helper():\n    print(\"hi\")\n\nfunc main():\n    helper()\n";
        let out = apply_rename_to_source(src, "helper", "greet").unwrap();
        assert!(out.contains("func greet"));
        assert!(out.contains("greet()"));
        assert!(!out.contains("helper"));
    }

    #[test]
    fn apply_rename_keyword_target_rejected() {
        let src = "func main():\n    print(\"x\")\n";
        assert!(apply_rename_to_source(src, "main", "func").is_err());
    }

    #[test]
    fn apply_rename_no_match_yields_unchanged_program() {
        let src = "func main():\n    print(\"x\")\n";
        let out = apply_rename_to_source(src, "missing", "renamed").unwrap();
        assert_eq!(out, src);
    }

    #[test]
    fn apply_extract_lifts_range_into_new_function() {
        let src = "func main():\n    let x = 1\n    print(x)\n    print(\"done\")\n";
        let out = apply_extract_to_source(src, 2, 3, "extracted").unwrap();
        assert!(out.contains("func extracted"));
        assert!(out.contains("extracted()"));
    }

    #[test]
    fn apply_inline_replaces_simple_int_binding() {
        let src = "func main():\n    let x = 42\n    print(x)\n    print(x)\n";
        let out = apply_inline_to_source(src, "x").unwrap();
        assert!(!out.contains("let x"));
        assert!(out.contains("42"));
    }

    #[test]
    fn apply_inline_rejects_complex_initializer() {
        let src = "func main():\n    let x = compute()\n    print(x)\n";
        assert!(apply_inline_to_source(src, "x").is_err());
    }

    #[test]
    fn apply_inline_missing_name_errors() {
        let src = "func main():\n    print(\"x\")\n";
        assert!(apply_inline_to_source(src, "missing").is_err());
    }

    #[test]
    fn apply_extract_invalid_range_errors() {
        let src = "func main():\n    print(\"x\")\n";
        assert!(apply_extract_to_source(src, 100, 200, "f").is_err());
    }
}
