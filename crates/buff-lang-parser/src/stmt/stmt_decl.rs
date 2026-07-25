//! Declaration parsers - extracted from `stmt.rs` (T106 mechanical split).
//!
//! Contains all top-level declaration parsers (`func`/`struct`/`enum`/
//! `import`/`export`/`extend`/`trait`/`extern`) plus the shared helpers they
//! depend on (`parse_type_ref`, `parse_params`, `parse_type_params`,
//! `parse_attributes`, `extract_ident`, `type_end`). All functions are
//! re-exported back to the parent `stmt` module via `pub use stmt_decl::*`.

use buff_lang_ast::{
    AssociatedType, AssociatedTypeBinding, Attribute, Block, Decl, EnumDecl, EnumVariant,
    ExportDecl, ExtendBlock, FuncDecl, Ident, ImplBlock, ImportDecl, MethodSig, Param,
    ReexportDecl, Stmt, StructDecl, TraitDecl, TypeParam, TypeRef,
};
use buff_lang_error::{Diagnostic, ParseError, Span};
use buff_lang_lexer::{Token, TokenKind};

use super::parse_block;
use crate::expr::parse_expression;
use crate::stream::TokenStream;

/// Parse a function declaration: `func name(params) -> Ret { body }`.
///
/// The leading `func` keyword is consumed here. Modifier keywords
/// (`async`, `extern`) preceding `func` are consumed here too (T31 added
/// `async`; T32 added `extern`). Encountering `unsafe` before `func` is the
/// caller's concern (not yet wired through the dispatcher). This function
/// is normally reached via [`crate::parser::parse`] which dispatches on
/// `KwFunc`, `KwAsync`+`KwFunc`, `KwExtern`+`KwFunc`, or `At`+...+`KwFunc`
/// (T35 — attributes).
///
/// # T31 — `async func` modifier
///
/// When this function is called with the cursor positioned at `KwAsync`
/// (the dispatcher routes `async func` here), it consumes the `async`
/// keyword and sets `is_async = true` on the resulting [`FuncDecl`].
/// Otherwise (`KwFunc` is the first token) `is_async` stays `false`.
/// Either way the `func` keyword must follow (and is consumed here).
///
/// # T32 — `extern func` modifier (FFI)
///
/// When the dispatcher routes `extern func` here, the leading `extern`
/// keyword is consumed and `is_extern = true` is set. **Extern funcs have
/// NO body** (they are foreign-function declarations); after parsing the
/// signature (`name(params) -> Ret`) the parser DOES NOT expect a block.
/// The codegen lowers an `is_extern` FuncDecl to a Rust
/// `extern "C" { fn name(params) -> Ret; }` foreign-mod item (the empty
/// placeholder [`Block`] stored on the AST is dropped at codegen time).
///
/// # T35 — `attributes` parameter
///
/// The caller may pass a `Vec<Attribute>` of already-parsed leading `@name`
/// attributes (collected by the top-level dispatcher when it saw `@` before
/// the function). These are attached verbatim to the resulting [`FuncDecl`].
/// The vast majority of call sites pass `Vec::new()` (no attributes).
///
/// # Errors
///
/// Returns [`ParseError`] on missing name, parameter list, return type
/// syntax, or (for non-extern funcs) body block.
pub fn parse_func_decl(
    stream: &mut TokenStream<'_>,
    attributes: Vec<Attribute>,
) -> Result<FuncDecl, ParseError> {
    // T32: consume the optional leading `extern` modifier (FFI declaration).
    let is_extern = if matches!(stream.peek_kind(), Some(TokenKind::KwExtern)) {
        let extern_tok = stream.advance_after_peek();
        let _ = extern_tok; // span tracking not needed for v0.5
        true
    } else {
        false
    };
    // T31: consume the optional leading `async` modifier.
    let is_async = if matches!(stream.peek_kind(), Some(TokenKind::KwAsync)) {
        let async_tok = stream.advance_after_peek();
        let _ = async_tok; // span tracking not needed for v0.5
        true
    } else {
        false
    };
    let func_tok = stream.expect(TokenKind::KwFunc)?;
    let start = func_tok.span.start;
    let source_id = stream.source_id();

    // Function name
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected function name after `func`",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;

    // T13: Optional generic parameters `<T, U, ...>` after the function name.
    // e.g. `func id<T>(x: T) -> T`. Uses the shared `parse_type_params` helper
    // (same as `enum`/`struct`). Returns empty Vec for non-generic funcs.
    let type_params = parse_type_params(stream)?;

    // Parameter list ( ... )
    stream.expect(TokenKind::LParen)?;
    let params = parse_params(stream)?;
    let rparen = stream.expect(TokenKind::RParen)?;

    // Optional return type: `-> Type`
    let mut end = rparen.span.end;
    let return_type = if matches!(stream.peek_kind(), Some(TokenKind::Arrow)) {
        stream.advance(); // consume `->`
        let ty = parse_type_ref(stream)?;
        end = type_end(&ty);
        Some(ty)
    } else {
        None
    };

    // Body: brace-delimited OR layout-sensitive block (T9), OR expression
    // function shorthand `=>` (T102).
    //
    // T102: `func f(x) => EXPR` is syntactic sugar for
    // `func f(x) { return EXPR }`. If the next token is `=>`, consume it,
    // parse a single expression, and synthesize a Block whose single
    // statement is `return EXPR`.
    //
    // T32: extern funcs are foreign-function declarations and have NO
    // body — synthesize an empty placeholder Block whose span ends at the
    // signature. The codegen detects `is_extern` and emits a Rust
    // `extern "C" { fn ...; }` foreign-mod item instead of a body-having
    // `ItemFn`, so the placeholder is never rendered.
    let body = if is_extern {
        Block {
            stmts: Vec::new(),
            span: Span::new(end, end, source_id),
        }
    } else if matches!(stream.peek_kind(), Some(TokenKind::FatArrow)) {
        // T102: expression function shorthand `=>`.
        let arrow_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected `=>` after function signature",
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
    let span = Span::new(start, body.span.end.max(end), source_id);

    Ok(FuncDecl {
        name,
        params,
        return_type,
        body,
        is_async,
        is_unsafe: false,
        is_extern,
        attributes,
        type_params,
        span,
    })
}

/// Parse a type reference: a named type optionally followed by generic
/// arguments in `<...>`.
///
/// Supported forms:
/// - `Int` → [`TypeRef::Named`]
/// - `Vector<Int>` → [`TypeRef::Generic`]
/// - `Map<String, Int>` → [`TypeRef::Generic`] with multiple args
/// - Nested: `Map<String, Vector<Int>>`
///
/// `Option<T>` and function types are recognized structurally as plain
/// [`TypeRef::Generic`] / [`TypeRef::Named`] for T8 — there is no special
/// sugar yet.
///
/// # Errors
///
/// Returns [`ParseError`] if the next token is not an identifier, or if a
/// generic argument list is missing its closing `>`.
pub fn parse_type_ref(stream: &mut TokenStream<'_>) -> Result<TypeRef, ParseError> {
    let source_id = stream.source_id();

    // T103: tuple type `(T, U, ...)` or grouping `(T)`. When the next token
    // is `(`, parse a comma-separated list of type refs until `)`. With 2+
    // members (counting trailing-comma `((T,)` as a 1-member list — Buff
    // does NOT have single-element tuples at the type layer for v0.5, so a
    // single `(T)` is grouping → return the bare `T`). With 2+ real members
    // build `TypeRef::Tuple(vec, span)`. This is the ONLY place tuple types
    // are produced; the rest of `parse_type_ref` handles named/generic/
    // option/union forms.
    if matches!(stream.peek_kind(), Some(TokenKind::LParen)) {
        let lp = stream.expect(TokenKind::LParen)?;
        let start = lp.span.start;
        let mut members: Vec<TypeRef> = Vec::new();
        // Empty `()` is not a valid type (unit isn't supported as a value
        // type in v0.5). Treat it as a 1-element error so the user gets a
        // clear message.
        if !matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
            loop {
                members.push(parse_type_ref(stream)?);
                match stream.peek_kind() {
                    Some(TokenKind::Comma) => {
                        stream.advance();
                        // Trailing comma: `(T, U,)` is allowed.
                        if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                            break;
                        }
                    }
                    Some(TokenKind::RParen) => break,
                    Some(other) => {
                        return Err(ParseError::new(Diagnostic::error(
                            format!("expected `,` or `)` in tuple type, found `{other}`"),
                            stream
                                .peek()
                                .map(|t| t.span)
                                .unwrap_or_else(|| stream.eof_span()),
                        )));
                    }
                    None => {
                        return Err(ParseError::new(Diagnostic::error(
                            "unterminated tuple type (missing `)`)",
                            stream.eof_span(),
                        )));
                    }
                }
            }
        }
        let rp = stream.expect(TokenKind::RParen)?;
        let span = Span::new(start, rp.span.end, source_id);
        // Empty `()` is not a valid type in v0.5 (unit is not a value type).
        if members.is_empty() {
            return Err(ParseError::new(Diagnostic::error(
                "empty `()` is not a valid type",
                span,
            )));
        }
        // The 2+-element disambiguation: a single `(T)` is grouping, NOT a
        // tuple. Return the lone member directly (its own span is preserved).
        // 2+ members build `TypeRef::Tuple(vec, span)`.
        return Ok(if members.len() >= 2 {
            TypeRef::Tuple(members, span)
        } else {
            // `members.len() == 1` (the loop above guarantees non-empty here).
            // `swap_remove(0)` is O(1) and avoids cloning; `members` is dropped
            // after this expression so the move is safe.
            members.swap_remove(0)
        });
    }

    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected type name, found end of input",
            stream.eof_span(),
        ))
    })?;
    let name_end = name_tok.span.end;
    let start = name_tok.span.start;
    let name = extract_ident(name_tok)?;
    let mut ty = TypeRef::Named {
        name,
        span: Span::new(start, name_end, source_id),
    };

    // Generic arguments: `<Type1, Type2, ...>`
    if matches!(stream.peek_kind(), Some(TokenKind::Lt)) {
        stream.advance(); // consume `<`
        let mut args = Vec::new();
        let mut last_end;
        loop {
            let arg = parse_type_ref(stream)?;
            last_end = type_end(&arg);
            args.push(arg);
            match stream.peek_kind() {
                Some(TokenKind::Comma) => {
                    stream.advance();
                }
                Some(TokenKind::Gt) => {
                    stream.advance();
                    break;
                }
                Some(other) => {
                    return Err(ParseError::new(Diagnostic::error(
                        format!("expected `,` or `>` in type argument list, found `{other}`"),
                        stream
                            .peek()
                            .map(|t| t.span)
                            .unwrap_or_else(|| stream.eof_span()),
                    )));
                }
                None => {
                    return Err(ParseError::new(Diagnostic::error(
                        "unterminated type argument list (missing `>`)",
                        stream.eof_span(),
                    )));
                }
            }
        }
        ty = TypeRef::Generic {
            base: Box::new(ty),
            args,
            span: Span::new(start, last_end, source_id),
        };
    }

    // T76: union types `A | B | C`. After parsing one type, if the next
    // token is `|` (Pipe), keep consuming `| Type` and collect into a
    // `TypeRef::Union`. This is ONLY active in TYPE position (here in
    // parse_type_ref) — it does NOT affect expression-level `|` (bitwise-
    // or), `||` (logical-or), or `|>` (pipeline).
    if matches!(stream.peek_kind(), Some(TokenKind::Pipe)) {
        let mut members = vec![ty];
        loop {
            stream.advance(); // consume `|`
            let member = parse_type_ref(stream)?;
            members.push(member);
            if !matches!(stream.peek_kind(), Some(TokenKind::Pipe)) {
                break;
            }
        }
        let union_end = match members.last() {
            Some(last) => type_end(last),
            None => start,
        };
        ty = TypeRef::Union(members, Span::new(start, union_end, source_id));
    }

    Ok(ty)
}

