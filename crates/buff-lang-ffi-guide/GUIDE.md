# FFI Safety Guide for Buff Framework Wrappers

Version 1.0.0. Last updated: 2026-07-21.

This document defines six hard rules that every Buff framework wrapper crate must follow when bridging Rust libraries into Buff's managed view of the world. These rules exist because the Buff-to-Rust boundary is where safety guarantees can leak, ownership can blur, and panics can unwind into user code.

Wave 4 wrappers (T17-T21: buff-web, buff-db, buff-template, buff-reactive, buff-observe) are the first consumers. Community bindings must follow the same rules.

---

## Background: how `extern` works in Buff today

A Buff user writes:

```buff
extern "C" from "serde_json" func parse_str(input: String) -> String
```

The Buff compiler (`buff-lang-codegen-rust`) lowers this to:

```rust
extern "C" {
    fn parse_str(input: String) -> String;
}
```

The compiler records `"serde_json"` in the extern-crates set and automatically wraps every call to `parse_str` in `unsafe { ... }`. The user never sees the `unsafe` keyword. This is the compiler's "no unsafe Rust" guarantee.

The foreign-mod functions are implicitly unsafe to call in Rust (the block itself has `unsafety: None` for edition compatibility). The call-site wrapping happens in `rust_codegen.rs` around line 2487: when the callee name appears in `extern_fn_names`, the lowered call expression gets wrapped in `syn::ExprUnsafe`.

Wrapper authors write the *body* of these functions in a companion Rust module. The guide below governs what that body may and must not do.

Existing examples of this pattern live in the `examples/` directory:
- `extern_serde_json.buff`, `extern_tokio.buff`, `extern_reqwest.buff`

Each declares the Buff-facing signature. The actual Rust implementation goes in a separate `externs.rs` file that the Cargo project includes alongside the generated code.

---

## Rule 1: No Raw Pointer Exposure

### What

Buff users must never see `*const T` or `*mut T` in any type signature, error message, or diagnostic. All raw pointers must be wrapped in safe Buff types before crossing the boundary.

### Why

Raw pointers bypass Rust's borrow checker. A Buff user cannot reason about pointer validity, dangling references, or aliasing. Exposing them breaks Buff's core promise: the user should never think about memory safety.

### How

If the underlying Rust API takes or returns raw pointers, the wrapper converts them to safe types at the boundary:

- `*const u8` with a length becomes `Bytes` (a Buff `Vector<UInt8>`).
- `*mut T` becomes an owned `Box<T>` lowered to a Buff struct.
- C strings (`*const c_char`) become `String`.

### Example violation (what not to do)

```rust
// WRONG: raw pointer leaks through the Buff boundary
#[no_mangle]
pub extern "C" fn get_buffer_ptr() -> *const u8 {
    buffer.as_ptr()
}
```

A Buff user calling `get_buffer_ptr()` receives something they cannot safely use. The type system provides no protection.

---

## Rule 2: Ownership Boundary

### What

