# Decision Record: Macro System Spike (T3 — v1.13-v1.17)

**Status:** FINAL
**Decision ID:** macro-system-v1x
**Date:** 2026-07-21
**Author:** Sisyphus-Junior (spike executor)
**Plan reference:** `.sisyphus/plans/buff-v1x-frameworks.md` L1101-1177 (T3 — Macro System Spike)
**Branch:** `v1x-frameworks`

---

## 1. VERDICT

**DEFER-POST-v1.17**

The macro system does NOT ship in v1.13-v1.17. All five frameworks (buff-web, buff-db, buff-reactive, buff-template, buff-ml) can ship their MVP using documented non-macro workarounds. The implementation cost (~2000-3000 LOC for a declarative `macro_rules!`-style system) exceeds the 1500-LOC threshold in the LOCKED decision rule, and fewer than 2 frameworks genuinely require macros for their MVP.

---

## 2. Use Cases Analyzed

### 2.1 ORM compile-time SQL validation (buff-db)

**Macro version:** A `query!` macro that parses a SQL string at compile time, validates it against a schema, and generates type-safe Rust bindings. Example Buff syntax:
```buff
macro query!($sql:expr) => { ... }
let users = query!("SELECT id, name FROM users WHERE id = ?", user_id)
```
The macro would parse the SQL string, validate column names/types against a schema registry, and emit a Rust struct with typed fields. Implementation: ~800-1200 LOC (SQL parser + schema lookup + codegen).

**Without macros (workaround):** Runtime query building via `sqlx::query(...)` with `.bind()` calls. The framework provides a builder API:
```buff
let users = db.query("SELECT id, name FROM users WHERE id = ?")
    .bind(user_id)
    .all::<User>()
```
SQL validation happens at test time (integration tests against a test DB) or at runtime (returns `Result`). No compile-time guarantee, but fully functional for MVP.

**LOC delta:** ~1000 LOC saved by deferring (macro system not needed).

### 2.2 Routing table generation (buff-web)

**Macro version:** A `routes!` macro that generates a match-based router from a declarative route table:
```buff
routes! {
    GET "/users" => list_users
    POST "/users" => create_user
    GET "/users/:id" => get_user
}
```
Expands to a `match (method, path)` tree with path-parameter extraction. Implementation: ~400-600 LOC.

**Without macros (workaround):** Runtime `Vec<Route>` registration with pattern matching:
```buff
router.add(Method::GET, "/users", list_users)
router.add(Method::POST, "/users", create_user)
router.add(Method::GET, "/users/:id", get_user)
```
The router uses a trie or regex-based matcher at runtime. Slightly slower dispatch (O(n) vs O(1) match), but negligible for MVP scale (<100 routes). Path parameters extracted via `params.get("id")` at runtime.

**LOC delta:** ~500 LOC saved by deferring.

### 2.3 Automatic Serialize/Deserialize derives

**Macro version:** A `#[derive(Serialize, Deserialize)]`-style attribute that auto-generates `impl Serialize for Foo` / `impl Deserialize for Foo` for user structs. In Buff, this would be a `@serialize` attribute on a struct declaration:
```buff
@serialize
struct User:
    name: String
    age: Int
```
The codegen would emit `#[derive(serde::Serialize, serde::Deserialize)]` on the generated Rust struct. Implementation: ~600-800 LOC (attribute parsing + codegen emit + serde dependency wiring).

**Without macros (workaround):** Manual `impl Serialize for Foo` / `impl Deserialize for Foo` using the existing `extend` mechanism (T75 extension methods). The framework provides a builder:
```buff
// Manual serialization via extend
extend User:
    fn to_json(self) -> String:
        // framework-provided JSON builder
        json.object()
            .field("name", self.name)
            .field("age", self.age)
            .build()
```
Or use the prelude `JSON.stringify(user)` which uses runtime reflection (limited). For MVP, the framework ships a code generator (separate CLI tool, not compiler-integrated) that reads `.buff` struct definitions and emits the serialization boilerplate.

**LOC delta:** ~700 LOC saved by deferring.

### 2.4 JSON schema derivation

