//! T38b: Mock GPU backend + CPU-fallback oracle + WGSL snapshot harness.
//!
//! This module gives T45 (real wgpu dispatch pipeline), T46 (tiling), and
//! T47 (cold-start pooling) a way to be unit-tested WITHOUT a real GPU:
//!
//! * [`GpuBackend`] — the object-safe trait that every "run a WGSL map
//!   kernel over an `&[f32]`, return the output" backend implements.
//!   T45 will impl it for a real wgpu-backed type; [`MockGpuBackend`] impls
//!   it here.
//! * [`MockGpuBackend`] — records every [`GpuBackend::dispatch_map`] call
//!   in interior-mutable state (`std::sync::Mutex<Vec<DispatchRecord>>`)
//!   and produces the "GPU" output via a caller-provided CPU closure (so
//!   tests get deterministic results while still exercising the dispatch
//!   recording path). Works with NO real GPU — never touches wgpu.
//! * [`DispatchRecord`] — `{ shader, input_len }` for each recorded
//!   dispatch, queryable by tests via [`MockGpuBackend::records`].
//! * [`cpu_fallback_map`] — a deterministic per-element CPU map (the
//!   oracle that GPU output is compared against in tests). Sequential,
//!   no allocation beyond the output `Vec`, no threads.
//!
//! # Why object-safety matters
//!
//! T45 will hold a `Box<dyn GpuBackend>` so the runtime can pick between
//! the real backend and this mock at execution time. The trait therefore
//! avoids generic methods and `Self`-by-value receivers — both of which
//! would break `dyn GpuBackend`. This mirrors the existing
//! [`crate::Dispatcher`] pattern.
//!
//! # Determinism
//!
//! Records are stored in a `Mutex<Vec<...>>` — push order is the test's
//! dispatch order, which is deterministic (no interior reordering, no
//! hashing). No [`std::collections::HashMap`] / [`std::collections::HashSet`]
//! anywhere in this module (project hard rule).
//!
//! # No-panic contract
//!
//! Every public method returns a value or `Result` — never panics.
//! [`MockGpuBackend::dispatch_map`] ignores a poisoned mutex (it just
//! skips recording) and still returns `Ok` with the CPU-computed output,
//! because the test oracle is more valuable than the record-keeping.

use std::fmt;
use std::sync::Mutex;

use crate::error::RuntimeError;

/// An object-safe "run this WGSL map kernel over this input, return the
/// output" backend.
///
/// T45 will implement this for a real wgpu-backed type (storage buffers +
/// compute pass + readback). [`MockGpuBackend`] implements it here with a
/// CPU closure oracle so T45/T46/T47 logic can be unit-tested without a
/// GPU.
///
/// # Shape
///
/// Single method, no generics, no `Self`-by-value — so callers may hold
/// `Box<dyn GpuBackend>` and swap backends at runtime (mirrors
/// [`crate::Dispatcher`]).
///
/// # Why `dispatch_map` (not `dispatch`/`launch`)
///
/// Buff's v1.0 GPU scope is **element-wise map kernels** only (T44 codegen
/// produces one WGSL `@compute` shader per `{ x => <expr> }` lambda). The
/// trait therefore models exactly that shape: WGSL source in, `&[f32]` in,
/// `Vec<f32>` out. Reductions/scans/gather kernels land post-v1.0 and will
/// extend this trait non-breakingly (default-bodied methods).
///
/// # Errors
///
/// Implementors return [`RuntimeError::GpuUnavailable`] when no GPU is
/// present, [`RuntimeError::GpuInit`] when the device pipeline fails, or
/// [`RuntimeError::Unsupported`] for shape/stride mismatches. The mock
/// never errors — its oracle always succeeds (so tests focus on the
/// dispatch-recording behavior, not error paths).
pub trait GpuBackend: fmt::Debug + Send + Sync {
    /// Run `shader_wgsl` as a compute kernel over `input`, returning the
    /// element-wise output `Vec<f32>`.
    ///
    /// Implementors MUST:
    /// 1. Accept the SAME WGSL source that T44 codegen produces (stable
    ///    binding layout: `@group(0) @binding(0)=input`,
    ///    `@binding(1)=output`, `@compute @workgroup_size(64)`).
    /// 2. Preserve input element order in the output (T44 codegen already
    ///    emits an ordered `for (i=0..N) { output[i] = f(input[i]); }`
    ///    pattern, so the contract is naturally met).
    /// 3. Never panic — return [`RuntimeError`] on any failure.
    fn dispatch_map(&self, shader_wgsl: &str, input: &[f32]) -> Result<Vec<f32>, RuntimeError>;
}