/// Parse a function parameter list body (without the surrounding parens).
///
/// Expects the cursor to be positioned just after `(`. Stops at the upcoming
/// `)`. Parameters are comma-separated; each one is `name: Type`.
///
/// # T75 — bare `self` receiver
///
/// As a SPECIAL CASE, the FIRST parameter may be a bare `self` (no
/// `: Type` annotation) — the receiver syntax used by extension methods
/// inside `extend TYPE { fn ... }` blocks. The synthesised type stored on
/// the resulting [`Param`] is `TypeRef::Named { name: "Self" }` (a marker;
/// the codegen uses the param NAME `self` to decide emission, not the
/// stored type). After the first param, every subsequent param requires
/// the `name: Type` shape as usual.
///
/// # Errors
///
/// Returns [`ParseError`] if any parameter is malformed.
pub fn parse_params(stream: &mut TokenStream<'_>) -> Result<Vec<Param>, ParseError> {
    let source_id = stream.source_id();
    let mut params = Vec::new();
    if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
        return Ok(params);
    }
    loop {
        let is_comptime =
            matches!(stream.peek_kind(), Some(TokenKind::Ident(s)) if s == "comptime");
        if is_comptime {
            stream.advance();
        }
        let name_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected parameter name, found end of input",
                stream.eof_span(),
            ))
        })?;
        let start = name_tok.span.start;
        let name = extract_ident(name_tok)?;
        // T75: bare `self` receiver — the first parameter of an extension
        // method is `self` (no `: Type`). Synthesise a placeholder type so
        // the resulting `Param` carries a valid TypeRef; the codegen emits
        // a Rust receiver (`self` / `&self` / `&mut self`) based on the
        // param NAME, not the stored type.
        let ty = if name.name == "self" && !matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
            TypeRef::Named {
                name: Ident::new("Self", Span::new(start, name.span.end, source_id)),
                span: Span::new(start, name.span.end, source_id),
            }
        } else {
            stream.expect(TokenKind::Colon)?;
            parse_type_ref(stream)?
        };
        let mut end = type_end(&ty);
        // T106: optional default value `name: Type = expr`. After the type,
        // if the next token is `=` (Assign), consume it and parse an
        // expression — the param carries `default_value: Some(expr)`. The
        // codegen fills omitted trailing args at the CALL SITE with this
        // default (Rust has no native default-param support). A bare `self`
        // receiver never has a default (no `=` follows it in well-formed
        // source), so this check is uniformly safe.
        let default_value = if matches!(stream.peek_kind(), Some(TokenKind::Assign)) {
            stream.advance(); // consume `=`
            let dv = parse_expression(stream)?;
            end = dv.span().end;
            Some(dv)
        } else {
            None
        };
        params.push(Param {
            name,
            ty,
            default_value,
            is_comptime,
            span: Span::new(start, end, source_id),
        });
        match stream.peek_kind() {
            Some(TokenKind::Comma) => {
                stream.advance();
                // Allow trailing comma: `(a: Int, b: Int,)`
                if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                    break;
                }
            }
            Some(TokenKind::RParen) => break,
            _ => {
                return Err(ParseError::new(Diagnostic::error(
                    format!(
                        "expected `,` or `)` in parameter list, found {}",
                        stream
                            .peek_kind()
                            .map(|k| k.to_string())
                            .unwrap_or_else(|| "end of input".to_string())
                    ),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                )));
            }
        }
    }
    Ok(params)
}

// ---------------------------------------------------------------------------
// T27 — Enum declarations.
//
// Buff enum syntax (brace form, consistent with map/struct-init "braces are
// data" rule from the README):
//
//   enum Color { Red, Green, Blue }
//   enum Shape { Circle(Float), Rect(Float, Float), Point }
//   enum Result<T, E> { Ok(T), Err(E) }
//
// Each variant is either a unit variant (`Red`) or a data-carrying tuple
// variant (`Circle(Float)`). Generic params `<T, E>` on the enum are parsed
// and stored on the decl; they are NOT validated against variant payloads at
// parse time (that is a later type-checking task). The closing `}` ends the
// span.
// ---------------------------------------------------------------------------