**Macro version:** A `@json_schema` attribute that walks a struct's fields at compile time and generates a JSON Schema document:
```buff
@json_schema
struct Config:
    host: String
    port: Int
    debug: Bool
```
Expands to a `const SCHEMA: &str = "{...}"` containing the compiled JSON Schema. Implementation: ~500-700 LOC (type walker + schema emitter).

**Without macros (workaround):** Runtime schema derivation using the same type information available through Buff's type system. The framework provides `Schema.from_type<Config>()` which uses runtime type inspection (via `std::any::TypeId` or a generated type registry). For MVP, a separate CLI tool (`buff schema generate`) scans `.buff` files and emits JSON Schema files as a build step.

**LOC delta:** ~600 LOC saved by deferring.

---

## 3. Per-Use-Case Workaround

### buff-web (routing)

**Without Macros:** Runtime `Vec<Route>` registration. The framework exports a `Router` struct with builder methods:
```buff
let app = Router.new()
    .route(Method::GET, "/users", list_users)
    .route(Method::POST, "/users", create_user)
    .route(Method::GET, "/users/:id", get_user)
```
Path parameters extracted via `req.param("id")` — a `Map<String, String>` lookup. Middleware is composed via `.wrap(middleware)` method chaining. Performance: O(n) route matching (linear scan of registered routes) vs O(1) with a generated match tree. Acceptable for MVP (<100 routes, <1ms per match).

**Macro version would add:** Compile-time route deduplication, O(1) match dispatch, compile-time path-parameter type extraction. Estimated 400-600 LOC in the macro system.

### buff-db (ORM)

**Without Macros:** Runtime query building. The framework provides a fluent query builder:
```buff
let users = db.query("SELECT id, name FROM users WHERE id = ?")
    .bind(user_id)
    .map(|row| User { id: row.get(0), name: row.get(1) })
    .all()
```
SQL validation happens in integration tests. Type safety is enforced at the `.get()` call site (runtime panic on type mismatch). For MVP, this is acceptable — the framework ships with a test helper that validates all queries against a test database schema.

**Macro version would add:** Compile-time SQL syntax validation, compile-time column type checking, zero-cost row mapping. Estimated 800-1200 LOC in the macro system.

### buff-reactive (signals + state management)

**Without Macros:** Manual signal wiring. The framework provides `Signal<T>` and `Effect` primitives:
```buff
let count = signal(0)
let doubled = computed(|| count.get() * 2)
effect(|| { print("count changed: {count.get()}") })
```
No macro needed for MVP — the reactive primitives are runtime constructs. A `@reactive` attribute that auto-wires dependencies would be a nice-to-have but is not required.

**Macro version would add:** `@reactive` attribute that auto-detects signal dependencies and generates optimized update paths. Estimated 300-500 LOC. **Not required for MVP.**

### buff-template (server-side rendering)

**Without Macros:** Runtime template rendering. The framework provides a `Template` trait with a `render()` method:
```buff
struct UserPage:
    user: User

    fn render(self) -> String:
        html("""
            <h1>{self.user.name}</h1>
            <p>Email: {self.user.email}</p>
        """)
```
Uses Buff's existing string interpolation for template expansion. No compile-time template parsing needed. For MVP, this is sufficient — template errors surface at runtime.

**Macro version would add:** Compile-time template parsing with syntax validation, optimized string building, component extraction. Estimated 500-700 LOC. **Not required for MVP.**

### buff-ml (machine learning)

**Without Macros:** Runtime graph construction. The framework provides a `Graph` builder:
```buff
let model = Graph.new()
    .input(shape: [28, 28])
    .dense(units: 128, activation: "relu")
    .dropout(rate: 0.5)
    .dense(units: 10, activation: "softmax")
    .build()
```
No macro needed — the graph is constructed at runtime. A `@layer` attribute that auto-generates gradient computation would be a nice-to-have but is not required for MVP.

**Macro version would add:** Compile-time graph optimization, auto-differentiation code generation, shape inference. Estimated 600-1000 LOC. **Not required for MVP.**

---

## 4. Cost Estimate

### Declarative `macro_rules!`-style system (recommended by plan)

