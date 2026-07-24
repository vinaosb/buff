# Memory Safety

> **Buff is memory-safe by design.** Buff inherits Rust's memory safety guarantees — no buffer overflows, use-after-free, null pointer dereferences, or data races can occur in safe Buff code.

This document aligns with the [CISA Secure by Design](https://www.cisa.gov/secure-by-design) initiative, which encourages programming languages that eliminate entire classes of memory safety vulnerabilities.

---

## How Buff Achieves Memory Safety

Buff is a transpiler: `.buff` source code is compiled to Rust, then to a native binary via `rustc`/LLVM. The Rust borrow checker runs on the generated code, catching memory errors at compile time — before the program ever runs.

**Crucially, Buff users never interact with the borrow checker directly.** The Buff compiler emits only "easy" Rust (owned data, intelligent clones, `Arc`/copy-on-write where sharing is needed). The user writes clean, indentation-based code and gets a binary that is provably memory-safe.

```
.buff source → Buff compiler → Rust source → rustc/LLVM → native binary
                                    ↑
                            borrow checker runs here
                            (free safety review)
```

---

## What Is Prevented

| Vulnerability Class | How Buff Prevents It |
|---|---|
| **Buffer overflows** | All array/vector access is bounds-checked at runtime (panics on out-of-bounds, never corrupts memory). |
| **Use-after-free** | Rust's ownership model ensures every value has exactly one owner; when the owner goes out of scope, the value is dropped. No dangling pointers. |
| **Null pointer dereferences** | Buff has no `null`. Absence is represented by `Option<T>` — the compiler forces the user to handle the `None` case. |
| **Data races** | Rust's `Send` and `Sync` traits ensure that shared data can only be accessed from multiple threads if it is safe to do so. Data races are compile-time errors. |
| **Stack smashing** | Stack canaries, ASLR, and DEP are applied by `rustc`/LLVM at link time. |
| **Format string vulnerabilities** | Buff's `print`/`format` use typed interpolation (`${expr}`), not C-style format strings. No `%n`, `%s` with wrong types. |
| **Integer overflow** | Debug builds panic on overflow; release builds use two's complement wrapping (configurable via `--overflow-checks`). |
| **Uninitialized memory** | Rust requires initialization before use; the compiler rejects reading uninitialized memory. |

---

## Comparison

| Property | Buff / Rust | C / C++ | Go / Java |
|---|---|---|---|
| Buffer overflows | **Impossible** in safe code | Possible (UB) | Impossible (bounds-checked) |
| Use-after-free | **Impossible** in safe code | Possible (UB) | Impossible (GC) |
| Null dereferences | **Impossible** (`Option<T>`) | Possible (UB) | Possible (`NullPointerException`) |
| Data races | **Compile-time error** | Possible (UB) | Possible (data races in Go) |
| Memory management | Zero-cost ownership (no GC) | Manual (`malloc`/`free`) | Garbage collector (pauses, overhead) |
| Performance cost | None (compile-time checks) | None | Runtime overhead (GC, bounds checks) |

---

## GPU Safety

Buff's heterogeneous computing path dispatches work to the GPU via [wgpu](https://wgpu.rs/) (WebGPU). GPU code is written in [WGSL](https://www.w3.org/TR/WGSL/) (WebGPU Shading Language), which is:

- **Memory-safe**: WGSL has no pointer arithmetic, no manual memory management.
- **Sandboxed**: GPU code runs in a wgpu-managed context with bounds-checked resource access.
- **Fallback-safe**: If the GPU is unavailable or a GPU operation fails, Buff's runtime transparently falls back to CPU execution (rayon parallel). The user program continues without error.

The dispatch decision is automatic and data-size-aware (see `--explain` flag for diagnostics). Users can hint with `@prefer(gpu)` but the decision never compromises correctness.

---

## FFI Safety

Buff provides `extern` blocks (v1.3) for Rust interop. These are explicitly `unsafe` — the user opts into unsafe code by writing `extern "Rust"` or `extern "C"` blocks. Buff **never auto-generates unsafe code** outside of user-written `extern` blocks.

All `extern` wrapper crates follow the [FFI Safety Guide](crates/buff-lang-ffi-guide/GUIDE.md) (6 hard rules: no raw pointers in public API, ownership boundaries, error mapping, thread safety, lifetime hiding, panic boundaries).

---

## Supply Chain

Buff's compiler is implemented in pure Rust (no C dependencies, no `cc-rs`, no Docker):

- `reqwest` uses `rustls-tls` (not `native-tls` — no OpenSSL)
- `zeromq` (pure-Rust, not `zmq` which links C `libzmq`)
- Hand-rolled lexer/parser (no `chumsky`/`logos` which transitively required C shims)

This eliminates an entire class of supply-chain memory safety risks from C dependencies.

---

## References

- [CISA Secure by Design](https://www.cisa.gov/secure-by-design)
- [Rust Memory Safety](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html)
- [The Rustonomicon](https://doc.rust-lang.org/nomicon/) (unsafe Rust pitfalls — Buff avoids these by never generating unsafe code)
- [wgpu / WebGPU Safety Model](https://www.w3.org/TR/WGSL/#security-considerations)
- [OWASP Memory Safety](https://owasp.org/www-community/vulnerabilities/Memory_corruption)

---

## License

Buff is dual-licensed under [MIT](LICENSE) or [Apache-2.0](LICENSE). This document is part of the Buff project and follows the same license.
