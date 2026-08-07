//! Statement parser — hand-rolled recursive-descent for Buff statements.
//!
//! Builds on T7's [`crate::expr::parse_expression`] for sub-expressions and
//! shares the same [`crate::stream::TokenStream`] cursor.
//!
//! # Supported statements (T8)
//!
//! - `let name[: Type] = expr` ([`Stmt::LetDecl`])
//! - `let mut name = expr`
//! - assignment: `target = expr`, `target += expr`, … ([`Stmt::Assignment`])
//! - bare expression as statement: `print(x)` ([`Stmt::ExprStmt`])
//! - `if cond { ... } else { ... }` — wrapped in [`Stmt::ExprStmt`] using
//!   [`Expr::IfExpr`] (if is an expression)
//! - `return expr` / bare `return` ([`Stmt::Return`])
//! - `break` / `continue`
//! - `for var in iter { ... }` iterator loop ([`Stmt::ForIn`])
//! - `for cond { ... }` conditional loop, while-style ([`Stmt::ForWhile`])
//! - `while cond { ... }` conditional loop ([`Stmt::While`], BUG-9)
//!
//! # Function declarations
//!
//! [`parse_func_decl`] parses a top-level `func name(params) -> Ret { body }`.
//! Inside `parse_statement`, encountering `func` is an error — function
//! declarations are not statements.
//!
//! # Layout (T9)
//!
//! Two block forms coexist:
//!
//! - **Braces**: `{ stmt; stmt; ... }` — explicit, indentation-agnostic.
//! - **Layout (offside rule)**: `: NEWLINE INDENT stmt stmt ... DEDENT` —
//!   Python/F#-style indentation-sensitive blocks.
//!
//! [`parse_block`] dispatches on the upcoming raw token: a `{` delegates to
//! [`parse_block_braces`]; a `:` triggers the layout path. Layout tokens
//! (`Newline`/`Indent`/`Dedent`) are observed via the `_raw` family of
//! [`TokenStream`] methods, while statement bodies still use the regular
//! skipping peek/advance so existing parsers compose unchanged.
use buff_lang_ast::{BinaryOp, Block, Expr, GuardCondition, Ident, Pattern, Stmt};
use buff_lang_error::{suggest_with_message, Diagnostic, ErrorCode, ParseError, SourceId, Span};
use buff_lang_lexer::TokenKind;

use crate::expr::{parse_expression, parse_pattern};
use crate::stream::TokenStream;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Parse a single statement.
///
/// Dispatches on the upcoming significant token:
///
/// | Token      | Result                                  |
/// |------------|------------------------------------------|
/// | `let`      | [`Stmt::LetDecl`]                        |
/// | `if`       | [`Stmt::ExprStmt`] wrapping [`Expr::IfExpr`] |
/// | `return`   | [`Stmt::Return`]                         |
/// | `break`    | [`Stmt::Break`]                          |
/// | `continue` | [`Stmt::Continue`]                       |
/// | `for`      | [`Stmt::ForIn`] or [`Stmt::ForWhile`]    |
/// | `while`    | [`Stmt::While`]                          |
/// | `func`     | **error** — func decls are top-level     |
/// | other      | assignment-or-expression statement       |
///
/// # Errors
///
/// Returns [`ParseError`] on any syntax error. The cursor position after an
/// error is unspecified (no error recovery yet).
pub fn parse_statement(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    match stream.peek_kind() {
        Some(TokenKind::KwLet) => parse_let(stream),
        // T56: property wrappers (`@State`, `@Published`, `@Cached`) —
        // Swift-inspired attribute-driven codegen. The parser rewrites
        // the following `let` into a regular `let` that initialises a
        // `ReactiveSignal` (for `@State`/`@Published`) or
        // `ReactiveComputed` (for `@Cached`). PURE parse-time desugar:
        // no new AST nodes (mirrors `|>`/`?.`/`??` precedent).
        Some(TokenKind::At) => parse_statement_with_property_wrappers(stream),
        Some(TokenKind::KwIf) => {
            // if-as-expression: parse as Expr, wrap in ExprStmt.
            let expr = parse_if_expr(stream)?;
            let span = expr.span();
            Ok(Stmt::ExprStmt(expr, span))
        }
        Some(TokenKind::KwFunc) => Err(ParseError::new(Diagnostic::error(
            "function declarations must be top-level",
            stream
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span()),
        ))),
        Some(TokenKind::KwReturn) => parse_return(stream),
        Some(TokenKind::KwBreak) => {
            let tok = stream.advance_after_peek();
            Ok(Stmt::Break(tok.span))
        }
        Some(TokenKind::KwContinue) => {
            let tok = stream.advance_after_peek();
            Ok(Stmt::Continue(tok.span))
        }
        Some(TokenKind::KwFor) => parse_for(stream),
        // BUG-9: `while cond { body }` (or layout `while cond:` + indent +
        // body + dedent). Structurally identical to `for cond { body }`
        // (Stmt::ForWhile) — both lower to Rust `while cond { body }`.
        Some(TokenKind::KwWhile) => parse_while(stream),
        Some(TokenKind::KwGuard) => parse_guard(stream),
        Some(TokenKind::KwDefer) => parse_defer(stream),
        // T53: `comptime` is NOT a reserved keyword. Route to the
        // comptime-block parser ONLY when `comptime` is immediately
        // followed by a block introducer (`{` for the brace form, `:`
        // for the layout form). A bare `comptime` identifier in any
        // other position (e.g. a variable named `comptime`, or
        // `comptime` at end of input) falls through to the ordinary
        // assignment-or-expr-statement path.
        Some(TokenKind::Ident(s))
            if s == "comptime"
                && matches!(
                    stream.peek_second_kind(),
                    Some(TokenKind::LBrace) | Some(TokenKind::Colon)
                ) =>
        {
            parse_comptime_block(stream)
        }
        _ => parse_assignment_or_expr_stmt(stream),
    }
}

/// Parse a brace-delimited block of statements: `{ stmt stmt ... }`.
///
/// The opening `{` and closing `}` are consumed. Statements are separated
/// by layout (newlines) which [`TokenStream`] transparently skips, so no
/// explicit separator handling is required. An empty block `{ }` is valid
/// and produces an empty [`Block`].
///
/// T9 will add an indent/dedent-based alternative; for T8 only braces are
/// supported.
///
/// # Errors
///
/// Returns [`ParseError`] if the opening or closing brace is missing or if
/// any inner statement fails to parse.
pub fn parse_block_braces(stream: &mut TokenStream<'_>) -> Result<Block, ParseError> {
    let lbrace = stream.expect(TokenKind::LBrace)?;
    let start = lbrace.span.start;
    let source_id = stream.source_id();
    let mut stmts = Vec::new();
    while !matches!(stream.peek_kind(), Some(TokenKind::RBrace) | None) {
        stmts.push(parse_statement(stream)?);
        // Optional separator between statements (semicolon). Newlines are
        // already auto-skipped by TokenStream::peek/advance.
        if matches!(stream.peek_kind(), Some(TokenKind::Semicolon)) {
            stream.advance();
        }
    }
    let rbrace = stream.expect(TokenKind::RBrace)?;
    Ok(Block {
        stmts,
        span: Span::new(start, rbrace.span.end, source_id),
    })
}

