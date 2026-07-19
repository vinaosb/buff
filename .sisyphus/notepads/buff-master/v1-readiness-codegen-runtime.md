# v1.0 Readiness Report: Codegen-Rust & Runtime Stubs

**Generated:** 2026-07-19
**Purpose:** Map current codegen infrastructure and runtime stubs before adding parallelism

## 1. buff-lang-codegen-rust/src Structure

### Source Files with Line Counts

- **lib.rs** (187 lines): Public API exports, convenience functions, test harness synthesis
- **rust_codegen.rs** (6305 lines): Main codegen visitor - lowers all Buff AST nodes to syn types
- **context.rs** (79 lines): Per-pass state - source mappings, temp name generation
- **move_analysis.rs** (374 lines): Move-by-default semantics - Copy/non-Copy classification, Arc tracking
- **format.rs** (47 lines): Wrapper around prettyplease::unparse() - single string producer

### Public API of lib.rs (pub-use exports)

`ust
pub use context::CodegenContext;
pub use format::format;
pub use move_analysis::MoveAnalyzer;
pub use rust_codegen::{buff_primitive_to_rust_name, RustCodegen};
`

### RustCodegen Struct Definition (rust_codegen.rs:94-239)

**Key fields:**
- ctx: CodegenContext - Per-pass state
- move_analyzer: MoveAnalyzer - Move/copy/Arc tracking
- 	ype_inferencer: TypeInferencer - Type inference for let bindings
- epr_c_struct_names: HashSet<String> - GPU-dispatch hook for #[repr(C)] structs (T26)
- sync_fns: BTreeSet<String> - Post-propagation async function names (T31)
- current_fn_name: Option<String> - Current function being lowered
- sync_block_depth: usize - Depth of async move blocks for spawn
- spawn_depth: usize - Depth of spawn bodies for Arc cloning
- closure_capture_stack: Vec<BTreeSet<String>> - Closure variable bypass sets (T34)
- warnings: Vec<Diagnostic> - Collected warnings
- extern_crates: BTreeSet<String> - External crate dependencies (T32)
- collected_unions: BTreeMap<String, Vec<TypeRef>> - Union type wrappers (T76)
- hash_safe_structs: BTreeSet<String> - Structs that can derive Hash (T107)
- deferred_exprs: Vec<SynExpr> - Defer statement accumulators (T100)
- unc_param_names: BTreeMap<String, Vec<String>> - Function parameter names (T105)
- unc_param_defaults: BTreeMap<String, Vec<Option<Expr>>> - Function parameter defaults (T106)

**Important methods:**
- 
ew() - Creates fresh codegen (line 243)
- generate(&mut self, decls: &[Decl]) -> Result<File> - Main entry point (line 334)
- context(&self) -> &CodegenContext - Borrow context (line 265)
- 	ake_warnings(&mut self) -> Vec<Diagnostic> - Drain warnings (line 276)
- extern_crates(&self) -> &BTreeSet<String> - Get extern crates (line 300)
- mark_struct_repr_c(&mut self, name: &str) - Mark struct for GPU dispatch (line 312)

### MoveAnalyzer Wiring

**Integration point:** rust_codegen.rs:705-707
`ust
self.move_analyzer.reset();
self.move_analyzer.preanalyze_func(f);
`

Called at start of lower_func() - resets per-function state and runs ownership analysis via uff_lang_types::analyze_ownership() (move_analysis.rs:92).

### Lambda/Closure Lowering

**Expression handler:** rust_codegen.rs:1877
`ust
Expr::Lambda { params, body, .. } => self.lower_lambda(params, body),
`

**Implementation:** rust_codegen.rs:3113-3165
- Emits Rust closure: |p1, p2, ...| body
- Closure capture analysis via uff_lang_types::closure_captures() (line 3143)
- Capture set pushed to closure_capture_stack (line 3147) to prevent spurious clones
- Parameters and captured variables bypass move analyzer (lines 134-158)

### Method Call Lowering

**Handler:** rust_codegen.rs:2479-2672

**Special cases:**
- Zero-arg disambiguation (field vs method): lines 2522-2525
- Matrix.new(rows, cols) ? Matrix::new(rows, cols): lines 2534-2540
- .context("msg") ? .map_err(|e| format!("msg: {:?}", e)): lines 2579-2582
- String methods (char_count, byte_len, chars, bytes, first, last, graphemes, slice): lines 2600-2622
- Vector iteration (.map(), .filter(), .reduce()): lines 2629-2640
- Map methods (.contains() ? .contains_key()): lines 2649-2652

**Default path:** ecv.method(args) (lines 2658-2669)

### Existing Parallel/Dispatch Code

**Async propagation (T31):**
- sync_fns set tracks all async functions via uff_lang_types::analyze_async() (line 364)

**Spawn lowering (T31):**
- spawn expr ? 	okio::spawn(async move { expr }) at lines 3414-3431
- Uses quote! to build tokio::spawn call with async move closure

**Async context tracking:**
- sync_block_depth and spawn_depth fields track context for .await insertion (lines 125, 133)
- current_fn_name tracks current function for async checks (line 118)

**Tokio integration:**
- #[tokio::main] emitted on async main functions (lines 812-813)
- lock(expr) ? one-shot tokio runtime with deadlock warning (lines 3459-3516)

**Arc shared bindings:**
- Arc::new(...) wrapping for spawn-captured variables (lines 1364-1378)
- CoW mutation: Arc::make_mut(&mut x) for mutated Arc bindings (lines 1495-1506)

