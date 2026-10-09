//! `trait` declarations (with associated types) and `impl` blocks
//! (with type bindings). ITER-52 pure move from stmt_decl.rs.

use buff_lang_ast::{
    AssociatedType, AssociatedTypeBinding, Block, FuncDecl, ImplBlock, MethodSig, Stmt, TraitDecl,
    TypeRef,
};
use buff_lang_error::{Diagnostic, ParseError, Span};
use buff_lang_lexer::TokenKind;

use crate::expr::parse_expression;
use crate::stmt::parse_block;
use crate::stream::TokenStream;

use super::func::*;
use super::shared::*;

/// Parse a top-level `trait Name [: Super, ...] { fn ...; fn ... { } }`
/// declaration with default methods and inheritance (T93).
///
/// Shape:
/// - `trait Greetable { fn name() -> String; fn greet() { print(name()) } }`
///   — `name` is a REQUIRED method (`;`-terminated, bodyless →
///   [`MethodSig`]); `greet` is a DEFAULT method (brace block → full
///   [`FuncDecl`] with body).
/// - `trait Pet : Animal { fn pet() { ... } }` — single supertrait.
/// - `trait A : B, C { ... }` — multiple comma-separated supertraits.
///
/// # Required vs default classification
///
/// Each `fn` member inside the trait body is parsed via the shared
/// [`parse_func_decl`] machinery UP TO the body decision point, then
/// classified by the trailing token:
/// - `;` (semicolon) → REQUIRED: the method has NO body; stored as a
///   [`MethodSig`] in [`TraitDecl::required`].
/// - `{ ... }` (brace block) or `=>` (expression shorthand) or layout
///   `: NEWLINE INDENT ... DEDENT` → DEFAULT: the method HAS a body;
///   stored as a full [`FuncDecl`] in [`TraitDecl::defaults`].
///
/// This mirrors Rust's trait syntax exactly: `fn sig;` is required,
/// `fn sig { body }` is a default method.
///
/// # Supertrait parsing
///
/// After the trait name, an optional `: Supertrait` clause introduces one
/// or more supertraits. Each supertrait is parsed via [`parse_type_ref`]
/// (today always a [`TypeRef::Named`]); multiple supertraits are
/// comma-separated. The colon is consumed only when the next token after
/// the name is `:` — so `trait Foo { ... }` (no supertraits) and
/// `trait Foo : Bar { ... }` (one supertrait) are both valid.
///
/// # Codegen target
///
/// Lowers to a Rust `syn::ItemTrait`: required methods become bodyless
/// trait method signatures; default methods become trait methods WITH a
/// default body (Rust default-method syntax); supertraits populate the
/// trait's `supertraits` Punctuated list.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - the token after `trait` is not an identifier,
/// - the opening `{` is missing,
/// - a method's `fn` declaration fails to parse,
/// - the body is empty (zero methods),
/// - the closing `}` is missing.
pub fn parse_trait_decl(stream: &mut TokenStream<'_>) -> Result<TraitDecl, ParseError> {
    let source_id = stream.source_id();
    let trait_tok = stream.expect(TokenKind::KwTrait)?;
    let start = trait_tok.span.start;

    // Trait name (mandatory identifier).
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected trait name after `trait`",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;
    let name_end = name.span.end;

    // Optional supertraits: `: SuperA, SuperB, ...`.
    let mut supertraits: Vec<TypeRef> = Vec::new();
    if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
        stream.advance(); // consume `:`
        loop {
            let st = parse_type_ref(stream)?;
            supertraits.push(st);
            match stream.peek_kind() {
                Some(TokenKind::Comma) => {
                    stream.advance();
                    // Allow trailing comma: `: A, B,`.
                    if matches!(stream.peek_kind(), Some(TokenKind::LBrace)) {
                        break;
                    }
                }
                Some(TokenKind::LBrace) => break,
                Some(other) => {
                    return Err(ParseError::new(Diagnostic::error(
                        format!("expected `,` or `{{` in supertrait list, found `{other}`"),
                        stream
                            .peek()
                            .map(|t| t.span)
                            .unwrap_or_else(|| stream.eof_span()),
                    )));
                }
                None => {
                    return Err(ParseError::new(Diagnostic::error(
                        "unterminated supertrait list (missing `}`)",
                        stream.eof_span(),
                    )));
                }
            }
        }
    }

    // Opening `{` of the member list.
    stream.expect(TokenKind::LBrace)?;

    let mut required: Vec<MethodSig> = Vec::new();
    let mut defaults: Vec<FuncDecl> = Vec::new();
    // T75b: associated-type declarations inside the trait body
    // (`type Item;` or `type Item: Bound;`). Each is collected here and
    // surfaced as `syn::TraitItemType` at codegen time.
    let mut associated_types: Vec<AssociatedType> = Vec::new();

    // Empty body `trait Foo { }` is a parse error — a trait with zero
    // members is meaningless and almost certainly a user typo.
    if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
        let rb = stream.expect(TokenKind::RBrace)?;
        return Err(ParseError::new(Diagnostic::error(
            "trait must declare at least one method",
            Span::new(start, rb.span.end, source_id),
        )));
    }

    // Parse members until the closing `}`. Each member starts with `func`
    // (Buff's function keyword). Layout tokens (newlines) between members
    // are transparently skipped by TokenStream::peek/advance.
    //
    // T75b: the loop also recognizes `type Item;` (associated-type
    // declarations). When `KwType` is seen, we branch to a dedicated
    // associated-type parser instead of entering the `fn` path.
    while matches!(
        stream.peek_kind(),
        Some(TokenKind::KwFunc)
            | Some(TokenKind::KwAsync)
            | Some(TokenKind::KwExtern)
            | Some(TokenKind::KwType)
    ) {
        // T75b: `type Item [: Bounds] ;` — associated-type declaration.
        // The bodyless form is the ONLY form inside a trait (impl-block
        // type bindings `type Item = T;` are parsed in `parse_impl_decl`).
        if matches!(stream.peek_kind(), Some(TokenKind::KwType)) {
            let at = parse_trait_associated_type(stream)?;
            associated_types.push(at);
            // Optional `;` separator (already consumed by the parser, but
            // tolerate a stray duplicate from `;;`).
            if matches!(stream.peek_kind(), Some(TokenKind::Semicolon)) {
                stream.advance();
            }
            continue;
        }
        // Parse the fn up to the body decision. We reuse parse_func_decl
        // but we need to intercept BEFORE it consumes the body — because
        // a required method (`fn ... ;`) has NO body. The trick: parse
        // the signature manually (name, params, return type), then peek
        // at the next token to decide required (`;`) vs default (block).
        //
        // We parse the signature inline rather than calling parse_func_decl
        // because parse_func_decl ALWAYS expects a body (or `=>`) — it has
        // no `;`-terminated path. Duplicating the ~30 lines of signature
        // parsing is cleaner than threading a "may be bodyless" flag
        // through parse_func_decl.
        let member_start_tok = stream.advance_after_peek();
        let member_start = member_start_tok.span.start;
        // Consume optional `extern` / `async` modifiers (same order as
        // parse_func_decl).
        let mut is_extern = false;
        let mut is_async = false;
        match member_start_tok.kind {
            TokenKind::KwExtern => {
                is_extern = true;
                // After `extern`, expect `func`.
                if matches!(stream.peek_second_kind(), Some(TokenKind::KwFunc))
                    && matches!(stream.peek_kind(), Some(TokenKind::Ident(_)))
                {
                    // `extern crate` — not valid inside a trait body.
                    return Err(ParseError::new(Diagnostic::error(
                        "`extern crate` is not allowed inside a trait body",
                        member_start_tok.span,
                    )));
                }
                // Consume optional async after extern (rare but valid).
                if matches!(stream.peek_kind(), Some(TokenKind::KwAsync)) {
                    is_async = true;
                    stream.advance();
                }
                stream.expect(TokenKind::KwFunc)?;
            }
            TokenKind::KwAsync => {
                is_async = true;
                // After async, expect `func`.
                stream.expect(TokenKind::KwFunc)?;
            }
            TokenKind::KwFunc => {}
            _ => {
                return Err(ParseError::new(Diagnostic::error(
                    "expected `func`, `async func`, or `extern func` inside trait body",
                    member_start_tok.span,
                )));
            }
        }
        // Now parse the signature: name(params) -> Ret.
        let m_name_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected method name after `func` in trait body",
                stream.eof_span(),
            ))
        })?;
        let m_name = extract_ident(m_name_tok)?;
        stream.expect(TokenKind::LParen)?;
        let m_params = parse_params(stream)?;
        let rparen = stream.expect(TokenKind::RParen)?;
        let mut sig_end = rparen.span.end;
        let m_return_type = if matches!(stream.peek_kind(), Some(TokenKind::Arrow)) {
            stream.advance();
            let ty = parse_type_ref(stream)?;
            sig_end = type_end(&ty);
            Some(ty)
        } else {
            None
        };

        // Body decision: `;` → required (bodyless); block/`=>`/layout → default.
        if matches!(stream.peek_kind(), Some(TokenKind::Semicolon)) {
            // REQUIRED method — bodyless signature.
            let semi = stream.advance_after_peek();
            required.push(MethodSig {
                name: m_name,
                params: m_params,
                return_type: m_return_type,
                span: Span::new(member_start, semi.span.end, source_id),
            });
        } else {
            // DEFAULT method — has a body. Build the FuncDecl by parsing
            // the body via the same logic as parse_func_decl (brace block,
            // `=>` expression shorthand, or layout block).
            let body = if is_extern {
                // extern fn inside a trait body with no `;` is unusual but
                // we synthesize an empty placeholder to match parse_func_decl.
                Block {
                    stmts: Vec::new(),
                    span: Span::new(sig_end, sig_end, source_id),
                }
            } else if matches!(stream.peek_kind(), Some(TokenKind::FatArrow)) {
                let arrow_tok = stream.advance().ok_or_else(|| {
                    ParseError::new(Diagnostic::error(
                        "expected `=>` after method signature",
                        stream.eof_span(),
                    ))
                })?;
                let expr = parse_expression(stream)?;
                let expr_end = expr.span().end;
                let ret_stmt = Stmt::Return(
                    Some(expr),
                    Span::new(arrow_tok.span.start, expr_end, source_id),
                );
                Block {
                    stmts: vec![ret_stmt],
                    span: Span::new(arrow_tok.span.start, expr_end, source_id),
                }
            } else {
                parse_block(stream)?
            };
            let body_end = body.span.end;
            defaults.push(FuncDecl {
                name: m_name,
                params: m_params,
                return_type: m_return_type,
                body,
                is_async,
                is_unsafe: false,
                is_extern,
                attributes: Vec::new(),
                type_params: Vec::new(),
                span: Span::new(member_start, body_end.max(sig_end), source_id),
            });
        }
        // Optional `;` separator between members (tolerated, not required).
        if matches!(stream.peek_kind(), Some(TokenKind::Semicolon)) {
            stream.advance();
        }
    }

    // Defensive: if no methods AND no associated types were collected
    // (stray tokens in body), error. The empty-body case `trait Foo { }`
    // is already caught above with the same message — this catches the
    // rarer "body of only stray tokens" case. T75b: associated types now
    // also satisfy the "non-empty body" requirement (a trait with only
    // `type Item;` is valid).
    if required.is_empty() && defaults.is_empty() && associated_types.is_empty() {
        return Err(ParseError::new(Diagnostic::error(
            "trait body must contain at least one method or associated type",
            stream.span_here(),
        )));
    }

    let rb = stream.expect(TokenKind::RBrace)?;
    Ok(TraitDecl {
        name,
        supertraits,
        associated_types,
        required,
        defaults,
        span: Span::new(start, name_end.max(rb.span.end), source_id),
    })
}