/// Parse a block — *either* brace-delimited *or* layout-sensitive (T9).
///
/// Dispatch rules:
///
/// 1. If the next raw token is `{`, delegate to [`parse_block_braces`].
/// 2. Otherwise, expect a `:` followed by `Newline Indent ... Dedent`
///    (Python-style offside-rule block).
///
/// The layout form requires a newline immediately after `:`; single-line
/// `func foo(): expr` is not supported in T9. An empty indented block
/// (missing body after `:\n`) yields a [`ParseError`] with the message
/// `"expected indented block after ':'"`.
///
/// # Errors
///
/// Returns [`ParseError`] on any of:
/// - missing `:` when neither `{` nor `:` is present,
/// - missing newline after `:`,
/// - missing `Indent` after the newline,
/// - any malformed inner statement.
///
/// The closing `Dedent` is consumed if present but its absence is not an
/// error (the lexer's `finalize` may have collapsed trailing dedents at
/// EOF in some edge cases — defensive consumption keeps the parser robust).
pub fn parse_block(stream: &mut TokenStream<'_>) -> Result<Block, ParseError> {
    // Form 1: braces — fast path.
    if stream.check_raw(&TokenKind::LBrace) {
        return parse_block_braces(stream);
    }

    // Form 2: layout-sensitive `: NEWLINE INDENT ... DEDENT`.
    let source_id = stream.source_id();
    let colon = stream.expect(TokenKind::Colon)?;
    let start = colon.span.start;

    // Expect Newline right after the colon.
    if !stream.check_raw(&TokenKind::Newline) {
        return Err(ParseError::new(
            Diagnostic::error(
                "expected newline after `:` for layout block",
                stream.span_here(),
            )
            .with_code(ErrorCode::ExpectedLayoutNewline),
        ));
    }
    stream.advance_raw(); // consume Newline

    // Expect Indent. Stray newlines/blanks between `:` line and first indented
    // line should not happen given the lexer collapses them, but defensively
    // skip extra newlines.
    while stream.consume_newline() {}

    if !stream.consume_indent() {
        return Err(ParseError::new(
            Diagnostic::error("expected indented block after `:`", stream.span_here())
                .with_code(ErrorCode::ExpectedIndentedBlock),
        ));
    }

    // Parse statements until we hit a Dedent or run out of tokens.
    let mut stmts: Vec<Stmt> = Vec::new();
    let mut end_off = start;
    loop {
        // Stop on Dedent.
        if stream.check_raw(&TokenKind::Dedent) {
            break;
        }
        // Stop at end of significant input.
        if stream.is_at_end() {
            break;
        }
        // Skip stray Newlines/Indents between statements (the lexer emits
        // Newline at the end of every non-blank line; extra Indents should
        // not normally appear here but we tolerate them defensively).
        if stream.check_raw(&TokenKind::Newline) {
            stream.advance_raw();
            continue;
        }
        if stream.check_raw(&TokenKind::Indent) {
            // Defensive: a stray Indent at the start of an inner statement
            // usually means nested layout — let the inner parser handle it.
            // Don't consume here; parse_statement will route via parse_block
            // when it sees the trailing `:` of the inner construct.
            // However if we see Indent without a corresponding construct,
            // silently consume to avoid an infinite loop.
            stream.advance_raw();
            continue;
        }
        let stmt = parse_statement(stream)?;
        end_off = stmt_end(&stmt).max(end_off);
        stmts.push(stmt);
    }

    // Consume the closing Dedent if present (may be absent at EOF).
    let _ = stream.consume_dedent();

    Ok(Block {
        stmts,
        span: Span::new(start, end_off, source_id),
    })
}

// T106: declaration parsers extracted to `stmt_decl.rs` (mechanical split).
// Re-export keeps the `crate::stmt::*` public surface unchanged.
mod stmt_decl;
pub use stmt_decl::*;

// ---------------------------------------------------------------------------
// Internal: per-statement parsers
// ---------------------------------------------------------------------------

// T56: property-wrapper attribute names recognised on `let` statements.
// These are the ONLY built-in wrappers in v1.13 (no user-defined wrappers
// in MVP per the T56 spec). Kept as a `const` slice so the parser,
// examples, and tests agree on the canonical surface.
const PROPERTY_WRAPPER_ATTRS: &[&str] = &["State", "Published", "Cached"];

