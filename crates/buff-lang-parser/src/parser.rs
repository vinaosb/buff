//! Top-level parser entry point.
//!
//! [`parse`] consumes a slice of lexer-produced tokens and emits a list of
//! top-level declarations ([`Decl`]). Statement-level parsing (`let`, `if`,
//! `func`, `return`, `for`, …) lives in [`crate::stmt`]; this module only
//! dispatches on top-level keywords.
//!
//! [`parse`] is the public boundary that downstream code calls. It hides
//! [`TokenStream`] construction and any pre-filtering of layout tokens.

use buff_lang_ast::Decl;
use buff_lang_error::{ParseError, SourceId};
use buff_lang_lexer::TokenKind;

use crate::stmt::{
    parse_attributes, parse_enum_decl, parse_export_decl, parse_extern_crate_decl, parse_func_decl,
    parse_import_decl,
};
use crate::stream::TokenStream;

/// Parse a slice of tokens into zero or more top-level [`Decl`]s.
///
/// # Dispatch table
///
/// | Token   | Result                                                |
/// |---------|-------------------------------------------------------|
/// | `func`  | [`Decl::FuncDecl`] (T8)                               |
/// | `async func` | [`Decl::FuncDecl`] with `is_async = true` (T31) |
/// | `enum`  | [`Decl::EnumDecl`] (T27)                              |
/// | `import`| [`Decl::ImportDecl`] (T29 — ES6 `from "..."` + legacy)|
/// | `export`| [`Decl::ExportDecl`] / [`Decl::ReexportDecl`] (T29)  |
/// | `export async func` | [`Decl::ExportDecl`] wrapping an async fn (T31) |
/// | `extern crate "name"` | [`Decl::ExternCrateDecl`] (T32 — FFI)        |
/// | `extern func ...`     | [`Decl::FuncDecl`] with `is_extern = true` (T32 — FFI) |
/// | `@name ... func`      | [`Decl::FuncDecl`] with `attributes` populated (T35 — `buff test`) |
///
/// Any other token at top level is an error — statements such as
/// `let`/`return`/`if` belong inside a function body, not at module scope.
///
/// # Errors
///
/// Returns [`ParseError`] on any syntax error, including unknown top-level
/// keywords or malformed function declarations.
pub fn parse(
    tokens: &[buff_lang_lexer::Token],
    source_id: SourceId,
) -> Result<Vec<Decl>, ParseError> {
    let mut stream = TokenStream::new(tokens, source_id);
    let mut decls = Vec::new();
    while !stream.is_at_end() {
        // T35: parse any leading `@name` attributes before the declaration.
        // When attributes are present, only a `func` declaration is valid
        // (the only attribute-attachable decl kind in v0.5). The attributes
        // are threaded into `parse_func_decl` so they land on the FuncDecl.
        let attributes = parse_attributes(&mut stream)?;
        let saw_attributes = !attributes.is_empty();
        match stream.peek_kind() {
            Some(TokenKind::KwFunc) => {
                let f = parse_func_decl(&mut stream, attributes)?;
                decls.push(Decl::FuncDecl(f));
            }
            // T31: `async func name(...) { ... }` — the async modifier on a
            // function declaration. The `async` keyword (`TokenKind::KwAsync`)
            // precedes `func`; `parse_func_decl` consumes the leading `async`
            // (if present) and sets `is_async` accordingly. The dispatcher
            // routes both `async func` and `func` to `parse_func_decl` so
            // the modifier is handled in one place.
            Some(TokenKind::KwAsync)
                if matches!(stream.peek_second_kind(), Some(TokenKind::KwFunc)) =>
            {
                let f = parse_func_decl(&mut stream, attributes)?;
                decls.push(Decl::FuncDecl(f));
            }
            // T27: top-level enum declarations. Functions and enums are the
            // two top-level forms supported at this stage; struct/trait/module
            // parsing arrives in later waves.
            Some(TokenKind::KwEnum) => {
                if saw_attributes {
                    let span = stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span());
                    return Err(ParseError::new(buff_lang_error::Diagnostic::error(
                        "attributes are not yet supported on `enum` declarations (only `func`)",
                        span,
                    )));
                }
                let e = parse_enum_decl(&mut stream)?;
                decls.push(Decl::EnumDecl(e));
            }
            // T29: top-level import / export declarations.
            Some(TokenKind::KwImport) => {
                if saw_attributes {
                    let span = stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span());
                    return Err(ParseError::new(buff_lang_error::Diagnostic::error(
                        "attributes are not supported on `import` declarations",
                        span,
                    )));
                }
                let imp = parse_import_decl(&mut stream)?;
                decls.push(Decl::ImportDecl(imp));
            }
            Some(TokenKind::KwExport) => {
                if saw_attributes {
                    let span = stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span());
                    return Err(ParseError::new(buff_lang_error::Diagnostic::error(
                        "attributes are not supported on `export` declarations (put the attribute on the inner declaration instead, e.g. `@test\\nfunc ...` without `export`)",
                        span,
                    )));
                }
                let exp = parse_export_decl(&mut stream)?;
                decls.push(exp);
            }
            // T32: top-level FFI declarations. Two shapes share the `extern`
            // keyword:
            //   1. `extern crate "name"` — record a dependency on an external
            //      Rust crate (→ [`Decl::ExternCrateDecl`]).
            //   2. `extern func name(...) -> Ret` — a foreign-function
            //      declaration with NO body (→ [`Decl::FuncDecl`] with
            //      `is_extern = true`; `parse_func_decl` consumes the leading
            //      `extern` and skips body parsing).
            // The dispatcher disambiguates by peeking at the second token.
            Some(TokenKind::KwExtern) => {
                if saw_attributes {
                    let span = stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span());
                    return Err(ParseError::new(buff_lang_error::Diagnostic::error(
                        "attributes are not supported on `extern` declarations",
                        span,
                    )));
                }
                match stream.peek_second_kind() {
                    Some(TokenKind::Ident(s)) if s == "crate" => {
                        let d = parse_extern_crate_decl(&mut stream)?;
                        decls.push(d);
                    }
                    Some(TokenKind::KwFunc) => {
                        let f = parse_func_decl(&mut stream, attributes)?;
                        decls.push(Decl::FuncDecl(f));
                    }
                    _ => {
                        return Err(ParseError::new(buff_lang_error::Diagnostic::error(
                            "expected `extern crate \"<name>\"` or `extern func ...` after `extern`",
                            stream
                                .peek()
                                .map(|t| t.span)
                                .unwrap_or_else(|| stream.eof_span()),
                        )));
                    }
                }
            }
            // T35: attributes were present but the next token is not a
            // recognised attribute-attachable declaration. This is a parse
            // error (e.g. `@test let x = 1` or `@test` at EOF).
            _ if saw_attributes => {
                let span = stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span());
                let found = stream
                    .peek_kind()
                    .map(|k| k.to_string())
                    .unwrap_or_else(|| "end of input".into());
                return Err(ParseError::new(buff_lang_error::Diagnostic::error(
                    format!("attributes must precede a `func` declaration, found `{found}`"),
                    span,
                )));
            }
            other => {
                let span = stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span());
                return Err(ParseError::new(buff_lang_error::Diagnostic::error(
                    format!(
                        "only function declarations are allowed at top level, found `{}`",
                        other
                            .map(|k| k.to_string())
                            .unwrap_or_else(|| "end of input".into())
                    ),
                    span,
                )));
            }
        }
    }
    Ok(decls)
}

/// Parse a single top-level expression from a token slice. Convenience
/// wrapper for tests and embedding tools that want to evaluate an
/// expression without setting up a [`TokenStream`] themselves.
///
/// # Errors
///
/// Returns [`ParseError`] if the tokens do not form exactly one expression,
/// or if there are leftover tokens after the expression.
pub fn parse_expression(
    tokens: &[buff_lang_lexer::Token],
    source_id: SourceId,
) -> Result<buff_lang_ast::Expr, ParseError> {
    let mut stream = TokenStream::new(tokens, source_id);
    let expr = crate::expr::parse_expression(&mut stream)?;
    if !stream.is_at_end() {
        return Err(stream.unexpected("extra tokens after expression"));
    }
    Ok(expr)
}