/// Parse a top-level `enum Name<generics> { Variant, Variant(T, U), ... }`
/// declaration (T27).
///
/// Shape:
/// - `enum Name { ... }` — non-generic enum, unit + data variants.
/// - `enum Name<T, E> { ... }` — generic enum; the `<...>` after the name
///   introduces type parameters that variants may reference in their payloads.
///
/// Each variant is one of:
/// - `Ident` — a unit variant (no payload).
/// - `Ident ( Type, Type, ... )` — a data-carrying tuple variant.
///
/// Variants are comma-separated; trailing comma is allowed. The body is
/// brace-delimited ( braces-for-data per the README convention, matching
/// map literals and struct-init). An empty body `enum Empty { }` is allowed
/// (zero variants — useful for type-level tricks and as a parsing edge case).
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - the token after `enum` is not an identifier,
/// - the opening `{` is missing,
/// - a variant name is missing or not an identifier,
/// - a variant payload type fails to parse via [`parse_type_ref`],
/// - the closing `}` is missing.
///
/// Parse an optional generic parameter list `<T, U, ...>` (T13).
///
/// Called after the decl name in `func`, `struct`, and `enum` declarations.
/// Returns an empty `Vec` when the next token is not `<` (the common case —
/// non-generic decls). When `<` is present, parses a comma-separated list of
/// type-parameter names, each wrapped in a [`TypeParam`] with empty bounds
/// (bounds are T38).
///
/// # Grammar
///
/// ```text
/// TypeParams ::= "<" Ident ("," Ident)* [","] ">"
///              |  /* empty — no `<` follows */
/// ```
///
/// Trailing comma is allowed: `<T, U,>`. The `>` closes the list.
///
/// # Disambiguation
///
/// The `<` token is shared with the less-than operator. This function is
/// ONLY called in declaration-name position (after `func NAME` / `struct
/// NAME` / `enum NAME`), where less-than is syntactically impossible —
/// so no peek-ahead disambiguation is needed.
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - a parameter name is not an identifier,
/// - the list is not comma-or-`>` separated,
/// - the closing `>` is missing.
pub fn parse_type_params(stream: &mut TokenStream<'_>) -> Result<Vec<TypeParam>, ParseError> {
    let source_id = stream.source_id();
    let mut params: Vec<TypeParam> = Vec::new();
    if !matches!(stream.peek_kind(), Some(TokenKind::Lt)) {
        return Ok(params);
    }
    stream.advance(); // consume `<`
    loop {
        let gtok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected generic parameter name, found end of input",
                stream.eof_span(),
            ))
        })?;
        let g_span = gtok.span;
        let g = extract_ident(gtok)?;
        // T38: optional trait bounds `: Bound (+ Bound)*`. Each bound is a
        // `TypeRef` (parsed via the shared `parse_type_ref` so `Clone`,
        // `Debug`, `Ord`, and generic bounds like `Iterator<Item=T>` all
        // parse uniformly). Multiple bounds are `+`-separated, mirroring
        // Rust's `<T: Clone + Debug>` syntax. When no `:` follows the name,
        // `bounds` stays empty (the T13 shape — fully backward-compatible).
        let mut bounds: Vec<TypeRef> = Vec::new();
        if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
            let colon_tok = stream.advance(); // consume `:`
            let _ = colon_tok;
            loop {
                bounds.push(parse_type_ref(stream)?);
                if matches!(stream.peek_kind(), Some(TokenKind::Plus)) {
                    stream.advance(); // consume `+`
                    continue;
                }
                break;
            }
        }
        params.push(TypeParam {
            name: g,
            bounds,
            span: g_span,
        });
        match stream.peek_kind() {
            Some(TokenKind::Comma) => {
                stream.advance();
                // Allow trailing comma: `<T,>`.
                if matches!(stream.peek_kind(), Some(TokenKind::Gt)) {
                    stream.advance();
                    break;
                }
            }
            Some(TokenKind::Gt) => {
                stream.advance();
                break;
            }
            Some(other) => {
                return Err(ParseError::new(Diagnostic::error(
                    format!("expected `,` or `>` in generic param list, found `{other}`"),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                )));
            }
            None => {
                return Err(ParseError::new(Diagnostic::error(
                    "unterminated generic param list (missing `>`)",
                    stream.eof_span(),
                )));
            }
        }
    }
    let _ = source_id; // source_id retained for future span construction
    Ok(params)
}

/// Parse an `enum` declaration (T27 + T13 generics).
///
/// See [`parse_enum_decl`] docs above for the full grammar.
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - the token after `enum` is not an identifier,
/// - the opening `{` is missing,
/// - a variant name is missing or not an identifier,
/// - a variant payload type fails to parse via [`parse_type_ref`],
/// - the closing `}` is missing.
pub fn parse_enum_decl(stream: &mut TokenStream<'_>) -> Result<EnumDecl, ParseError> {
    let enum_tok = stream.expect(TokenKind::KwEnum)?;
    let start = enum_tok.span.start;
    let source_id = stream.source_id();

    // Enum name (mandatory identifier).
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected enum name after `enum`",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;

    // Optional generic parameters: `<T, E>` (T13 — shared helper).
    let type_params = parse_type_params(stream)?;

    // Opening `{` of the variant list.
    stream.expect(TokenKind::LBrace)?;
    let mut variants: Vec<EnumVariant> = Vec::new();
    // Empty body: `enum Empty { }`.
    if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
        let rb = stream.expect(TokenKind::RBrace)?;
        return Ok(EnumDecl {
            name,
            type_params,
            variants,
            span: Span::new(start, rb.span.end, source_id),
        });
    }
    loop {
        // Variant name (mandatory identifier).
        let vname_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected enum variant name, found end of input",
                stream.eof_span(),
            ))
        })?;
        let vname = extract_ident(vname_tok.clone())?;
        let vstart = vname_tok.span.start;
        // Optional payload `( Type, Type, ... )`.
        let mut data: Option<Vec<TypeRef>> = None;
        if matches!(stream.peek_kind(), Some(TokenKind::LParen)) {
            stream.advance(); // consume `(`
            let mut tys: Vec<TypeRef> = Vec::new();
            // Empty payload `()` is allowed — treat as no payload (unit variant).
            if !matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                loop {
                    let ty = parse_type_ref(stream)?;
                    tys.push(ty);
                    match stream.peek_kind() {
                        Some(TokenKind::Comma) => {
                            stream.advance();
                            // Allow trailing comma.
                            if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                                break;
                            }
                        }
                        Some(TokenKind::RParen) => break,
                        Some(other) => {
                            return Err(ParseError::new(Diagnostic::error(
                                format!("expected `,` or `)` in variant payload, found `{other}`"),
                                stream
                                    .peek()
                                    .map(|t| t.span)
                                    .unwrap_or_else(|| stream.eof_span()),
                            )));
                        }
                        None => {
                            return Err(ParseError::new(Diagnostic::error(
                                "unterminated variant payload (missing `)`)",
                                stream.eof_span(),
                            )));
                        }
                    }
                }
            }
            let rparen = stream.expect(TokenKind::RParen)?;
            // Only record the payload if it has at least one type — `()` is
            // equivalent to no payload (unit variant) for codegen purposes.
            if !tys.is_empty() {
                data = Some(tys);
            }
            // Span end of the variant covers the closing `)`.
            let vend = rparen.span.end;
            variants.push(EnumVariant {
                name: vname,
                data,
                span: Span::new(vstart, vend, source_id),
            });
        } else {
            // Unit variant (no payload).
            let vend = vname_tok.span.end;
            variants.push(EnumVariant {
                name: vname,
                data,
                span: Span::new(vstart, vend, source_id),
            });
        }
        // Comma separator or end of list.
        match stream.peek_kind() {
            Some(TokenKind::Comma) => {
                stream.advance();
                // Allow trailing comma.
                if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                    break;
                }
            }
            Some(TokenKind::RBrace) => break,
            Some(other) => {
                return Err(ParseError::new(Diagnostic::error(
                    format!("expected `,` or `}}` in enum body, found `{other}`"),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                )));
            }
            None => {
                return Err(ParseError::new(Diagnostic::error(
                    "unterminated enum body (missing `}`)",
                    stream.eof_span(),
                )));
            }
        }
    }
    let rb = stream.expect(TokenKind::RBrace)?;
    Ok(EnumDecl {
        name,
        type_params,
        variants,
        span: Span::new(start, rb.span.end, source_id),
    })
}