/// T56: parse a statement led by a property-wrapper attribute
/// (`@State`/`@Published`/`@Cached`).
///
/// Behaviour:
/// - Collects leading `@name` attributes via the existing
///   [`parse_attributes`] helper (same machinery as `@test`/`@prefer(gpu)`).
/// - Validates that AT MOST ONE wrapper attribute is present and that it
///   is one of the three recognised names (`State`/`Published`/`Cached`).
/// - Expects the next token to be `let` and delegates to [`parse_let`]
///   to obtain the underlying `Stmt::LetDecl`.
/// - Rewrites the LetDecl's `value` to desugar the wrapper:
///   - `@State let x = init`      → `let x = ReactiveSignal.new(init)`
///   - `@Published let x = init`  → `let x = ReactiveSignal.new(init)`
///   - `@Cached(fn) let x [= ..]` → `let x = ReactiveComputed.new({ || fn() })`
///
/// This is a PURE parse-time desugar: no new AST nodes (mirrors `|>`/`?.`/
/// `??`). The codegen sees a normal `Stmt::LetDecl` whose value is a
/// `MethodCall` on `ReactiveSignal`/`ReactiveComputed`, which the existing
/// prelude-type-assoc-fn codegen arm lowers to
/// `buff_reactive::Signal::new(..)`/`buff_reactive::Computed::new(..)`.
/// The existing `program_uses_namespace(decls, "ReactiveSignal" |
/// "ReactiveComputed")` walker then records `buff-reactive` in
/// `extern_crates` automatically.
///
/// # Errors
///
/// Returns [`ParseError`] (using the existing [`ErrorCode::UnexpectedToken`]
/// code, since the T56 spec does not introduce a new ErrorCode — ErrorCodes
/// are STABLE FOREVER per the AGENTS.md anti-pattern list) when:
/// - an unknown attribute name appears (e.g. `@Observed`),
/// - more than one wrapper attribute is stacked (e.g. `@State @Published`),
/// - the next token after the attribute is not `let`,
/// - `@Cached` is used without exactly one positional arg,
/// - the underlying `let` parses as a destructuring `Stmt::LetPattern`
///   (destructuring + property wrappers is rejected for MVP simplicity).
fn parse_statement_with_property_wrappers(
    stream: &mut TokenStream<'_>,
) -> Result<Stmt, ParseError> {
    let attrs = parse_attributes(stream)?;
    debug_assert!(
        !attrs.is_empty(),
        "parse_statement_with_property_wrappers called without leading `@`"
    );

    // T70: `@pin` is a statement-level perf attribute (NOT a property
    // wrapper). Route it to a dedicated desugar before the property-wrapper
    // validation loop rejects it as "unknown property wrapper". This keeps
    // `@pin` orthogonal to the reactive property wrappers (`@State`/
    // `@Published`/`@Cached`) and follows the same parse-time-desugar
    // precedent.
    if attrs.len() == 1 && attrs[0].name.name == "pin" {
        let attr_span = attrs[0].span;
        // `@pin` takes no arguments.
        if !attrs[0].args.is_empty() || !attrs[0].named_args.is_empty() {
            return Err(ParseError::new(
                Diagnostic::error(
                    "`@pin` takes no arguments (use plain `@pin let x = expr`)",
                    attr_span,
                )
                .with_code(ErrorCode::UnexpectedToken),
            ));
        }
        return desugar_pin_statement(stream, attr_span);
    }

    // Validate the attribute list: exactly ONE recognised wrapper, no extras.
    let mut wrapper: Option<&str> = None;
    for a in &attrs {
        let name = a.name.name.as_str();
        if !PROPERTY_WRAPPER_ATTRS.contains(&name) {
            let mut diag = Diagnostic::error(
                format!(
                    "unknown property wrapper `@{name}`; recognised wrappers are: State, Published, Cached"
                ),
                a.span,
            )
            .with_code(ErrorCode::UnexpectedToken);
            // T53: suggest the closest recognised wrapper name.
            if let Some(msg) = suggest_with_message(name, PROPERTY_WRAPPER_ATTRS) {
                diag = diag.with_note(format!("help: {msg}"));
            }
            return Err(ParseError::new(diag));
        }
        if let Some(prev) = wrapper {
            return Err(ParseError::new(
                Diagnostic::error(
                    format!(
                        "only one property wrapper per `let` is allowed (saw `@{prev}` and `@{name}`)"
                    ),
                    a.span,
                )
                .with_code(ErrorCode::UnexpectedToken),
            ));
        }
        wrapper = Some(name);
    }
    // `parse_attributes` only returns a non-empty Vec when it consumed at
    // least one `@` token, and this function is only called when the
    // dispatcher saw `TokenKind::At`. Match (not `.expect`) so the
    // unreachable branch returns a ParseError rather than panicking.
    let Some(wrapper) = wrapper else {
        return Err(ParseError::new(
            Diagnostic::error(
                "internal: parse_statement_with_property_wrappers called without a wrapper attribute",
                stream.span_here(),
            )
            .with_code(ErrorCode::UnexpectedToken),
        ));
    };

    // @Cached requires exactly one positional arg naming the compute fn.
    let cached_fn_name: Option<&str> = if wrapper == "Cached" {
        if attrs.len() != 1 {
            return Err(ParseError::new(
                Diagnostic::error(
                    "`@Cached` cannot be combined with other property wrappers",
                    attrs[0].span,
                )
                .with_code(ErrorCode::UnexpectedToken),
            ));
        }
        let args = &attrs[0].args;
        if args.len() != 1 {
            return Err(ParseError::new(
                Diagnostic::error(
                    format!(
                        "`@Cached(compute_fn)` requires exactly 1 positional arg (the compute function name); got {}",
                        args.len()
                    ),
                    attrs[0].span,
                )
                .with_code(ErrorCode::UnexpectedToken),
            ));
        }
        // Reject named args on @Cached (e.g. `@Cached(fn = "x")`).
        if !attrs[0].named_args.is_empty() {
            return Err(ParseError::new(
                Diagnostic::error(
                    "`@Cached` does not accept named arguments (use positional: `@Cached(compute_fn)`)",
                    attrs[0].span,
                )
                .with_code(ErrorCode::UnexpectedToken),
            ));
        }
        Some(args[0].as_str())
    } else {
        // @State/@Published: reject any args (e.g. `@State(42)` is invalid).
        if !attrs[0].args.is_empty() || !attrs[0].named_args.is_empty() {
            return Err(ParseError::new(
                Diagnostic::error(
                    format!(
                        "`@{wrapper}` takes no arguments (use plain `@{wrapper} let x = init`)"
                    ),
                    attrs[0].span,
                )
                .with_code(ErrorCode::UnexpectedToken),
            ));
        }
        None
    };
    let cached_fn_span = attrs[0].span;

    // The next significant token MUST be `let`.
    if !matches!(stream.peek_kind(), Some(TokenKind::KwLet)) {
        let span = stream
            .peek()
            .map(|t| t.span)
            .unwrap_or_else(|| stream.eof_span());
        let found = stream
            .peek_kind()
            .map(|k| k.to_string())
            .unwrap_or_else(|| "end of input".into());
        return Err(ParseError::new(
            Diagnostic::error(
                format!(
                    "property wrapper `@{wrapper}` must precede a `let` binding, found `{found}`"
                ),
                span,
            )
            .with_code(ErrorCode::UnexpectedToken),
        ));
    }

    // Parse the underlying let (returns Stmt::LetDecl or Stmt::LetPattern).
    let mut let_stmt = parse_let(stream)?;

    // Reject destructuring patterns (LetPattern) for MVP — the desugar
    // assumes a single binding name.
    if !matches!(let_stmt, Stmt::LetDecl { .. }) {
        return Err(ParseError::new(
            Diagnostic::error(
                "property wrappers do not support destructuring `let` targets (use a plain `let` and apply the wrapper to each binding)",
                cached_fn_span,
            )
            .with_code(ErrorCode::UnexpectedToken),
        ));
    }

    // Destructure the LetDecl so we can rewrite `value`. The `mutable`
    // and `ty` fields are intentionally discarded: the wrapper cell is
    // always immutable (mutation goes through `.set(..)`/`.update(..)`),
    // and the user's annotation would describe `T` not `Signal<T>`/`Computed<T>`
    // (emitting it would produce incoherent Rust like `let x: Int = Signal::new(0)`).
    let Stmt::LetDecl {
        name,
        value: original_value,
        mutable: _,
        ty: _,
        span: let_span,
    } = let_stmt
    else {
        return Err(ParseError::new(
            Diagnostic::error(
                "internal: parse_let returned non-LetDecl after guard",
                cached_fn_span,
            )
            .with_code(ErrorCode::UnexpectedToken),
        ));
    };

    let desugared_value = match wrapper {
        "State" | "Published" => build_reactive_signal_new(original_value, let_span),
        "Cached" => {
            // `cached_fn_name` is `Some(_)` here because the @Cached
            // validation above returns Err when the arg is missing.
            // Match (not `.expect`) to keep the unreachable branch
            // panic-free (AGENTS.md hard rule).
            let Some(fn_name) = cached_fn_name else {
                return Err(ParseError::new(
                    Diagnostic::error(
                        "internal: @Cached reached desugar without a compute-fn name",
                        cached_fn_span,
                    )
                    .with_code(ErrorCode::UnexpectedToken),
                ));
            };
            build_reactive_computed_new(fn_name, cached_fn_span, let_span)
        }
        _ => {
            return Err(ParseError::new(
                Diagnostic::error(
                    format!("internal: unrecognised property wrapper `@{wrapper}` reached desugar"),
                    cached_fn_span,
                )
                .with_code(ErrorCode::UnexpectedToken),
            ));
        }
    };

    let_stmt = Stmt::LetDecl {
        name,
        value: desugared_value,
        // The wrapper is immutable: the Signal/Computed cell itself is
        // never reassigned (mutation happens through `.set(..)`/`.update(..)`).
        // Dropping `mut` keeps the generated Rust idiomatic.
        mutable: false,
        // Drop any user type annotation: the binding's actual Rust type is
        // `Signal<T>`/`Computed<T>`, not the `T` the user may have annotated.
        // Letting Rust infer keeps the generated source compiling.
        ty: None,
        span: let_span,
    };
    Ok(let_stmt)
}