| Component | LOC Estimate | Notes |
|---|---|---|
| Lexer: new `KwMacro` token + `!` token disambiguation | ~100 | `!` is already a token; macro invocation `name!()` needs new parse rule |
| Parser: `macro name { ... }` definition syntax | ~300 | New `parse_macro_decl` in `parser.rs` + `parse_macro_invocation` in `expr.rs` |
| Parser: macro invocation `name!(args)` in expression position | ~200 | Extend `parse_expression` to detect `Ident ! ( ... )` pattern |
| AST: `Decl::MacroDecl` + `Expr::MacroInvocation` | ~100 | New variants in `decl.rs` and `expr.rs` |
| Macro expansion engine: pattern matching + token rewriting | ~800 | Core engine: match macro arms, substitute `$var`, emit expanded tokens |
| Codegen: lower macro invocations (expand before Rust lowering) | ~200 | Hook into `generate()` to expand macros before the main lowering loop |
| Error handling: macro-related diagnostics | ~200 | Error codes for undefined macros, arity mismatch, pattern match failure |
| Tests | ~300 | Snapshot tests for macro expansion, edge cases |
| **Total (declarative)** | **~2200** | |

### Procedural macro system (more powerful, more complex)

| Component | LOC Estimate | Notes |
|---|---|---|
| All of the above | ~2200 | Declarative baseline |
| Token stream API for proc-macro authors | ~500 | `TokenStream` type exposed to Buff macro authors |
| Compile-time function execution | ~800 | Run Buff functions at compile time to generate tokens |
| Attribute macro support (`@derive(Serialize)`) | ~300 | Extend existing `@attribute` parsing |
| **Total (procedural)** | **~3800** | |

### Timeline estimate

| System | Estimated Weeks | Team |
|---|---|---|
| Declarative `macro_rules!`-style | 4-6 weeks | 1 engineer (full-time) |
| Procedural macro system | 8-12 weeks | 1-2 engineers (full-time) |

### Comparison: workaround implementation cost

| Framework | Workaround LOC | Workaround Weeks |
|---|---|---|
| buff-web routing | ~200 (Vec<Route> + matcher) | 1 week |
| buff-db ORM | ~400 (query builder + test helpers) | 2 weeks |
| buff-reactive | ~100 (Signal/Effect primitives) | 0.5 weeks |
| buff-template | ~150 (Template trait + string interp) | 1 week |
| buff-ml | ~300 (Graph builder) | 2 weeks |
| **Total workarounds** | **~1150** | **~6.5 weeks** |

The workarounds cost ~1150 LOC and ~6.5 weeks total — less than the macro system itself (~2200 LOC, 4-6 weeks) — and can be built incrementally per framework.

---

## 5. Decision Rule Applied

**LOCKED decision rule (from plan L1110):**
> DEFER if (implementation > 1500 LOC) OR (< 2 frameworks genuinely require it for MVP) OR (spike exceeds 5 days).

### Rule evaluation

| Criterion | Value | Pass/Fail |
|---|---|---|
| Implementation > 1500 LOC? | ~2200 LOC (declarative) | **FAIL** (exceeds threshold) |
| < 2 frameworks genuinely require it for MVP? | 0 frameworks require it | **FAIL** (all 5 have workarounds) |
| Spike exceeds 5 days? | 1 session (~4 hours) | **PASS** (within limit) |

**Result:** 2 of 3 criteria trigger DEFER. The rule is unambiguous.

### Verdict justification

1. **Implementation cost (~2200 LOC) exceeds the 1500-LOC threshold** by 47%. The macro expansion engine alone (pattern matching + token rewriting) is ~800 LOC — a non-trivial compiler subsystem that touches lexer, parser, AST, and codegen. The ripple across 4 crates (lexer, parser, ast, codegen-rust) adds integration risk.

2. **Zero frameworks genuinely require macros for MVP.** Every use case has a documented runtime workaround that is simpler to implement and maintain. The workarounds are not "hacks" — they are standard patterns (runtime route registration, query builders, manual serialization) used by production frameworks in other ecosystems (Express.js, sqlx, etc.).