/// Parse a `struct` declaration (T13 — generics + monomorphization).
///
/// # Grammar
///
/// ```text
/// StructDecl ::= "struct" Ident TypeParams? ":" Newline
///                  Indent FieldDecl+ Dedent
///              | "struct" Ident TypeParams? "{" FieldList "}"
///
/// TypeParams ::= "<" Ident ("," Ident)* [","] ">"   (shared helper)
///
/// FieldDecl  ::= Ident ":" TypeRef Newline
/// FieldList  ::= FieldEntry ("," FieldEntry)* [","]
/// FieldEntry ::= Ident ":" TypeRef
/// ```
///
/// **Layout-sensitive form** (primary — matches Buff's indentation-based
/// philosophy and the T13 example `struct Pair<T, U>:`):
///
/// ```text
/// struct Pair<T, U>:
///     x: T
///     y: U
/// ```
///
/// **Brace form** (secondary — matches enum syntax for compact one-liners):
///
/// ```text
/// struct Point { x: Float, y: Float }
/// ```
///
/// The parser peeks at the token after the type-param list: `:` → layout
/// form, `{` → brace form. Both produce the same [`StructDecl`] AST.
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - the token after `struct` is not an identifier,
/// - the type-param list is malformed,
/// - the opening `{` or `:` is missing,
/// - a field name is missing or not an identifier,
/// - a field type fails to parse via [`parse_type_ref`],
/// - the closing `}` is missing (brace form).
pub fn parse_struct_decl(stream: &mut TokenStream<'_>) -> Result<StructDecl, ParseError> {
    let struct_tok = stream.expect(TokenKind::KwStruct)?;
    let start = struct_tok.span.start;
    let source_id = stream.source_id();

    // Struct name.
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected struct name after `struct`",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;

    // Optional generic parameters: `<T, U>` (T13 — shared helper).
    let type_params = parse_type_params(stream)?;

    // Field list: layout-sensitive (`: \n Indent ...`) OR brace-delimited.
    let mut fields: Vec<(Ident, TypeRef)> = Vec::new();

    if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
        // Layout-sensitive form: `struct Name:` + indented field lines.
        stream.advance(); // consume `:`
                          // Expect a Newline then an Indent (the offside-rule tokens emitted by
                          // `indent.rs`). If either is missing, it's a parse error.
        if !matches!(stream.peek_kind(), Some(TokenKind::Newline)) {
            let span = stream
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span());
            return Err(ParseError::new(Diagnostic::error(
                "expected newline after `struct Name:`",
                span,
            )));
        }
        stream.advance(); // consume Newline
        if !matches!(stream.peek_kind(), Some(TokenKind::Indent)) {
            let span = stream
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span());
            return Err(ParseError::new(Diagnostic::error(
                "expected indented field list after `struct Name:`",
                span,
            )));
        }
        stream.advance(); // consume Indent
                          // Parse fields until Dedent.
        loop {
            // Field name.
            let fname_tok = stream.advance().ok_or_else(|| {
                ParseError::new(Diagnostic::error(
                    "expected field name, found end of input",
                    stream.eof_span(),
                ))
            })?;
            let fname = extract_ident(fname_tok.clone())?;
            // `:` separator.
            stream.expect(TokenKind::Colon)?;
            // Field type.
            let ftype = parse_type_ref(stream)?;
            fields.push((fname, ftype));
            // Consume the trailing Newline (required between fields in
            // layout-sensitive form — the offside rule doesn't insert them
            // automatically between same-indentation items on separate lines).
            if matches!(stream.peek_kind(), Some(TokenKind::Newline)) {
                stream.advance();
            }
            // Check for Dedent (end of field list) or continue.
            if matches!(stream.peek_kind(), Some(TokenKind::Dedent)) {
                stream.advance(); // consume Dedent
                break;
            }
            if stream.is_at_end() {
                break;
            }
        }
        let span_end = stream.peek().map(|t| t.span.start).unwrap_or_else(|| 0);
        return Ok(StructDecl {
            name,
            fields,
            traits: Vec::new(),
            type_params,
            span: Span::new(start, span_end, source_id),
        });
    }

    // Brace-delimited form: `struct Name { field: Type, ... }`.
    stream.expect(TokenKind::LBrace)?;
    // Empty body: `struct Empty { }`.
    if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
        let rb = stream.expect(TokenKind::RBrace)?;
        return Ok(StructDecl {
            name,
            fields: Vec::new(),
            traits: Vec::new(),
            type_params,
            span: Span::new(start, rb.span.end, source_id),
        });
    }
    loop {
        // Field name.
        let fname_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected field name, found end of input",
                stream.eof_span(),
            ))
        })?;
        let fname = extract_ident(fname_tok)?;
        // `:` separator.
        stream.expect(TokenKind::Colon)?;
        // Field type.
        let ftype = parse_type_ref(stream)?;
        fields.push((fname, ftype));
        // Comma separator or end of list.
        match stream.peek_kind() {
            Some(TokenKind::Comma) => {
                stream.advance();
                // Allow trailing comma.
                if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                    break;
                }
            }
            Some(TokenKind::RBrace) => break,
            Some(other) => {
                return Err(ParseError::new(Diagnostic::error(
                    format!("expected `,` or `}}` in struct body, found `{other}`"),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                )));
            }
            None => {
                return Err(ParseError::new(Diagnostic::error(
                    "unterminated struct body (missing `}`)",
                    stream.eof_span(),
                )));
            }
        }
    }
    let rb = stream.expect(TokenKind::RBrace)?;
    Ok(StructDecl {
        name,
        fields,
        traits: Vec::new(),
        type_params,
        span: Span::new(start, rb.span.end, source_id),
    })
}
//
// Buff v0.5 module system syntax (ES6-style):
//
//   import { greet, farewell } from "./hello.buff"
//   import * from "./utils.buff"
//   import greet from "./hello.buff"            (default import — sugar
//                                                 for `import { default as greet }`)
//   export func public() { ... }
//   export enum Color { Red, Green, Blue }
//   export * from "./other.buff"
//   export { greet } from "./other.buff"
//
// Visibility rules:
// - `export <decl>` wraps the decl in `Decl::ExportDecl` and marks it PUBLIC.
// - Any top-level decl NOT wrapped in `export` is module-PRIVATE.
// - `export * from "..."` re-exports ALL of the target module's public
//   symbols through this module.
// - `export { names } from "..."` re-exports specific named symbols.
//
// Path resolution & cycle detection happen in the module-graph pass
// (`buff_lang_types::modules`), not here.
// ---------------------------------------------------------------------------

