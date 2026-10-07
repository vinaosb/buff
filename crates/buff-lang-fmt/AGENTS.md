# buff-lang-fmt

Buff source formatter behind `buff fmt` (and the LSP's formatting capability). Lex → parse → re-emit canonical layout from the AST, preserving comments via the lossless tree. Kept as its own crate so both the CLI binary and `buff-lsp` produce byte-identical formatting.

## STRUCTURE

```
src/
├── lib.rs     # format_source (public entry), format_decls,
│              #   format_decls_with_comments, is_already_formatted, FormatError
├── expr.rs    # Expression formatting (Pratt-aware re-emission)
└── decl.rs    # Declaration formatting (funcs/structs/enums/traits/impls)
```

## WHERE TO LOOK

| Task | Location |
|---|---|
| Change top-level entry behavior | `lib.rs::format_source` |
| Fix expression spacing/precedence | `expr.rs` |
| Fix decl/indentation layout | `decl.rs` |
| Preserve a comment style | `lib.rs::format_decls_with_comments` (consumes `LosslessTree`) |
| `buff fmt --check` semantics | `lib.rs::is_already_formatted` (format + byte-compare) |

## CONVENTIONS (this crate only)

- **Formatter = parser output, not regex.** Never pattern-match source text; format from the parsed AST so `format_source` output is guaranteed to re-parse identically.
- **Comments ride the lossless tree.** `format_decls_with_comments` takes the lexer's `LosslessTree`; do not synthesize comment placement from spans alone.
- **Byte-identical consumers.** `buff-lsp`'s formatting handler calls `format_source` directly — any change here changes both `buff fmt` and editor formatting simultaneously.
- **Errors via `thiserror`.** Malformed input yields `FormatError` (wrapping parse/lex errors), never panics.

## DEPENDENCIES

- Buff compiler crates: `buff-lang-ast`, `buff-lang-lexer`, `buff-lang-parser`, `buff-lang-error`
- `thiserror` (FormatError)