/// T70: desugar `@pin let x = expr` into a normal `Stmt::LetDecl` whose
/// value is wrapped in `__buff_pin(expr)` — a sentinel function call that
/// the codegen lowers to `std::hint::black_box(expr)`.
///
/// This is a **pure parse-time desugar** (no new AST nodes, no AST field
/// changes) — mirroring the T56 property-wrapper pattern. The codegen's
/// `lower_expr` `Expr::FuncCall` arm intercepts the `__buff_pin` sentinel
/// name and emits `std::hint::black_box(#inner)`, which prevents rustc/LLVM
/// from eliminating, moving, or reordering the binding (useful for
/// memory-mapped I/O and hardware register access).
///
/// # Behaviour
///
/// - Expects the next token to be `let` (errors otherwise).
/// - Parses the underlying `let` via [`parse_let`].
/// - Rejects destructuring `let` targets (`@pin let (x, y) = ...`) for MVP
///   simplicity — pin applies to a single named binding.
/// - Rewrites the `let`'s `value` to `__buff_pin(original_value)`.
/// - Preserves the `mutable` flag and type annotation (unlike property
///   wrappers which drop them — a pinned binding keeps its original
///   mutability and type).
///
/// # Errors
///
/// Returns [`ParseError`] (using `ErrorCode::UnexpectedToken` — no new
/// ErrorCode per the stability promise) when:
/// - the next token is not `let`,
/// - the underlying `let` parses as a destructuring `Stmt::LetPattern`.
fn desugar_pin_statement(
    stream: &mut TokenStream<'_>,
    attr_span: Span,
) -> Result<Stmt, ParseError> {
    // The next significant token MUST be `let`.
    if !matches!(stream.peek_kind(), Some(TokenKind::KwLet)) {
        let span = stream
            .peek()
            .map(|t| t.span)
            .unwrap_or_else(|| stream.eof_span());
        let found = stream
            .peek_kind()
            .map(|k| k.to_string())
            .unwrap_or_else(|| "end of input".into());
        return Err(ParseError::new(
            Diagnostic::error(
                format!("`@pin` must precede a `let` binding, found `{found}`"),
                span,
            )
            .with_code(ErrorCode::UnexpectedToken),
        ));
    }

    // Parse the underlying let.
    let let_stmt = parse_let(stream)?;

    // Reject destructuring patterns for MVP — pin applies to a single
    // named binding.
    if !matches!(let_stmt, Stmt::LetDecl { .. }) {
        return Err(ParseError::new(
            Diagnostic::error(
                "`@pin` does not support destructuring `let` targets (use a plain `let` and apply `@pin` to each binding)",
                attr_span,
            )
            .with_code(ErrorCode::UnexpectedToken),
        ));
    }

    let Stmt::LetDecl {
        name,
        value: original_value,
        mutable,
        ty,
        span: let_span,
    } = let_stmt
    else {
        return Err(ParseError::new(
            Diagnostic::error(
                "internal: parse_let returned non-LetDecl after guard",
                attr_span,
            )
            .with_code(ErrorCode::UnexpectedToken),
        ));
    };

    // Wrap the original value in `__buff_pin(original_value)`. The
    // codegen intercepts this sentinel name and emits
    // `std::hint::black_box(original_value)`.
    let pinned_value = Expr::FuncCall {
        callee: Box::new(Expr::Ident(Ident::new("__buff_pin", attr_span), attr_span)),
        args: vec![original_value],
        span: let_span,
    };

    Ok(Stmt::LetDecl {
        name,
        value: pinned_value,
        mutable,
        ty,
        span: let_span,
    })
}

/// T56 helper: build the AST Expr for `ReactiveSignal.new(value)`.
///
/// Produces `Expr::MethodCall { receiver: Ident("ReactiveSignal"),
/// method: "new", args: [value] }`. The codegen's existing
/// `lower_prelude_type_assoc_fn` arm for `(PreludeType::ReactiveSignal,
/// PreludeAssocFn::New)` lowers this to `buff_reactive::Signal::new(value)`.
fn build_reactive_signal_new(value: Expr, span: Span) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(Expr::Ident(Ident::new("ReactiveSignal", span), span)),
        method: Ident::new("new", span),
        args: vec![value],
        span,
    }
}

/// T56 helper: build the AST Expr for
/// `ReactiveComputed.new({ || fn_name() })`.
///
/// The closure body is the ExprStmt of a no-arg call to `fn_name`. The
/// codegen's existing `(PreludeType::ReactiveComputed, PreludeAssocFn::New)`
/// arm lowers this to `buff_reactive::Computed::new(|| fn_name())`.
fn build_reactive_computed_new(fn_name: &str, name_span: Span, let_span: Span) -> Expr {
    let fn_call = Expr::FuncCall {
        callee: Box::new(Expr::Ident(Ident::new(fn_name, name_span), name_span)),
        args: Vec::new(),
        span: name_span,
    };
    let lambda = Expr::Lambda {
        params: Vec::new(),
        body: Block {
            stmts: vec![Stmt::ExprStmt(fn_call, name_span)],
            span: name_span,
        },
        return_type: None,
        span: name_span,
    };
    Expr::MethodCall {
        receiver: Box::new(Expr::Ident(
            Ident::new("ReactiveComputed", name_span),
            name_span,
        )),
        method: Ident::new("new", name_span),
        args: vec![lambda],
        span: let_span,
    }
}

