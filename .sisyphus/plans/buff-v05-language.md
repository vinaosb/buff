# Buff v0.5 "Real Language" — Complete Type System + Modern Syntax

> **Phase 2 of 3.** Depends on [Phase 1 (v0.1)](./buff-v01-mvp.md) completion.
> Shared context: [Master Plan](./buff-master.md) | Numeric spec: [buff-numeric-system.md](./buff-numeric-system.md) | [Conventions](./buff-conventions.md) | [Project Structure](./buff-project-structure.md)

---

## TL;DR

> **Goal**: Transform Buff from MVP into a complete, usable language with all 13 types, pattern matching, modules, async, FFI, and modern syntax features.
>
> **Exit criteria**: `buff build` compiles multi-file programs with collections, enums, closures, error handling, and async I/O. `buff test` runs test suites. `buff fmt` formats code.
>
> **Tasks**: 20 core (T18-T37) + 27 enhancement = **47 tasks**
> **Waves**: 4 (Waves 5-8) + enhancement tasks within same waves

---

## Prerequisites

Phase 1 (v0.1) must be complete:
- Lexer, parser, AST, type checker, codegen all working
- `buff run` and `buff build` functional
- Int, Float, Double, Bool, String types supported
- CLI with source maps

---

## Core Tasks (T18-T37)

### Wave 5 — Remaining Primitive Types + Stdlib (parallel, depends on T10)

- [x] **T18**: Double (f64) full support [quick]

  **What to do** (TDD):
  - **RED**: Write test: `3.14d` infers as Double (f64), not Float (f32)
  - **RED**: Write test: `Double + Double → Double`, `Double + Float → Double` (widening)
  - **RED**: Write snapshot: `let x = 3.14d` → `let x: f64 = 3.14;`
  - **GREEN**: Add f64 type inference for `d` suffix literals in buff-lang-types
  - **GREEN**: Add codegen mapping: Double → Rust f64 in buff-lang-codegen-rust
  - **GREEN**: Implement widening: Float + Double → Double in type checker
  - **REFACTOR**: Extract suffix parsing into shared literal handler

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-types double_inference` passes
  - [ ] `cargo test -p buff-lang-codegen-rust double_codegen` passes
  - [ ] `3.14d` infers as f64, `3.14` infers as f32
  - [ ] Snapshot: `let x = 3.14d` → `let x: f64 = 3.14;`

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Double literal inference and codegen
    Tool: Bash (cargo test)
    Steps:
      1. Infer type of `3.14d` → assert Double (f64)
      2. Infer type of `3.14` → assert Float (f32)
      3. Codegen `let x = 3.14d` → assert snapshot `let x: f64 = 3.14;`
      4. Codegen `let y = 1.0d + 2.0` → assert type is Double (widening)
    Expected Result: d suffix produces f64, mixed ops widen to Double
    Evidence: .sisyphus/evidence/task-18-double-support.txt
  ```

  **Commit**: YES — Message: `feat(types): add Double (f64) type with d suffix inference`

- [x] **T19**: Byte (Bits<8>) support [quick]

  **What to do** (TDD):
  - **RED**: Write test: `0xFF` infers as Byte (u8), `0b1010` infers as Byte
  - **RED**: Write snapshot: `let b: Byte = 0xFF` → `let b: u8 = 0xFF;`
  - **RED**: Write test: Byte buffer operations: `buffer[0]`, `buffer.len()`
  - **GREEN**: Add Byte type mapping to Rust u8 in type checker and codegen
  - **GREEN**: Add hex (`0xFF`) and binary (`0b1010`) literal parsing in lexer
  - **GREEN**: Implement buffer indexing codegen: `buf[i]` → `buf[i as usize]`
  - **REFACTOR**: Group Byte with Bits<W> type family

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-lexer hex_binary_literals` passes
  - [ ] `cargo test -p buff-lang-types byte_type` passes
  - [ ] `0xFF` → Byte, `0b1010` → Byte
  - [ ] Snapshot: `let b: Byte = 0xFF` → `let b: u8 = 0xFF;`

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Hex and binary literals
    Tool: Bash (cargo test)
    Steps:
      1. Parse `0xFF` → assert Byte literal with value 255
      2. Parse `0b1010` → assert Byte literal with value 10
      3. Codegen `let b = 0xFF` → assert `let b: u8 = 255;`
    Expected Result: Hex/binary literals produce Byte values
    Evidence: .sisyphus/evidence/task-19-byte-literals.txt
  ```

  **Commit**: YES — Message: `feat(types): add Byte (Bits<8>) with hex and binary literals`

- [x] **T20**: Decimal (128-bit) — rust_decimal integration [deep]

  **What to do** (TDD):
  - **RED**: Write test: `99.90m` infers as Decimal, NOT Double or Float
  - **RED**: Write snapshot: `let price = 99.90m` → `let price = rust_decimal_macros::dec!(99.90);`
  - **RED**: Write test: `Decimal + Decimal → Decimal`, `Decimal * Decimal → Decimal`
  - **RED**: Write test: `0.1m + 0.2m == 0.3m` → TRUE (unlike Float where 0.1 + 0.2 ≠ 0.3)
  - **RED**: Write test: Decimal type forces CPU parallel (NOT GPU dispatch)
  - **GREEN**: Add Decimal type to buff-lang-types using `rust_decimal::Decimal`
  - **GREEN**: Add `m` suffix literal parsing in lexer → Decimal
  - **GREEN**: Codegen Decimal literals via `rust_decimal_macros::dec!()` macro
  - **GREEN**: Implement Decimal arithmetic codegen (maps to Rust Decimal ops)
  - **GREEN**: Mark Decimal as CPU-only in dispatch type checker (never GPU)
  - **REFACTOR**: Extract Decimal operations into trait impl block

  **Must NOT do**: NO GPU dispatch for Decimal (always CPU parallel via Rayon)

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-types decimal_type` passes (target: 12+ tests)
  - [ ] `99.90m` infers as Decimal
  - [ ] `0.1m + 0.2m == 0.3m` is TRUE (exact arithmetic)
  - [ ] Snapshot: `let price = 99.90m` → uses `dec!()` macro
  - [ ] Decimal marked CPU-only in type metadata

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Decimal exact arithmetic
    Tool: Bash (cargo test)
    Steps:
      1. Evaluate `0.1m + 0.2m` → assert result equals `0.3m` (not 0.30000000000000004)
      2. Evaluate `99.90m * 1.15m` → assert exact result
      3. Codegen both → compile → run → verify output
    Expected Result: Decimal arithmetic is exact, no floating point errors
    Evidence: .sisyphus/evidence/task-20-decimal-exact.txt

  Scenario: Decimal forces CPU dispatch
    Tool: Bash (cargo test)
    Steps:
      1. Analyze: `let prices: Vector<Decimal> = ...; prices.par_map({p => p * 1.1m})`
      2. Assert dispatch target = CpuParallel (NOT GpuCompute)
    Expected Result: Decimal data stays on CPU
    Evidence: .sisyphus/evidence/task-20-decimal-cpu-only.txt
  ```

  **Commit**: YES — Message: `feat(types): add Decimal (128-bit) with m suffix and rust_decimal integration`