**GPU dispatch hook:**
- epr_c_struct_names set for #[repr(C)] struct emission (T26, line 312)
- Currently unused - future GPU-dispatch analysis should populate this set

**Missing:**
- No GPU shader generation (buff-lang-codegen-wgsl is a stub)
- No runtime dispatch logic (runtime crate is a stub)

## 2. buff-lang-runtime Current State

**src/lib.rs (1 line):**
`ust
//! Buff Runtime crate — Async runtime, parallel execution, and GPU compute support.
`

**Cargo.toml dependencies:**
- 	hiserror.workspace = true
- uff-lang-error.workspace = true
- NO wgpu, rayon, tokio, bytemuck, pollster deps declared

**Tests directory:** Does NOT exist

## 3. buff-lang-codegen-wgsl Current State

**src/lib.rs (1 line):**
`ust
//! Buff WGSL Codegen crate — Generates WGSL shader source code from Buff AST.
`

**Cargo.toml dependencies:**
- 	hiserror.workspace = true
- uff-lang-error.workspace = true
- NO WGSL-specific deps

**Tests directory:** Does NOT exist

## 4. Root Cargo.toml [workspace.dependencies]

**All workspace dependencies:**
- logos = "0.14"
- chumsky = { version = "1.0.0-alpha.8", features = ["pratt"] }
- syn = { version = "2.0", features = ["full", "parsing", "printing", "extra-traits"] }
- quote = "1.0"
- proc-macro2 = "1.0"
- prettyplease = "0.2"
- insta = "1.40"
- proptest = "1.5"
- wgpu = "26.0" ? (declared but unused)
- ayon = "1.10" ? (declared but unused)
- 	okio = { version = "1.40", features = ["full"] } ? (declared, used in codegen)
- ytemuck = { version = "1.18", features = ["derive"] } ? (declared but unused)
- ust_decimal = "1.36"
- ust_decimal_macros = "1.36"
- unicode-segmentation = "1.12"
- clap = { version = "4.5", features = ["derive"] }
- nyhow = "1.0"
- 	hiserror = "1.0"
- serde = { version = "1", features = ["derive"] }
- 	oml = "0.8"

**GPU/parallel-adjacent crates already declared:**
- ? wgpu = "26.0" - GPU compute
- ? rayon = "1.10" - CPU parallelism
- ? tokio = "1.40" - Async runtime (already used for spawn)
- ? bytemuck = "1.18" - GPU buffer casting (derive enabled)
- Missing: pollster for async block (needed for one-shot runtime)

## 5. buff-lang-cli Pipeline Flow

**Current pipeline (pipeline.rs:46-77):**
`ust
pub fn compile_to_rust(file: &Path) -> Result<CompileOutput> {
    // 1. Read source
    let source = std::fs::read_to_string(file)?;
    
    // 2. Lex
    let tokens = tokenize(&source, source_id)?;
    
    // 3. Parse
    let decls = parse(&tokens, source_id)?;
    
    // 4. Codegen (type inference integrated inside)
    let rust_source = generate_rust(&decls)?;
    
    // 5. Write .rs file
    std::fs::write(&rust_file_path, &rust_source)?;
    
    Ok(CompileOutput { rust_source, rust_file_path })
}
`

**Rust compilation (pipeline.rs:96-124):**
`ust
pub fn compile_rust_to_exe(rust_file: &Path, output: &Path, buff_file: &Path) -> Result<PathBuf> {
    Command::new("rustc")
        .arg("--edition").arg("2021")
        .arg("-O")
        .arg(rust_file)
        .arg("-o").arg(output)
        .output()?;
    
    // Translate rustc errors from .rs references to .buff references
}
`

**Run command (commands/run.rs:37-91):**
`ust
pub fn run(file: &Path, args: &[String]) -> Result<()> {
    let compile_out = pipeline::compile_to_rust(file)?;
    
    // Build temp exe
    let exe_path = pipeline::compile_rust_to_exe(&compile_out.rust_file_path, &exe_stem, file)?;
    
    // Execute and capture output
    let output = Command::new(&exe_path).args(args).output()?;
    
    // Translate panic errors from .rs to .buff
    let translated = crate::error_mapper::translate_panic(&stderr_str, ...);
    
    // Cleanup exe and .rs
    remove_file_best_effort(&exe_path);
    remove_file_best_effort(&compile_out.rust_file_path);
}
`

### Where Runtime Would Plug In for Parallel Dispatch

1. **Codegen phase** (generate_rust): Add GPU shader generation in buff-lang-codegen-wgsl
2. **Cargo project generation** (deferred): Switch from single-file rustc to full Cargo project
3. **Runtime linking** (deferred): Add buff-lang-runtime to generated Cargo.toml
4. **Dispatch insertion** (codegen): Insert runtime calls for parallel operations
5. **CLI pipeline** (deferred): Emit both Rust source AND WGSL shaders, invoke runtime for GPU

**Current gap:** Pipeline uses direct rustc invocation on single .rs files - no Cargo project, no runtime crate linking. This is a v0.5 limitation (T29, T32 deferred).

## Key Integration Points for v1.0

1. **Complete buff-lang-runtime crate** with GPU dispatch logic (wgpu) and CPU parallelism (rayon)
2. **Complete buff-lang-codegen-wgsl crate** with AST?WGSL shader generation
3. **Add runtime calls to rust_codegen.rs** for parallel operations (par_map, GPU dispatch)
4. **Extend CLI pipeline** to emit Cargo projects and link runtime instead of single-file rustc
5. **Populate repr_c_struct_names** from GPU-dispatch auto-detection analysis