Rust owns all heap memory. Buff sees only borrowed views or owned copies. Memory allocated by Rust must be freed by Rust. Memory allocated by Buff's generated code must be freed by Buff's generated code (which means Rust's drop runs automatically).

### Why

Mixed ownership across a language boundary is the classic source of double-frees, use-after-free, and leaks. By giving Rust exclusive ownership of heap allocations that originate in Rust libraries, we eliminate an entire class of bugs.

### How

- Wrapper functions return owned Buff types (`String`, `Vector<T>`, structs). These are lowered to Rust `String`, `Vec<T>`, etc., and Rust's drop frees the memory.
- If a Rust API requires a mutable buffer, the wrapper allocates internally, fills it, and returns an owned value. The caller never holds a pointer into Rust's heap.
- Callbacks from Rust back into Buff code receive owned copies, not references into Rust's memory.

### Practical implication

Never hand a `&mut [u8]` slice from Rust to Buff code that outlives the wrapper call. If the Buff code needs to keep the data, copy it first.

---

## Rule 3: Error Mapping

### What

Every Rust `Result<T, E>` must be mapped to a Buff `Result<T, BuffError>`. The error variant must carry a human-readable message with the Buff source span when available.

### Why

Buff has its own error type (`BuffError`) that participates in the `?` propagation operator. If a wrapper returns a raw Rust error type, Buff's `?` operator cannot unwrap it, and the user gets an opaque failure instead of a useful diagnostic.

### How

The wrapper catches the Rust error, converts it to a string message, and returns it as Buff's error variant:

```rust
// Inside the wrapper's externs.rs
pub extern "C" fn parse_json(input: String) -> String {
    match serde_json::from_str::<serde_json::Value>(&input) {
        Ok(value) => value.to_string(),
        Err(e) => format!("JSON parse error: {}", e),
    }
}
```

The current convention (visible in `examples/extern_serde_json.buff`) uses `String` return with error-prefix strings because `Result<T,E>` lowering through the full pipeline is still maturing. Wrapper authors should structure errors so that a future `Result<T, BuffError>` migration is straightforward: prefix the error string with a known tag, and include the original Rust error message verbatim.

### Span awareness

When the wrapper is called from a Buff source file, the compiler's span information is available via `buff-lang-error::Span`. Future wrapper infrastructure should thread spans through so errors reference the Buff source location, not just the Rust call site.

---

## Rule 4: Thread Safety

### What

Only types that implement `Send + 'static` may cross `spawn` boundaries in Buff. This means any value captured by a `spawn { ... }` closure, or passed to an async function that may run on a different thread, must satisfy both bounds.

### Why

Buff's `spawn` lowers to `tokio::spawn` (or `rayon::spawn` for parallel closures). Both require `Send + 'static` on captured values. If a wrapper type contains an `Rc<T>`, a raw pointer, or a reference to stack memory, passing it across a spawn boundary causes a compile error in the generated Rust. Worse, if the wrapper bypasses the type system somehow, it creates data races.

### How

- Wrapper structs that need shared ownership must use `Arc<T>` instead of `Rc<T>`.
- Wrapper structs that hold OS handles (file descriptors, sockets) must ensure the handle type is `Send`. Most Rust OS handle types already are.
- Wrapper authors should document which of their types are safe to capture in `spawn` closures and which are not.

### Check

Before shipping a wrapper, verify with `cargo check` that the generated Rust compiles when the wrapper's types are captured inside a `spawn` block. The Rust compiler enforces `Send + 'static` at the type level.

---

## Rule 5: Lifetime Hiding

### What

Rust lifetimes must never appear in Buff-visible types. All Buff types are either owned or `'static`. Wrapper authors must convert borrowed Rust types to owned types at the boundary.

### Why

Buff has no lifetime syntax. There is no `'a`, no `&str` at the language level. A wrapper that returns a `&str` with a non-`'static` lifetime creates a dangling reference the moment the Buff code tries to use it beyond the wrapper call. Since Buff users cannot annotate or reason about lifetimes, the only safe choice is to eliminate them at the boundary.

### How

- `&str` becomes `String` (clone at the boundary).
- `&[u8]` becomes `Vec<u8>` (copy at the boundary).
- `&T` where `T: Clone` becomes `T` (clone).
- `&T` where `T` is not `Clone` needs redesign: wrap in an `Arc<T>` or restructure the API.

The cost is occasional extra allocations. That is an acceptable tradeoff for correctness across a language boundary where the caller cannot participate in lifetime reasoning.

---

## Rule 6: Panic Boundary

### What

Rust panics inside wrapper functions must be caught with `std::panic::catch_unwind` and converted to Buff errors. Panics must never propagate across the FFI boundary into Buff code.

### Why

An unwinding panic across an `extern "C"` boundary is undefined behavior in Rust. Even within the same Rust binary, an uncaught panic inside a wrapper aborts the process instead of producing a recoverable error. Buff users expect errors to be values, not process crashes.

### How

Every public wrapper function should wrap its body:

```rust
use std::panic::catch_unwind;

pub extern "C" fn wrapper_fn(input: String) -> String {
    let result = catch_unwind(|| {
        // actual work here
        inner_logic(&input)
    });
    match result {
        Ok(value) => value,
        Err(_) => "internal error: wrapper panicked".to_string(),
    }
}
```

Note: `catch_unwind` requires the closure to be `UnwindSafe`. If the closure captures `&mut T` or other non-UnwindSafe types, use `AssertUnwindSafe` from `std::panic::AssertUnwindSafe` after verifying the capture is safe to resume after a panic (which it almost always is for wrapper logic that does not share state with the caller).

### Edge case

If the wrapper calls a Rust function known to never panic (all fallible operations return `Result`, all indexing is bounds-checked via `Result`, no `unwrap()` in non-test code), the `catch_unwind` wrapper is technically unnecessary. Include it anyway. The performance cost is negligible, and the defense-in-depth prevents a future change from introducing a latent UB vector.

---

## Reference Examples

### Example 1: Stateless function wrapper (url::Url::parse)

This is the simplest wrapper pattern. A pure function that takes owned input and returns an owned result.

**Buff side:**

```buff
extern "C" from "buff-url" func parse_url(input: String) -> String
```

**Rust side (`externs.rs`):**

```rust
use std::panic::{catch_unwind, AssertUnwindSafe};

#[no_mangle]
pub extern "C" fn parse_url(input: String) -> String {
    let result = catch_unwind(AssertUnwindSafe(|| {
        match url::Url::parse(&input) {
            Ok(url) => url.to_string(),
            Err(e) => format!("url parse error: {}", e),
        }
    }));
    match result {
        Ok(value) => value,
        Err(_) => "internal error: url parser panicked".to_string(),
    }
}
```

**Rules applied:**
- R1: No raw pointers. Input is `String`, output is `String`.
- R2: Rust allocates the output `String`. Buff's generated code receives an owned value that Rust drops normally.
- R3: Rust `Result` mapped to error-prefixed `String`. Ready for future `BuffError` migration.
- R6: `catch_unwind` wraps the entire body.

---

### Example 2: Stateful struct wrapper (regex::Regex)

Wrapping a stateful Rust struct requires the struct to live in Rust's memory. Buff holds an opaque handle (an integer ID or similar) and passes it to each wrapper call.

**Buff side:**

```buff
extern "C" from "buff-regex" func regex_compile(pattern: String) -> Int
extern "C" from "buff-regex" func regex_is_match(handle: Int, text: String) -> Bool
extern "C" from "buff-regex" func regex_drop(handle: Int)
```

**Rust side (`externs.rs`):**

```rust
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;
use std::collections::HashMap;

static REGEX_STORE: Mutex<HashMap<u64, regex::Regex>> = Mutex::new(HashMap::new());
static mut NEXT_ID: u64 = 1;

fn next_id() -> u64 {
    unsafe { let id = NEXT_ID; NEXT_ID += 1; id }
}

#[no_mangle]
pub extern "C" fn regex_compile(pattern: String) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        match regex::Regex::new(&pattern) {
            Ok(re) => {
                let id = next_id();
                REGEX_STORE.lock().unwrap().insert(id, re);
                id
            }
            Err(e) => {
                // Use 0 as sentinel for "compile failed"
                eprintln!("regex compile error: {}", e);
                0
            }
        }
    }));
    result.unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn regex_is_match(handle: u64, text: String) -> bool {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let store = REGEX_STORE.lock().unwrap();
        if let Some(re) = store.get(&handle) {
            re.is_match(&text)
        } else {
            false
        }
    }));
    result.unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn regex_drop(handle: u64) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        REGEX_STORE.lock().unwrap().remove(&handle);
    }));
}
```

**Rules applied:**
- R1: No raw pointers. Buff sees `Int` (handle ID), not a pointer to the `Regex` struct.
- R2: Rust owns the `HashMap` and all `Regex` values. Buff only holds integer IDs. `regex_drop` tells Rust when to free.
- R3: Compile errors print to stderr and return sentinel value. A production wrapper would return a `Result` type.
- R4: `Regex` is `Send + Sync`, and the `Mutex<HashMap>` is `Send + Sync`. Safe to use across `spawn` boundaries.
- R5: No lifetimes exposed. All strings are owned. The `Regex` struct lives in Rust's heap with a stable integer key.
- R6: Every function uses `catch_unwind`.

---

### Example 3: Async function wrapper (reqwest::get)

Wrapping an async Rust function. The wrapper runs the future on a tokio runtime and returns the result synchronously to Buff.

**Buff side:**

```buff
extern "C" from "buff-http" func fetch_text(url: String) -> String
```

**Rust side (`externs.rs`):**

```rust
use std::panic::{catch_unwind, AssertUnwindSafe};

#[no_mangle]
pub extern "C" fn fetch_text(url: String) -> String {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("failed to create runtime: {}", e))?;
        rt.block_on(async {
            match reqwest::get(&url).await {
                Ok(resp) => match resp.text().await {
                    Ok(body) => Ok(body),
                    Err(e) => Err(format!("failed to read response body: {}", e)),
                },
                Err(e) => Err(format!("HTTP request failed: {}", e)),
            }
        })
    }));
    match result {
        Ok(Ok(body)) => body,
        Ok(Err(e)) => e,
        Err(_) => "internal error: HTTP client panicked".to_string(),
    }
}
```

**Rules applied:**
- R1: No raw pointers. Pure owned-string interface.
- R2: The runtime allocates and drops internally. Buff gets an owned `String`.
- R3: Two layers of error mapping: the HTTP error and the body-reading error both become descriptive strings.
- R6: `catch_unwind` at the outermost level catches panics from both the runtime creation and the async logic.

**Note on async patterns:** A more sophisticated wrapper would reuse a single runtime rather than creating one per call. The example above is intentionally minimal to show the safety boundary. Production wrappers should hold a long-lived runtime in a `lazy_static!` or `once_cell` and dispatch futures onto it.

---

### Example 4: Anti-pattern (raw pointer exposure)

This example shows what NOT to do. It violates Rule 1, Rule 2, and Rule 5 simultaneously.

**WRONG Buff side:**

```buff
// DANGEROUS: exposes raw memory through the FFI boundary
extern "C" from "buff-image" func image_get_pixels(handle: Int) -> Vector<UInt8>
extern "C" from "buff-image" func image_create_buffer(width: Int, height: Int) -> Int
```

**WRONG Rust side:**

```rust
// ANTI-PATTERN: Do NOT write code like this

#[no_mangle]
pub extern "C" fn image_get_pixels(handle: u64) -> *const u8 {
    // VIOLATION R1: returns a raw pointer
    // VIOLATION R2: Buff code now holds a pointer into Rust's heap
    //   with no way to know when Rust frees it
    // VIOLATION R5: the returned pointer has a lifetime tied to
    //   the Image struct's internal buffer, which Buff cannot express
    let images = IMAGE_STORE.lock().unwrap();
    let img = images.get(&handle).unwrap();
    img.buffer.as_ptr() // dangling risk if Image is dropped
}

#[no_mangle]
pub extern "C" fn image_create_buffer(width: u64, height: u64) -> *mut u8 {
    // VIOLATION R1: returns a mutable raw pointer
    // VIOLATION R2: who frees this allocation? If Rust does,
    //   Buff has a dangling pointer. If Buff does, it needs to
    //   call Rust's allocator, which is a separate FFI concern.
    let size = (width * height * 4) as usize;
    let layout = std::alloc::Layout::from_size_align(size, 8).unwrap();
    unsafe { std::alloc::alloc(layout) as *mut u8 }
}
```

**Why this is dangerous:**

1. `image_get_pixels` returns a pointer into the Image struct's internal buffer. If the Image is dropped (via `image_drop` or a store cleanup), the pointer dangles. Buff code holding that pointer has no way to know.

2. `image_create_buffer` allocates memory via Rust's global allocator and returns a raw pointer. There is no mechanism for Buff code to free this memory correctly unless it calls a corresponding `free_buffer` wrapper, and even then the allocation layout must match exactly.

3. Neither function uses `catch_unwind` (R6 violation). A panic in the locking or lookup code would unwind across the `extern "C"` boundary, which is UB.

**Correct approach:**

Return owned `Vector<UInt8>` (lowered to `Vec<u8>`) and let Rust's normal drop handle deallocation:

```rust
#[no_mangle]
pub extern "C" fn image_get_pixels(handle: u64) -> Vec<u8> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let images = IMAGE_STORE.lock().unwrap();
        match images.get(&handle) {
            Some(img) => Ok(img.buffer.clone()),
            None => Err("invalid image handle".to_string()),
        }
    }));
    match result {
        Ok(Ok(data)) => data,
        Ok(Err(e)) => { eprintln!("{}", e); Vec::new() },
        Err(_) => Vec::new(),
    }
}
```

The clone costs an allocation, but the result is safe, owned, and correct. Buff's generated code receives a `Vec<u8>` that Rust drops normally when the Buff binding goes out of scope.

---

## Checklist for wrapper authors

Before submitting a wrapper crate for review, verify each item:

- [ ] No `*const T` or `*mut T` in any public signature visible to Buff.
- [ ] All heap memory owned by Rust. Buff holds handles or owned copies only.
- [ ] Every `Result<T, E>` mapped to an error-prefixed string (or future `BuffError`).
- [ ] All types that may cross `spawn` are `Send + 'static`. Verified with `cargo check`.
- [ ] No Rust lifetimes in any Buff-visible type. All borrowed types converted to owned at the boundary.
- [ ] Every public wrapper function wrapped in `std::panic::catch_unwind`.
- [ ] Wrapper compiles with `cargo check -p <wrapper>` and the generated Buff code compiles with `buff check`.

---

## Relationship to the compiler

The Buff compiler (`buff-lang-codegen-rust`) automatically inserts `unsafe { ... }` at every call site of an `extern` function. This happens in `rust_codegen.rs` around line 2487. The `extern_fn_names` set is populated by `collect_extern_fn_names` (line 8960), which collects names from both legacy `Decl::FuncDecl(f) where f.is_extern` and new `Decl::ExternFuncDecl` forms.

The compiler's unsafe wrapping is a safety net, not a substitute for the rules in this guide. The wrapper author is responsible for ensuring the function body is safe. The compiler ensures the call site is syntactically correct.

---

## External references

- Rust FFI guide: <https://doc.rust-lang.org/nomicon/ffi.html>
- Rust unsafe code guidelines: <https://rust-lang.github.io/unsafe-code-guidelines/>
- Buff conventions: `.sisyphus/plans/buff-conventions.md`
- Buff extern examples: `examples/extern_serde_json.buff`, `examples/extern_tokio.buff`, `examples/extern_reqwest.buff`
