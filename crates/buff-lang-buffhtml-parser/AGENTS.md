# buff-lang-buffhtml-parser

Hand-rolled lexer + recursive-descent parser for `.buffhtml` SFCs. Produces `RsxTemplateFile` from `buff-lang-ast-rsx`. Expression contents inside `{...}` are stored verbatim (T133 scope); full expression-level integration is T134+.

## STRUCTURE

```
src/
├── lib.rs      # 35 lines — re-exports: tokenize(), parse(), BuffHtmlToken, BuffHtmlTokenKind, BuffHtmlParseError
├── lexer.rs    # 1528 lines — 3-mode byte scanner + token kinds + brace-matching helpers
├── parser.rs   # 1228 lines — recursive-descent: token stream → RsxTemplateFile
└── error.rs    # 54 lines — BuffHtmlParseError (Lex/Parse variants, each with message + Span)
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new RSX directive (e.g. `{#portal}`) | `lexer.rs` (new `BuffHtmlTokenKind` + scanner) + `parser.rs` (new `parse_*` method + `Terminator` variant) |
| Tune mode boundaries (Buff/HTML/attribute) | `lexer.rs::LexerState` — `scan_tag`, `scan_brace`, `scan_attributes` |
| Add a new attribute form | `lexer.rs` (`scan_one_attribute`) + `parser.rs` (`parse_eq_attribute`) |
| Change brace-matching / string-skipping logic | `lexer.rs::find_matching_brace` (nested braces + `"..."` strings) |
| Validate block nesting (e.g. `{#if}`/`{/if}` mismatch) | `parser.rs::parse_children_until_terminators` + `Terminator` enum |

## 3-MODE LEXER

The scanner has three implicit modes (not an explicit state enum, but dispatch on byte context):

1. **TEXT mode** (default): accumulates raw HTML text until `<` or `{`. Whitespace-only runs are preserved as tokens; the parser decides whether to trim.
2. **TAG mode** (inside `<...>`): dispatches on byte after `<`: `!` → HTML comment, `/` → close tag/fragment, `>` → fragment open, alpha → open tag. Inside open tags, `scan_attributes` handles attribute scanning (AttrName, AttrEq, AttrColon, AttrStrLit, `{...spread}`).
3. **BRACE mode** (inside `{...}`): dispatches on byte after `{`: `#` → directive (each/if/await/comment), `:` → else/then/catch, `/` → block close, `@` → at-directive (html), else → interpolation. Brace-matching respects nested `{}` and `"..."` strings via `find_matching_brace`.

Key helper: `find_each_as_boundary()` matches the FIRST top-level ` as ` in each-directives, respecting paren/bracket/string depth so `items.read()` works as an iterable.

## CONVENTIONS (this crate only)

- **HAND-ROLLED** (same rationale as `buff-lang-parser` — no chumsky/logos, no cc-rs C shim). Do NOT introduce parser combinator crates.
- **All fallible paths return `Result<_, BuffHtmlParseError>`**. No `unwrap`/`expect`/`panic!` in non-test code.
- **Span on every token and AST node** — `BuffHtmlToken.span` from `buff_lang_error::Span`, carried through to `RsxNode` structs.
- **`<script>` blocks are top-level only** — parser rejects nested `<script>` with an error. The lexer captures `lang="..."` and optional `props="..."` via `ScriptOpen` token.
- **Expression source is verbatim** — `{expr}` interpolation tokens store raw text. Codegen parses them into Rust later. T134 will add `buff_lang_parser::parse_expression()` integration.
- **Component detection** — `is_component_tag()` from `buff-lang-ast-rsx` (first char ASCII uppercase). Both parser and codegen use it defensively.
- **Tests**: inline `#[cfg(test)]` modules in `lexer.rs` (40+ tests) and `parser.rs` (30+ tests). No separate `tests/` directory.
