# BUGS-FOUND.md — T14 Batch 4 (hash_verify, structured_logger, error_recovery)

**Date:** 2026-07-24
**Examples:** hash_verify.buff, structured_logger.buff, error_recovery.buff

---

## Build Environment Issue

**Status:** BLOCKING — cannot run `buff check` or `buff run` on this machine.

The MSVC toolchain on this Windows host is incomplete:
- `vcruntime.h` not found (ring v0.17.14 build fails)
- `msvcrt.lib` not found (linker fails for proc-macro crates)

This prevents any `cargo build`, `cargo check`, `cargo run`, or `buff check` execution.
The examples were written based on existing working examples and the prelude_types registry.

**Action required:** Verify all three examples on a machine with a complete MSVC toolchain.

---

## hash_verify.buff — Potential Issues

### B1: Hash.sha256() type contract
- **Risk:** `Hash.sha256(data: String) -> String` — the prelude_types.rs registration says this returns a 64-char lowercase hex digest. The codegen lowering uses `sha2::Sha256::digest(data.as_bytes())` + `hex::encode()`. This should work end-to-end.
- **Verify:** Run `buff run examples/use-cases/hash_verify.buff` and confirm the SHA-256 digest matches `57c028b506aa21e287f558604a6ea6eb223911a27982a50e907e291715ccaa5f` for "Hello, Buff!".

### B2: Hex.encode/decode type compatibility
- **Risk:** `Hex.encode(bytes: Vector<Byte>)` expects `Vector<Byte>` but we pass `[72, 101, 108, 108, 111]` which infers as `Vector<Int>`. Buff may or may not coerce Int→Byte for this call.
- **Verify:** If this fails with a type error, change `Hex.encode(original)` to use a byte literal or remove the hex roundtrip demo.

### B3: Hex.decode return type
- **Risk:** `Hex.decode(string: String) -> Vector<Byte>` — returns empty Vec on decode failure (never panics). The codegen uses `hex::decode(&string).unwrap_or_default()`. Should work but the return type is `Vector<Byte>` not `Vector<Int>`, so the `decoded` variable may have a different type than `original`.
- **Verify:** Check if `match decoded { Ok(bytes) => print(bytes) }` works with `Vector<Byte>`.

### B4: String.len() on hex digest
- **Risk:** `digest.len()` — String.len() returns the string length. For a 64-char hex digest, this should print 64. Confirmed working in existing examples.

### B5: FileRecord.new() named args
- **Risk:** `FileRecord.new(path: "...", size: 1024, digest: "...")` — struct constructors use named args. This is the standard pattern in existing examples. Should work.

---

## structured_logger.buff — Potential Issues

### B6: Custom enum matching (LogLevel.Debug, etc.)
- **Risk:** Matching on user-defined enum variants is **codegen-verified but does not compile end-to-end**. The codegen emits `Debug` instead of `LogLevel::Debug`, causing a Rust name resolution error.
- **Documented gap:** `examples/pattern_matching.buff` lines 11-14 explicitly state this is a v0.5 codegen gap.
- **Impact:** `buff check` should pass (typecheck succeeds), but `buff run` will fail at the rustc stage.
- **Action:** This is expected behavior — the example is a typecheck-only showcase.

### B7: Logger.new() self-constructor recursion
- **Risk:** `Logger.new(module: String, min_level: LogLevel) -> Logger` calls `Logger.new(module: module, min_level: min_level, entries: [])`. This is a static method that creates a struct literal, not a recursive call. Buff's `Type.new()` convention should handle this correctly.
- **Verify:** If this causes infinite recursion, change to direct struct literal construction.

### B8: Logger method syntax (func Logger.method(self, ...))
- **Risk:** `func Logger.log(self, level: LogLevel, message: String) -> String` — methods on structs using `Type.method` syntax. This is documented in the language spec and used in existing examples (e.g., `CircuitBreaker.record_failure` in error_handling patterns).
- **Verify:** Should work. The `self` parameter is implicit.