/// Parse an associated-type declaration inside a trait body (T75b —
/// associated types in traits).
///
/// Shape: `type Item [: Bound + Bound2 ...] ;`
///
/// - The leading `type` keyword is consumed here.
/// - The associated-type name is the next identifier.
/// - Optional bounds follow `:` (comma-separated would be wrong — Rust uses
///   `+` for bound lists, and so do we). Each bound is parsed via
///   [`parse_type_ref`] (today always a [`TypeRef::Named`]).
/// - The trailing `;` is mandatory (no `type Item` form without `;` —
///   that would be ambiguous with the type-alias top-level decl which is
///   not currently a Buff feature).
///
/// Returns an [`AssociatedType`] capturing the name, optional bounds, and
/// the span covering `type` through `;`.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - missing identifier after `type`,
/// - missing `;` at the end,
/// - malformed bound type-ref.
fn parse_trait_associated_type(stream: &mut TokenStream<'_>) -> Result<AssociatedType, ParseError> {
    let source_id = stream.source_id();
    let type_tok = stream.expect(TokenKind::KwType)?;
    let start = type_tok.span.start;

    // Associated-type name (mandatory identifier).
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected associated-type name after `type` in trait body",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;
    let mut end = name.span.end;

    // Optional bounds: `: BoundA + BoundB + ...`. Each bound is a typeref.
    // (Comma would conflict with supertrait lists at the trait header, and
    // Rust uses `+` here, so we follow Rust.)
    let mut bounds: Vec<TypeRef> = Vec::new();
    if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
        stream.advance(); // consume `:`
        loop {
            let b = parse_type_ref(stream)?;
            end = type_end(&b);
            bounds.push(b);
            match stream.peek_kind() {
                Some(TokenKind::Plus) => {
                    stream.advance();
                    // Allow trailing `+`: `type Item: Clone +`.
                    if matches!(stream.peek_kind(), Some(TokenKind::Semicolon)) {
                        break;
                    }
                }
                Some(TokenKind::Semicolon) => break,
                Some(other) => {
                    return Err(ParseError::new(Diagnostic::error(
                        format!(
                            "expected `+` or `;` in associated-type bound list, found `{other}`"
                        ),
                        stream
                            .peek()
                            .map(|t| t.span)
                            .unwrap_or_else(|| stream.eof_span()),
                    )));
                }
                None => {
                    return Err(ParseError::new(Diagnostic::error(
                        "unterminated associated-type declaration (missing `;`)",
                        stream.eof_span(),
                    )));
                }
            }
        }
    }

    // Mandatory trailing `;`.
    let semi = stream.expect(TokenKind::Semicolon)?;
    end = end.max(semi.span.end);

    Ok(AssociatedType {
        name,
        bounds,
        span: Span::new(start, end, source_id),
    })
}