/// One recorded dispatch call — what [`MockGpuBackend`] stores per
/// [`GpuBackend::dispatch_map`] invocation.
///
/// Queryable by tests via [`MockGpuBackend::records`]. Both fields are
/// deterministic (no pointers, no thread ids, no timestamps).
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchRecord {
    /// The WGSL shader source passed to [`GpuBackend::dispatch_map`].
    /// Stored as `String` so tests can pattern-match on substrings
    /// (e.g. `record.shader.contains("@compute")`).
    pub shader: String,
    /// Number of `f32` elements in the input slice. Stored separately
    /// from the (not-retained) input data so the record stays cheap and
    /// `Clone`.
    pub input_len: usize,
}

/// Mock GPU backend — records every dispatch and produces output via a
/// caller-provided CPU closure.
///
/// Construct with [`MockGpuBackend::new`], passing the CPU oracle closure
/// that should be applied to each input. The closure receives the whole
/// `&[f32]` input and returns the full output `Vec<f32>` (use
/// [`cpu_fallback_map`] inside it for the common per-element case):
///
/// ```ignore
/// use buff_lang_runtime::{GpuBackend, MockGpuBackend, cpu_fallback_map};
///
/// let backend = MockGpuBackend::new(|input: &[f32]| {
///     cpu_fallback_map(input, |x| x * 2.0)
/// });
/// let input = vec![1.0_f32, 2.0, 3.0];
/// let out = backend.dispatch_map("@compute ...", &input).unwrap();
/// assert_eq!(out, vec![2.0, 4.0, 6.0]);
/// assert_eq!(backend.recorded_dispatches(), 1);
/// ```
///
/// # Interior mutability
///
/// Records are stored in a [`std::sync::Mutex::new`] `Vec<DispatchRecord>`.
/// The mutex is never held across the CPU closure call (recorded BEFORE
/// the closure runs, lock released immediately), so a panic inside the
/// closure cannot poison the mutex via the recording path.
///
/// # No GPU required
///
/// This type never touches wgpu. It works on any host — that's the whole
/// point. The CPU closure is the "GPU".
///
/// # Thread-safety
///
/// `Send + Sync` because the closure bound is `Fn + Send + Sync` and the
/// only interior-mutable state is the `Mutex<Vec<_>>`. Held as
/// `Box<dyn GpuBackend>` in cross-thread tests without issue.
pub struct MockGpuBackend<F>
where
    F: Fn(&[f32]) -> Vec<f32> + Send + Sync,
{
    /// Caller-provided CPU oracle — receives the whole input slice,
    /// returns the full output. Used to produce the "GPU" output that
    /// [`GpuBackend::dispatch_map`] returns.
    cpu_fn: F,
    /// Every dispatch call, in invocation order. Interior-mutable so
    /// [`GpuBackend::dispatch_map`] (which takes `&self`) can push.
    records: Mutex<Vec<DispatchRecord>>,
}

