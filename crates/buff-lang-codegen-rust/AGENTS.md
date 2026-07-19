# buff-lang-codegen-rust

Lowers Buff AST → Rust source via `syn`/`quote`/`prettyplease`. NEVER hand-format Rust strings.

## STRUCTURE

```
src/
├── lib.rs            # 59 lines — exports generate_rust() convenience fn + example in doc
├── rust_codegen.rs   # RustCodegen: AST → syn::File (the main visitor)
├── context.rs        # CodegenContext: per-function state (locals, scopes)
├── move_analysis.rs  # MoveAnalyzer: tracks move-by-default semantics
└── format.rs         # format(syn::File) → String via prettyplease::unparse
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Lower a new AST node to Rust | `rust_codegen.rs` (add a match arm in the visitor) |
| Track a new per-function state | `context.rs::CodegenContext` |
| Change move/copy/borrow decision | `move_analysis.rs::MoveAnalyzer` |
| Change output formatting | `format.rs` (only prettyplease wrapper) |

## CONVENTIONS (this crate only)

- **HARD RULE: every Rust construct via `syn` types.** The single string producer is `prettyplease::unparse` in `format.rs`. Never `format!()`, `write!()`, or string-concat Rust code.
- **Entry point**: `generate_rust(&[Decl]) -> Result<String, CodegenError>`. Convenience wrapper around `RustCodegen::generate` + `format`.
- **Move-by-default semantics** — Buff hides borrow checking from users; `MoveAnalyzer` decides where to insert `.clone()`, `Arc`, or copy. Generated Rust must compile WITHOUT lifetime annotations or visible ownership errors.
- **Deterministic output**: same AST → byte-identical Rust source. CI snapshot tests enforce this. If output changes intentionally, run `cargo insta review`.
- **Numerics**: `rust_decimal` for decimals, `rust_decimal_macros` for literals. Used here only.
- **Unicode**: `unicode-segmentation` for string operations.
- **Tests**: 6 files in `tests/` — `codegen_tests`, `control_tests`, `literal_tests`, `move_tests`, `prelude_codegen`, `string_methods`. Snapshots in `tests/snapshots/`.
- **v0.1 scope**: primitives + funcs + control flow. Collections/structs/async deferred to v0.5.