- [x] **T21**: String + Char operations + interpolation [deep]

  **What to do** (TDD):
  - **RED**: Write snapshot: `"Hello {name}!"` → `format!("Hello {}!", name)`
  - **RED**: Write test: `'A'` parses as Char literal (single quotes), `"A"` parses as String
  - **RED**: Write test: `s.char_count()` on `"Hello"` → 5, `s.byte_len()` → 5
  - **RED**: Write test: `s.chars()` returns iterator of Char, `s.graphemes()` returns iterator of String
  - **RED**: Write test: `"café".chars()` yields 4 chars (é is one Unicode scalar)
  - **RED**: Write test: NO direct indexing `s[0]` → compile error
  - **GREEN**: Implement string interpolation codegen: `"text {expr}"` → `format!("text {}", expr)`
  - **GREEN**: Implement Char type: single-quote literals `'A'`, `'é'`, `'🚀'`, maps to Rust `char`
  - **GREEN**: Implement string methods: `.char_count()`, `.byte_len()`, `.chars()`, `.bytes()`, `.graphemes()`
  - **GREEN**: Implement `.first()` → Option<Char>, `.last()` → Option<Char>, `.slice(0..5)` → String
  - **GREEN**: Implement multi-line strings: `"""..."""` → Rust raw string
  - **GREEN**: Compile-time error for direct string indexing `s[0]`
  - **REFACTOR**: Extract grapheme segmentation into `unicode-segmentation` crate wrapper

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-codegen-rust string_methods` passes (target: 15+ tests)
  - [ ] `"Hello {name}"` → `format!("Hello {}", name)`
  - [ ] `'A'` → Char, `"A"` → String
  - [ ] `.char_count()`, `.byte_len()`, `.chars()`, `.graphemes()` all work
  - [ ] `s[0]` → compile error with message "use .chars() or .first() instead"
  - [ ] Multi-line `"""..."""` works

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: String interpolation codegen
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `let name = "World"; print("Hello {name}!")`
      2. Assert output uses `format!("Hello {}!", name)`
      3. Compile + run → assert "Hello World!" printed
    Expected Result: Interpolation generates format! macro
    Evidence: .sisyphus/evidence/task-21-string-interp.txt

  Scenario: Char vs String literals
    Tool: Bash (cargo test)
    Steps:
      1. Parse `'A'` → assert CharLiteral('A')
      2. Parse `"A"` → assert StringLiteral("A")
      3. Parse `'🚀'` → assert CharLiteral('🚀') (emoji is valid Unicode scalar)
    Expected Result: Single quotes = Char, double quotes = String
    Evidence: .sisyphus/evidence/task-21-char-vs-string.txt

  Scenario: No direct string indexing
    Tool: Bash (cargo test)
    Steps:
      1. Parse + check: `let s = "hello"; let c = s[0]`
      2. Assert CompileError: "strings cannot be indexed directly, use .chars() or .first()"
    Expected Result: Indexing rejected with helpful message
    Evidence: .sisyphus/evidence/task-21-no-indexing.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): add String+Char methods, interpolation, and grapheme iteration`

