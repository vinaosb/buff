# buff-lang-error

LEAF crate. Error types, span tracking, source maps, diagnostics. Depended on by ALL other crates — change carefully.

## STRUCTURE

```
src/
├── lib.rs          # 9 lines — module wiring + glob re-exports
├── span.rs         # Span (byte range), LexError, ParseError, TypeError, CodegenError variants
├── source_map.rs   # SourceFile / SourceMap — line/col lookup from byte offset
└── diagnostic.rs   # Diagnostic type for user-facing error rendering
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new error variant | `span.rs` (extend the relevant `*Error` enum) |
| Add a new error category | `span.rs` (new enum) + `#[derive(thiserror::Error)]` |
| Change how spans render to line:col | `source_map.rs` |
| Change user-facing diagnostic format | `diagnostic.rs` |

## CONVENTIONS (this crate only)

- **ZERO internal deps** — only `thiserror`. This is the foundation; do not add crate deps without strong reason.
- **All error enums use `#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]`.**
- **`Span` is THE location type** — re-exported by `buff-lang-ast` and `buff-lang-types`. Every AST node, every Token carries one. Don't introduce a competing location type.
- **Error messages**: lowercase, no trailing period, include context (`"file not found: {path}"`, not `"not found"`). See `.sisyphus/plans/buff-conventions.md` §4.
- **No hand-written `Display`** — `thiserror::Error` derives `Display` from `#[error("...")]` attributes.
- **Tests**: 2 files in `tests/`.