impl<F> MockGpuBackend<F>
where
    F: Fn(&[f32]) -> Vec<f32> + Send + Sync,
{
    /// Construct a mock backend with a caller-provided CPU oracle.
    ///
    /// The oracle receives the whole input slice and returns the full
    /// output `Vec<f32>`. For the common per-element map case, use
    /// [`cpu_fallback_map`] inside the closure:
    ///
    /// ```ignore
    /// let backend = MockGpuBackend::new(|input: &[f32]| {
    ///     cpu_fallback_map(input, |x| x * 2.0)
    /// });
    /// ```
    ///
    /// Starts with zero recorded dispatches — see [`Self::recorded_dispatches`].
    #[must_use]
    pub fn new(cpu_fn: F) -> Self {
        Self {
            cpu_fn,
            records: Mutex::new(Vec::new()),
        }
    }

    /// How many dispatches have been recorded (the QA accessor).
    ///
    /// Matches the T38b QA spec literal: `assert_eq!(backend.recorded_dispatches(), 1)`
    /// after one [`GpuBackend::dispatch_map`] call. Returns `0` on a
    /// poisoned mutex (which cannot happen in normal operation since the
    /// lock is never held across a panic-risk boundary).
    #[must_use]
    pub fn recorded_dispatches(&self) -> usize {
        self.records.lock().map(|guard| guard.len()).unwrap_or(0)
    }

    /// Alias for [`Self::recorded_dispatches`] (some test sites prefer the
    /// "count" spelling). Same value.
    #[must_use]
    pub fn dispatch_count(&self) -> usize {
        self.recorded_dispatches()
    }

    /// Snapshot of every recorded dispatch, in invocation order.
    ///
    /// Returns a `Vec<DispatchRecord>` (cloned from the lock guard) so
    /// tests can assert on the captured shader source and input length
    /// without holding the mutex. Empty if no dispatches have occurred.
    ///
    /// Returns `Vec::new()` on a poisoned mutex (cannot happen in normal
    /// operation — see [`Self::recorded_dispatches`]).
    #[must_use]
    pub fn records(&self) -> Vec<DispatchRecord> {
        self.records.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// Drop all recorded dispatches. Useful when a test reuses a backend
    /// across multiple sub-assertions and wants to reset the count.
    ///
    /// No-op on a poisoned mutex (cannot happen in normal operation).
    pub fn clear_records(&self) {
        if let Ok(mut records) = self.records.lock() {
            records.clear();
        }
    }
}

impl<F> GpuBackend for MockGpuBackend<F>
where
    F: Fn(&[f32]) -> Vec<f32> + Send + Sync,
{
    fn dispatch_map(&self, shader_wgsl: &str, input: &[f32]) -> Result<Vec<f32>, RuntimeError> {
        // Record FIRST (before invoking the CPU closure) so the lock is
        // released before any potentially-panicking user code runs. If
        // the closure panics, the mutex is NOT poisoned via this path.
        if let Ok(mut records) = self.records.lock() {
            records.push(DispatchRecord {
                shader: shader_wgsl.to_string(),
                input_len: input.len(),
            });
        }
        // If the lock was poisoned (some other thread panicked while
        // holding it — impossible in this module but possible if a
        // downstream user wraps the backend), we still produce the
        // oracle output: tests care about the output more than the
        // record-keeping.
        Ok((self.cpu_fn)(input))
    }
}

impl<F> fmt::Debug for MockGpuBackend<F>
where
    F: Fn(&[f32]) -> Vec<f32> + Send + Sync,
{
    // Manual Debug impl — closures don't auto-derive Debug. Reports the
    // record count so debug-spans / tracing show useful state.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.records.lock().map(|guard| guard.len()).unwrap_or(0);
        f.debug_struct("MockGpuBackend")
            .field("recorded_dispatches", &count)
            .finish_non_exhaustive()
    }
}

/// CPU-fallback oracle — the deterministic per-element map that GPU
/// output is compared against in tests.
///
/// Sequential, no threads, no allocation beyond the output `Vec<f32>`.
/// Same input + same closure → byte-identical output on every host, every
/// run (inherent to sequential iterator semantics — no thread scheduling
/// nondeterminism).
///
/// # When to use this
///
/// * Inside a [`MockGpuBackend`]'s CPU closure — so the mock produces the
///   oracle output as the "GPU" result.
/// * In tests, as the expected value — `assert_eq!(gpu_output,
///   cpu_fallback_map(&input, |x| ...))`.
///
/// # Why not just use [`crate::CpuDispatcher::par_map`]
///
/// `par_map` is the parallel production path (rayon-backed). It is
/// overkill for an oracle that needs to be maximally simple — sequential
/// `iter().map().collect()` is harder to get wrong and has no thread
/// pool to spin up. The two produce identical element-order results for
/// the same closure (rayon preserves input order), so they may be used
/// interchangeably as oracles; this one is just cheaper.
///
/// # Example
///
/// ```
/// use buff_lang_runtime::cpu_fallback_map;
/// let input = vec![1.0_f32, 2.0, 3.0];
/// let out = cpu_fallback_map(&input, |x| x * 2.0);
/// assert_eq!(out, vec![2.0, 4.0, 6.0]);
/// ```
#[must_use]
pub fn cpu_fallback_map<F>(input: &[f32], f: F) -> Vec<f32>
where
    F: Fn(f32) -> f32,
{
    input.iter().copied().map(f).collect()
}

#[cfg(test)]
mod tests {
    //! Smoke tests at the module level — full behavioral coverage lives
    //! in `tests/gpu_harness_tests.rs` so the QA filter
    //! `cargo test -p buff-lang-runtime gpu_harness` matches.

    use super::*;

    #[test]
    fn gpu_harness_module_smoke_dispatch_records_one() {
        let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
        let input = vec![1.0_f32, 2.0, 3.0];
        let _ = backend.dispatch_map("@compute", &input);
        assert_eq!(backend.recorded_dispatches(), 1);
    }
}