/// Parse an `impl Trait for Type { ... }` trait-implementation block
/// (T75b — associated types in traits).
///
/// Shape:
///
/// ```text
/// impl TraitName for TargetType {
///     type Item = ConcreteType;      // associated-type bindings
///     func method(...) -> Ret { ... } // method implementations
/// }
/// ```
///
/// The leading `impl` keyword is consumed here. After the trait name,
/// `for` is mandatory (no inherent-impl form — Buff uses [`parse_extend_decl`]
/// for inherent-method blocks). The target type follows `for`. The body
/// uses braces (same convention as `trait`/`extend`).
///
/// # Body member parsing
///
/// The body accepts two member kinds, in any order, separated by newlines
/// (and an optional `;`):
/// - `type Name = TypeRef ;` — associated-type binding. Consumed via
///   [`parse_impl_type_binding`].
/// - `func ... { body }` — method implementation. Routed through the
///   shared [`parse_func_decl`] (the SAME path used by top-level funcs and
///   extend-block methods, so all parameter/return-type/body parsing is
///   unified).
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - missing trait name after `impl`,
/// - missing `for` between trait name and target type,
/// - missing target type after `for`,
/// - missing `{` opening the body,
/// - empty body `{ }`,
/// - malformed type binding (`type X = ;`),
/// - malformed method body,
/// - missing closing `}`.
pub fn parse_impl_decl(stream: &mut TokenStream<'_>) -> Result<ImplBlock, ParseError> {
    let source_id = stream.source_id();
    let impl_tok = stream.expect(TokenKind::KwImpl)?;
    let start = impl_tok.span.start;

    // Trait name (mandatory). Today always a `TypeRef::Named` (bare trait
    // name like `Container`); generic trait impls (`impl Iterable<Int> for
    // ...`) are deferred.
    let trait_name = parse_type_ref(stream)?;
    let trait_end = type_end(&trait_name);

    // Mandatory `for`.
    stream.expect(TokenKind::KwFor)?;

    // Target type the trait is being implemented FOR. Today always a
    // `TypeRef::Named` (bare type name); generic targets deferred.
    let target = parse_type_ref(stream)?;
    let target_end = type_end(&target);

    // Opening `{` of the body.
    stream.expect(TokenKind::LBrace)?;

    let mut type_bindings: Vec<AssociatedTypeBinding> = Vec::new();
    let mut methods: Vec<FuncDecl> = Vec::new();

    // Empty body `impl T for U { }` is a parse error — an impl with zero
    // members is meaningless (the trait would be unimplemented) and almost
    // certainly a user typo.
    if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
        let rb = stream.expect(TokenKind::RBrace)?;
        return Err(ParseError::new(Diagnostic::error(
            "impl block must declare at least one method or type binding",
            Span::new(start, rb.span.end, source_id),
        )));
    }

    // Parse members until the closing `}`. Two kinds: `type X = T;` (type
    // binding) and `func ... { body }` (method). The `type` keyword is
    // unambiguous inside an impl body — there is no top-level type-alias
    // decl in Buff, so `type` here is always an associated-type binding.
    loop {
        match stream.peek_kind() {
            Some(TokenKind::KwType) => {
                let b = parse_impl_type_binding(stream)?;
                type_bindings.push(b);
            }
            Some(TokenKind::KwFunc) | Some(TokenKind::KwAsync) | Some(TokenKind::KwExtern) => {
                let f = parse_func_decl(stream, Vec::new())?;
                methods.push(f);
            }
            Some(TokenKind::RBrace) => break,
            Some(other) => {
                return Err(ParseError::new(Diagnostic::error(
                    format!(
                        "expected `type` binding or `func` method inside impl body, found `{other}`"
                    ),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                )));
            }
            None => {
                return Err(ParseError::new(Diagnostic::error(
                    "unterminated impl block (missing `}`)",
                    stream.eof_span(),
                )));
            }
        }
        // Optional `;` separator between members (tolerated, not required).
        if matches!(stream.peek_kind(), Some(TokenKind::Semicolon)) {
            stream.advance();
        }
    }

    if type_bindings.is_empty() && methods.is_empty() {
        // Defensive: we already error on empty `{ }` above, but a body of
        // only stray tokens (which would have errored at the match above
        // anyway) lands here.
        return Err(ParseError::new(Diagnostic::error(
            "impl block must contain at least one `func` method or `type` binding",
            stream.span_here(),
        )));
    }

    let rb = stream.expect(TokenKind::RBrace)?;
    Ok(ImplBlock {
        trait_name,
        target,
        type_bindings,
        methods,
        span: Span::new(start, trait_end.max(target_end).max(rb.span.end), source_id),
    })
}