/// Parse an `import` declaration (T29).
///
/// Supported shapes:
/// - `import { a, b } from "./path"` — ES6 named imports.
/// - `import * from "./path"` — wildcard import.
/// - `import name from "./path"` — default import (one identifier, no
///   braces). Sugar stored as `imports: [name]`.
/// - `import a.b.c [as alias]` — legacy dotted module path.
///
/// The leading `import` keyword is consumed here.
///
/// # Errors
///
/// Returns [`ParseError`] on malformed shapes (missing braces, missing
/// `from`, missing path string, missing `}`).
pub fn parse_import_decl(stream: &mut TokenStream<'_>) -> Result<ImportDecl, ParseError> {
    let source_id = stream.source_id();
    let import_tok = stream.expect(TokenKind::KwImport)?;
    let start = import_tok.span.start;

    // Disambiguate by peeking at the next significant token:
    //   `*`    → wildcard ES6
    //   `{`    → named ES6
    //   Ident  → either default-import ES6 (followed by `from`) or legacy
    //            dotted path (followed by `.` or `as`/EOF)
    //   String → not valid syntactically; fall through to error
    let next = stream.peek_kind();
    match next {
        Some(TokenKind::Star) => {
            // `import * from "<path>"`
            stream.advance(); // consume `*`
            stream.expect(TokenKind::KwFrom)?;
            let (path_str, path_end) = expect_path_string(stream)?;
            Ok(ImportDecl {
                path: Vec::new(),
                imports: Vec::new(),
                alias: None,
                from_path: Some(path_str),
                wildcard: true,
                span: Span::new(start, path_end, source_id),
            })
        }
        Some(TokenKind::LBrace) => {
            // `import { a, b } from "<path>"`
            stream.advance(); // consume `{`
            let mut imports: Vec<Ident> = Vec::new();
            // Empty `{}` is allowed (rare but valid).
            if !matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                loop {
                    let tok = stream.advance().ok_or_else(|| {
                        ParseError::new(Diagnostic::error(
                            "expected import name, found end of input",
                            stream.eof_span(),
                        ))
                    })?;
                    imports.push(extract_ident(tok)?);
                    match stream.peek_kind() {
                        Some(TokenKind::Comma) => {
                            stream.advance();
                            if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                                break;
                            }
                        }
                        Some(TokenKind::RBrace) => break,
                        Some(other) => {
                            return Err(ParseError::new(Diagnostic::error(
                                format!("expected `,` or `}}` in import list, found `{other}`"),
                                stream
                                    .peek()
                                    .map(|t| t.span)
                                    .unwrap_or_else(|| stream.eof_span()),
                            )));
                        }
                        None => {
                            return Err(ParseError::new(Diagnostic::error(
                                "unterminated import list (missing `}`)",
                                stream.eof_span(),
                            )));
                        }
                    }
                }
            }
            let rb = stream.expect(TokenKind::RBrace)?;
            let after_brace = stream.peek_kind();
            // `from` is required after the closing brace.
            if !matches!(after_brace, Some(TokenKind::KwFrom)) {
                return Err(ParseError::new(Diagnostic::error(
                    format!(
                        "expected `from` after import list, found `{}`",
                        after_brace
                            .map(|k| k.to_string())
                            .unwrap_or_else(|| "end of input".into())
                    ),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                )));
            }
            stream.advance(); // consume `from`
            let (path_str, path_end) = expect_path_string(stream)?;
            Ok(ImportDecl {
                path: Vec::new(),
                imports,
                alias: None,
                from_path: Some(path_str),
                wildcard: false,
                span: Span::new(start, path_end.max(rb.span.end), source_id),
            })
        }
        Some(TokenKind::Ident(_)) => {
            // Either ES6 default-import (`ident from "..."`) or legacy
            // dotted module path (`ident.ident...`).
            let ident_tok = stream.advance_after_peek();
            let ident = extract_ident(ident_tok.clone())?;
            match stream.peek_kind() {
                Some(TokenKind::KwFrom) => {
                    // `import name from "..."` — default import.
                    stream.advance(); // consume `from`
                    let (path_str, path_end) = expect_path_string(stream)?;
                    Ok(ImportDecl {
                        path: Vec::new(),
                        imports: vec![ident],
                        alias: None,
                        from_path: Some(path_str),
                        wildcard: false,
                        span: Span::new(start, path_end, source_id),
                    })
                }
                Some(TokenKind::Dot) => {
                    // Legacy: `import a.b.c [as alias]`.
                    let mut path = vec![ident];
                    while matches!(stream.peek_kind(), Some(TokenKind::Dot)) {
                        stream.advance(); // consume `.`
                        let tok = stream.advance().ok_or_else(|| {
                            ParseError::new(Diagnostic::error(
                                "expected module path segment after `.`, found end of input",
                                stream.eof_span(),
                            ))
                        })?;
                        path.push(extract_ident(tok)?);
                    }
                    let (alias, alias_end) = if matches!(stream.peek_kind(), Some(TokenKind::KwAs))
                    {
                        stream.advance(); // consume `as`
                        let a = stream.advance().ok_or_else(|| {
                            ParseError::new(Diagnostic::error(
                                "expected alias name after `as`, found end of input",
                                stream.eof_span(),
                            ))
                        })?;
                        let end = a.span.end;
                        let a = extract_ident(a)?;
                        (Some(a), end)
                    } else {
                        (None, path.last().map(|i| i.span.end).unwrap_or(start))
                    };
                    let end = alias_end.max(path.last().map(|i| i.span.end).unwrap_or(start));
                    Ok(ImportDecl {
                        path,
                        imports: Vec::new(),
                        alias,
                        from_path: None,
                        wildcard: false,
                        span: Span::new(start, end, source_id),
                    })
                }
                Some(other) => Err(ParseError::new(Diagnostic::error(
                    format!("expected `from` or `.` in import, found `{other}`"),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                ))),
                None => {
                    // `import name` with nothing after — treat as legacy
                    // single-segment module path (rare, but allow).
                    let end = ident.span.end;
                    Ok(ImportDecl {
                        path: vec![ident],
                        imports: Vec::new(),
                        alias: None,
                        from_path: None,
                        wildcard: false,
                        span: Span::new(start, end, source_id),
                    })
                }
            }
        }
        other => Err(ParseError::new(Diagnostic::error(
            format!(
                "expected `*`, `{{`, or identifier after `import`, found `{}`",
                other
                    .map(|k| k.to_string())
                    .unwrap_or_else(|| "end of input".into())
            ),
            stream
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span()),
        ))),
    }
}

/// Parse an `export` declaration (T29).
///
/// Supported shapes:
/// - `export func ...` / `export enum ...` — wraps the inner decl in
///   [`Decl::ExportDecl`].
/// - `export * from "..."` — wildcard re-export → [`Decl::ReexportDecl`].
/// - `export { a, b } from "..."` — named re-export → [`Decl::ReexportDecl`].
/// - `export { a, b }` (no `from`) — names a local symbol for export without
///   re-export (also [`Decl::ReexportDecl`] with empty `from`).
///
/// The leading `export` keyword is consumed here.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - `export export`, `export import` (nested/non-exportable forms),
/// - missing `from` after `export { ... }` followed by string path,
/// - missing path string after `from`,
/// - missing `}` in named list.
pub fn parse_export_decl(stream: &mut TokenStream<'_>) -> Result<Decl, ParseError> {
    let source_id = stream.source_id();
    let export_tok = stream.expect(TokenKind::KwExport)?;
    let start = export_tok.span.start;

    match stream.peek_kind() {
        // `export func ...` → wrap FuncDecl in ExportDecl.
        Some(TokenKind::KwFunc) => {
            let f = parse_func_decl(stream, Vec::new())?;
            let span = f.span;
            Ok(Decl::ExportDecl(ExportDecl {
                inner: Box::new(Decl::FuncDecl(f)),
                span,
            }))
        }
        // T31: `export async func ...` → wrap an async FuncDecl in ExportDecl.
        // The dispatcher reaches this arm when the user wrote `export async
        // func name(...)`; `parse_func_decl` consumes the leading `async`
        // and sets `is_async = true` on the inner FuncDecl.
        Some(TokenKind::KwAsync)
            if matches!(stream.peek_second_kind(), Some(TokenKind::KwFunc)) =>
        {
            let f = parse_func_decl(stream, Vec::new())?;
            let span = f.span;
            Ok(Decl::ExportDecl(ExportDecl {
                inner: Box::new(Decl::FuncDecl(f)),
                span,
            }))
        }
        // `export enum ...` → wrap EnumDecl.
        Some(TokenKind::KwEnum) => {
            let e = parse_enum_decl(stream)?;
            let span = e.span;
            Ok(Decl::ExportDecl(ExportDecl {
                inner: Box::new(Decl::EnumDecl(e)),
                span,
            }))
        }
        // `export * from "..."` → wildcard ReexportDecl.
        Some(TokenKind::Star) => {
            stream.advance(); // consume `*`
            stream.expect(TokenKind::KwFrom)?;
            let (path_str, path_end) = expect_path_string(stream)?;
            Ok(Decl::ReexportDecl(ReexportDecl {
                from: path_str,
                names: Vec::new(),
                wildcard: true,
                span: Span::new(start, path_end, source_id),
            }))
        }
        // `export { a, b } [from "..."]` → named ReexportDecl.
        Some(TokenKind::LBrace) => {
            stream.advance(); // consume `{`
            let mut names: Vec<Ident> = Vec::new();
            if !matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                loop {
                    let tok = stream.advance().ok_or_else(|| {
                        ParseError::new(Diagnostic::error(
                            "expected exported name, found end of input",
                            stream.eof_span(),
                        ))
                    })?;
                    names.push(extract_ident(tok)?);
                    match stream.peek_kind() {
                        Some(TokenKind::Comma) => {
                            stream.advance();
                            if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                                break;
                            }
                        }
                        Some(TokenKind::RBrace) => break,
                        Some(other) => {
                            return Err(ParseError::new(Diagnostic::error(
                                format!("expected `,` or `}}` in export list, found `{other}`"),
                                stream
                                    .peek()
                                    .map(|t| t.span)
                                    .unwrap_or_else(|| stream.eof_span()),
                            )));
                        }
                        None => {
                            return Err(ParseError::new(Diagnostic::error(
                                "unterminated export list (missing `}`)",
                                stream.eof_span(),
                            )));
                        }
                    }
                }
            }
            let rb = stream.expect(TokenKind::RBrace)?;
            // Optional `from "..."` — without it, this is a "export names"
            // statement re-exporting already-declared locals. We store an
            // empty string in `from` to signal "no source path".
            let (from, end) = if matches!(stream.peek_kind(), Some(TokenKind::KwFrom)) {
                stream.advance();
                let (s, e) = expect_path_string(stream)?;
                (s, e)
            } else {
                (String::new(), rb.span.end)
            };
            Ok(Decl::ReexportDecl(ReexportDecl {
                from,
                names,
                wildcard: false,
                span: Span::new(start, end, source_id),
            }))
        }
        // `export export`, `export import`, `export module`, `export trait`,
        // `export struct` (until struct parsing lands) — not exportable yet.
        Some(other) => Err(ParseError::new(Diagnostic::error(
            format!(
                "only `func`, `enum`, `*`, or `{{` are allowed after `export`, found `{other}`"
            ),
            stream
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span()),
        ))),
        None => Err(ParseError::new(Diagnostic::error(
            "expected item after `export`, found end of input",
            stream.eof_span(),
        ))),
    }
}

