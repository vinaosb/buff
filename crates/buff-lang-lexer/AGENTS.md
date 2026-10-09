# buff-lang-lexer

Tokenizes `.buff` source → `Vec<Token>`. **Hand-rolled byte-scanner** (logos was removed from Cargo.toml at commit 9af2f5c).

## STRUCTURE

```
src/
├── lib.rs            # 23 lines — exports tokenize, Token, TokenKind, LexerError
├── token.rs          # TokenKind enum + spanned Token struct
├── lexer.rs          # Hand-rolled byte-scanner: entry point tokenize()
├── indent.rs         # Offside-rule indentation tracker (emits Indent/Dedent)
├── string_interp.rs  # String-literal scanner with {expr} interpolation
└── error.rs          # LexerError wrapping buff_lang_error::LexError
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new token kind | `token.rs` (TokenKind) + `lexer.rs` (scan rule) + parser `crates/buff-lang-parser/src/stream.rs` |
| Change indentation rules | `indent.rs` (offside-rule algorithm) |
| Change string interpolation | `string_interp.rs` |
| Add a new lex error | `error.rs` + `crates/buff-lang-error/src/span.rs` (LexError variant) |

## CONVENTIONS (this crate only)

- **HAND-ROLLED, not logos.** `logos` was removed from `[workspace.dependencies]` at commit 9af2f5c (it was never actually used — kept only as a v0.1 placeholder). Do not switch back without a plan to also fix the parser's chumsky issue (same root cause — see root AGENTS.md NOTES).
- **Offside rule** (Python/Haskell-style): indentation level defines blocks. `indent.rs` synthesizes synthetic `Indent` / `Dedent` tokens. Tabs are REJECTED — 4 spaces only.
- **Dedent emission invariants (ITER-36 audit)**: (1) a multi-level dedent emits ONE `Dedent` per closed block level, back-to-back, before the dedented line's first significant token — the parser (`parse_block`) consumes exactly one per block and MUST be able to see the leftovers; (2) comment-only and blank lines NEVER change the indent stack; (3) lines inside brackets (`paren_depth > 0` in `lexer.rs`) emit no `Indent`/`Dedent`; (4) `finalize()` drains the stack at EOF. Changing any of these re-opens the ITER-36 else-steal bug class (see `crates/buff-lang-parser/tests/dedent_else_chain.rs`).
- **String interpolation** is lexed here, not parsed: `"hello {name}!"` produces a sequence of tokens the parser assembles. See `string_interp.rs`.
- **Entry point**: `tokenize(source: &str) -> Result<Vec<Token>, LexerError>`.
- **Tests**: `tests/lexer_tests.rs` (insta snapshots of token streams) + `tests/proptest_template.rs` (proptest fuzzing — lexer must NEVER panic on arbitrary input). Snapshots in `tests/snapshots/`.
- **Span tracking**: every Token carries a `Span` (re-exported from `buff_lang_error`).