/// Parse a single `type Item = ConcreteType;` binding inside an
/// [`ImplBlock`] body (T75b — associated types in traits).
///
/// The leading `type` keyword is consumed here. The associated-type name
/// follows (an identifier), then a mandatory `=`, then a type-reference
/// (parsed via the shared [`parse_type_ref`]), then a mandatory `;`.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - missing identifier after `type`,
/// - missing `=` between name and target type,
/// - malformed target type-ref,
/// - missing `;` at the end.
fn parse_impl_type_binding(
    stream: &mut TokenStream<'_>,
) -> Result<AssociatedTypeBinding, ParseError> {
    let source_id = stream.source_id();
    let type_tok = stream.expect(TokenKind::KwType)?;
    let start = type_tok.span.start;

    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected associated-type name after `type` in impl body",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;
    let mut end = name.span.end;

    // Mandatory `=`.
    stream.expect(TokenKind::Assign)?;

    // Target type (any type-ref).
    let target = parse_type_ref(stream)?;
    end = end.max(type_end(&target));

    // Mandatory `;`.
    let semi = stream.expect(TokenKind::Semicolon)?;
    end = end.max(semi.span.end);

    Ok(AssociatedTypeBinding {
        name,
        target,
        span: Span::new(start, end, source_id),
    })
}