/// Parse a top-level `extend TYPE { fn ...; fn ...; ... }` extension-method
/// block (T75).
///
/// Shape:
/// - `extend String { fn shout(self) -> String { ... } }` — adds the
///   method `shout` to the `String` type.
/// - `extend Int { fn squared(self) -> Int { ... } }` — same shape with a
///   different (primitive) target.
/// - Multiple methods per block:
///   `extend MyType { fn m1(self) { ... } fn m2(self) { ... } }`.
///
/// The target is a bare type name (parsed via [`parse_type_ref`] so future
/// support for generic targets needs no AST migration). The method list is
/// a brace-delimited block of `fn` declarations; each `fn` is parsed via
/// the shared [`parse_func_decl`] (the leading `func`/`async func`/`extern
/// func` keyword is consumed there). Trailing commas between methods are
/// NOT supported — methods are separated by layout (newlines) which
/// [`TokenStream`] transparently skips. An empty body `extend T { }` is a
/// parse error (zero methods is meaningless for an extension block).
///
/// # Codegen target
///
/// The block lowers to a Rust extension trait + blanket-free impl — the
/// standard Rust extension-trait pattern that lets `recv.my_method()`
/// resolve on a type the user didn't define. The trait name is derived
/// from the target type as `BuffExt{Type}` (e.g. `extend String` →
/// `BuffExtString`). v0.5 single extend-block per target type is the
/// common case; multi-block merging is deferred.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - the token after `extend` is not a type name,
/// - the opening `{` is missing,
/// - a method's `fn` declaration fails to parse via `parse_func_decl`,
/// - the body is empty (zero methods),
/// - the closing `}` is missing.
pub fn parse_extend_decl(stream: &mut TokenStream<'_>) -> Result<ExtendBlock, ParseError> {
    let source_id = stream.source_id();
    let extend_tok = stream.expect(TokenKind::KwExtend)?;
    let start = extend_tok.span.start;

    // Target type name. Today always a `TypeRef::Named`; the parser uses
    // the shared `parse_type_ref` so future support for generic targets
    // (`extend Vector<T>`) needs no AST or parser change beyond handling
    // the new TypeRef shapes at codegen time.
    let target = parse_type_ref(stream)?;
    let target_end = type_end(&target);

    // Opening `{` of the method list.
    stream.expect(TokenKind::LBrace)?;

    let mut methods: Vec<FuncDecl> = Vec::new();
    // Empty body is a parse error — an extension block with zero methods
    // is meaningless and almost certainly indicates a user typo.
    if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
        let rb = stream.expect(TokenKind::RBrace)?;
        return Err(ParseError::new(Diagnostic::error(
            "extend block must declare at least one method",
            Span::new(start, rb.span.end, source_id),
        )));
    }

    // Parse `fn ...` declarations until the closing `}`. Layout tokens
    // (newlines) between methods are transparently skipped by
    // `TokenStream::peek`/`advance`, so no explicit separator handling is
    // needed. An optional trailing `;` between methods is also tolerated.
    while matches!(
        stream.peek_kind(),
        Some(TokenKind::KwFunc) | Some(TokenKind::KwAsync) | Some(TokenKind::KwExtern)
    ) {
        let f = parse_func_decl(stream, Vec::new())?;
        methods.push(f);
        // Optional `;` separator between methods.
        if matches!(stream.peek_kind(), Some(TokenKind::Semicolon)) {
            stream.advance();
        }
    }

    if methods.is_empty() {
        // Defensive: we already error on empty `{ }` above, but a body of
        // only stray tokens (e.g. comments, which don't exist as tokens in
        // Buff's lexer) would land here. Surface a helpful message.
        return Err(ParseError::new(Diagnostic::error(
            "extend block must contain at least one `fn` declaration",
            stream.span_here(),
        )));
    }

    let rb = stream.expect(TokenKind::RBrace)?;
    Ok(ExtendBlock {
        target,
        methods,
        span: Span::new(start, target_end.max(rb.span.end), source_id),
    })
}

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

/// Parse an `extern crate "name"` declaration (T32 — FFI basics).
///
/// Records a dependency on an external Rust crate. The Buff form uses a
/// STRING literal for the crate name (`extern crate "serde"`) — unlike
/// Rust's `extern crate serde;`, which uses a bare path — so the crate
/// name may be any crates.io identifier (including names with `-`, which
/// are not valid Buff identifiers). The codegen emits a `use <name>;`
/// item and records the name in its extern-crate dep set.
///
/// The leading `extern` keyword is consumed here; this function expects
/// the NEXT significant token to be the bare identifier `crate`. The
/// crate name string follows immediately.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - missing `crate` identifier after `extern`,
/// - missing crate-name string after `crate`,
/// - interpolation inside the crate-name string.
pub fn parse_extern_crate_decl(stream: &mut TokenStream<'_>) -> Result<Decl, ParseError> {
    let source_id = stream.source_id();
    let extern_tok = stream.expect(TokenKind::KwExtern)?;
    let start = extern_tok.span.start;

    // Expect the bare identifier `crate` (NOT a keyword in Buff — it is
    // parsed as a regular Ident). Reject anything else with a helpful msg.
    let crate_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected `crate` after `extern`, found end of input",
            stream.eof_span(),
        ))
    })?;
    match &crate_tok.kind {
        TokenKind::Ident(s) if s == "crate" => {}
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!(
                    "expected `crate` after `extern`, found `{other}` \
                     (Buff supports `extern crate \"<name>\"` and `extern func ...`)"
                ),
                crate_tok.span,
            )));
        }
    }

    // Consume the crate-name string literal (`"serde"`). Reuse the same
    // StringStart/StringPart/StringEnd machinery the lexer uses for every
    // string — interpolation is rejected (crate names must be plain).
    let (name, name_end) = expect_crate_name_string(stream)?;
    let span = Span::new(start, name_end, source_id);
    Ok(Decl::ExternCrateDecl(buff_lang_ast::ExternCrateDecl {
        name,
        span,
    }))
}

/// Parse an `extern "ABI" [from "crate"] func name(params) -> Ret`
/// declaration (T119 — minimal extern/bindgen). The leading `extern` is
/// consumed here; the cursor MUST be positioned at the `StringStart` of
/// the ABI literal when this function is called.
///
/// Returns a [`Decl::ExternFuncDecl`] carrying:
/// - the ABI string (validated to `"C"` in v1.3 — other ABIs are a
///   parse error per the T119 spec),
/// - the optional source crate from the `from "..."` annotation,
/// - the function name + parameter list + return type.
///
/// # Generics rejection
///
/// If the function name is followed by `<` (the start of a generic
/// parameter list), the parser returns a clear error: generics are NOT
/// supported on extern functions in v1.3 (per the T119 spec: "A
/// generic/trait-heavy Rust API is REJECTED with a clear error").
///
/// # Body
///
/// Extern functions have NO body — after the signature we expect either
/// EOF, a Newline, or the start of the next top-level decl. We do NOT
/// consume a trailing block.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - missing ABI string after `extern`,
/// - ABI string is not `"C"` (v1.3 accept-list),
/// - missing `func` keyword after ABI / `from "..."`,
/// - missing function name,
/// - missing parameter list `(`...`)`,
/// - generic `<...>` after the function name (rejected),
/// - interpolation in either string literal.
pub fn parse_extern_func_decl_with_abi(stream: &mut TokenStream<'_>) -> Result<Decl, ParseError> {
    let source_id = stream.source_id();
    let extern_tok = stream.expect(TokenKind::KwExtern)?;
    let start = extern_tok.span.start;

    // 1. Consume the ABI string literal (`"C"`). Reuse the helper that
    //    rejects interpolation — ABIs are plain strings.
    let (abi, abi_end) = expect_abi_string(stream)?;
    // Validate against the v1.3 accept-list. Only `"C"` is supported
    // (per the T119 spec: "use `"C"` ABI for stability/cross-language
    // compatibility"). Other ABIs (`"system"`, `"stdcall"`, `"fastcall"`)
    // are a parse error — surface a clear message naming the unsupported
    // ABI.
    if abi != "C" {
        // Re-compute the span of the ABI literal for the diagnostic. We
        // don't carry it back from `expect_abi_string` (it returns end
        // only); approximate with a span ending at `abi_end` and starting
        // at `abi_end - abi.len() - 2` (the two quotes). For the rare
        // user-written ABI this is good enough — they'll see "at column
        // N" pointing at the literal.
        let approx_start = abi_end.saturating_sub(abi.len() + 2);
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "unsupported ABI `{:?}` in `extern \"ABI\" func ...`: only `\"C\"` is supported in v1.3 \
                 (the T119 spec mandates the C ABI for cross-language stability; other ABIs are deferred)",
                abi
            ),
            Span::new(approx_start, abi_end, source_id),
        )));
    }

    // 2. Optionally consume `from "crate-name"`. The `from` keyword is a
    //    reserved Buff keyword (`TokenKind::KwFrom`), so we can peek for
    //    it directly. When present, the next token must be a string
    //    literal naming the source crate.
    let mut crate_name: Option<String> = None;
    if matches!(stream.peek_kind(), Some(TokenKind::KwFrom)) {
        stream.advance(); // consume `from`
        let (name, _name_end) = expect_crate_name_string(stream)?;
        crate_name = Some(name);
    }

    // 3. Consume the `func` keyword.
    let func_tok = stream.expect(TokenKind::KwFunc)?;
    let _ = func_tok; // span tracking not needed for v1.3

    // 4. Function name.
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected function name after `func`",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;

    // 5. Generic-parameter rejection. If the next token is `<`, the user
    //    is trying to declare a generic extern fn (`extern "C" func
    //    parse<T>(...) -> T`). Buff does NOT support generics on
    //    functions in v1.3 (let alone on externs), so this is a clear
    //    parse error per the T119 spec.
    if matches!(stream.peek_kind(), Some(TokenKind::Lt)) {
        let lt_tok = stream.peek().expect("peek guaranteed Lt").span;
        return Err(ParseError::new(Diagnostic::error(
            "generics are not supported on `extern` functions in v1.3 \
             (the T119 spec rejects generic/trait-heavy Rust APIs; declare a separate \
             concrete wrapper per type you need, e.g. `extern \"C\" from \"serde_json\" \
             func parse_int(s: String) -> Int`)",
            lt_tok,
        )));
    }

    // 6. Parameter list `( ... )`.
    stream.expect(TokenKind::LParen)?;
    let params = parse_params(stream)?;
    let rparen = stream.expect(TokenKind::RParen)?;

    // 7. Optional return type: `-> Type`.
    let mut end = rparen.span.end;
    let return_type = if matches!(stream.peek_kind(), Some(TokenKind::Arrow)) {
        stream.advance(); // consume `->`
        let ty = parse_type_ref(stream)?;
        end = type_end(&ty);
        Some(ty)
    } else {
        None
    };

    let span = Span::new(start, end, source_id);
    Ok(Decl::ExternFuncDecl(buff_lang_ast::ExternFuncDecl {
        abi,
        crate_name,
        name,
        params,
        return_type,
        span,
    }))
}

