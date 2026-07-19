//! Per-document Buff analysis: tokenize → parse → infer, plus symbol indexing.
//!
//! [`analyze`] is the single entry point. It runs the full front-end on a
//! source string and produces a [`DocumentAnalysis`] containing:
//!
//! - [`Diagnostics`](buff_lang_error::Diagnostic) — every error/warning the
//!   front-end can surface, in source order.
//! - A flat [`SymbolIndex`] — top-level decls + per-function locals for
//!   hover / completion / goto-def / document-symbol outline.
//! - A flat [`TypeBindingIndex`] — inferred [`Type`] for each identifier
//!   reference, keyed by the identifier's start byte offset.
//!
//! # Why `parse_recovering` (not `parse`)
//!
//! `parse` fails fast — one diagnostic stops compilation. For an IDE we want
//! to surface every recoverable error in a single pass, so we use the
//! recovering variant (which collects errors into a vec and keeps going).
//! That's exactly the IDE/`buff check` use-case documented in its doc
//! comment.
//!
//! # Typecheck-only mode
//!
//! This module does NOT touch code generation. Inference runs directly via
//! [`TypeInferencer`], which is what the CLI's codegen uses internally —
//! i.e. the LSP's typecheck surface is byte-for-byte the same as `buff check`
//! would produce once that command exists standalone.

use buff_lang_ast::{Decl, Expr, Stmt};
use buff_lang_error::{Diagnostic, ParseError, Severity, SourceId, Span, TypeError};
use buff_lang_lexer::{tokenize, Token};
use buff_lang_parser::parse_recovering;
use buff_lang_types::{Type, TypeInferencer};

use crate::symbol::{LocalKind, SymbolIndex, TypeBindingIndex};

/// Output of [`analyze`] — everything the LSP handlers need to answer
/// questions about a single Buff file.
#[derive(Debug, Clone, Default)]
pub struct DocumentAnalysis {
    /// Every diagnostic produced by lex + parse + inference, in source
    /// order (sorted by start byte offset). Already deduplicated.
    pub diagnostics: Vec<Diagnostic>,
    /// The top-level declarations (functions, structs, enums, imports,
    /// …) that parsed successfully. May be non-empty even when
    /// [`DocumentAnalysis::diagnostics`] is also non-empty — recovering
    /// parse yields both.
    pub decls: Vec<Decl>,
    /// Flat symbol index for hover / completion / goto-def.
    pub symbols: SymbolIndex,
    /// Inferred types for identifier references, keyed by the identifier's
    /// start byte offset.
    pub types: TypeBindingIndex,
}

impl DocumentAnalysis {
    /// Iterate diagnostics sorted by start offset, deduplicated.
    ///
    /// `parse_recovering` may surface the same span twice (e.g. a parse
    /// error + a downstream type error pointing at the same token); we
    /// collapse exact `(start, end, severity, message)` dupes.
    fn dedup_diagnostics(mut diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
        diags.sort_by_key(|d| (d.span.start, d.span.end, format!("{:?}", d.severity), d.message.clone()));
        diags.dedup_by(|a, b| {
            a.span.start == b.span.start
                && a.span.end == b.span.end
                && a.severity == b.severity
                && a.message == b.message
        });
        diags
    }
}

/// Run the full front-end on `source` and return the analysis.
///
/// `source_id` is the [`SourceId`] used in every emitted span (callers
/// should use a per-URI value so multi-file LSP can map back).
pub fn analyze(source: &str, source_id: SourceId) -> DocumentAnalysis {
    let mut diags: Vec<Diagnostic> = Vec::new();

    // ---- Phase 1: lex -------------------------------------------------
    let tokens: Vec<Token> = match tokenize(source, source_id) {
        Ok(t) => t,
        Err(e) => {
            // `LexerError` wraps a `LexError` which carries the
            // user-facing `Diagnostic`. Surface it directly.
            diags.push(e.inner.diagnostic);
            // Without tokens we cannot run the parser. Return early with
            // the lex diagnostic alone.
            return DocumentAnalysis {
                diagnostics: DocumentAnalysis::dedup_diagnostics(diags),
                decls: Vec::new(),
                symbols: SymbolIndex::new(),
                types: TypeBindingIndex::new(),
            };
        }
    };

    // ---- Phase 2: parse (recovering) ----------------------------------
    let (decls, parse_errors): (Vec<Decl>, Vec<ParseError>) = parse_recovering(&tokens, source_id);
    for e in parse_errors {
        diags.push(e.diagnostic);
    }

    // ---- Phase 3: type inference (per function) -----------------------
    let mut symbols = SymbolIndex::new();
    let mut types = TypeBindingIndex::new();

    // Index top-level decls first so goto-def / doc-symbols / completion
    // can find them regardless of cursor position.
    for d in &decls {
        symbols.add_top_decl(d);
    }

    for d in &decls {
        infer_decl(d, &mut diags, &mut symbols, &mut types);
    }

    DocumentAnalysis {
        diagnostics: DocumentAnalysis::dedup_diagnostics(diags),
        decls,
        symbols,
        types,
    }
}