- [x] **T22**: Numeric coercion rules — flexible vs fixed modes [deep]

  **What to do** (TDD):
  - **RED**: Write test: `Int + Float → Float` (widen), `Int + Double → Double`
  - **RED**: Write test: flexible `Int` with value 5 → compiler picks `Int<8>` (smallest fitting)
  - **RED**: Write test: flexible `Int` with value 300 → compiler picks `Int<16>` (doesn't fit i8)
  - **RED**: Write test: `[20, 25, 18]` → `Vector<Int<8>>` (auto-width collection)
  - **RED**: Write test: `Int<32> + Int<32>` → `Int<32>` (fixed mode preserves type)
  - **RED**: Write test: overflow in fixed mode → panic in debug, wrap in release
  - **GREEN**: Implement range analysis: track (min, max) for flexible Int variables
  - **GREEN**: Implement auto-width: pick smallest Rust type fitting the range
  - **GREEN**: Implement widening rules: `Int + Float → Float`, `Float + Double → Double`
  - **GREEN**: Implement checked arithmetic for fixed `Int<W>` (debug panic, release wrap)
  - **REFACTOR**: Extract range tracker into `buff-lang-types/src/range_analysis.rs`

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-types numeric_coercion` passes (target: 15+ tests)
  - [ ] `Int + Float → Float`, `Float + Double → Double`
  - [ ] Flexible Int: value 5 → i8, value 300 → i16
  - [ ] Collection auto-width: `[20, 25]` → `Vector<Int<8>>`
  - [ ] Fixed `Int<32>` preserves type on all operations

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Auto-width collection optimization
    Tool: Bash (cargo test)
    Steps:
      1. Analyze: `let temps = [20, 25, 18, 30]`
      2. Assert inferred type: Vector<Int<8>> (all values fit in i8)
      3. Analyze: `let big = [100000, 200000]`
      4. Assert inferred type: Vector<Int<32>> (need i32 range)
    Expected Result: Compiler picks smallest width fitting all elements
    Evidence: .sisyphus/evidence/task-22-auto-width.txt

  Scenario: Fixed vs flexible overflow behavior
    Tool: Bash (cargo test)
    Steps:
      1. Codegen `let x: Int<8> = 127; let y = x + 1` → assert debug panic
      2. Codegen `let x: Int = 127; let y = x + 1` → assert auto-grows to Int<16>
    Expected Result: Fixed panics on overflow, flexible auto-grows
    Evidence: .sisyphus/evidence/task-22-overflow-modes.txt
  ```

  **Commit**: YES — Message: `feat(types): implement numeric coercion with flexible auto-width and fixed overflow modes`

- [x] **T96**: Standard library prelude [deep]

  **What to do** (TDD):
  - **RED**: Write test: `abs(-5)` returns 5 without import
  - **RED**: Write test: `min(3, 7)` returns 3, `max(3, 7)` returns 7
  - **RED**: Write test: `Int("42")` converts String to Int, `String(42)` converts Int to String
  - **RED**: Write test: `print("hello")` generates `println!("hello")` in Rust
  - **RED**: Write test: all prelude functions available without `import` statement
  - **GREEN**: Define prelude module with all built-in functions
  - **GREEN**: Implement math: `abs()`, `min()`, `max()`, `sqrt()`, `floor()`, `ceil()`, `round()`, `pow()`
  - **GREEN**: Implement type conversions: `Int(x)`, `Float(x)`, `String(x)`, `Bool(x)`
  - **GREEN**: Implement I/O: `print()` → `println!()`, `println()` → `println!()`, `read_line()`
  - **GREEN**: Auto-import prelude in every Buff program (no explicit import needed)
  - **REFACTOR**: Group prelude functions by category (math, conversion, io, collection)

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-types prelude_functions` passes (target: 12+ tests)
  - [ ] `abs(-5) == 5`, `min(3,7) == 3`, `max(3,7) == 7` — no import needed
  - [ ] `String(42)` → `"42"`, `Int("42")` → 42 — no import needed
  - [ ] `print("hello")` → generates `println!("hello")`

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Prelude functions work without import
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `func main(): print(abs(-5)); print(max(3, 7))` (NO imports)
      2. Compile + run → assert "5" and "7" printed
    Expected Result: All prelude functions available by default
    Evidence: .sisyphus/evidence/task-96-prelude.txt
  ```

  **Commit**: YES — Message: `feat(stdlib): add standard library prelude with math, conversion, and I/O functions`

- [x] **T99**: Process environment access [quick]

  **What to do** (TDD):
  - **RED**: Write test: `args()` returns Vector<String> with command-line arguments
  - **RED**: Write test: `env("HOME")` returns Option<String>
  - **RED**: Write test: `exit(1)` generates `std::process::exit(1)`
  - **GREEN**: Implement `args()` → `std::env::args().collect::<Vec<String>>()`
  - **GREEN**: Implement `env("NAME")` → `std::env::var("NAME").ok()` → Option<String>
  - **GREEN**: Implement `exit(code)` → `std::process::exit(code)`
  - **REFACTOR**: Group env functions in prelude

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-codegen-rust env_access` passes
  - [ ] `args()` returns Vector<String>
  - [ ] `env("PATH")` returns Option<String>
  - [ ] `exit(0)` generates `std::process::exit(0)`

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: CLI args accessible in Buff program
    Tool: Bash (buff CLI)
    Steps:
      1. Codegen: `func main(): let a = args(); print(a[0])`
      2. Run `buff run prog.buff -- hello`
      3. Assert output contains "hello"
    Expected Result: Command-line arguments accessible via args()
    Evidence: .sisyphus/evidence/task-99-cli-args.txt
  ```

  **Commit**: YES — Message: `feat(stdlib): add process environment access (args, env, exit)`

### Wave 6 — Collections + User Types (parallel, depends on Wave 5)

- [x] **T23**: Vector<T> type + codegen [deep]

  **What to do** (TDD):
  - **RED**: Write snapshot: `[1, 2, 3]` → `vec![1, 2, 3]`
  - **RED**: Write test: `v[0]` → `v[0 as usize]` (Int to usize conversion)
  - **RED**: Write test: `v.push(4)`, `v.len()`, `v.map({x => x * 2})`, `v.filter({x => x > 0})`
  - **RED**: Write test: auto-width `[1, 2, 3]` → `Vector<Int<8>>` (all fit i8)
  - **GREEN**: Implement Vector<T> type → maps to Rust `Vec<T>`
  - **GREEN**: Implement literal codegen: `[1, 2, 3]` → `vec![1, 2, 3]`
  - **GREEN**: Implement indexing: `v[i]` → `v[i as usize]`
  - **GREEN**: Implement methods: `.push()`, `.pop()`, `.len()`, `.map()`, `.filter()`, `.reduce()`
  - **GREEN**: Auto-detect collection element width (from T22 range analysis)
  - **REFACTOR**: Extract collection type family (Vector, Matrix, Map share patterns)

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-codegen-rust vector_codegen` passes (target: 12+ tests)
  - [ ] `[1, 2, 3]` → `vec![1, 2, 3]`
  - [ ] `v[i]` generates `v[i as usize]`
  - [ ] `.map()`, `.filter()`, `.reduce()` chain correctly

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Vector literal and method chain
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `let v = [1, 2, 3]; let d = v.map({x => x * 2}); print(d[0])`
      2. Compile + run → assert "2" printed
    Expected Result: Vector literal, map, and indexing work end-to-end
    Evidence: .sisyphus/evidence/task-23-vector-methods.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): add Vector<T> with literals, indexing, and iterator methods`

---

- [x] **T24**: Matrix<T> type + codegen [deep]

  **What to do** (TDD):
  - **RED**: Write test: `Matrix.new(3, 3)` creates 3x3 matrix
  - **RED**: Write test: `m[1, 2]` → `m.data[(1 * m.cols + 2) as usize]` (flat indexing)
  - **RED**: Write test: matrix is flat contiguous (GPU-ready check)
  - **GREEN**: Define `Matrix<T>` as `struct { data: Vec<T>, rows: usize, cols: usize }`
  - **GREEN**: Implement 2D indexing: `m[row, col]` → flat index `row * cols + col`
  - **GREEN**: Codegen to Rust struct with flat Vec storage
  - **REFACTOR**: Share flat-storage pattern with GPU buffer codegen

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-codegen-rust matrix_codegen` passes (target: 8+ tests)
  - [ ] `m[1, 2]` generates correct flat index
  - [ ] Matrix data is contiguous (no nesting) — GPU-transferable

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Matrix 2D indexing
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `let m = Matrix.new(2, 3); m[1, 2] = 42`
      2. Assert flat index: 1*3 + 2 = 5 → `m.data[5] = 42`
    Expected Result: 2D index correctly maps to flat storage
    Evidence: .sisyphus/evidence/task-24-matrix-indexing.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): add Matrix<T> with flat 2D storage and indexing`

---

- [x] **T25**: Map<K,V> type + codegen [deep]

  **What to do** (TDD):
  - **RED**: Write snapshot: `{"key": 42}` → `HashMap::from([("key", 42)])`
  - **RED**: Write test: `m.get("key")` → `m.get("key")` returns Option
  - **RED**: Write test: `m.insert("k", 1)`, `m.contains("k")`, `m.remove("k")`
  - **GREEN**: Implement Map<K,V> → Rust `HashMap<K, V>`
  - **GREEN**: Implement literal: `{"k": v}` → `HashMap::from([("k", v)])`
  - **GREEN**: Implement methods: `.get()`, `.insert()`, `.contains()`, `.remove()`, `.len()`
  - **REFACTOR**: Extract collection literal parsing shared with Vector

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-codegen-rust map_codegen` passes (target: 8+ tests)
  - [ ] `{"key": 42}` → `HashMap::from([("key", 42)])`
  - [ ] `.get()` returns Option<V>, `.insert()`, `.contains()` work

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Map literal and access
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `let m = {"name": "Alice", "age": 30}; print(m.get("name"))`
      2. Compile + run → assert "Some(Alice)" or "Alice" printed
    Expected Result: Map literal creates HashMap, get returns value
    Evidence: .sisyphus/evidence/task-25-map-access.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): add Map<K,V> with HashMap codegen and collection literal`

---

- [x] **T26**: Struct type + repr(C) codegen [deep]

  **What to do** (TDD):
  - **RED**: Write snapshot: `struct Person { name: String, age: Int }` → Rust struct with `#[derive(Clone, Debug)]`
  - **RED**: Write test: `Person { name: "Alice", age: 30 }` → struct init expression
  - **RED**: Write test: field access `p.name` → `p.name`
  - **RED**: Write test: struct used in par_map → `#[repr(C)]` auto-added
  - **GREEN**: Implement struct declaration → Rust struct with auto-derived Clone, Debug
  - **GREEN**: Implement struct init: `Type { field: value }` → Rust struct expression
  - **GREEN**: Implement field access codegen: `obj.field` → `obj.field`
  - **GREEN**: Auto-add `#[repr(C)]` when struct is in GPU dispatch context
  - **REFACTOR**: Extract derive attribute generation into shared codegen helper

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-codegen-rust struct_codegen` passes (target: 10+ tests)
  - [ ] Struct declaration → Rust struct with `#[derive(Clone, Debug)]`
  - [ ] Init syntax: `Person { name: "X", age: 30 }` → valid Rust
  - [ ] Field access: `p.name` → `p.name`

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Struct declaration and init
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `struct Point { x: Float, y: Float }`
      2. Assert output: `#[derive(Clone, Debug)] pub struct Point { pub x: f32, pub y: f32 }`
      3. Codegen: `let p = Point { x: 1.0, y: 2.0 }`
      4. Assert: valid Rust struct init
    Expected Result: Struct generates valid Rust with derives
    Evidence: .sisyphus/evidence/task-26-struct-codegen.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): add Struct type with auto-derive and field access`

---

- [x] **T27**: Enum type + pattern matching [deep]

  **What to do** (TDD):
  - **RED**: Write test: `enum Color { Red, Green, Blue }` → Rust enum
  - **RED**: Write test: `match c { Red => 1, Green => 2, Blue => 3 }` → Rust match
  - **RED**: Write test: missing case → compile error "non-exhaustive match"
  - **RED**: Write test: `_` wildcard catches remaining cases
  - **RED**: Write test: `enum Result<T,E> { Ok(T), Err(E) }` → generic enum with data
  - **RED**: Write test: `match r { Ok(v) => v, Err(e) => 0 }` → pattern with binding
  - **GREEN**: Implement enum declaration → Rust enum (simple variants + data-carrying)
  - **GREEN**: Implement match expression → Rust match with `=>` arms
  - **GREEN**: Implement exhaustiveness checking (compiler error if cases missing)
  - **GREEN**: Implement `_` wildcard arm
  - **GREEN**: Implement pattern bindings: `Ok(v) => ...` binds inner value
  - **REFACTOR**: Extract exhaustiveness checker into reusable analysis pass

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-parser enum_match` passes (target: 12+ tests)
  - [ ] `cargo test -p buff-lang-types exhaustiveness` passes
  - [ ] Simple enum: `Color { Red, Green, Blue }` → valid Rust enum
  - [ ] Match with `_` wildcard compiles
  - [ ] Missing case → "non-exhaustive match" error
  - [ ] Data enum: `Ok(T)`, `Err(E)` → Rust generic enum

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Exhaustive match
    Tool: Bash (cargo test)
    Steps:
      1. Parse + check:
         ```
         enum Light { Red, Green, Yellow }
         let l = Light.Red
         match l
             Red => "stop"
             Green => "go"
         ```
      2. Assert CompileError: "non-exhaustive match: missing Yellow"
      3. Add `Yellow => "slow"` → assert compiles
    Expected Result: Exhaustiveness enforced, all cases required
    Evidence: .sisyphus/evidence/task-27-exhaustive-match.txt

  Scenario: Match with data binding
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `match result { Ok(v) => v, Err(_) => 0 }`
      2. Assert Rust output: `match result { Ok(v) => v, Err(_) => 0 }`
    Expected Result: Pattern bindings correctly transpile
    Evidence: .sisyphus/evidence/task-27-match-binding.txt
  ```

  **Commit**: YES — Message: `feat(types): add Enum with exhaustive pattern matching and data variants`

---

- [x] **T28**: Option<T> + null safety [deep]

  **What to do** (TDD):
  - **RED**: Write test: `None` is valid Option<T> value (prelude enum variant)
  - **RED**: Write test: `Some(42)` wraps value in Option
  - **RED**: Write test: using `Option<Int>` as `Int` without checking → compile error
  - **RED**: Write test: `if let Some(x) = opt { use x }` unwraps safely
  - **RED**: Write test: `opt ?? 0` provides default for None (T101 null coalescing)
  - **GREEN**: Define Option<T> as built-in enum: `enum Option<T> { Some(T), None }`
  - **GREEN**: Auto-import None/Some from prelude (NOT keywords — prelude enum variants)
  - **GREEN**: Implement null safety: Option<T> cannot be used as T without unwrap/check
  - **GREEN**: Codegen: `None` → `None`, `Some(x)` → `Some(x)`, maps to Rust Option
  - **REFACTOR**: Ensure None/Some are prelude imports, NOT reserved keywords

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-types option_null_safety` passes (target: 10+ tests)
  - [ ] `None` and `Some(x)` work as Option variants
  - [ ] Using `Option<Int>` as `Int` → compile error
  - [ ] `None`/`Some` are NOT in reserved keyword list (they're prelude)

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Null safety enforcement
    Tool: Bash (cargo test)
    Steps:
      1. Check: `let x: Option<Int> = None; let y: Int = x` (assign Option to Int)
      2. Assert CompileError: "expected Int, found Option<Int>. Use if-let or ?? to unwrap."
    Expected Result: Cannot use Option as bare type without unwrapping
    Evidence: .sisyphus/evidence/task-28-null-safety.txt
  ```

  **Commit**: YES — Message: `feat(types): add Option<T> with null safety enforcement`

### Wave 7 — Modules + Async + FFI (parallel, depends on Wave 6)

- [x] **T29**: Module system (import/export, multi-file, path resolution) [deep]

  **What to do** (TDD):
  - **RED**: Write test: `import { greet } from "./hello.buff"` resolves to file, finds `export func greet`
  - **RED**: Write test: circular import (A imports B, B imports A) → compile error
  - **RED**: Write test: `export func public()` is visible to importers, `func private()` is NOT
  - **RED**: Write test: `export * from "./other"` re-exports all public symbols
  - **RED**: Write test: path `./utils/math.buff` resolves relative to importing file
  - **GREEN**: Implement import/export parsing in lexer/parser
  - **GREEN**: Implement module resolution: find file from import path, parse it
  - **GREEN**: Implement visibility: `export` = public, default = module-private
  - **GREEN**: Implement circular dependency detection (topological sort, cycle = error)
  - **GREEN**: Implement path resolution: relative (`./`), std (`std/`), canonicalization
  - **REFACTOR**: Extract module graph into `buff-lang-types/src/modules.rs`

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-parser module_system` passes (target: 12+ tests)
  - [ ] Multi-file program compiles: `main.buff` imports from `./utils.buff`
  - [ ] Circular import detected and rejected
  - [ ] `export` makes symbol public; non-exported = private
  - [ ] `export *` re-exports work

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Multi-file program compiles
    Tool: Bash (buff CLI)
    Steps:
      1. Create main.buff: `import { greet } from "./hello.buff"; func main(): greet()`
      2. Create hello.buff: `export func greet(): print("Hello from module!")`
      3. Run `buff run main.buff`
      4. Assert output: "Hello from module!"
    Expected Result: Module system resolves imports and links files
    Evidence: .sisyphus/evidence/task-29-multi-file.txt

  Scenario: Circular import detected
    Tool: Bash (cargo test)
    Steps:
      1. a.buff imports from b.buff, b.buff imports from a.buff
      2. Run `buff build a.buff`
      3. Assert error: "circular import detected: a.buff → b.buff → a.buff"
    Expected Result: Circular dependency caught at compile time
    Evidence: .sisyphus/evidence/task-29-circular.txt
  ```

  **Commit**: YES — Message: `feat(parser): implement module system with import/export and path resolution`

---

- [x] **T30**: Error types + `?` operator [deep]

  **What to do** (TDD):
  - **RED**: Write test: `Result<T, E>` type works as enum with Ok/Err variants
  - **RED**: Write test: `let x = operation()?` propagates Err (early return on error)
  - **RED**: Write test: `return Error("msg")` creates and returns an error
  - **RED**: Write test: custom error enum: `enum MyError { NotFound, Invalid(String) }`
  - **RED**: Write snapshot: `func read(): Result<String, Error> { return Error("fail") }` → Rust Result
  - **GREEN**: Define `Result<T, E>` as built-in enum: `Ok(T)`, `Err(E)` (prelude)
  - **GREEN**: Implement `?` operator: `expr?` → match expr { Ok(v) => v, Err(e) => return Err(e) }
  - **GREEN**: Implement `return Error("msg")` → `return Err(Error::new("msg"))`
  - **GREEN**: Allow user-defined error enums with automatic `Error` trait derivation
  - **REFACTOR**: Extract error propagation into codegen visitor

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-codegen-rust error_handling` passes (target: 10+ tests)
  - [ ] `?` operator generates correct early-return pattern
  - [ ] `Result<T, E>` maps to Rust `Result<T, E>`
  - [ ] Custom error enums work

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: ? operator propagation
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `func process(): Result<Int, Error> { let x = read_data()?; return x }`
      2. Assert Rust output has early return on Err
      3. Compile + run → assert correct behavior
    Expected Result: ? operator produces correct error propagation
    Evidence: .sisyphus/evidence/task-30-error-propagation.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): add Result type with ? operator and error propagation`

---

- [x] **T31**: Async with call graph propagation [deep]

  **What to do** (TDD):
  - **RED**: Write test: `async func http_get(url)` is marked as I/O boundary in call graph
  - **RED**: Write test: `func fetch()` that calls `http_get()` → auto-marked as async (propagation)
  - **RED**: Write test: `func main()` that calls `fetch()` → auto-marked as async (transitive)
  - **RED**: Write test: function that does NOT call any async → remains sync
  - **RED**: Write test: `spawn task()` → generates `tokio::spawn(async move { task() })`
  - **RED**: Write test: `task.result()` → generates `.await` in Rust (auto-suspension)
  - **RED**: Write test: `block(expr)` in sync context → generates `runtime.block_on(expr)`
  - **RED**: Write test: `block()` inside async function → compiler warning (deadlock risk)
  - **GREEN**: Build call graph after type checking
  - **GREEN**: Implement async propagation: fixpoint algorithm marking callers of async functions
  - **GREEN**: Auto-insert suspension points at async call sites in codegen
  - **GREEN**: Generate `#[tokio::main]` for main function (auto-async)
  - **GREEN**: Implement `spawn` → `tokio::spawn`, `Task<T>` → `JoinHandle<T>`
  - **GREEN**: Implement `task.result()` → `.await` (the ONLY place await appears in generated Rust)
  - **GREEN**: Implement `block(expr)` → `tokio::runtime::Runtime::block_on(expr)`
  - **REFACTOR**: Extract call graph + propagation into `buff-lang-types/src/async_analysis.rs`

  **Must NOT do**: NO `await` keyword in Buff source. The word `await` only appears in generated Rust.

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-types async_propagation` passes (target: 15+ tests)
  - [ ] `cargo test -p buff-lang-codegen-rust async_codegen` passes
  - [ ] Call graph propagation marks all transitive callers of async functions
  - [ ] Sync function calling async → auto-becomes async
  - [ ] `spawn` generates `tokio::spawn`
  - [ ] `task.result()` generates `.await`
  - [ ] `func main()` gets `#[tokio::main]` annotation
  - [ ] `block()` in async → warning

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Async propagation through call chain
    Tool: Bash (cargo test)
    Steps:
      1. Define: `async func io() { ... }`
      2. Define: `func business() { io() }` (no async keyword)
      3. Define: `func main() { business() }` (no async keyword)
      4. Assert: business() is auto-marked async (calls io)
      5. Assert: main() is auto-marked async (calls business)
      6. Assert: generated main has #[tokio::main]
    Expected Result: Async propagates transitively, user writes NO async/await
    Evidence: .sisyphus/evidence/task-31-async-propagation.txt

  Scenario: spawn and task.result()
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `let t = spawn background_work(); let r = t.result()`
      2. Assert Rust: `let t = tokio::spawn(async move { background_work() }); let r = t.await`
    Expected Result: spawn creates task, result() awaits it
    Evidence: .sisyphus/evidence/task-31-spawn-result.txt
  ```

  **Commit**: YES — Message: `feat(types): implement async call graph propagation with auto-suspension`

---

- [x] **T32**: FFI basics — import Rust crates [unspecified-high]

  **What to do** (TDD):
  - **RED**: Write test: `extern crate "serde"` → adds `serde` to generated Cargo.toml + `use serde;`
  - **RED**: Write test: type mapping: Buff `String` → Rust `String`, Buff `Int` → Rust `i64`
  - **RED**: Write test: calling extern function: `extern func rust_fn(x: Int) -> Int` → Rust `unsafe extern "C" fn`
  - **GREEN**: Implement `extern crate "name"` → add dependency to generated Cargo.toml
  - **GREEN**: Implement `extern func` declarations → Rust FFI function signatures
  - **GREEN**: Implement type mapping table: Buff types ↔ Rust types
  - **REFACTOR**: Extract type mapping into configurable table

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-codegen-rust ffi` passes (target: 8+ tests)
  - [ ] `extern crate "serde"` → dependency in Cargo.toml
  - [ ] `extern func` → valid Rust FFI declaration
  - [ ] Type mapping correct for all 13 types

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: FFI crate import
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `extern crate "serde"`
      2. Assert generated Cargo.toml contains `serde = "..."`
      3. Assert generated Rust contains appropriate `use serde::*;`
    Expected Result: Rust crate made available in Buff program
    Evidence: .sisyphus/evidence/task-32-ffi-crate.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): add FFI basics for importing Rust crates`

---

- [x] **T33**: intelligent clone analysis [ultrabrain]

  **What to do** (TDD):
  - **RED**: Write test: `let s = "hi"; use(s); use(s)` → second use gets `.clone()` (use after move)
  - **RED**: Write test: `let v = [1, 2]; let v2 = v; use(v)` → `v` used after move to `v2` → clone at move
  - **RED**: Write test: `let x = 42; let y = x; use(x)` → NO clone needed (Int is Copy, not Move)
  - **RED**: Write test: shared across spawn → `Arc::new()` wrapper inserted
  - **RED**: Write test: mutation of Arc-shared data → `Arc::make_mut()` inserted (CoW)
  - **GREEN**: Implement use-after-move analysis: track ownership of each binding
  - **GREEN**: Auto-insert `.clone()` when moved binding is used again (only for non-Copy types)
  - **GREEN**: Skip `.clone()` for Copy types (Int, Float, Bool, Byte, Char — these are copied, not moved)
  - **GREEN**: Insert `Arc::new()` for data shared across `spawn` boundaries
  - **GREEN**: Insert `Arc::make_mut()` when Arc-shared data is mutated (Copy-on-Write)
  - **REFACTOR**: Extract ownership analysis into `buff-lang-types/src/ownership.rs`

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-codegen-rust clone_analysis` passes (target: 12+ tests)
  - [ ] String used after move → `.clone()` inserted
  - [ ] Int used after move → NO clone (Copy type)
  - [ ] Shared across spawn → `Arc::new()` + `Arc::clone()`
  - [ ] Mutated Arc data → `Arc::make_mut()`
  - [ ] Generated Rust compiles without borrow checker errors

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Clone on use-after-move
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `let s = "hello"; consume(s); print(s)`
      2. Assert: `s` is cloned before second use: `consume(s.clone()); print(s)`
      3. Compile generated Rust → success (no "use of moved value")
    Expected Result: Clone inserted only where needed
    Evidence: .sisyphus/evidence/task-33-clone-insertion.txt

  Scenario: Copy type needs no clone
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `let x = 42; let y = x; print(x)`
      2. Assert: NO `.clone()` in output (Int is Copy)
    Expected Result: Copy types don't need clone
    Evidence: .sisyphus/evidence/task-33-copy-no-clone.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): implement intelligent clone analysis with Arc/CoW`

### Wave 8 — Closures + Polish (parallel, depends on Wave 7)

- [x] **T34**: Closures/lambdas codegen [deep]

  **What to do** (TDD):
  - **RED**: Write snapshot: `{ x => x * 2 }` → Rust `|x| x * 2`
  - **RED**: Write test: `{ x, y => x + y }` → `|x, y| x + y`
  - **RED**: Write test: closure captures external variable: `let f = 10; [1,2,3].map({ x => x + f })` → captures `f`
  - **GREEN**: Implement closure parsing: `{ params => body }` syntax
  - **GREEN**: Codegen: Buff `{ x => expr }` → Rust `|x| expr`
  - **GREEN**: Implement variable capture: analyze free variables, generate captures
  - **REFACTOR**: Extract capture analysis (shared with T33 clone analysis)

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-codegen-rust closures` passes (target: 8+ tests)
  - [ ] `{ x => x * 2 }` → `|x| x * 2`
  - [ ] Multi-arg closures work
  - [ ] Variable capture generates correct Rust

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Closure with capture
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `let f = 10; let r = [1, 2, 3].map({ x => x + f })`
      2. Assert Rust captures `f`: `let f = 10; let r = vec![1,2,3].iter().map(|x| x + f).collect()`
    Expected Result: Closure captures external variable correctly
    Evidence: .sisyphus/evidence/task-34-closure-capture.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): implement closures with variable capture`

---

- [x] **T35**: `buff test` command [unspecified-high]

  **What to do** (TDD):
  - **RED**: Write test: `@test func test_addition()` is discovered by `buff test`
  - **RED**: Write test: `buff test` runs all `@test` functions and reports pass/fail
  - **RED**: Write test: `buff test --pattern "test_*"` filters by name pattern
  - **RED**: Write test: failing test → exit code 1, output shows failure + source location
  - **GREEN**: Implement `@test` attribute parsing
  - **GREEN**: Implement test discovery: scan AST for `@test` functions
  - **GREEN**: Generate Rust test harness: `#[test]` annotations, test runner
  - **GREEN**: Implement `--pattern` flag for filtering
  - **REFACTOR**: Extract test runner into reusable module

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-cli test_command` passes (target: 8+ tests)
  - [ ] `@test` functions discovered and run
  - [ ] Pass/fail count reported
  - [ ] `--pattern` filtering works
  - [ ] Exit code: 0 if all pass, 1 if any fail

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: buff test discovers and runs tests
    Tool: Bash (buff CLI)
    Steps:
      1. Create file with: `@test func test_basic(): assert_eq(1 + 1, 2)`
      2. Run `buff test`
      3. Assert exit code 0
      4. Assert output contains "1 passed, 0 failed"
    Expected Result: Test runner discovers and executes @test functions
    Evidence: .sisyphus/evidence/task-35-buff-test.txt
  ```

  **Commit**: YES — Message: `feat(cli): implement buff test command with @test discovery`