/// Expect a plain string literal naming an ABI (the `"C"` part of
/// `extern "C"`). Returns the ABI string + the end offset of the closing
/// `"`. Mirrors [`expect_crate_name_string`] but with ABI-specific error
/// messages.
fn expect_abi_string(stream: &mut TokenStream<'_>) -> Result<(String, usize), ParseError> {
    let start_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected ABI string after `extern`, found end of input \
             (Buff supports `extern \"C\" func ...` in v1.3)",
            stream.eof_span(),
        ))
    })?;
    if !matches!(start_tok.kind, TokenKind::StringStart) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "expected ABI string after `extern` (e.g. `extern \"C\" func ...`), found `{}`",
                start_tok.kind
            ),
            start_tok.span,
        )));
    }
    let part_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected ABI string content, found end of input",
            stream.eof_span(),
        ))
    })?;
    let abi = match part_tok.kind {
        TokenKind::StringPart(s) => s,
        TokenKind::InterpStart => {
            return Err(ParseError::new(Diagnostic::error(
                "ABI string cannot contain interpolation",
                part_tok.span,
            )));
        }
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!("expected ABI string content, found `{other}`"),
                part_tok.span,
            )));
        }
    };
    let end_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "unterminated ABI string (missing closing quote)",
            stream.eof_span(),
        ))
    })?;
    if !matches!(end_tok.kind, TokenKind::StringEnd) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "ABI string cannot contain interpolation; expected end of string, found `{}`",
                end_tok.kind
            ),
            end_tok.span,
        )));
    }
    Ok((abi, end_tok.span.end))
}

/// Expect a plain string literal naming a crate (the `"serde"` part of
/// `extern crate "serde"`). Returns the crate name + the end offset of
/// the closing `"`.
///
/// Mirrors [`expect_path_string`] but with crate-specific error messages.
/// The Buff lexer tokenizes every `"..."` as `StringStart, StringPart,
/// StringEnd`; we consume that three-token sequence and reject any
/// interpolated form.
///
/// # Errors
///
/// Returns [`ParseError`] if the next token is not a `StringStart`, if
/// the `StringPart` is missing, if interpolation appears, or if the
/// closing `StringEnd` is missing.
fn expect_crate_name_string(stream: &mut TokenStream<'_>) -> Result<(String, usize), ParseError> {
    let start_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected crate-name string after `extern crate`, found end of input",
            stream.eof_span(),
        ))
    })?;
    if !matches!(start_tok.kind, TokenKind::StringStart) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "expected crate-name string after `extern crate`, found `{}`",
                start_tok.kind
            ),
            start_tok.span,
        )));
    }

    let part_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected crate-name string content, found end of input",
            stream.eof_span(),
        ))
    })?;
    let name = match part_tok.kind {
        TokenKind::StringPart(s) => s,
        TokenKind::InterpStart => {
            return Err(ParseError::new(Diagnostic::error(
                "crate-name string cannot contain interpolation",
                part_tok.span,
            )));
        }
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!("expected crate-name string content, found `{other}`"),
                part_tok.span,
            )));
        }
    };

    let end_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "unterminated crate-name string (missing closing quote)",
            stream.eof_span(),
        ))
    })?;
    if !matches!(end_tok.kind, TokenKind::StringEnd) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "crate-name string cannot contain interpolation; expected end of string, found `{}`",
                end_tok.kind
            ),
            end_tok.span,
        )));
    }

    Ok((name, end_tok.span.end))
}

/// Expect a string-literal path token (the `"./foo.buff"` part of an
/// import/export `from` clause). Returns the path string + the end offset
/// of the closing `"`.
///
/// The Buff lexer tokenizes every `"..."` as `StringStart, StringPart,
/// StringEnd` (the interpolation machinery), even for plain non-interpolated
/// strings. We consume that three-token sequence here and reject any
/// interpolated form (`InterpStart`) inside a path — paths must be plain
/// string literals.
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - the next significant token is not `StringStart`,
/// - the `StringPart` is missing,
/// - an interpolation appears inside the path string,
/// - the closing `StringEnd` is missing.
fn expect_path_string(stream: &mut TokenStream<'_>) -> Result<(String, usize), ParseError> {
    // Consume `StringStart`.
    let start_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected path string after `from`, found end of input",
            stream.eof_span(),
        ))
    })?;
    if !matches!(start_tok.kind, TokenKind::StringStart) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "expected path string after `from`, found `{}`",
                start_tok.kind
            ),
            start_tok.span,
        )));
    }

    // Expect exactly one StringPart (no interpolation allowed).
    let part_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected path string content, found end of input",
            stream.eof_span(),
        ))
    })?;
    let path = match part_tok.kind {
        TokenKind::StringPart(s) => s,
        TokenKind::InterpStart => {
            return Err(ParseError::new(Diagnostic::error(
                "path string cannot contain interpolation",
                part_tok.span,
            )));
        }
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!("expected path string content, found `{other}`"),
                part_tok.span,
            )));
        }
    };

    // Consume `StringEnd`.
    let end_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "unterminated path string (missing closing quote)",
            stream.eof_span(),
        ))
    })?;
    if !matches!(end_tok.kind, TokenKind::StringEnd) {
        // If interpolation slipped in (InterpStart between parts), the
        // token after the part would be InterpStart, not StringEnd.
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "path string cannot contain interpolation; expected end of string, found `{}`",
                end_tok.kind
            ),
            end_tok.span,
        )));
    }

    Ok((path, end_tok.span.end))
}

// ---------------------------------------------------------------------------
// T35 — Attribute parsing (`@name`).
//
// Buff attributes are `@`-prefixed identifiers preceding a declaration:
//
//   @test
//   func test_addition():
//       assert_eq(add(2, 3), 5)
//
// For v0.5 only the argument-less form `@test` is meaningful; the parser
// also accepts `@name(arg, arg)` for forward-compat with the `@prefer(gpu)`
// shape the README anticipates, storing the args as raw strings on the
// [`Attribute`] node. Attributes attach to [`FuncDecl`]s today; attaching
// them to structs/enums is a future task.
// ---------------------------------------------------------------------------