/// `let [mut] name[: Type] = expr`
fn parse_let(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let let_tok = stream.expect(TokenKind::KwLet)?;
    let start = let_tok.span.start;

    let mutable = matches!(stream.peek_kind(), Some(TokenKind::KwMut));
    if mutable {
        stream.advance();
    }

    // T71: destructuring dispatch. Two shapes route to `Stmt::LetPattern`
    // (the existing bare-name `Stmt::LetDecl` path is left 100% untouched):
    // - `let (x, y) = ...`        → tuple pattern (next token is `(`).
    // - `let Point { x, y } = ...` → struct pattern (next is an `Ident`
    //   immediately followed by `{`; in `let`-target position `Ident {` can
    //   ONLY be a struct destructuring pattern — a struct literal can't be a
    //   binding target).
    let is_tuple_pat = matches!(stream.peek_kind(), Some(TokenKind::LParen));
    let is_struct_pat = matches!(
        (stream.peek_kind(), stream.peek_second_kind()),
        (Some(TokenKind::Ident(_)), Some(TokenKind::LBrace))
    );
    if is_tuple_pat || is_struct_pat {
        let pattern = parse_pattern(stream)?;
        // Optional type annotation `: Type` (rare for destructuring, but the
        // AST carries the field so we honour it for parity with `LetDecl`).
        let ty = if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
            stream.advance();
            Some(parse_type_ref(stream)?)
        } else {
            None
        };
        stream.expect(TokenKind::Assign)?;
        let value = parse_expression(stream)?;
        let end = value.span().end;
        let span = Span::new(start, end, source_id);
        return Ok(Stmt::LetPattern {
            pattern,
            value,
            mutable,
            ty,
            span,
        });
    }

    // Expect identifier
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected identifier after `let`, found end of input",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;

    // Optional type annotation `: Type`
    let ty = if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
        stream.advance();
        Some(parse_type_ref(stream)?)
    } else {
        None
    };

    // Expect `=`
    stream.expect(TokenKind::Assign)?;

    // Value expression
    let value = parse_expression(stream)?;
    let end = value.span().end;
    let span = Span::new(start, end, source_id);
    Ok(Stmt::LetDecl {
        name,
        value,
        mutable,
        ty,
        span,
    })
}

/// `return [expr]`
fn parse_return(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let ret_tok = stream.expect(TokenKind::KwReturn)?;
    let start = ret_tok.span.start;
    // No value if next is `}`, `;`, EOF, or any obvious statement terminator.
    let terminates = matches!(
        stream.peek_kind(),
        None | Some(TokenKind::RBrace) | Some(TokenKind::Semicolon)
    );
    if terminates {
        return Ok(Stmt::Return(None, ret_tok.span));
    }
    let expr = parse_expression(stream)?;
    let span = Span::new(start, expr.span().end, source_id);
    Ok(Stmt::Return(Some(expr), span))
}

/// `for var in iter { body }` or `for cond { body }`.
///
/// Disambiguation is via two-token lookahead: `IDENT in` → iterator form,
/// otherwise → conditional form.
fn parse_for(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let for_tok = stream.expect(TokenKind::KwFor)?;
    let start = for_tok.span.start;

    // T72: `for let PATTERN = EXPR { body }` — detect the `let` keyword
    // immediately after `for` and route to the looping-binding path. The
    // existing iterator-form (`for v in iter`) and conditional-form
    // (`for cond`) paths stay 100% untouched. Single let-binding only;
    // let-chains are T74.
    if matches!(stream.peek_kind(), Some(TokenKind::KwLet)) {
        return parse_for_let(stream, start, source_id);
    }

    let is_iterator = matches!(
        (stream.peek_kind(), stream.peek_second_kind()),
        (Some(TokenKind::Ident(_)), Some(TokenKind::KwIn))
    );

    if is_iterator {
        let var_tok = stream.advance_after_peek();
        let var = extract_ident(var_tok)?;
        stream.expect(TokenKind::KwIn)?;
        let iter_expr = parse_expression(stream)?;
        let body = parse_block(stream)?;
        let span = Span::new(start, body.span.end, source_id);
        Ok(Stmt::ForIn {
            var,
            iter: iter_expr,
            body,
            span,
        })
    } else {
        let cond = parse_expression(stream)?;
        let body = parse_block(stream)?;
        let span = Span::new(start, body.span.end, source_id);
        Ok(Stmt::ForWhile { cond, body, span })
    }
}

/// `while cond { body }` or `while cond:` + layout block (BUG-9).
///
/// Mirrors the conditional branch of [`parse_for`]: the leading `while`
/// keyword is consumed, then a condition expression, then a body block (via
/// [`parse_block`], which accepts BOTH brace `{ }` and layout `:` forms).
/// The resulting [`Stmt::While`] is structurally identical to
/// [`Stmt::ForWhile`] — the only difference is the surface keyword.
fn parse_while(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let while_tok = stream.expect(TokenKind::KwWhile)?;
    let start = while_tok.span.start;
    let cond = parse_expression(stream)?;
    let body = parse_block(stream)?;
    let span = Span::new(start, body.span.end, source_id);
    Ok(Stmt::While { cond, body, span })
}

/// Parse the looping-binding form `for let PATTERN = EXPR { body }` (T72).
/// The leading `for` is already consumed; the cursor is positioned at the
/// `let` keyword. `start` is the `for`'s start offset; `source_id` is the
/// current source.
///
/// Shape: `let PATTERN = EXPR`. The PATTERN is parsed via the shared
/// [`parse_pattern`] (same one `match` arms, T71 destructuring, and T72
/// if-let use), so `for let Some(x) = iter.next()`,
/// `for let (a, b) = pair_stream.next()`, etc. all work. The `=` is
/// mandatory. The EXPR is re-evaluated each iteration; when it stops
/// matching the pattern, the loop terminates.
///
/// Codegen lowers this to Rust's `while let PAT = EXPR { body }` (Buff
/// spells it `for let` because `while` is NOT a reserved Buff keyword and
/// the loop reads like the iterator-form `for v in iter`).
///
/// Single let-binding only. Let-chains are T74.
fn parse_for_let(
    stream: &mut TokenStream<'_>,
    start: usize,
    source_id: SourceId,
) -> Result<Stmt, ParseError> {
    // Consume `let`.
    let _let_tok = stream.expect(TokenKind::KwLet)?;
    // Parse the pattern.
    let pattern = parse_pattern(stream)?;
    // Expect `=`.
    stream.expect(TokenKind::Assign)?;
    // Parse the value expression.
    let value = parse_expression(stream)?;
    // Parse the body block.
    let body = parse_block(stream)?;
    let span = Span::new(start, body.span.end, source_id);
    Ok(Stmt::ForLet {
        pattern,
        value,
        body,
        span,
    })
}