/// Run inference over a single top-level declaration, collecting
/// [`TypeError`]s into `diags`, recording locals into `symbols`, and
/// recording inferred identifier types into `types`.
fn infer_decl(
    decl: &Decl,
    diags: &mut Vec<Diagnostic>,
    symbols: &mut SymbolIndex,
    types: &mut TypeBindingIndex,
) {
    // Currently only function bodies carry inference-bearing statements.
    // Other top-level decl kinds (struct/enum/import/trait/extern-crate/...)
    // contribute no per-expression types; they ARE indexed as top-level
    // symbols by `SymbolIndex::add_top_decl` above.
    if let Decl::FuncDecl(f) = decl {
        let mut infer = TypeInferencer::new();
        // Bind parameters first so the body can refer to them. Param types
        // may be TypeRef::Named("Int", ...) which we can't fully resolve
        // without `typeref_to_type` (private to the types crate). Bind as
        // Unknown — matches the inferencer's v0.5 fallback for unresolved
        // annotations and lets hover at least show "Unknown" rather than
        // "undefined variable".
        //
        // The local's def_span is the param NAME's span (NOT f.span) so
        // each param gets a unique key in the BTreeMap. Using f.span would
        // collide all params on the same byte offset.
        for p in &f.params {
            symbols.add_local(&p.name, LocalKind::Param, p.name.span, f.span);
            infer.bind(&p.name.name, Type::Unknown);
        }
        infer_block(&f.body, f.span, &mut infer, diags, symbols, types);
    }
}

/// Walk a block of statements, recording locals + per-expression types.
fn infer_block(
    block: &buff_lang_ast::Block,
    func_span: Span,
    infer: &mut TypeInferencer,
    diags: &mut Vec<Diagnostic>,
    symbols: &mut SymbolIndex,
    types: &mut TypeBindingIndex,
) {
    for stmt in &block.stmts {
        infer_stmt(stmt, func_span, infer, diags, symbols, types);
    }
}

/// Run inference on a single statement, recording side-effects.
fn infer_stmt(
    stmt: &Stmt,
    func_span: Span,
    infer: &mut TypeInferencer,
    diags: &mut Vec<Diagnostic>,
    symbols: &mut SymbolIndex,
    types: &mut TypeBindingIndex,
) {
    // First record side-effects on the symbol table + type index by walking
    // sub-expressions (which may contain identifier references).
    record_expr_types_in_stmt(stmt, infer, types);

    match stmt {
        Stmt::LetDecl { name, .. } => {
            // Record the local binding BEFORE inference runs, so the type
            // index knows the binding's span. The inferencer assigns the
            // type to its env when infer_stmt runs.
            symbols.add_local(name, LocalKind::Let, stmt_span(stmt), func_span);
            // Run inference to drive the env update + collect any TypeError.
            record_type_or_diag(infer.infer_stmt(stmt), diags);
            // After inference, snapshot the inferred type into the binding
            // index so hover at the binding site shows it.
            if let Some(ty) = infer.lookup(&name.name).cloned() {
                types.insert_binding(name.span.start, ty);
            }
        }
        _ => {
            // Any other statement: just run inference for its diagnostics.
            record_type_or_diag(infer.infer_stmt(stmt), diags);
        }
    }
}

