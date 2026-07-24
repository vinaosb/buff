//! Expression formatting - extracted from `fmt.rs` (T106 mechanical split).
//!
//! impl block for the expression/literal/pattern-writing methods on [`Formatter`].

use std::fmt::Write;

use buff_lang_ast::{
    Block, Expr, GuardCondition, Ident, InterpPart, Literal, MatchArm, Param, Pattern,
    TypeRef,
};

use super::Formatter;

impl<'a> Formatter<'a> {
    pub(super) fn write_expr(&mut self, expr: &Expr) {
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
    pub(super) fn write_expr_operand(&mut self, expr: &Expr) {
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

    pub(super) fn write_literal(&mut self, lit: &Literal) {
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
    pub(super) fn write_string_literal(&mut self, s: &str) {
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
    pub(super) fn write_string_interp(&mut self, parts: &[InterpPart]) {
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
                InterpPart::Expr(e, spec) => {
                    self.buf.push('{');
                    self.write_expr(e);
                    if let Some(s) = spec {
                        self.buf.push(':');
                        self.buf.push_str(s);
                    }
                    self.buf.push('}');
                }
            }
        }
        self.buf.push('"');
    }

    pub(super) fn write_call_args(&mut self, args: &[Expr]) {
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
    pub(super) fn would_overflow(&self, args: &[Expr]) -> bool {
        if args.len() < 4 {
            return false;
        }
        let column = self.current_column();
        let est_inline_len =
            column + args.iter().map(est_expr_len).sum::<usize>() + (args.len() - 1) * 2;
        est_inline_len > MAX_LINE_LEN
    }

    pub(super) fn current_column(&self) -> usize {
        match self.buf.rfind('\n') {
            Some(i) => self.buf.len() - i - 1,
            None => self.buf.len(),
        }
    }

    pub(super) fn write_struct_init(&mut self, name: &Ident, fields: &[(Ident, Expr)]) {
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

    pub(super) fn write_array_lit(&mut self, elements: &[Expr]) {
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

    pub(super) fn write_map_lit(&mut self, entries: &[(Expr, Expr)]) {
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

    pub(super) fn write_if_expr(&mut self, cond: &Expr, then_block: &Block, else_block: Option<&Block>) {
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

    pub(super) fn write_if_let(
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

    pub(super) fn write_lambda(&mut self, params: &[Param], body: &Block, _return_type: Option<&TypeRef>) {
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

    pub(super) fn write_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) {
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

    pub(super) fn write_pattern(&mut self, pat: &Pattern) {
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
            // T39: or-pattern `A | B | C`. Renders with ` | `-separated
            // alternatives, mirroring the source form (the formatter
            // round-trips or-patterns byte-faithfully).
            Pattern::Or(alts, _) => {
                for (i, p) in alts.iter().enumerate() {
                    if i > 0 {
                        self.raw(" | ");
                    }
                    self.write_pattern(p);
                }
            }
        }
    }
}