/// Parse an early-return guard: `guard <cond>[, <cond>...] else { block }`
/// (T73).
///
/// Shape:
/// - `guard BOOL_EXPR else { ... }` — boolean condition.
/// - `guard let PATTERN = EXPR else { ... }` — let-binding (introduces the
///   pattern's names into the enclosing scope when the guard passes).
/// - `guard let PATTERN = EXPR, BOOL_EXPR, ... else { ... }` — multiple
///   comma-separated conditions (mixed let/bool); ALL must succeed for the
///   guard to pass.
///
/// The leading `guard` is consumed here. Conditions are parsed left-to-right;
/// for each, a peek of `let` routes to the let-binding path (reusing the
/// shared [`parse_pattern`], same one match/T71-destructuring/T72-if-let use),
/// anything else is parsed as a boolean expression via [`parse_expression`].
/// The separator is `,` (comma). The list ends at the mandatory `else`
/// keyword. The else-block is parsed via [`parse_block`] (brace OR layout).
///
/// Codegen emits, IN ORDER, one Rust statement per condition:
/// - `let PATTERN = EXPR else { ... };` (Rust let-else; bindings stay in
///   scope — that's the whole point of guard).
/// - `if !(BOOL_EXPR) { ... }` (negated condition; the else-block runs when
///   the original is false).
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - missing `else` after the condition list,
/// - empty condition list (`guard else { ... }`),
/// - malformed pattern / expression / else-block.
fn parse_guard(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let guard_tok = stream.expect(TokenKind::KwGuard)?;
    let start = guard_tok.span.start;

    // Parse a comma-separated list of conditions. Stop at `else`.
    //
    // Shape: `cond ("," cond)* ","? "else"`. The first condition is
    // mandatory (an empty `guard else { ... }` is a parse error). After
    // each condition we accept either `,` (with optional trailing comma
    // before `else`) or `else` directly.
    let mut conditions: Vec<GuardCondition> = Vec::new();
    loop {
        // After the first condition, the next token must be `,` or `else`.
        if !conditions.is_empty() {
            match stream.peek_kind() {
                Some(TokenKind::KwElse) => break,
                Some(TokenKind::Comma) => {
                    stream.advance(); // consume `,`
                                      // Trailing comma: `guard a, else { ... }` is allowed.
                    if matches!(stream.peek_kind(), Some(TokenKind::KwElse)) {
                        break;
                    }
                }
                Some(other) => {
                    return Err(ParseError::new(Diagnostic::error(
                        format!("expected `,` or `else` in guard, found `{other}`"),
                        stream
                            .peek()
                            .map(|t| t.span)
                            .unwrap_or_else(|| stream.eof_span()),
                    )));
                }
                None => {
                    return Err(ParseError::new(Diagnostic::error(
                        "unterminated guard (missing `else`)",
                        stream.eof_span(),
                    )));
                }
            }
        }
        // The next token starts a condition (or it's `else`/EOF, which is
        // an error: empty condition list, or trailing-comma-only).
        if matches!(stream.peek_kind(), Some(TokenKind::KwElse)) | stream.peek_kind().is_none() {
            return Err(ParseError::new(Diagnostic::error(
                "expected at least one condition after `guard`",
                stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span()),
            )));
        }
        // Parse one condition: either `let PATTERN = EXPR` or a BOOL expr.
        let cond = if matches!(stream.peek_kind(), Some(TokenKind::KwLet)) {
            let let_tok = stream.expect(TokenKind::KwLet)?;
            let cond_start = let_tok.span.start;
            let pattern = parse_pattern(stream)?;
            stream.expect(TokenKind::Assign)?;
            let value = parse_expression(stream)?;
            let end = value.span().end;
            GuardCondition::Let {
                pattern,
                value,
                span: Span::new(cond_start, end, source_id),
            }
        } else {
            let expr = parse_expression(stream)?;
            GuardCondition::Bool(expr)
        };
        conditions.push(cond);
    }

    // Consume `else`.
    stream.expect(TokenKind::KwElse)?;

    // Parse the else-block (brace OR layout form).
    let else_block = parse_block(stream)?;
    let span = Span::new(start, else_block.span.end, source_id);
    Ok(Stmt::Guard {
        conditions,
        else_block,
        span,
    })
}

/// Parse a deferred-execution statement: `defer EXPR` (T100).
///
/// Shape: `defer <expression>`. The leading `defer` keyword is consumed
/// here; then a single expression is parsed via [`parse_expression`]. The
/// resulting [`Stmt::Defer`] carries that expression. The codegen collects
/// all `Stmt::Defer` in a function body (in registration order) and emits
/// them in REVERSE order at every function exit point (each `return` and
/// the implicit fall-through at the body end), implementing LIFO semantics.
///
/// v0.5 defers a single expression. A deferred block (`defer { ... }`) is a
/// future extension — the parser does NOT recognise the brace form today
/// (the expression parser will reject a `{` at expression-statement start).
///
/// # Errors
///
/// Returns [`ParseError`] if the deferred expression fails to parse.
fn parse_defer(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let defer_tok = stream.expect(TokenKind::KwDefer)?;
    let start = defer_tok.span.start;
    let expr = parse_expression(stream)?;
    let end = expr.span().end;
    let span = Span::new(start, end, source_id);
    Ok(Stmt::Defer { expr, span })
}

fn parse_comptime_block(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let ct_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected `comptime`, found end of input",
            stream.eof_span(),
        ))
    })?;
    let start = ct_tok.span.start;
    let body = parse_block(stream)?;
    let span = Span::new(start, body.span.end, source_id);
    Ok(Stmt::ComptimeBlock { body, span })
}

/// Either an assignment (`x = ...`, `x += ...`) or a bare expression
/// statement (`foo()`, `print(x)`).
///
/// T7's expression parser already folds `=`/`+=`/etc. into
/// [`Expr::BinaryOp`] at the assignment-precedence level. To present these
/// as [`Stmt::Assignment`] we unwrap the resulting BinaryOp here. Any other
/// expression becomes [`Stmt::ExprStmt`].
fn parse_assignment_or_expr_stmt(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let expr = parse_expression(stream)?;

    // If T7's expression parser already produced an assignment BinaryOp,
    // peel it apart into a Stmt::Assignment.
    if let Expr::BinaryOp { op, lhs, rhs, span } = expr {
        if matches!(
            op,
            BinaryOp::Assign
                | BinaryOp::AddAssign
                | BinaryOp::SubAssign
                | BinaryOp::MulAssign
                | BinaryOp::DivAssign
                | BinaryOp::ModAssign
        ) {
            return Ok(Stmt::Assignment {
                target: *lhs,
                op,
                value: *rhs,
                span,
            });
        }
        // Non-assignment BinaryOp falls through to ExprStmt.
        let span_field = span;
        return Ok(Stmt::ExprStmt(
            Expr::BinaryOp {
                op,
                lhs,
                rhs,
                span: span_field,
            },
            span_field,
        ));
    }

    let span = expr.span();
    Ok(Stmt::ExprStmt(expr, span))
}

