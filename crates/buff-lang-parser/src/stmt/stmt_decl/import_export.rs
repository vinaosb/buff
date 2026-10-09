//! `import` / `export` declarations. ITER-52 pure move from stmt_decl.rs.

use buff_lang_ast::{Decl, ExportDecl, Ident, ImportDecl, ReexportDecl};
use buff_lang_error::{Diagnostic, ParseError, Span};
use buff_lang_lexer::TokenKind;

use crate::stream::TokenStream;

use super::aggregates::*;
use super::func::*;
use super::shared::*;
use super::trait_impl::*;

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
/// - `export func ...` / `export enum ...` / `export struct ...` /
///   `export trait ...` — wraps the inner decl in [`Decl::ExportDecl`].
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
        // `export struct ...` → wrap StructDecl. Lets a module ship a
        // public struct type for cross-file `import { Point } ...` use
        // (the cross-file symbol table unwraps ExportDecl and records
        // SymbolKind::Struct). Struct parsing landed in T0.5; the
        // `export struct` arm closes the gap the cross-file tests
        // exercise.
        Some(TokenKind::KwStruct) => {
            let s = parse_struct_decl(stream)?;
            let span = s.span;
            Ok(Decl::ExportDecl(ExportDecl {
                inner: Box::new(Decl::StructDecl(s)),
                span,
            }))
        }
        // `export trait ...` → wrap TraitDecl. Same shape as
        // `export struct` — lets a module ship a public trait.
        Some(TokenKind::KwTrait) => {
            let t = parse_trait_decl(stream)?;
            let span = t.span;
            Ok(Decl::ExportDecl(ExportDecl {
                inner: Box::new(Decl::TraitDecl(t)),
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
        // `export export`, `export import`, `export module` — nested/
        // non-exportable forms. (`export struct` / `export trait` ARE
        // supported — see the arms above.)
        Some(other) => Err(ParseError::new(Diagnostic::error(
            format!(
                "only `func`, `enum`, `struct`, `trait`, `*`, or `{{` are allowed after `export`, found `{other}`"
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