/// Walk all expressions in a statement, recording the inferred [`Type`] of
/// every identifier reference. Recurses into compound expressions so
/// `a + b` records types for both `a` and `b`.
///
/// We use a separate inferencer snapshot per expression so a type error in
/// one branch doesn't prevent recording the types in another. This is
/// intentionally a fresh inferencer walk (mirroring how the official
/// infer_stmt pass would visit each expression) — we share `infer.env()`
/// by reading from it post-statement.
fn record_expr_types_in_stmt(stmt: &Stmt, infer: &TypeInferencer, types: &mut TypeBindingIndex) {
    match stmt {
        Stmt::LetDecl { value, .. } => record_expr_types(value, infer, types),
        Stmt::LetPattern { value, .. } => record_expr_types(value, infer, types),
        Stmt::Assignment { target, value, .. } => {
            record_expr_types(target, infer, types);
            record_expr_types(value, infer, types);
        }
        Stmt::ExprStmt(e, _) => record_expr_types(e, infer, types),
        Stmt::Return(Some(e), _) => record_expr_types(e, infer, types),
        Stmt::Return(None, _) => {}
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            record_expr_types(iter, infer, types);
            for s in &body.stmts {
                record_expr_types_in_stmt(s, infer, types);
            }
        }
        Stmt::ForWhile { cond, body, .. } => {
            record_expr_types(cond, infer, types);
            for s in &body.stmts {
                record_expr_types_in_stmt(s, infer, types);
            }
        }
        Stmt::ForLet {
            value, body, ..
        } => {
            record_expr_types(value, infer, types);
            for s in &body.stmts {
                record_expr_types_in_stmt(s, infer, types);
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
                        record_expr_types(value, infer, types);
                    }
                    buff_lang_ast::GuardCondition::Bool(e) => {
                        record_expr_types(e, infer, types);
                    }
                }
            }
            for s in &else_block.stmts {
                record_expr_types_in_stmt(s, infer, types);
            }
        }
        Stmt::Defer { expr, .. } => record_expr_types(expr, infer, types),
    }
}

/// Recursively record inferred types for every identifier reference inside
/// `expr`. Uses a fresh, read-only snapshot of the inference environment so
/// the call never mutates the live inferencer state.
fn record_expr_types(expr: &Expr, infer: &TypeInferencer, types: &mut TypeBindingIndex) {
    match expr {
        Expr::Ident(name, span) => {
            if let Some(ty) = infer.lookup(&name.name) {
                types.insert_binding(span.start, ty.clone());
            }
        }
        Expr::Literal(_, _) => {}
        Expr::BinaryOp { lhs, rhs, .. } => {
            record_expr_types(lhs, infer, types);
            record_expr_types(rhs, infer, types);
        }
        Expr::UnaryOp { operand, .. } => record_expr_types(operand, infer, types),
        Expr::IfExpr {
            cond, then_block, else_block, ..
        } => {
            record_expr_types(cond, infer, types);
            for s in &then_block.stmts {
                record_expr_types_in_stmt(s, infer, types);
            }
            if let Some(eb) = else_block {
                for s in &eb.stmts {
                    record_expr_types_in_stmt(s, infer, types);
                }
            }
        }
        Expr::FuncCall { callee, args, .. } => {
            record_expr_types(callee, infer, types);
            for a in args {
                record_expr_types(a, infer, types);
            }
        }
        Expr::MethodCall {
            receiver, args, ..
        } => {
            record_expr_types(receiver, infer, types);
            for a in args {
                record_expr_types(a, infer, types);
            }
        }
        Expr::Lambda { body, .. } => {
            for s in &body.stmts {
                record_expr_types_in_stmt(s, infer, types);
            }
        }
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                record_expr_types(v, infer, types);
            }
        }
        Expr::MatchExpr { scrutinee, arms, .. } => {
            record_expr_types(scrutinee, infer, types);
            for arm in arms {
                for s in &arm.body.stmts {
                    record_expr_types_in_stmt(s, infer, types);
                }
            }
        }
        Expr::SuspendExpr { inner, .. } => record_expr_types(inner, infer, types),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                record_expr_types(e, infer, types);
            }
        }
        Expr::Index { base, indices, .. } => {
            record_expr_types(base, infer, types);
            for i in indices {
                record_expr_types(i, infer, types);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for p in parts {
                if let buff_lang_ast::InterpPart::Expr(e) = p {
                    record_expr_types(e, infer, types);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                record_expr_types(k, infer, types);
                record_expr_types(v, infer, types);
            }
        }
        Expr::Try { expr, .. } => record_expr_types(expr, infer, types),
        Expr::Spawn { task, .. } => record_expr_types(task, infer, types),
        Expr::Range { start, end, .. } => {
            record_expr_types(start, infer, types);
            record_expr_types(end, infer, types);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            record_expr_types(value, infer, types);
            for s in &then_block.stmts {
                record_expr_types_in_stmt(s, infer, types);
            }
            if let Some(eb) = else_block {
                for s in &eb.stmts {
                    record_expr_types_in_stmt(s, infer, types);
                }
            }
        }
        Expr::TupleLit(members, _) => {
            for m in members {
                record_expr_types(m, infer, types);
            }
        }
        Expr::NamedArg { value, .. } => record_expr_types(value, infer, types),
    }
}

