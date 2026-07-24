# buff-lang-codegen-rust

Lowers Buff AST → Rust source via `syn`/`quote`/`prettyplease`. ~16,004 LOC across 8 src files.

## STRUCTURE

```
src/
├── lib.rs                # 197 lines — generate_rust(), generate_test_rust(), format_file() + re-exports
├── rust_codegen.rs       # 12,777 lines — RustCodegen visitor: ALL AST→syn lowering (see below)
├── multi_crate.rs        # T8 — multi-crate emission (one .rs per Buff module when imports present)
├── atomic_analysis.rs    # 1025 lines — T42 atomic promotion (let mut → AtomicI64 fetch_add)
├── race_analysis.rs      # 856 lines — T41 race detection in parallel closures
├── gpu_alignment.rs     # 649 lines — T50 GPU-bound struct #[repr(C)] + bytemuck derives
├── move_analysis.rs      # 374 lines — T33a/T33 move-by-default: clones/Arc/CoW tracking
├── context.rs            # 79 lines — CodegenContext: temp names (__buff_tmp_N), source mappings
└── format.rs             # 47 lines — prettyplease::unparse wrapper (the SINGLE string producer)
```

### rust_codegen.rs (12,777 lines)

Core visitor. Three prelude lowering paths:
- `lower_prelude_call` (~20 arms for free fns: print, sqrt, sleep, etc.)
- `lower_prelude_type_assoc_fn` (~100+ arms: DateTime/Regex/Toml/Math/Random/Strings/Log/Base64/Hex/UUID/URL/Csv/Yaml/Env/Args/Process/TCP/UDP/WebSocket)
- `extern_crates` BTreeSet: chrono, tracing, regex, toml, rand, tokio, base64, hex, sha2, md5, hmac, walkdir, tempfile, num_cpus, tokio-tungstenite, futures-util, serde_yml, csv

### atomic_analysis.rs

Detects `let mut t = <int>` captured by par_map/par_reduce and mutated ONLY via `+=`. Promotes to AtomicI64 with fetch_add(Relaxed). All 5 conditions must hold simultaneously.

### race_analysis.rs

Rejects captured variables mutated inside par_map/par_filter/par_reduce closures. ParallelMutabilityError → CodegenError. Has exemption hook consumed by atomic_analysis (atomics are not races).

### gpu_alignment.rs

Two signals trigger GPU-bound struct detection: closure param type annotation + struct construction inside parallel closure. Adds #[repr(C)] + bytemuck::Pod/Zeroable derives.

## EXECUTION ORDER (matters for correctness)

In `generate()`: atomic analysis → race analysis (with exemption hook from atomic) → async propagation → hash-safety fixpoint → GPU-bound analysis → named-arg/default collection → extern-fn collection → main lowering loop.

## CONVENTIONS

- **HARD RULE: every Rust construct via `syn` types.** The single string producer is `prettyplease::unparse` in `format.rs`. Never `format!()`, `write!()`, or string-concat Rust code.
- **`parse_quote!` BANNED** in non-test code. Use explicit syn struct construction OR `quote!`+`syn::parse2` (returns Result, never panics).
- **Deterministic output**: same AST → byte-identical Rust source. ALL codegen state collections are BTreeMap/BTreeSet (never HashMap/HashSet). CI snapshot tests enforce this.
- **Entry point**: `generate_rust(&[Decl]) -> Result<String, CodegenError>`.
- **Move-by-default**: MoveAnalyzer inserts `.clone()`, `Arc`, or copy. Generated Rust must compile WITHOUT lifetime annotations or visible ownership errors.
- **Type inference EMBEDDED**: RustCodegen owns a TypeInferencer (from buff_lang_types). Reset and rebound with param types at start of `lower_func`. Consulted at each `Stmt::LetDecl` without explicit Buff type. Failure → `Type::Unknown` → no annotation (rustc catches downstream).
- **Numerics**: `rust_decimal` for decimals, `rust_decimal_macros` for literals.
- **Tests**: 53 files in `tests/`, 95 snapshots in `tests/snapshots/`.

## WHERE TO LOOK

| Task | File |
|---|---|
| Lower a new AST node to Rust | `rust_codegen.rs` (add match arm in visitor) |
| Lower a new prelude type/assoc fn | `rust_codegen.rs::lower_prelude_type_assoc_fn` + `prelude_types.rs` in buff-lang-types |
| Change atomic promotion logic | `atomic_analysis.rs` |
| Change race detection rules | `race_analysis.rs` |
| Change GPU struct alignment | `gpu_alignment.rs` |
| Track new per-function state | `context.rs::CodegenContext` |
| Change output formatting | `format.rs` (only prettyplease wrapper) |
| Emit multi-module program as multiple .rs files (T8) | `multi_crate.rs` + `pipeline.rs::compile_to_rust_multi` in buff-lang-cli |