/// Parse an `if` expression: `if cond BLOCK [else BLOCK | else if ...]`.
///
/// The leading `if` is consumed. The condition is parsed via
/// [`parse_expression`]; the blocks via [`parse_block`] (so both
/// brace-delimited `{ ... }` and layout-sensitive `: NEWLINE INDENT ...
/// DEDENT` forms work, per T9). `else if` chains are desugared into a
/// nested [`Expr::IfExpr`] wrapped in a single-statement block.
///
/// This is invoked from [`parse_statement`] when an `if` starts a statement
/// AND from [`crate::expr::parse_primary`] (T9) so `if` can appear inside
/// arbitrary expressions (e.g. `let x = if c { 1 } else { 2 }`).
///
/// **Dangling-else**: the recursive call inside the `else if` branch binds
/// the else to the nearest (innermost) `if` — the standard lexical-scope
/// rule. Layout blocks enforce the same: the lexer's Dedent tokens
/// naturally delimit inner-if blocks before `else` is observed.
///
/// **T74 — let-chains**: the condition may be a comma-separated list of
/// conditions, each either `let PATTERN = EXPR` or a boolean `EXPR`. A
/// multi-condition `if` desugars to NESTED single-condition [`Expr::IfLet`]
/// / [`Expr::IfExpr`] via [`fold_if_chain`] (see its docs for the exact
/// nesting shape and else-block replication). A single-condition `if`
/// produces the identical AST shape as before T74 (zero regression).
pub fn parse_if_expr(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let source_id = stream.source_id();
    let if_tok = stream.expect(TokenKind::KwIf)?;
    let start = if_tok.span.start;

    // Parse a comma-separated list of conditions. Each is either
    // `let PATTERN = EXPR` (a binding condition) or a boolean `EXPR`. Stop
    // at the then-block starter (`{` or `:` for the layout form) or `else`.
    //
    // A single condition reproduces the pre-T74 behaviour exactly. Two or
    // more conditions trigger the nested-desugar in [`fold_if_chain`].
    let mut conditions: Vec<IfCondition> = Vec::new();
    loop {
        // After the first condition, expect `,` (with optional trailing
        // comma before the block/else) or stop at the block/else directly.
        if !conditions.is_empty() {
            match stream.peek_kind() {
                Some(TokenKind::Comma) => {
                    stream.advance(); // consume `,`
                                      // Trailing comma: `if a, { }` / `if a, else { }`.
                    if matches!(
                        stream.peek_kind(),
                        Some(TokenKind::LBrace) | Some(TokenKind::Colon) | Some(TokenKind::KwElse)
                    ) {
                        break;
                    }
                }
                Some(TokenKind::LBrace) | Some(TokenKind::Colon) | Some(TokenKind::KwElse) => break,
                Some(other) => {
                    return Err(ParseError::new(Diagnostic::error(
                        format!("expected `,`, `{{`, or `else` in if-chain, found `{other}`"),
                        stream
                            .peek()
                            .map(|t| t.span)
                            .unwrap_or_else(|| stream.eof_span()),
                    )));
                }
                None => {
                    return Err(ParseError::new(Diagnostic::error(
                        "unterminated if (missing block)",
                        stream.eof_span(),
                    )));
                }
            }
        }
        // Parse one condition. `let` starts a binding condition; anything
        // else is a boolean expression.
        let cond = if matches!(stream.peek_kind(), Some(TokenKind::KwLet)) {
            let let_tok = stream.expect(TokenKind::KwLet)?;
            let cond_start = let_tok.span.start;
            let pattern = parse_pattern(stream)?;
            stream.expect(TokenKind::Assign)?;
            let value = parse_expression(stream)?;
            let end = value.span().end;
            IfCondition::Let {
                pattern,
                value,
                span: Span::new(cond_start, end, source_id),
            }
        } else {
            let expr = parse_expression(stream)?;
            IfCondition::Bool(expr)
        };
        conditions.push(cond);
    }

    // Then-block.
    let then_block = parse_block(stream)?;
    let mut end = then_block.span.end;

    // Optional else: `else BLOCK` or `else if ...` (chains).
    let else_block = if matches!(stream.peek_kind(), Some(TokenKind::KwElse)) {
        stream.advance(); // consume `else`
        if matches!(stream.peek_kind(), Some(TokenKind::KwIf)) {
            // `else if ...` — recurse (handles both `else if let` chains and
            // plain `else if`). Wrap the nested if-expr in a single-stmt
            // block, matching the plain IfExpr path's shape.
            let nested = parse_if_expr(stream)?;
            end = nested.span().end;
            Some(Block {
                stmts: vec![Stmt::ExprStmt(nested.clone(), nested.span())],
                span: nested.span(),
            })
        } else {
            let blk = parse_block(stream)?;
            end = blk.span.end;
            Some(blk)
        }
    } else {
        None
    };

    // Fold the conditions into a (possibly nested) IfLet/IfExpr chain.
    // `end` carries the overall span end (includes the else block if present).
    Ok(fold_if_chain(
        conditions, then_block, else_block, start, end, source_id,
    ))
}

/// Parser-internal accumulator for one condition in a T74 let-chain (`if cond,
/// cond, ... { }`). Mirrors the shape of [`buff_lang_ast::GuardCondition`]
/// (T73) but kept parser-local to avoid semantic conflation with the `guard`
/// statement. The chain is desugared to nested [`Expr::IfLet`] / [`Expr::IfExpr`]
/// by [`fold_if_chain`], so this type never appears in the final AST.
#[derive(Debug, Clone)]
enum IfCondition {
    /// `let PATTERN = EXPR` — a binding condition. On match the pattern's
    /// bindings are in scope for later conditions and the then-block.
    Let {
        pattern: Pattern,
        value: Expr,
        span: Span,
    },
    /// A boolean condition (`x > 0`, `flag`, …).
    Bool(Expr),
}

