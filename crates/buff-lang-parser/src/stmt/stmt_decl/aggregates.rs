//! Aggregate-type declarations: `struct`, `enum` (with variant payloads),
//! and `extend` blocks. ITER-52 pure move from stmt_decl.rs.

use buff_lang_ast::{EnumDecl, EnumVariant, ExtendBlock, FuncDecl, Ident, StructDecl, TypeRef};
use buff_lang_error::{Diagnostic, ParseError, Span};
use buff_lang_lexer::TokenKind;

use crate::stream::TokenStream;

use super::func::*;
use super::shared::*;

/// Parse an `enum` declaration (T27 + T13 generics).
///
/// Supports two syntactic forms:
///
/// **Layout form** (primary — indentation-based, BUG-6 fix):
///
/// ```text
/// enum Color:
///     Red
///     Green(Int)
/// ```
///
/// **Brace form** (compact one-liners):
///
/// ```text
/// enum Color { Red, Green(Int) }
/// ```
///
/// The parser peeks at the token after the type-param list: `:` → layout
/// form, `{` → brace form. Both produce the same [`EnumDecl`] AST.
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - the token after `enum` is not an identifier,
/// - neither `:` nor `{` follows the name/generics,
/// - a variant name is missing or not an identifier,
/// - a variant payload type fails to parse via [`parse_type_ref`],
/// - the closing `}` is missing (brace form).
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

    let mut variants: Vec<EnumVariant> = Vec::new();

    // Layout-sensitive form (BUG-6 fix): `enum Name:` + indented variant lines.
    //
    //   enum Color:
    //       Red
    //       Green(Int)
    //
    // Mirrors the layout arm of `parse_struct_decl`: peek at the token after
    // the type-param list — `:` → layout form, `{` (fall-through) → brace
    // form. `peek_kind` skips layout tokens but `:` is not one, so the
    // dispatch sees it directly.
    if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
        stream.advance(); // consume `:`
                          // Expect a Newline then an Indent (offside-rule tokens emitted by
                          // `indent.rs`). Use RAW stream access because the regular TokenStream
                          // skips layout tokens (Newline/Indent/Dedent).
        if !matches!(stream.peek_raw_kind(), Some(TokenKind::Newline)) {
            let span = stream
                .peek_raw()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span());
            return Err(ParseError::new(Diagnostic::error(
                "expected newline after `enum Name:`",
                span,
            )));
        }
        stream.advance_raw(); // consume Newline
        if !matches!(stream.peek_raw_kind(), Some(TokenKind::Indent)) {
            let span = stream
                .peek_raw()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span());
            return Err(ParseError::new(Diagnostic::error(
                "expected indented variant list after `enum Name:`",
                span,
            )));
        }
        stream.advance_raw(); // consume Indent
                              // Parse variants until Dedent. Each variant is an identifier plus an
                              // optional `( Type, Type, ... )` payload — same shape as the brace
                              // form below, only the separator differs (newline vs comma).
        loop {
            variants.push(parse_enum_variant_payload(stream, source_id)?);
            // Consume the trailing Newline (required between variants in
            // layout-sensitive form — use RAW stream because the regular
            // stream skips layout tokens).
            if matches!(stream.peek_raw_kind(), Some(TokenKind::Newline)) {
                stream.advance_raw();
            }
            // Check for Dedent (end of variant list) using RAW stream.
            if matches!(stream.peek_raw_kind(), Some(TokenKind::Dedent)) {
                stream.advance_raw(); // consume Dedent
                break;
            }
            if stream.is_at_end() {
                break;
            }
        }
        let span_end = stream.peek().map(|t| t.span.start).unwrap_or_else(|| 0);
        return Ok(EnumDecl {
            name,
            type_params,
            variants,
            span: Span::new(start, span_end, source_id),
        });
    }

    // Brace-delimited form: `enum Name { Variant, ... }`.
    stream.expect(TokenKind::LBrace)?;
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
        variants.push(parse_enum_variant_payload(stream, source_id)?);
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

/// Parse a single enum variant: an identifier name plus an optional
/// `( Type, Type, ... )` payload, returned as a fully-constructed
/// [`EnumVariant`]. Shared by the brace-form and layout-form arms of
/// [`parse_enum_decl`] so both shapes parse variants identically (BUG-6 fix
/// — extracted to avoid divergence between the two forms).
///
/// # Errors
///
/// Returns [`ParseError`] if the variant name is missing/not an identifier,
/// or a payload type fails to parse via [`parse_type_ref`].
fn parse_enum_variant_payload(
    stream: &mut TokenStream<'_>,
    source_id: buff_lang_error::SourceId,
) -> Result<EnumVariant, ParseError> {
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
    let vend;
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
        vend = rparen.span.end;
    } else {
        // Unit variant (no payload).
        vend = vname_tok.span.end;
    }
    Ok(EnumVariant {
        name: vname,
        data,
        span: Span::new(vstart, vend, source_id),
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
                          // `indent.rs`). Use RAW stream access because the regular
                          // TokenStream skips layout tokens (Newline/Indent/Dedent).
        if !matches!(stream.peek_raw_kind(), Some(TokenKind::Newline)) {
            let span = stream
                .peek_raw()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span());
            return Err(ParseError::new(Diagnostic::error(
                "expected newline after `struct Name:`",
                span,
            )));
        }
        stream.advance_raw(); // consume Newline
        if !matches!(stream.peek_raw_kind(), Some(TokenKind::Indent)) {
            let span = stream
                .peek_raw()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span());
            return Err(ParseError::new(Diagnostic::error(
                "expected indented field list after `struct Name:`",
                span,
            )));
        }
        stream.advance_raw(); // consume Indent
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
            // layout-sensitive form — use RAW stream because regular stream
            // skips layout tokens).
            if matches!(stream.peek_raw_kind(), Some(TokenKind::Newline)) {
                stream.advance_raw();
            }
            // Check for Dedent (end of field list) using RAW stream.
            if matches!(stream.peek_raw_kind(), Some(TokenKind::Dedent)) {
                stream.advance_raw(); // consume Dedent
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