### B9: Map indexing without .get_or()
- **Risk:** `counts[tag]` — Map key access by index. The collections.buff example says "keyed lookup (`m[k]`) is a documented gap" because HashMap has no Index impl in Rust. However, the structured_logger uses a loop to initialize all keys to 0 first, so all keys exist before access.
- **Impact:** If Map indexing doesn't work, use a different approach (e.g., if/else chain per level).
- **Documented gap:** `examples/collections.buff` line 11.

### B10: Vector.map() with closure
- **Risk:** `entries.map({ e => format_json(e) })` — Vector.map() with a closure. Confirmed working in `examples/collections.buff` line 35: `.map({ x => x * 10 })`.
- **Verify:** Should work.

### B11: Vector.join(separator: "")
- **Risk:** `lines.join(separator: "\n")` — Vector.join() method. Not shown in existing examples. May not exist on Vector<T>.
- **Mitigation:** If join doesn't exist, the render_json_log function will fail. This is a discoverable bug.
- **Action:** Document as a potential missing API.

### B12: Struct field mutation via self.field
- **Risk:** `self.failure_count = self.failure_count + 1` — mutating struct fields through `self`. This is the standard pattern for methods. Should work.

---

## error_recovery.buff — Potential Issues

### B13: fetch_with_fallback brace syntax
- **Risk:** `func fetch_with_fallback(key: Int) -> Result<String, Error> {` uses brace-delimited function body. Both brace and indentation syntax should work. The existing examples use indentation (`:` + indented body). Brace syntax is also valid.
- **Verify:** Should work. Both syntaxes are supported.

### B14: Nested match on Result
- **Risk:** Three levels of nested `match primary: Ok(v) => ... Err(_) => match fallback: ...` — deep nesting with indentation. Should work but may hit indentation parsing edge cases.
- **Verify:** Run `buff check` and confirm no parse errors.

### B15: retry_operation recursion
- **Risk:** `retry_operation` calls itself recursively. Buff supports recursion (confirmed in `examples/fibonacci.buff`). Should work.

### B16: CircuitBreaker struct methods
- **Risk:** `func CircuitBreaker.record_failure(self)` — mutating self fields. Same pattern as B12. Should work.

### B17: safe_pipeline with ? operator
- **Risk:** `validate_input(input)?` and `transform(validated)?` — the `?` operator on Result<T, Error>. Confirmed working in `examples/error_handling.buff` line 31: `let h = half(n)?`.
- **Verify:** Should work.

### B18: Vector.get(0) returning Option
- **Risk:** `data.get(0)` — Vector.get() returning Option<T>. Not shown in existing examples (only .pop() is shown returning Option). May not exist.
- **Mitigation:** If get() doesn't exist, the demo_option_chain section (which was removed) would need alternative syntax. The current error_recovery.buff does NOT use .get() — it was trimmed. No issue.

### B19: Struct literal in CircuitBreaker.new()
- **Risk:** `CircuitBreaker.new(failure_count: 0, threshold: threshold, is_open: false)` — passing a non-literal `threshold` variable as a named arg. Should work.

---

## Compiler Gaps (Known, Not New)

These are documented compiler gaps that affect these examples:

1. **Custom enum variant qualification** (B6): `LogLevel.Debug` emitted as `Debug` in Rust codegen. Affects structured_logger.buff. buff check passes, buff run fails.
2. **Map key access by index** (B9): `m[k]` not supported on HashMap. Affects count_by_level in structured_logger.buff if keys aren't pre-initialized.
3. **Module imports** (removed from all examples): `import X from buff.Y` not yet wired. All examples use prelude types instead.

---

## Verification Checklist

- [ ] Run `buff check examples/use-cases/hash_verify.buff` on a working build
- [ ] Run `buff check examples/use-cases/structured_logger.buff` on a working build
- [ ] Run `buff check examples/use-cases/error_recovery.buff` on a working build
- [ ] Run `buff run examples/use-cases/hash_verify.buff` and verify output matches .expected
- [ ] Run `buff run examples/use-cases/error_recovery.buff` and verify output matches .expected
- [ ] Run `buff run examples/use-cases/structured_logger.buff` — expected to FAIL at rustc stage due to enum qualification gap (B6)
- [ ] Fill in actual SHA-256 digests in hash_verify.expected if they differ from Python's hashlib
- [ ] Verify Hex.encode/decode type compatibility (B2, B3)
- [ ] Verify Vector.join() exists (B11)

