# buff-lang-codegen-rust

Lowers Buff AST → Rust source via `syn`/`quote`/`prettyplease`. ~26,037 LOC across 30 src files: 11 top-level + 16 under `rust_codegen/` + 2 under `rust_codegen/prelude_lowering/` + 1 under `rust_codegen/prelude_types/` (post ITER-31..44 module split; re-measured ITER-46).

## STRUCTURE

```
src/
├── lib.rs                # 326 lines — generate_rust(), generate_test_rust(), format_file() + re-exports
├── rust_codegen.rs       # 2,437 lines — RustCodegen visitor core: generate() orchestration + dispatch (lowering bodies live in rust_codegen/ submodules, see below)
├── multi_crate.rs        # 734 lines — T8 multi-crate emission (one .rs per Buff module when imports present)
├── atomic_analysis.rs    # 1,042 lines — T42 atomic promotion (let mut → AtomicI64 fetch_add)
├── race_analysis.rs      # 866 lines — T41 race detection in parallel closures
├── gpu_alignment.rs      # 662 lines — T50 GPU-bound struct #[repr(C)] + bytemuck derives
├── move_analysis.rs      # 378 lines — T33a/T33 move-by-default: clones/Arc/CoW tracking
├── context.rs            # 79 lines — CodegenContext: temp names (__buff_tmp_N), source mappings
├── format.rs             # 47 lines — prettyplease::unparse wrapper (the SINGLE string producer)
├── comptime.rs           # 155 lines — T53 comptime block lowering: evaluated const → syn::Item::Const
├── passes.rs             # 1,321 lines — T77/T78 AST-level optimization passes: DCE + constant propagation
└── rust_codegen/         # submodule split (T105a + ITER-31..44): 16 files + 2 nested subdirs, see below
```

### rust_codegen.rs (2,437 lines)

Core visitor: `generate()` orchestration, main AST dispatch, and the glue between pre-passes and lowering. The heavy lowering bodies were extracted into `rust_codegen/` child modules (each `pub(super)`, inheriting parent imports via `use super::*`):

- `lower_prelude_call` → `rust_codegen/prelude_fns.rs` (~20 arms for free fns: print, sqrt, sleep, etc.)
- `lower_prelude_type_assoc_fn` → `rust_codegen/prelude_types.rs` (dispatch; ~100+ arms: DateTime/Regex/Toml/Math/Random/Strings/Log/Base64/Hex/UUID/URL/Csv/Yaml/Env/Args/Process/TCP/UDP/WebSocket), with instance-fn arms in `rust_codegen/prelude_lowering/instance_fns{,_extra}.rs` and framework types in `rust_codegen/prelude_types/framework.rs`
- `extern_crates` BTreeSet population → `rust_codegen/extern_crate_registration.rs` (chrono, tracing, regex, toml, rand, tokio, base64, hex, sha2, md5, hmac, walkdir, tempfile, num_cpus, tokio-tungstenite, futures-util, serde_yml, csv)

### atomic_analysis.rs

Detects `let mut t = <int>` captured by par_map/par_reduce and mutated ONLY via `+=`. Promotes to AtomicI64 with fetch_add(Relaxed). All 5 conditions must hold simultaneously.

### race_analysis.rs

Rejects captured variables mutated inside par_map/par_filter/par_reduce closures. ParallelMutabilityError → CodegenError. Has exemption hook consumed by atomic_analysis (atomics are not races).

### gpu_alignment.rs

Two signals trigger GPU-bound struct detection: closure param type annotation + struct construction inside parallel closure. Adds #[repr(C)] + bytemuck::Pod/Zeroable derives.

### comptime.rs

T53 — consumes `ComptimeFacts` from type analysis and emits `syn::Item::Const` for each successfully evaluated comptime block. Each const gets a deterministic name `__BUFF_COMPTIME_<offset>` (byte-offset of source span). The runtime never re-evaluates the comptime body.

### passes.rs

T77/T78 — pure-function AST-level optimization passes applied before `generate_rust`. Dead code elimination (removes `let` bindings with pure literal values that are never read) and constant propagation (replaces references to constant bindings with their literal values). Deliberately conservative — only transforms when provably safe.

### rust_codegen/ (T105a + ITER-31..44 submodule split)

Mechanical extraction of `impl RustCodegen` methods from `rust_codegen.rs` into child modules. Each file is `pub(super)` and inherits parent imports via `use super::*`:

```
rust_codegen/
├── prelude_types.rs                   # 1,940 lines — lower_prelude_type_assoc_fn dispatch (assoc-fn arms per prelude type)
│   └── framework.rs                   # 1,614 lines — framework-type lowering (Tensor/DataFrame/Pipeline/ML/Game/…)
├── prelude_lowering.rs                # 138 lines — instance-fn + assoc-const seam: entry points + assoc-const table; delegates the ~170-arm instance-fn match
│   ├── instance_fns.rs                # 1,832 lines — PreludeInstanceFn arms, first half (DateTime..ChatMessage)
│   └── instance_fns_extra.rs          # 1,702 lines — PreludeInstanceFn arms, second half (Faker..RsaKeypair) + arity closure + wildcard fallback
├── prelude_fns.rs                     # 204 lines — lower_prelude_call free-fn lowering
├── extern_crate_detection.rs          # 1,769 lines — program_uses_* AST walkers, part 1
├── extern_crate_detection_extra.rs    # 928 lines — program_uses_* walkers, part 2 + error_struct_items
├── extern_crate_registration.rs       # 899 lines — extern_crates BTreeSet population + use-item emission
├── decl_lowering.rs                   # 1,633 lines — type_params/decl/struct/enum/func/extern/trait/extend lowering
├── expr_lowering.rs                   # 1,052 lines — expr construct/pattern/literal/op lowering
├── method_call_lowering.rs            # 1,343 lines — method/builtin-call lowering (one_arg_method..matrix_new)
├── type_lowering.rs                   # 789 lines — ast_typeref_to_syn + buff_type_to_syn type mapping
├── syn_helpers.rs                     # 979 lines — syn-construction helpers (idents, paths, attrs, atomic/Arc)
├── lowering_helpers.rs                # 292 lines — expr/lowering syn builders (generic paths, calls, str coercion)
├── conv_helpers.rs                    # 138 lines — index-cast / arg-parse / typeref→Type conversion helpers
├── derive_attrs.rs                    # 261 lines — derive/repr attribute builders
├── dependency_detection.rs            # 383 lines — named-arg/default/extern-fn dependency collectors
└── analysis.rs                        # 94 lines — ITER-38 pre-pass seam: generate()'s race/atomic/GPU pre-pass orchestration + atomic consultation helpers
```

## EXECUTION ORDER (matters for correctness)

In `generate()` (orchestration in `rust_codegen.rs`, pre-pass seam in `rust_codegen/analysis.rs`): atomic analysis → race analysis (with exemption hook from atomic) → async propagation → hash-safety fixpoint → GPU-bound analysis → named-arg/default collection → extern-fn collection → main lowering loop.

## CONVENTIONS

- **HARD RULE: every Rust construct via `syn` types.** The single string producer is `prettyplease::unparse` in `format.rs`. Never `format!()`, `write!()`, or string-concat Rust code.
- **`parse_quote!` BANNED** in non-test code. Use explicit syn struct construction OR `quote!`+`syn::parse2` (returns Result, never panics).
- **Deterministic output**: same AST → byte-identical Rust source. ALL codegen state collections are BTreeMap/BTreeSet (never HashMap/HashSet). CI snapshot tests enforce this.
- **Entry point**: `generate_rust(&[Decl]) -> Result<String, CodegenError>`.
- **Move-by-default**: MoveAnalyzer inserts `.clone()`, `Arc`, or copy. Generated Rust must compile WITHOUT lifetime annotations or visible ownership errors.
- **Type inference EMBEDDED**: RustCodegen owns a TypeInferencer (from buff_lang_types). Reset and rebound with param types at start of `lower_func`. Consulted at each `Stmt::LetDecl` without explicit Buff type. Failure → `Type::Unknown` → no annotation (rustc catches downstream).
- **Numerics**: `rust_decimal` for decimals, `rust_decimal_macros` for literals.
- **Tests**: 81 files in `tests/`, 120 snapshots in `tests/snapshots/`.

## WHERE TO LOOK

| Task | File |
|---|---|
| Lower a new AST node to Rust | `rust_codegen.rs` visitor → match arm in `rust_codegen/expr_lowering.rs` / `decl_lowering.rs` |
| Lower a new prelude type/assoc fn | `rust_codegen/prelude_types.rs::lower_prelude_type_assoc_fn` (+ `prelude_lowering/instance_fns*.rs`) + `prelude_types.rs` in buff-lang-types |
| Change atomic promotion logic | `atomic_analysis.rs` |
| Change race detection rules | `race_analysis.rs` |
| Change GPU struct alignment | `gpu_alignment.rs` |
| Track new per-function state | `context.rs::CodegenContext` |
| Change output formatting | `format.rs` (only prettyplease wrapper) |
| Emit multi-module program as multiple .rs files (T8) | `multi_crate.rs` + `pipeline.rs::compile_to_rust_multi` in buff-lang-cli |
