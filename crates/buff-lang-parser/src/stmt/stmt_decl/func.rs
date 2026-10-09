//! Function-declaration parsing: `func` headers, signatures, params,
//! type params, and the shared `parse_type_ref` machinery. ITER-52 pure
//! move from stmt_decl.rs.

use buff_lang_ast::{Attribute, Block, FuncDecl, Ident, Param, Stmt, TypeParam, TypeRef};
use buff_lang_error::{Diagnostic, ParseError, Span};
use buff_lang_lexer::TokenKind;

use crate::expr::parse_expression;
use crate::stmt::parse_block;
use crate::stream::TokenStream;

use super::shared::*;

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

    // DR-020 / P2.1b: contextual recognition of `dyn Trait` in type
    // position. `dyn` is NOT a reserved keyword — it lexes as `Ident`
    // (per Stability Promise). When the parsed identifier is literally
    // `dyn` and is followed by another identifier (the trait name), we
    // build a `TypeRef::TraitObject` instead of `TypeRef::Named("dyn")`.
    // This lets existing Buff code that uses `dyn` as a variable name
    // continue to work in expression position; only type-position
    // `dyn <ident>` is intercepted.
    //
    // MVP: the `lifetime` field is recorded in the AST for future
    // expansion but the parser does not yet accept explicit lifetime
    // syntax (`('static)` etc.) — Buff's lexer does not produce an
    // `Ident("static")` token for `'static` (single-quote lexes as the
    // start of a char literal). Per DR-020 §Autoboxing Rules, codegen
    // always emits `Box<dyn Trait>` regardless, so the lifetime field
    // has no observable effect today. A future T-numbered task will
    // add Lifetime token support to the lexer when borrowed-form
    // lifetimes become a real user need.
    if name.name == "dyn" {
        let trait_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected trait name after `dyn`",
                stream.eof_span(),
            ))
        })?;
        let trait_end = trait_tok.span.end;
        let trait_name = extract_ident(trait_tok)?;
        return Ok(TypeRef::TraitObject {
            trait_name,
            lifetime: None,
            span: Span::new(start, trait_end, source_id),
        });
    }

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
                Some(TokenKind::Shr) => {
                    // `>>` closing two nested generics at once (e.g.
                    // `Map<String, Vector<String>>`). Split into two `>`
                    // tokens — one closes this generic, the other is
                    // queued for the outer generic context.
                    stream.split_shr();
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
/// `)`. Parameters are comma-separated; each one is `name: Type` OR a bare
/// `name` (BUG-10 — type inferred from context/use, matching the README's
/// "Statically typed with aggressive inference — types rarely written" claim).
///
/// # BUG-10 — inferred (unannotated) params
///
/// After the param NAME, the `: Type` annotation is OPTIONAL. When `:` is
/// absent, the parameter carries a placeholder [`TypeRef::Named`] whose name
/// is `"_"` (the conventional wildcard). Downstream maps `"_"` to `None` via
/// [`typeref_to_type`], which the type inferencer treats as `Type::Unknown`
/// (already handled permissively — e.g. binary-op checking falls through when
/// either side is `Unknown`). This brings the parser in line with Buff's
/// inference-first philosophy; full param-type inference at codegen is a
/// follow-up (Rust requires explicit fn-signature param types, so the
/// codegen must eventually infer + emit a concrete type).
///
/// # T75 — bare `self` receiver
///
/// As a SPECIAL CASE, a bare `self` (no `: Type` annotation) — the receiver
/// syntax used by extension methods inside `extend TYPE { fn ... }` blocks —
/// keeps its dedicated `TypeRef::Named { name: "Self" }` placeholder (NOT the
/// generic `"_"`). This distinction matters because the codegen uses the
/// param NAME `self` (plus the `Self` marker) to decide receiver emission.
///
/// # Errors
///
/// Returns [`ParseError`] if any parameter name is missing or not an
/// identifier. (The `: Type` annotation is optional as of BUG-10.)
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
        // The `: Type` annotation is OPTIONAL (BUG-10). Three cases:
        //
        // 1. T75: bare `self` receiver (no `: Type`) → `Self` placeholder.
        //    The codegen emits a Rust receiver based on the param NAME, and
        //    uses the `Self` marker to distinguish from a generic inferred
        //    param. This arm MUST stay ahead of the generic inferred branch
        //    so `self` does not collapse to `_`.
        //
        // 2. BUG-10: any other param without `: Type` → inferred placeholder
        //    `TypeRef::Named { name: "_" }`. Downstream maps `_` to `None`
        //    (→ `Type::Unknown`), which the inferencer handles permissively.
        //
        // 3. Existing: `: Type` present → parse the type annotation as before.
        let ty = if name.name == "self" && !matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
            TypeRef::Named {
                name: Ident::new("Self", Span::new(start, name.span.end, source_id)),
                span: Span::new(start, name.span.end, source_id),
            }
        } else if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
            stream.expect(TokenKind::Colon)?;
            parse_type_ref(stream)?
        } else {
            // BUG-10: no `: Type` and not `self` → inferred placeholder.
            // The `_` name is the conventional wildcard; it is NOT a real
            // type and maps to `Type::Unknown` downstream.
            TypeRef::Named {
                name: Ident::new("_", Span::new(start, name.span.end, source_id)),
                span: Span::new(start, name.span.end, source_id),
            }
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