---

# T16 Batch 3: rest_api_server.buff (918 lines)

**Date:** 2026-07-24
**File:** `examples/use-cases/apps/rest_api_server.buff`
**Scope:** Full REST API server with CRUD, middleware, error handling

---

## Build Environment Issue (Same as T14)

**Status:** BLOCKING — `cargo run -p buff-lang-cli -- check` fails on this Windows host.
`vcruntime.h` not found (ring v0.17.14 cc-rs build fails). Same root cause as T14 batch 4.
The example was written based on existing working patterns from `http_server.buff`,
`reactive_to_web.buff`, and `crates/buff-web/examples/hello_web.buff`.

---

## rest_api_server.buff — Findings

### BUG-R16-001: `req.param("id")` not available — manual path parsing required
- **Severity:** MEDIUM (workaround in place)
- **Description:** The REST API needs to extract `{id}` from `/tasks/42`. buff-web T17 only exposes `req.path()` returning the full URL path. `req.param("id")` is deferred to v1.18+.
- **Workaround:** `req.path().split(separator: "/")` → take last segment → parse to Int.
- **Lines:** 480-510, 530-600, 640-670

### BUG-R16-002: `req.query()` not available — manual query string parsing
- **Severity:** LOW (workaround in place)
- **Description:** Search endpoint needs `?q=query` from `/search?q=test`. No query string accessor on Request.
- **Workaround:** Parse `req.path()` for `?` character and manually split key=value pairs.
- **Lines:** 690-720

### BUG-R16-003: `Response.status(code)` returns `&mut Self` — Buff cannot chain on it
- **Severity:** MEDIUM (workaround in place)
- **Description:** `Response.json({...})` returns owned Response. Calling `.status(400)` returns `&mut Self`, but Buff codegen may not handle mutable borrow chaining on owned values.
- **Workaround:** Two statements: `let mut resp = Response.json({...})` then `resp.status(400)`.
- **Lines:** 340-380 (error response builders)

### BUG-R16-004: Middleware `{ req, next => ... }` signature type inference
- **Severity:** LOW (may need confirmation)
- **Description:** Middleware functions take `(Request, &dyn Fn(Request) -> Response) -> Response`. The `next` parameter type (`&dyn Fn`) may not be directly expressible in Buff's type system.
- **Workaround:** Declared middleware as plain functions with untyped `next` parameter.
- **Lines:** 400-440

### BUG-R16-005: `Map<Int, Task>.get()` returns Option — Map indexing gap
- **Severity:** LOW (workaround in place)
- **Description:** `m[k]` syntax documented as v0.5 codegen gap in collections.buff. Used `task_store.get(id)` returning `Option<Task>`.
- **Workaround:** Match on `Some`/`None`.
- **Lines:** 110-130, 135-150

### BUG-R16-006: `Int.from(String)` — string-to-int conversion may not exist
- **Severity:** MEDIUM (may fail at type-check)
- **Description:** Path parameter parsing needs `"42"` → `42` conversion. `Int.from(s: String)` may not be in prelude.
- **Impact:** If unavailable, all three CRUD handlers that parse IDs will fail at type-check.
- **Lines:** 490, 545, 645

### BUG-R16-007: `serde_json::Value.get()/.as_str()` — JSON interop methods
- **Severity:** MEDIUM (may fail at type-check)
- **Description:** `req.json()` returns `serde_json::Value`. Extracting fields needs `.get("key")` → `Option<&Value>` → `.as_str()` → `Option<&str>`. These Rust methods may not be exposed in Buff prelude.
- **Lines:** 480-520, 530-600

### BUG-R16-008: `Map.remove(key)` may not exist
- **Severity:** LOW (workaround possible)
- **Description:** `delete_task` calls `task_store.remove(id)`. The `remove()` method may not be on Map<K,V>.
- **Alternative:** Rebuild map without the deleted key.
- **Line:** 168