3. **The hand-rolled parser is extensible** (adding a new `parse_one_decl` arm is straightforward), but the macro expansion engine is the hard part — it requires a mini-language for pattern matching (`$name:expr`, `$($arg),*`, etc.) that is itself a parsing problem. This is not a simple extension of the existing Pratt/recursive-descent approach.

4. **Deferring does not block any framework.** All five frameworks (buff-web, buff-db, buff-reactive, buff-template, buff-ml) can ship their MVP in v1.13-v1.17 using the documented workarounds. Macros can be added post-v1.17 as a performance optimization and DX improvement, not a prerequisite.

---

## 6. Recommendation

**DEFER-POST-v1.17.** The macro system is a valuable long-term investment for Buff's ecosystem, but it is not required for the v1.13-v1.17 frameworks roadmap. The LOCKED decision rule triggers DEFER on two independent criteria (LOC > 1500, frameworks < 2).

### When to revisit

Re-evaluate the macro system decision when:

1. **A framework explicitly blocks on macro support.** If buff-db or buff-web hits a usability wall where the runtime workaround is genuinely unacceptable (not just "less ergonomic"), escalate to the orchestrator.

2. **Post-v1.17, when all five frameworks have shipped.** At that point, the team can assess whether the workarounds are causing real pain and whether the macro system is worth the investment.

3. **If a contributor volunteers to implement it.** The declarative `macro_rules!`-style system is well-scoped (~2200 LOC, 4-6 weeks) and could be a good onboarding task for a new contributor familiar with parser internals.

### Recommended approach for future implementation

When the macro system is greenlit, the recommended approach is:

1. **Declarative `macro_rules!`-style first** (as recommended by the plan). Procedural macros can follow in a later release.
2. **New `KwMacro` token** in the lexer — no existing token changes needed.
3. **New `parse_macro_decl` in `parser.rs`** — follows the existing `parse_one_decl` dispatch pattern.
4. **Macro expansion as a pre-pass** in `generate()` — expand all macro invocations to AST nodes before the main lowering loop. This keeps the codegen layer clean (it never sees macro nodes).
5. **Pattern matching engine** as a new module in `buff-lang-parser` — reuses the existing `TokenStream` cursor for pattern matching.

---

## 7. Acceptance Criteria Check

| Criterion | Status | Evidence |
|---|---|---|
| Decision document exists with verdict | ✅ | This file, §1 |
| "Use Cases Analyzed" with ≥3 use cases | ✅ | §2 (4 use cases) |
| "Per-Use-Case Workaround" section | ✅ | §3 (5 frameworks) |
| "Cost Estimate" with LOC and weeks | ✅ | §4 (declarative vs procedural) |
| "Decision Rule Applied" section | ✅ | §5 (LOCKED rule cited) |
| Verdict aligns with decision rule | ✅ | DEFER (LOC > 1500 AND frameworks < 2) |
| Every framework has workaround documented | ✅ | buff-web, buff-db, buff-reactive, buff-template, buff-ml in §3 |

---

## 8. Spike Provenance

This decision record was produced by a single spike session (2026-07-21):

- **Parser assessment:** `crates/buff-lang-parser/src/parser.rs` (362 lines) — `parse_one_decl` dispatches on `TokenKind` with 10 match arms. Adding a macro arm is straightforward (~50 LOC). The hard part is the macro expansion engine, not the parser hook.
- **Codegen assessment:** `crates/buff-lang-codegen-rust/src/rust_codegen.rs` (12,777 lines) — `generate()` runs pre-passes (atomic, race, async, etc.) before the main lowering loop. A macro expansion pre-pass would slot in naturally before the main loop.
- **Lexer assessment:** `crates/buff-lang-lexer/src/token.rs` (395 lines) — 28 keywords today. Adding `KwMacro` is trivial. The `!` token already exists (used for `not` in some contexts); macro invocation `name!()` needs a new parse rule but no new token.
- **AST assessment:** `crates/buff-lang-ast/src/decl.rs` (918 lines) — `Decl` enum has 12 variants. Adding `MacroDecl` and `MacroInvocation` follows the established pattern.
- **Conventions:** Root `AGENTS.md` hard rules (no raw-string codegen, no `unwrap`/`expect`/`panic!`) are compatible with a macro system — macro expansion produces AST nodes, not raw strings.
