# buff-lang-ast

Pure-data AST node definitions. NO parsing, NO type checking. Depended on by `parser`, `types`, `codegen-rust` — changes here ripple everywhere.

## STRUCTURE

```
src/
├── lib.rs       # 37 lines — module wiring + `pub use <mod>::*` glob re-exports
├── common.rs    # Ident, Block, Param
├── op.rs        # BinaryOp, UnaryOp enums
├── ty.rs        # TypeRef — unresolved type references (e.g. `Int`, `Vector<T>`)
├── expr.rs      # Literal, Expr, MatchArm, Pattern
├── stmt.rs      # Stmt
├── decl.rs      # Decl enum + FuncDecl, StructDecl, EnumDecl, ...
└── ir.rs        # Dataflow IR: IrGraph, IrNode, AstLowerer (lowering from AST)
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new expression variant | `expr.rs` (extend `Expr` enum) + parser `crates/buff-lang-parser/src/expr.rs` + codegen `crates/buff-lang-codegen-rust/src/rust_codegen.rs` |
| Add a new declaration kind | `decl.rs` (extend `Decl` enum) — same ripple |
| Add a new operator | `op.rs` + lexer `crates/buff-lang-lexer/src/token.rs` + codegen operator match |
| Lower AST to dataflow IR | `ir.rs::AstLowerer` |

## CONVENTIONS (this crate only)

- **`pub use <mod>::*` glob re-exports** at lib.rs root — every type is importable as `buff_lang_ast::Foo`.
- **`Span` is re-exported from `buff-lang-error`**, not redefined: `pub use buff_lang_error::Span;`. Do NOT add a local Span.
- **Every node carries a `span: Span` field** for diagnostics. Don't add spanless nodes.
- **Derive `Debug, Clone, PartialEq`** on every node (+ `Eq, Hash` if needed for IR).
- **`ir.rs` is the lowering target** for future optimizations (dataflow graph). Scaffolded in v0.1; full use is post-v1.0 work.
- **Snapshot tests** in `tests/snapshots/` — insta captures `format!("{:#?}", node)`. Run `cargo insta review` after structural changes (snapshots will need re-acceptance).
- **Tests**: `ir_tests.rs`, `snapshot_tests.rs`, `snapshot_helper.rs` in `tests/`.