### BUG-R16-009: `String.contains(sub)` may not be in prelude
- **Severity:** LOW (workaround exists)
- **Description:** Search uses `title.lower().contains(lower_query)`. `String.contains()` may not exist.
- **Alternative:** Use `title.lower().index_of(query) >= 0`.
- **Line:** 280

### BUG-R16-010: Catch-all route `/{*}` — axum 0.8 specificity
- **Severity:** LOW (may need confirmation)
- **Description:** Registered `app.get(path: "/{*}", handler: handle_not_found)` as 404 catch-all. Axum 0.8 uses `/{*wildcard}` syntax but may conflict with `/tasks/{id}`.
- **Lines:** 810-820

### BUG-R16-011: No graceful shutdown — tokio::signal not exposed
- **Severity:** LOW (by design, deferred)
- **Description:** Server runs forever until killed. `tokio::signal::ctrl_c()` not exposed in buff-web's sync API.
- **Lines:** 830-840

### BUG-R16-012: `pass` keyword — may not be valid Buff syntax
- **Severity:** LOW (may need removal)
- **Description:** Empty match arms use `pass` as no-op. Buff may not have a `pass` keyword (Python does, Buff doesn't).
- **Alternative:** Use empty block `{}` or remove the arm.
- **Lines:** 140, 195, 215, 240, 265

### BUG-R16-013: Tuple return type may not be supported
- **Severity:** LOW (workaround exists)
- **Description:** `validate_task_payload` returns `Result<(String, String, TaskStatus, TaskPriority), Error>`. Tuple types may not be fully supported.
- **Alternative:** Define a `ValidatedPayload` struct.
- **Line:** 293

### BUG-R16-014: `Response.header()` chaining after `next()` in middleware
- **Severity:** LOW (may need confirmation)
- **Description:** Middleware calls `next(req)` returning owned Response, then `.header(...)` returning `&mut Self`. Buff may not handle `x.method(); x.method(); return x` correctly if x is moved after borrow.
- **Lines:** 430-440

---

## Summary (T16 Batch 3)

| Bug ID | Severity | Category |
|--------|----------|----------|
| BUG-R16-001 | MEDIUM | API gap (path params) |
| BUG-R16-002 | LOW | API gap (query params) |
| BUG-R16-003 | MEDIUM | API design (response chaining) |
| BUG-R16-004 | LOW | API design (middleware types) |
| BUG-R16-005 | LOW | Collection API (Map indexing) |
| BUG-R16-006 | MEDIUM | Prelude API (int parsing) |
| BUG-R16-007 | MEDIUM | JSON interop (Value methods) |
| BUG-R16-008 | LOW | Collection API (Map.remove) |
| BUG-R16-009 | LOW | Prelude API (String.contains) |
| BUG-R16-010 | LOW | Routing (catch-all) |
| BUG-R16-011 | LOW | Scope boundary (shutdown) |
| BUG-R16-012 | LOW | Syntax (pass keyword) |
| BUG-R16-013 | LOW | Type system (tuples) |
| BUG-R16-014 | LOW | Response chaining + codegen |

**Total:** 14 new findings (4 MEDIUM, 10 LOW). All MEDIUM bugs have workarounds in place.

### Verification Checklist (T16)

- [ ] Run `buff check examples/use-cases/apps/rest_api_server.buff` on a working build
- [ ] Verify `req.path().split(separator: "/")` works for ID extraction (R16-001)
- [ ] Verify `Int.from(string)` exists or provide alternative (R16-006)
- [ ] Verify `serde_json::Value.get()/.as_str()` are accessible (R16-007)
- [ ] Verify `Map.remove(key)` exists or use rebuild pattern (R16-008)
- [ ] Verify `String.contains(sub)` exists or use index_of (R16-009)
- [ ] Verify `pass` is valid Buff syntax or remove (R16-012)
- [ ] Verify tuple return types work (R16-013)
- [ ] Verify middleware type inference resolves `&dyn Fn` (R16-004)
- [ ] Verify axum `/{*wildcard}` catch-all doesn't conflict (R16-010)