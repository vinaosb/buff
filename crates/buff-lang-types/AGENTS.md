# buff-lang-types

Type representation + local inference + standard-library prelude. Sits between `parser` and `codegen-*`.

## STRUCTURE

```
src/
├── lib.rs            # 37 lines — re-exports Type, TypeInferencer, prelude fns
├── ty.rs             # Resolved Type enum (Int widths, Float widths, Bool, etc.)
├── env.rs            # TypeEnv — flat symbol table for inference
├── infer.rs          # TypeInferencer — walks AST, assigns Type
├── promote.rs        # Numeric promotion rules: promote_binary, assignable_to
├── range_analysis.rs # IntRange, smallest_int_width, collection_int_width
└── prelude.rs        # Built-in implicit fns (print, etc.) — no `import` needed
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a primitive type | `ty.rs` (extend `Type` enum) + `promote.rs` (promotion matrix) |
| Add type inference for new node | `infer.rs::TypeInferencer` (add visit method) |
| Add a built-in/prelude fn | `prelude.rs` (PreludeFn + return_type) + `crates/buff-lang-codegen-rust` (Rust lowering) |
| Tune numeric width inference | `range_analysis.rs` + `promote.rs` |
| Inspect available symbols during inference | `env.rs::TypeEnv` |

## CONVENTIONS (this crate only)

- **Resolved vs unresolved types**: `Type` here is RESOLVED. Unresolved type REFERENCES live in `crates/buff-lang-ast/src/ty.rs::TypeRef`.
- **Prelude is implicit**: `print` etc. don't need `import`. Add new built-ins in `prelude.rs` — they auto-appear in every Buff program.
- **Re-exports at crate root** (see `lib.rs`): `is_prelude`, `lookup`, `category_of`, `PreludeFn`, `PreludeCategory`, `IntRange`, `smallest_int_width`, `collection_int_width` — all top-level for downstream callers.
- **Numeric width inference** is "flexible mode" — picks smallest Int width that fits the range. See `range_analysis.rs::smallest_int_width`.
- **v1.0 scope**: primitives, collections (Vector/Map/Matrix), user-defined types (struct/enum), traits, full type inference, exhaustiveness checking, recursion detection. All shipped.
- **`Span` re-exported** from `buff_lang_error` (don't redefine).
- **Tests**: 3 files in `tests/` including `prelude_functions.rs` and `infer_tests.rs`.