/// Parse zero-or-more leading `@name` attribute forms (T35).
///
/// Each attribute is one of:
/// - `@ident` — argument-less form (e.g. `@test`).
/// - `@ident(args, ...)` — parenthesised form (e.g. `@prefer(gpu)`). The
///   args are stored as raw identifier/string text; no type-checking is
///   done at parse time (deferred to the semantic pass).
///
/// The function consumes attributes greedily and stops as soon as the next
/// significant token is not `@`. Returns the collected attributes in
/// declaration order (leftmost first). An empty `Vec` means no attributes
/// were present (the common case).
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - `@` is not followed by an identifier (the attribute name),
/// - a parenthesised form is missing its closing `)`.
pub fn parse_attributes(stream: &mut TokenStream<'_>) -> Result<Vec<Attribute>, ParseError> {
    let source_id = stream.source_id();
    let mut attrs = Vec::new();
    while matches!(stream.peek_kind(), Some(TokenKind::At)) {
        let at_tok = stream.advance_after_peek();
        let start = at_tok.span.start;
        let name_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected attribute name after `@`, found end of input",
                stream.eof_span(),
            ))
        })?;
        let name = extract_ident(name_tok.clone())?;
        // Optional `( arg, arg, ... )` — args are bare identifiers or
        // string-literal text. Stored as raw strings for forward-compat.
        let mut args: Vec<String> = Vec::new();
        // T0-G3: named `key = "value"` args go in a separate map so
        // codegen can look up by name (e.g. `@deprecated(since = "2.0",
        // replacement = "new_fn")`). Both forms can coexist on the same
        // attribute.
        let mut named_args: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        let end = if matches!(stream.peek_kind(), Some(TokenKind::LParen)) {
            stream.advance(); // consume `(`
            if !matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                loop {
                    let arg_tok = stream.advance().ok_or_else(|| {
                        ParseError::new(Diagnostic::error(
                            "expected attribute argument, found end of input",
                            stream.eof_span(),
                        ))
                    })?;
                    // T0-G3: detect named-arg form `ident = "string"`.
                    // The identifier becomes the key; the following `=`
                    // and string literal become the value.
                    if let TokenKind::Ident(key) = &arg_tok.kind {
                        if matches!(stream.peek_kind(), Some(TokenKind::Assign)) {
                            stream.advance(); // consume `=`
                                              // Value must be a string literal (the parser
                                              // already rejects interpolation; we re-use
                                              // the same StringStart/StringPart/StringEnd
                                              // triple walk below for the value).
                            let val_tok = stream.advance().ok_or_else(|| {
                                ParseError::new(Diagnostic::error(
                                    "expected string after `=` in named attribute argument",
                                    stream.eof_span(),
                                ))
                            })?;
                            let value = match val_tok.kind {
                                TokenKind::StringStart => {
                                    let part = stream.advance().ok_or_else(|| {
                                        ParseError::new(Diagnostic::error(
                                            "expected string content in named attribute argument",
                                            stream.eof_span(),
                                        ))
                                    })?;
                                    let s = match part.kind {
                                        TokenKind::StringPart(s) => s,
                                        other => {
                                            return Err(ParseError::new(Diagnostic::error(
                                                format!(
                                                    "expected string content in named arg, found `{other}`"
                                                ),
                                                part.span,
                                            )));
                                        }
                                    };
                                    let end_tok = stream.advance().ok_or_else(|| {
                                        ParseError::new(Diagnostic::error(
                                            "unterminated string in named attribute argument",
                                            stream.eof_span(),
                                        ))
                                    })?;
                                    if !matches!(end_tok.kind, TokenKind::StringEnd) {
                                        return Err(ParseError::new(Diagnostic::error(
                                            "string interpolation not allowed in named attribute argument",
                                            end_tok.span,
                                        )));
                                    }
                                    s
                                }
                                other => {
                                    return Err(ParseError::new(Diagnostic::error(
                                        format!(
                                            "expected string literal after `=` in named attribute argument, found `{other}`"
                                        ),
                                        val_tok.span,
                                    )));
                                }
                            };
                            named_args.insert(key.clone(), value);
                            match stream.peek_kind() {
                                Some(TokenKind::Comma) => {
                                    stream.advance();
                                    if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                                        break;
                                    }
                                }
                                Some(TokenKind::RParen) => break,
                                Some(other) => {
                                    return Err(ParseError::new(Diagnostic::error(
                                        format!(
                                            "expected `,` or `)` after named attribute argument, found `{other}`"
                                        ),
                                        stream
                                            .peek()
                                            .map(|t| t.span)
                                            .unwrap_or_else(|| stream.eof_span()),
                                    )));
                                }
                                None => {
                                    return Err(ParseError::new(Diagnostic::error(
                                        "unterminated attribute argument list (missing `)`)",
                                        stream.eof_span(),
                                    )));
                                }
                            }
                            continue;
                        }
                    }
                    let arg = match &arg_tok.kind {
                        TokenKind::Ident(s) => s.clone(),
                        // T66: accept integer literals as attribute args so
                        // `@workgroup(64)` parses (the workgroup size is a
                        // numeric value). The integer is stored as its string
                        // representation in `Attribute::args` alongside the
                        // existing identifier/string forms. This is purely
                        // additive — existing attribute forms (`@test`,
                        // `@prefer(gpu)`, `@deprecated(since = "2.0")`) are
                        // unaffected.
                        TokenKind::IntLit(n) => n.to_string(),
                        TokenKind::StringStart => {
                            // Consume the full string-token triple.
                            let part = stream.advance().ok_or_else(|| {
                                ParseError::new(Diagnostic::error(
                                    "expected string content in attribute argument",
                                    stream.eof_span(),
                                ))
                            })?;
                            let s = match part.kind {
                                TokenKind::StringPart(s) => s,
                                other => {
                                    return Err(ParseError::new(Diagnostic::error(
                                        format!(
                                            "expected string content in attribute, found `{other}`"
                                        ),
                                        part.span,
                                    )));
                                }
                            };
                            let end_tok = stream.advance().ok_or_else(|| {
                                ParseError::new(Diagnostic::error(
                                    "unterminated string in attribute argument",
                                    stream.eof_span(),
                                ))
                            })?;
                            if !matches!(end_tok.kind, TokenKind::StringEnd) {
                                return Err(ParseError::new(Diagnostic::error(
                                    "string interpolation not allowed in attribute argument",
                                    end_tok.span,
                                )));
                            }
                            s
                        }
                        other => {
                            return Err(ParseError::new(Diagnostic::error(
                                format!("expected identifier or string in attribute argument, found `{other}`"),
                                arg_tok.span,
                            )));
                        }
                    };
                    args.push(arg);
                    match stream.peek_kind() {
                        Some(TokenKind::Comma) => {
                            stream.advance();
                            if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                                break;
                            }
                        }
                        Some(TokenKind::RParen) => break,
                        Some(other) => {
                            return Err(ParseError::new(Diagnostic::error(
                                format!(
                                    "expected `,` or `)` in attribute arguments, found `{other}`"
                                ),
                                stream
                                    .peek()
                                    .map(|t| t.span)
                                    .unwrap_or_else(|| stream.eof_span()),
                            )));
                        }
                        None => {
                            return Err(ParseError::new(Diagnostic::error(
                                "unterminated attribute argument list (missing `)`)",
                                stream.eof_span(),
                            )));
                        }
                    }
                }
            }
            let rp = stream.expect(TokenKind::RParen)?;
            rp.span.end
        } else {
            name_tok.span.end
        };
        attrs.push(Attribute {
            name,
            args,
            named_args,
            span: Span::new(start, end, source_id),
        });
    }
    Ok(attrs)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Pull an [`Ident`] out of a token whose kind is [`TokenKind::Ident`].
/// Errors on any other kind.
pub fn extract_ident(tok: Token) -> Result<Ident, ParseError> {
    match tok.kind {
        TokenKind::Ident(s) => Ok(Ident::new(s, tok.span)),
        other => Err(ParseError::new(Diagnostic::error(
            format!("expected identifier, found `{other}`"),
            tok.span,
        ))),
    }
}

/// End byte offset of a [`TypeRef`]'s span. Small helper so call sites don't
/// need to repeat the variant match.
pub fn type_end(ty: &TypeRef) -> usize {
    match ty {
        TypeRef::Named { span, .. }
        | TypeRef::Generic { span, .. }
        | TypeRef::Option(_, span)
        | TypeRef::Function { span, .. }
        | TypeRef::Union(_, span)
        | TypeRef::Tuple(_, span) => span.end,
    }
}