---

- [x] **T36**: Error message improvements + parser error recovery [unspecified-high]

  **What to do** (TDD):
  - **RED**: Write test: type error shows source line with caret (^) pointing to error location
  - **RED**: Write test: misspelled identifier `pritn` → "Did you mean `print`?" suggestion
  - **RED**: Write test: two syntax errors in same file → BOTH reported (not just first)
  - **RED**: Write test: parser recovers after error (skips to sync token, continues)
  - **RED**: Snapshot test: 5 error messages produce stable output
  - **GREEN**: Implement error span rendering: source line + caret indicator
  - **GREEN**: Implement Levenshtein distance for "Did you mean?" suggestions
  - **GREEN**: Implement parser error recovery: skip tokens until sync point (func, let, match, newline)
  - **GREEN**: Collect multiple errors before reporting (don't stop at first)
  - **REFACTOR**: Extract error rendering into `buff-lang-error/src/diagnostic.rs`

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lang-error error_messages` passes (target: 10+ tests)
  - [ ] Errors show source line + caret
  - [ ] "Did you mean?" suggestions for close matches
  - [ ] Multiple errors reported in one pass
  - [ ] Parser recovers and continues after first error
  - [ ] 5 error message snapshots stable

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Multiple errors in one pass
    Tool: Bash (cargo test)
    Steps:
      1. Parse file with 2 syntax errors on different lines
      2. Assert BOTH errors reported (not just first)
      3. Assert each error has correct line number
    Expected Result: Error recovery finds all errors
    Evidence: .sisyphus/evidence/task-36-multi-error.txt

  Scenario: Did you mean suggestion
    Tool: Bash (cargo test)
    Steps:
      1. Parse: `pritn("hello")` (misspelled print)
      2. Assert diagnostic contains "Did you mean `print`?"
    Expected Result: Typo suggestion helps user
    Evidence: .sisyphus/evidence/task-36-did-you-mean.txt
  ```

  **Commit**: YES — Message: `feat(error): improve error messages with spans, suggestions, and parser recovery`

---

- [x] **T37**: v0.5 milestone — comprehensive example suite [deep]

  **What to do** (TDD):
  - **RED**: Write integration tests: each example compiles and runs correctly
  - **GREEN**: Create examples: collections, enums, pattern matching, async, modules, closures, error handling
  - **GREEN**: Create `examples/collections.buff` — Vector, Matrix, Map usage
  - **GREEN**: Create `examples/pattern_matching.buff` — exhaustive match, Option handling
  - **GREEN**: Create `examples/async_demo.buff` — async propagation, spawn, task.result()
  - **GREEN**: Create `examples/modules/` — multi-file program with imports
  - **GREEN**: Create `examples/error_handling.buff` — Result, ? operator, custom errors
  - **GREEN**: Update README with v0.5 features
  - **GREEN**: Tag `v0.5.0`

  **Acceptance Criteria**:
  - [ ] All example programs pass `buff run`
  - [ ] `cargo test --workspace` → 100% pass
  - [ ] `cargo clippy --workspace -- -D warnings` → clean
  - [ ] Git tag `v0.5.0` created

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: All v0.5 examples work
    Tool: Bash (buff CLI)
    Steps:
      1. Run `buff run examples/collections.buff` → correct output
      2. Run `buff run examples/pattern_matching.buff` → correct output
      3. Run `buff run examples/async_demo.buff` → correct output
      4. Run `buff run examples/error_handling.buff` → correct output
      5. Run `cargo test --workspace` → 0 failures
      6. Run `cargo clippy --workspace -- -D warnings` → clean
    Expected Result: All v0.5 features working end-to-end
    Evidence: .sisyphus/evidence/task-37-v05-milestone.txt
  ```

  **Commit**: YES — Message: `release: v0.5.0 "Real Language" — complete type system, modules, async, modern syntax`

---

## Enhancement Tasks (Modern Syntax — from Best Practices Research)

### Wave 5 Enhancement — Modern Syntax Sugar (parallel with Wave 5)

- [x] **T67**: Collection literals [deep]
  **What to do** (TDD): RED: snapshot `[1,2,3]` → `vec![1,2,3]`, `{"k":v}` → `HashMap::from`. GREEN: parse collection literals, codegen to vec!/HashMap::from.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust collection_literals` passes. `[1,2,3]` → `vec![1,2,3]`.
  **QA**: Codegen `[1,2,3]` → assert `vec![1, 2, 3]` in output. Evidence: task-67-collection-literals.txt
  **Commit**: `feat(parser): add collection literals for Vector and Map`

- [x] **T68**: Range syntax [quick]
  **What to do** (TDD): RED: `0..10` → exclusive range, `0..=10` → inclusive. GREEN: parse range operators, codegen to Rust ranges.
  **Acceptance**: `cargo test -p buff-lang-parser ranges` passes. `0..10` → Rust `0..10`.
  **QA**: Parse `for i in 0..5` → assert range expression in AST. Evidence: task-68-ranges.txt
  **Commit**: `feat(parser): add range syntax 0..10 and 0..=10`

- [x] **T69**: Pipeline operator `|>` [deep]
  **What to do** (TDD): RED: `data |> process() |> filter()` → `filter(process(data))`. GREEN: parse `|>` operator, desugar to nested calls in codegen.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust pipeline` passes. `x |> f()` → `f(x)`.
  **QA**: Codegen `"hello" |> print()` → assert `print("hello")`. Evidence: task-69-pipeline.txt
  **Commit**: `feat(parser): add pipeline operator |>`

- [x] **T70**: Null-conditional `?.` [deep]
  **What to do** (TDD): RED: `user?.name` → Option chain with short-circuit. GREEN: parse `?.` operator, codegen to `.and_then()` chain.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust null_conditional` passes. `u?.name` → `u.and_then(|x| x.name)`.
  **QA**: Codegen `opt?.value` → assert `.and_then()` in output. Evidence: task-70-null-conditional.txt
  **Commit**: `feat(parser): add null-conditional operator ?.`

- [x] **T71**: Destructuring assignment [deep]
  **What to do** (TDD): RED: `let (x, y) = point` → tuple destructuring. `let Point { x, y } = p` → struct destructuring. GREEN: parse destructuring patterns in let, codegen to Rust.
  **Acceptance**: `cargo test -p buff-lang-parser destructuring` passes. Both tuple and struct destructuring work.
  **QA**: Codegen `let (a, b) = pair` → assert Rust destructuring. Evidence: task-71-destructuring.txt
  **Commit**: `feat(parser): add destructuring assignment for tuples and structs`

- [x] **T102**: Expression functions `=>` [quick]
  **What to do** (TDD): RED: `func double(x) => x * 2` → shorthand single-expression function. GREEN: parse `=>` in function decl, codegen as function with return.
  **Acceptance**: `cargo test -p buff-lang-parser expr_functions` passes. `func f(x) => x + 1` works.
  **QA**: Parse `func sq(x: Int) => x * x` → assert FuncDecl with expression body. Evidence: task-102-expr-fn.txt
  **Commit**: `feat(parser): add expression function shorthand =>`

- [x] **T104**: Raw strings [quick]
  **What to do** (TDD): RED: `r"\d+"` → literal backslashes (no escape processing). GREEN: parse `r"..."` prefix in lexer, codegen to Rust raw string.
  **Acceptance**: `cargo test -p buff-lang-lexer raw_strings` passes. `r"\n"` → backslash-n literal (NOT newline).
  **QA**: Parse `r"C:\path"` → assert backslashes preserved. Evidence: task-104-raw-strings.txt
  **Commit**: `feat(lexer): add raw string literals r"..."`

### Wave 6 Enhancement — Advanced Control Flow + Types (parallel with Wave 6)

- [x] **T72**: If-let / For-let [deep]
  **What to do** (TDD): RED: `if let Some(x) = opt` → conditional binding. `for let Some(x) = iter.next()` → looping binding. GREEN: parse let-patterns in if/for, codegen to Rust if let / while let.
  **Acceptance**: `cargo test -p buff-lang-parser let_bindings` passes. Both if-let and for-let work.
  **QA**: Codegen `if let Some(x) = opt { print(x) }` → assert Rust `if let`. Evidence: task-72-if-let.txt
  **Commit**: `feat(parser): add if-let and for-let pattern bindings`

- [x] **T73**: Early return guards [deep]
  **What to do** (TDD): RED: `guard let Some(x) = opt, x > 0 else { return }` → early exit if condition fails. GREEN: parse `guard` keyword, codegen to inverted if + early return.
  **Acceptance**: `cargo test -p buff-lang-parser guards` passes. Guard with multiple conditions works.
  **QA**: Codegen `guard x > 0 else { return 0 }` → assert early return pattern. Evidence: task-73-guards.txt
  **Commit**: `feat(parser): add guard statement for early returns`

- [x] **T74**: Let chains [deep]
  **What to do** (TDD): RED: `if let Some(x) = opt, let Some(y) = opt2, x > 0 { }` → flat conditions. GREEN: parse comma-separated let conditions, codegen to nested if-lets.
  **Acceptance**: `cargo test -p buff-lang-parser let_chains` passes. Multiple let conditions in one if.
  **QA**: Parse `if let Some(a) = x, let Some(b) = y, a > b { }` → assert AST has chain. Evidence: task-74-let-chains.txt
  **Commit**: `feat(parser): add let chains for flat conditional binding`

- [x] **T75**: Extension methods [deep]
  **What to do** (TDD): RED: `extend String { fn is_email() -> Bool { ... } }` → adds method to String type. GREEN: parse `extend` blocks, codegen to Rust trait + impl.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust extensions` passes. `"x".is_email()` calls extension.
  **QA**: Codegen `extend String { fn shout(self) -> String { self.to_uppercase() } }` → assert trait+impl. Evidence: task-75-extensions.txt
  **Commit**: `feat(parser): add extension methods via extend blocks`

- [x] **T76**: Union types `A | B` [deep]
  **What to do** (TDD): RED: `String | Int` as a type, match to discriminate. GREEN: parse `|` in type position, create auto-generated enum wrapper, codegen match.
  **Acceptance**: `cargo test -p buff-lang-types union_types` passes. `String | Int` usable as parameter type.
  **QA**: Codegen `func process(x: String | Int)` → assert generated enum wrapper. Evidence: task-76-union-types.txt
  **Commit**: `feat(types): add union types A | B with pattern discrimination`

- [x] **T77**: Expected-type driven inference [deep]
  **What to do** (TDD): RED: `items.map({ x => x * 2 })` — infer x type from items element type. GREEN: propagate expected types into lambda parameters during inference.
  **Acceptance**: `cargo test -p buff-lang-types expected_type_inference` passes. Lambda params inferred from context.
  **QA**: Infer `{ x => x * 2 }` in context of `Vector<Float>.map()` → assert x: Float. Evidence: task-77-expected-type.txt
  **Commit**: `feat(types): add expected-type driven inference for lambda parameters`

- [x] **T92**: Struct embedding + delegation [deep]
  **What to do** (TDD): RED: `struct Employee { person: Person, salary: Float }` — `employee.name()` auto-delegates to `employee.person.name()`. GREEN: analyze struct fields, auto-generate delegation methods for embedded types.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust embedding` passes. Embedded struct methods promoted.
  **QA**: Codegen Employee with embedded Person → assert `employee.name()` delegates. Evidence: task-92-embedding.txt
  **Commit**: `feat(codegen-rust): add struct embedding with auto-delegation`

- [x] **T93**: Traits with default methods [deep]
  **What to do** (TDD): RED: `trait Greetable { fn name() -> String; fn greet() { print(name()) } }` — default impl uses required method. GREEN: parse trait keyword, default method bodies, codegen to Rust trait.
  **Acceptance**: `cargo test -p buff-lang-parser traits` passes. Traits with defaults and inheritance (`trait Pet : Animal`).
  **QA**: Codegen trait with default method → assert Rust trait with default impl. Evidence: task-93-traits.txt
  **Commit**: `feat(parser): add traits with default methods and inheritance`

- [x] **T101**: Null coalescing `??` [quick]
  **What to do** (TDD): RED: `opt ?? "default"` → `.unwrap_or("default")`. GREEN: parse `??` operator, codegen to unwrap_or.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust null_coalescing` passes. `opt ?? 0` → `opt.unwrap_or(0)`.
  **QA**: Codegen `name ?? "unknown"` → assert `.unwrap_or()`. Evidence: task-101-coalescing.txt
  **Commit**: `feat(parser): add null coalescing operator ??`

- [x] **T103**: Tuples [deep]
  **What to do** (TDD): RED: `(String, Int)` as type, `(name, age)` as value. GREEN: parse tuple types and literals, codegen to Rust tuples.
  **Acceptance**: `cargo test -p buff-lang-types tuples` passes. `(String, Int)` works as return type.
  **QA**: Codegen `func pair() -> (String, Int) { return ("A", 42) }` → assert Rust tuple. Evidence: task-103-tuples.txt
  **Commit**: `feat(types): add tuple types and multi-return`

- [x] **T107**: Auto-derived record methods [deep]
  **What to do** (TDD): RED: struct auto-generates equals, hash, to_string, copy. `p1.copy(age: 31)` → immutable update. GREEN: auto-derive Clone, PartialEq, Hash, Debug for structs in codegen.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust record_derives` passes. Structs get auto-equals and copy.
  **QA**: Codegen struct → assert `#[derive(Clone, PartialEq, Hash, Debug)]`. Evidence: task-107-record-derives.txt
  **Commit**: `feat(codegen-rust): auto-derive record methods (equals, hash, copy)`

### Wave 7 Enhancement — Error Context + Advanced Features (parallel with Wave 7)

- [x] **T78**: Error context chaining [deep]
  **What to do** (TDD): RED: `.context("msg")?` wraps error with context. GREEN: implement context method on Result, codegen to error wrapping.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust error_context` passes. `.context("msg")?` adds context.
  **QA**: Codegen `read_file()?.context("config load")` → assert error chain. Evidence: task-78-error-context.txt
  **Commit**: `feat(codegen-rust): add error context chaining .context()`

- [x] **T79**: Regex literals [deep]
  **What to do** (TDD): RED: `/\d{3}/` → `Regex::new(r"\d{3}")`. Compile-time validation. GREEN: parse `/pattern/` in lexer, codegen to Regex::new with compile-time check.
  **Acceptance**: `cargo test -p buff-lang-lexer regex_literals` passes. `/\d+/` → valid Regex.
  **QA**: Parse `/\d{3}-\d{4}/` → assert Regex literal with pattern. Evidence: task-79-regex.txt
  **Commit**: `feat(lexer): add regex literals /pattern/`

- [x] **T100**: `defer` statement [deep]
  **What to do** (TDD): RED: `defer f.close()` runs on function exit (any path). Multiple defers LIFO. GREEN: parse `defer` keyword, codegen using RAII wrapper or scope guard.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust defer` passes. Defer runs on all exit paths.
  **QA**: Codegen `func f(): defer print("done"); return 0` → assert "done" printed before return. Evidence: task-100-defer.txt
  **Commit**: `feat(codegen-rust): add defer statement with LIFO execution`

- [x] **T105**: Named arguments [deep]
  **What to do** (TDD): RED: `create(host: "x", port: 80)` — args by name not position. GREEN: parse `name: value` in call args, validate against params, codegen reordered.
  **Acceptance**: `cargo test -p buff-lang-parser named_args` passes. Args can be in any order with names.
  **QA**: Codegen `greet(name: "Alice", greeting: "Hi")` → assert correct arg mapping. Evidence: task-105-named-args.txt
  **Commit**: `feat(parser): add named arguments for function calls`

- [x] **T106**: Default parameter values [deep]
  **What to do** (TDD): RED: `func fetch(url, timeout = 30)` — omit timeout → uses 30. GREEN: parse default values in func decl, codegen fills defaults for omitted args.
  **Acceptance**: `cargo test -p buff-lang-parser default_params` passes. Omitted params use defaults.
  **QA**: Codegen `fetch("url")` where fetch has `timeout = 30` → assert `fetch("url", 30)`. Evidence: task-106-default-params.txt
  **Commit**: `feat(parser): add default parameter values`

- [x] **T111**: `buff.toml` config + project structure enforcement [deep]
  **What to do** (TDD): RED: parse `buff.toml` with [package], [dependencies], [profile.release]. Enforce `src/`, `tests/` layout. GREEN: implement TOML parsing, workspace support, lock file generation.
  **Acceptance**: `cargo test -p buff-lang-cli config_parsing` passes. `buff.toml` parsed correctly.
  **QA**: Parse sample `buff.toml` → assert name, version, deps extracted. Evidence: task-111-buff-toml.txt
  **Commit**: `feat(cli): implement buff.toml config parsing with workspace support`

- [ ] **T112**: `buff new` templates [unspecified-high]
  **What to do** (TDD): RED: `buff new app --lib` creates library structure. `--server` creates async template. GREEN: implement template system with starter code for each type.
  **Acceptance**: `cargo test -p buff-lang-cli templates` passes. All 5 templates create valid projects.
  **QA**: `buff new mylib --lib` → assert `src/lib.buff` exists. Evidence: task-112-templates.txt
  **Commit**: `feat(cli): add buff new templates (--lib, --server, --gpu, --workspace)`

---

## Phase Exit Criteria

- [ ] all 13 types working: Int, Bits, Float, Double, Decimal, Byte, Bool, String, Vector, Matrix, Map, Struct, Enum
- [ ] Pattern matching with exhaustiveness checking
- [ ] Module system with import/export
- [ ] async with Tokio
- [ ] Error handling with `?` operator
- [ ] Closures with type inference
- [ ] Modern syntax: pipeline, null-conditional, destructuring, guards, ranges, collection literals
- [ ] `buff test` runs test suites
- [ ] Error messages with spans and suggestions
- [ ] `cargo test --workspace` passes 100%
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Git tag `v0.5.0` created
