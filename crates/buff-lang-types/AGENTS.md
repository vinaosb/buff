# buff-lang-types

Type representation, local inference, async propagation, ownership analysis, prelude registry. ~9,900 LOC across 12 src files. Crate version: 1.2.0.

## STRUCTURE

```
src/
├── lib.rs                # 107 lines — 12 pub mod + 35+ pub use re-exports
├── prelude_types.rs      # 4527 lines — THE LARGEST FILE (see below)
├── ownership.rs          # 1432 lines — T33 Copy/Arc/CoW classification
├── infer.rs              # 1053 lines — TypeInferencer + expr/stmt inference
├── async_analysis.rs     # 867 lines — T31 fixpoint async propagation (no await keyword)
├── ty.rs                 # 831 lines — Type enum (20 variants) + IntWidth + FloatWidth
├── recursion.rs          # 759 lines — T48 DFS cycle detection, GPU-ineligible marking
├── exhaustiveness.rs     # 682 lines — T27 match coverage checking
├── modules.rs            # 615 lines — T29 module graph, FsLoader, MemoryLoader
├── prelude.rs            # 483 lines — 21 free-fn preludes (Math/Convert/Io/System)
├── range_analysis.rs     # 274 lines — IntRange (i128 interval) + flexible width
├── promote.rs            # 155 lines — promote_binary + assignable_to
└── env.rs                # 60 lines — TypeEnv (flat HashMap<String, Type>)
```

### prelude_types.rs (4527 lines)

The extensible stdlib registry. Every future prelude type (URL, Base64, Hash, TCP, etc.) adds here.

- **PreludeType** enum: DateTime, Date, Time, Duration, Instant, Regex, Math (and growing)
- **PreludeAssocFn**: 400+ type-associated methods (e.g. `DateTime.parse()`, `Regex.match()`)
- **PreludeAssocConst**: PI, E
- **PreludeInstanceFn**: 190+ instance methods (e.g. `str.trim()`, `Vec.len()`)
- Lookup helpers: `assoc_fn_lookup`, `instance_fn_lookup`, `assoc_const_lookup`, `prelude_type_lookup`, `is_prelude_type`

### async_analysis.rs

Fixpoint propagation on call graph. `analyze_async` walks CallGraph → AsyncSet until stable. This is why Buff has no `await` keyword: async propagates automatically.

### ownership.rs

`analyze_func` (re-exported as `analyze_ownership`) classifies Copy vs Arc vs CoW. Detects Arc-across-spawn and CoW mutation patterns.

## STANDALONE TYPECHECK (SHIPPED)

`buff check` (T55) at `buff-lang-cli/src/check.rs::check_source()` runs TypeInferencer WITHOUT codegen. The root AGENTS.md statement "standalone typecheck is post-v1.0" is OUTDATED.

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new stdlib type | `prelude_types.rs` (PreludeType + assoc fns + instance fns) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs` |
| Add a free-fn prelude (print, etc.) | `prelude.rs` (PreludeFn + return_type) + codegen-rust |
| Add a primitive type | `ty.rs` (Type enum) + `promote.rs` (promotion matrix) |
| Add type inference for new AST node | `infer.rs::TypeInferencer` |
| Change async propagation logic | `async_analysis.rs` |
| Change ownership classification | `ownership.rs` |
| Tune numeric width inference | `range_analysis.rs` + `promote.rs` |

## CONVENTIONS

- **Resolved vs unresolved**: `Type` here is RESOLVED. Unresolved references live in `buff-lang-ast/src/ty.rs::TypeRef`.
- **Prelude is implicit**: no `import` needed. Free fns in `prelude.rs`, type-assoc in `prelude_types.rs`.
- **TypeInferencer is reset per-function**: `infer.rs` rebinds TypeEnv with param types at each `infer_func`. Consulted by codegen-rust when lowering `LetDecl` without explicit Buff type.
- **Numeric width inference** is "flexible mode": picks smallest Int width that fits the range.
- **`Span` re-exported** from `buff_lang_error`.
- **Tests**: 9 files in `tests/`: infer_tests (835), recursion_test (499), modules (472), option_null_safety (438), async_propagation (425), exhaustiveness (411), prelude_functions (353), numeric_coercion (346), expected_type_inference (189), tuples (179).