/// Fold a comma-separated list of if-conditions into a (possibly nested)
/// chain of single-condition [`Expr::IfLet`] / [`Expr::IfExpr`] (T74).
///
/// The desugar nests from the OUTER condition inward:
///
/// ```text
/// if c1, c2, c3 { BODY } else { ELSE }
/// ```
///
/// becomes
///
/// ```text
/// if c1 {
///     if c2 {
///         if c3 {
///             BODY
///         } else { ELSE }
///     } else { ELSE }
/// } else { ELSE }
/// ```
///
/// where each `ci` is `let PATTERN = EXPR` (→ [`Expr::IfLet`]) or a boolean
/// `EXPR` (→ [`Expr::IfExpr`]).
///
/// **Else-block replication**: when an `else` is present, it is cloned at
/// EVERY nesting level so ANY failing condition triggers it. This is
/// semantically equivalent to a single shared else (each failing condition
/// independently runs the same user-written else) and is the simplest path
/// that avoids control-flow-graph reshaping. [`Block`] derives `Clone`, so
/// the clone is cheap and correct. When there is NO else, each level gets
/// `else_block: None` (Rust `if let ... { if let ... { body } }`).
///
/// **Single condition**: when `conditions` has exactly one element the fold
/// produces a single flat [`Expr::IfLet`] or [`Expr::IfExpr`] whose shape is
/// byte-identical to the pre-T74 single-condition `if`/`if let` — zero
/// regression for existing programs.
///
/// `conditions` MUST be non-empty (the caller, [`parse_if_expr`], parses at
/// least one condition or returns a [`ParseError`] first). This invariant is
/// upheld by the peel-then-fold structure below (the first element is always
/// consumed as the outermost), so no `unwrap`/`expect`/`unreachable!` is
/// needed.
fn fold_if_chain(
    conditions: Vec<IfCondition>,
    then_block: Block,
    else_block: Option<Block>,
    start: usize,
    end: usize,
    source_id: SourceId,
) -> Expr {
    // Peel the FIRST condition (the outermost). Fold the REMAINING conditions
    // (indices 1..) into the then-block, building from the INNERMOST (last)
    // outward; then wrap the first condition around the folded body.
    let mut conds = conditions;
    let outermost = conds.remove(0);
    // Remaining conditions, innermost-first (reversed) for the fold.
    let inner_conds_rev: Vec<IfCondition> = conds.into_iter().rev().collect();

    // `body_block` is the then-block for the level currently being built.
    // It starts as the original then-block (the innermost body) and is
    // replaced by a wrapper block after each inner condition is folded in.
    let mut body_block = then_block;
    let mut body_end = body_block.span.end;

    for cond in inner_conds_rev {
        // The else for THIS level: a clone of the original (replicated).
        let level_else = else_block.clone();
        let cond_end = match &cond {
            IfCondition::Let { span, .. } => span.end,
            IfCondition::Bool(e) => e.span().end,
        };
        let level_end = body_end.max(cond_end);
        let expr = match cond {
            IfCondition::Let { pattern, value, .. } => Expr::IfLet {
                pattern,
                value: Box::new(value),
                then_block: body_block,
                else_block: level_else,
                span: Span::new(start, level_end, source_id),
            },
            IfCondition::Bool(e) => Expr::IfExpr {
                cond: Box::new(e),
                then_block: body_block,
                else_block: level_else,
                span: Span::new(start, level_end, source_id),
            },
        };
        let es = expr.span();
        // Wrap the built expression as the then-block for the NEXT (outer)
        // level. (Mirrors how `else if` wraps its nested if in a single-
        // statement block.)
        body_block = Block {
            stmts: vec![Stmt::ExprStmt(expr, es)],
            span: es,
        };
        body_end = es.end;
    }

    // Build the outermost condition wrapping the folded body. The outermost
    // span end is the overall `end` (which includes the else block if any).
    let outermost_else = else_block; // original moved here (last consumer)
    match outermost {
        IfCondition::Let { pattern, value, .. } => Expr::IfLet {
            pattern,
            value: Box::new(value),
            then_block: body_block,
            else_block: outermost_else,
            span: Span::new(start, end, source_id),
        },
        IfCondition::Bool(e) => Expr::IfExpr {
            cond: Box::new(e),
            then_block: body_block,
            else_block: outermost_else,
            span: Span::new(start, end, source_id),
        },
    }
}

/// End byte offset of a [`Stmt`]'s span. Used by [`parse_block`] to compute
/// the parent block's end position from its last child statement.
fn stmt_end(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::LetDecl { span, .. }
        | Stmt::Assignment { span, .. }
        | Stmt::ExprStmt(_, span)
        | Stmt::Return(_, span)
        | Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::ForIn { span, .. }
        | Stmt::ForWhile { span, .. }
        | Stmt::While { span, .. }
        | Stmt::LetPattern { span, .. }
        | Stmt::ForLet { span, .. }
        | Stmt::Guard { span, .. }
        | Stmt::Defer { span, .. }
        | Stmt::ComptimeBlock { span, .. } => span.end,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::TypeRef;
    use buff_lang_error::SourceId;

    fn sid() -> SourceId {
        SourceId(0)
    }

    fn stream_of(src: &str) -> TokenStream<'static> {
        // Leak the tokens so we get a 'static lifetime for test ergonomics.
        let toks = buff_lang_lexer::tokenize(src, sid()).expect("lexer should succeed");
        let boxed: &'static [buff_lang_lexer::Token] = Box::leak(toks.into_boxed_slice());
        TokenStream::new(boxed, sid())
    }

    #[test]
    fn unit_let_int() {
        let mut s = stream_of("let x = 42");
        let st = parse_statement(&mut s).expect("parse let");
        match st {
            Stmt::LetDecl {
                name, mutable, ty, ..
            } => {
                assert_eq!(name.name, "x");
                assert!(!mutable);
                assert!(ty.is_none());
            }
            other => panic!("expected LetDecl, got {other:?}"),
        }
    }

    #[test]
    fn unit_break_continue() {
        let mut s = stream_of("break");
        assert!(matches!(parse_statement(&mut s), Ok(Stmt::Break(_))));
        let mut s = stream_of("continue");
        assert!(matches!(parse_statement(&mut s), Ok(Stmt::Continue(_))));
    }

    #[test]
    fn unit_func_at_stmt_level_errors() {
        let mut s = stream_of("func foo() { }");
        let err = parse_statement(&mut s).expect_err("func should error at stmt level");
        assert!(err.diagnostic.message.contains("top-level"));
    }

    #[test]
    fn unit_type_ref_named() {
        let mut s = stream_of("Int");
        let ty = parse_type_ref(&mut s).expect("parse type");
        assert!(matches!(ty, TypeRef::Named { .. }));
    }

    #[test]
    fn unit_type_ref_generic() {
        let mut s = stream_of("Vector<Int>");
        let ty = parse_type_ref(&mut s).expect("parse type");
        match ty {
            TypeRef::Generic { args, .. } => assert_eq!(args.len(), 1),
            other => panic!("expected Generic, got {other:?}"),
        }
    }
}