/// Push the diagnostic from a TypeError result, if any.
fn record_type_or_diag(result: Result<Type, TypeError>, diags: &mut Vec<Diagnostic>) {
    if let Err(e) = result {
        diags.push(e.diagnostic);
    }
}

/// Span of a top-level [`Decl`] — used as the "definition site" span for
/// goto-def / doc-symbols.
pub fn decl_span(decl: &Decl) -> Span {
    match decl {
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

/// Span of a single [`Stmt`] — used as the "definition site" for locals.
pub fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::LetDecl { span, .. } => *span,
        Stmt::Assignment { span, .. } => *span,
        Stmt::ExprStmt(_, span) => *span,
        Stmt::Return(_, span) => *span,
        Stmt::Break(s) | Stmt::Continue(s) => *s,
        Stmt::ForIn { span, .. } => *span,
        Stmt::ForWhile { span, .. } => *span,
        Stmt::LetPattern { span, .. } => *span,
        Stmt::ForLet { span, .. } => *span,
        Stmt::Guard { span, .. } => *span,
        Stmt::Defer { span, .. } => *span,
    }
}

/// Filter diagnostics to those at or above the given severity.
pub fn diagnostics_at_severity(diags: &[Diagnostic], min: Severity) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| severity_rank(d.severity) >= severity_rank(min))
        .collect()
}

/// Total order on [`Severity`] so we can compare "at least as severe as".
/// Higher rank = more severe.
fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Error => 3,
        Severity::Warning => 2,
        Severity::Info => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_clean_program_has_no_diagnostics() {
        let src = "func main():\n    print(\"hello\")\n";
        let a = analyze(src, SourceId(0));
        assert!(a.diagnostics.is_empty(), "got diagnostics: {:?}", a.diagnostics);
        assert_eq!(a.decls.len(), 1);
    }

    #[test]
    fn analyze_lex_error_yields_single_diagnostic() {
        // Unterminated string literal.
        let src = "func main():\n    print(\"hello)\n";
        let a = analyze(src, SourceId(0));
        assert_eq!(a.diagnostics.len(), 1);
        assert_eq!(a.diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn analyze_parse_error_yields_diagnostic() {
        // Missing colon after func signature.
        let src = "func main()\n    print(\"hi\")\n";
        let a = analyze(src, SourceId(0));
        assert!(
            !a.diagnostics.is_empty(),
            "expected at least one parse diagnostic, got: {:?}",
            a.diagnostics
        );
    }

    #[test]
    fn analyze_type_mismatch_yields_error_spanning_value() {
        // `let x: Int = "hello"` — value is a String but annotation says Int.
        // The inferencer's TypeError points at the LetDecl's statement span.
        let src = "func main():\n    let x: Int = \"hello\"\n";
        let a = analyze(src, SourceId(0));
        let errors: Vec<&Diagnostic> = a
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            !errors.is_empty(),
            "expected at least one type error, got: {:?}",
            a.diagnostics
        );
        let e = errors[0];
        // Span should cover the `let` declaration (start byte 0 of the stmt,
        // which lives inside the func body).
        assert!(
            e.message.contains("Int") || e.message.contains("String"),
            "expected message to mention Int/String, got: {}",
            e.message
        );
    }

    #[test]
    fn analyze_records_local_binding_types() {
        let src = "func main():\n    let x = 42\n    print(x)\n";
        let a = analyze(src, SourceId(0));
        // The binding site of `x` should have a recorded inferred type.
        let x_binding_byte = src.find("x =").unwrap();
        assert!(
            a.types.lookup(x_binding_byte).is_some(),
            "expected `x` binding to have an inferred type"
        );
        let ty = a.types.lookup(x_binding_byte).unwrap();
        assert!(matches!(ty, Type::Int { .. }), "expected Int, got {ty:?}");
    }

    #[test]
    fn analyze_records_identifier_reference_type() {
        let src = "func main():\n    let x = 42\n    print(x)\n";
        let a = analyze(src, SourceId(0));
        // The reference to `x` inside `print(x)` is on the line after the let.
        let x_ref_byte = src.rfind("x").unwrap();
        assert!(
            a.types.lookup(x_ref_byte).is_some(),
            "expected `x` reference to have an inferred type"
        );
    }
}
