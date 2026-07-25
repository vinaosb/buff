# buff-lang-parser

Parses `Vec<Token>` → `Vec<Decl>`. **Hand-rolled recursive-descent + Pratt** (chumsky was removed from Cargo.toml at commit 9af2f5c).

## STRUCTURE

```
src/
├── lib.rs       # 31 lines — exports parse(), parse_expression(), statement helpers
├── stream.rs    # TokenStream cursor: peek / next / expect
├── options.rs   # Parser edition selection (T57): Standard / Scientific
├── expr.rs      # Expression parser (Pratt climbing for precedence)
├── expr/        # Expression sub-parsers
│   ├── expr_pattern.rs  # Pattern-matching expression parsing
│   └── expr_postfix.rs  # Postfix expression parsing (calls, indexing, etc.)
├── stmt.rs      # Statement parser + block / func_decl / if / params / type_ref helpers
├── stmt/        # Statement sub-parsers
│   └── stmt_decl.rs     # Declaration statement parsing (struct, enum, trait, impl)
└── parser.rs    # Top-level parse() returning Vec<Decl>
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new expression form | `expr.rs` (extend `parse_expression`) |
| Add a new statement form | `stmt.rs` (extend `parse_statement`) |
| Add a new top-level decl | `parser.rs` (extend parse loop) + `crates/buff-lang-ast/src/decl.rs` |
| Change operator precedence | `expr.rs` (Pratt binding-power table) |
| Add a token-stream helper | `stream.rs::TokenStream` |

## CONVENTIONS (this crate only)

- **HAND-ROLLED, not chumsky.** chumsky 1.0.0-alpha.8 transitively needed `stacker`, which uses `cc-rs` to compile a C shim — fails on Windows hosts missing `excpt.h` from the Windows SDK. Hand-rolling won. chumsky was removed from Cargo.toml at commit 9af2f5c. Do NOT re-introduce chumsky without solving the stacker/cc-rs issue.
- **Pratt parsing** for expressions (operator precedence via binding powers). Statements use recursive descent.
- **Layout-sensitive**: parser consumes synthetic `Indent`/`Dedent` tokens emitted by `crates/buff-lang-lexer/src/indent.rs`. Don't re-implement indentation tracking here.
- **Public API** (see `lib.rs`): `parse()` (top-level, returns `Vec<Decl>`), plus lower-level helpers `parse_block`, `parse_func_decl`, `parse_if_expr`, `parse_params`, `parse_type_ref`, `parse_statement`, `parse_expression`, `TokenStream`.
- **Tests**: 3 files in `tests/` including `expr_tests.rs`. Snapshot tests use `crates/buff-lang-parser/tests/snapshots/`.
