//! T46: VRAM-aware tiling dispatcher with CPU fallback.
//!
//! When an input is too large to fit VRAM in a single GPU dispatch, this
//! module splits it into tiles that each fit, dispatches each tile through
//! any [`GpuBackend`] (T45's [`crate::WgpuBackend`] in production,
//! [`crate::MockGpuBackend`] in tests), and concatenates the per-tile
//! outputs in input order. If even one tile cannot fit, or no GPU is
//! available, the whole computation runs on the CPU via a caller-provided
//! oracle (typically T38b's [`crate::cpu_fallback_map`] or T39's
//! `CpuDispatcher::par_map`).
//!
//! # The VRAM budget formula
//!
//! Each tile's dispatch uses three buffers of `tile_size *
//! bytes_per_element` bytes each:
//!
//! 1. **Input storage** (`var<storage, read>`) — T44 binding 0
//! 2. **Output storage** (`var<storage, read_write>`) — T44 binding 1
//! 3. **Staging buffer** (`MAP_READ | COPY_DST`) — host-visible readback
//!
//! Total VRAM per tile ≈ `3 * tile_size * bytes_per_element`. Given a
//! VRAM budget `V` (in bytes), the maximum tile size that fits is:
//!
//! ```text
//! max_elements_per_tile(V, bpe) = V / (3 * bpe)
//! ```
//!
//! wgpu 26 does not expose total VRAM; the practical per-buffer cap is
//! `device.limits().max_storage_buffer_binding_size` (with
//! `max_buffer_size` as a secondary ceiling). [`vram_budget_from_device`]
//! returns the min of those two as the budget `V`.
//!
//! # Determinism
//!
//! Tiles are processed sequentially in input order, and each tile's
//! output is concatenated to the running output `Vec` via `extend`. No
//! interior reordering, no hashing. Same `(shader, input, max_tile)` →
//! byte-identical output on every run.
//!
//! For an element-wise map kernel (T44's only v1.0 scope), tiling does
//! not change the result — the kernel has no inter-element dependencies.
//! The tiled output is therefore identical to the single-dispatch output
//! and to the CPU oracle.
//!
//! # `max_tile == 0` semantics — IMPORTANT
//!
//! The meaning of `max_tile == 0` differs by API level:
//!
//! * **[`tile_ranges`]** (low-level pure helper): treats 0 as "no
//!   tiling" — returns a single range covering the whole input. Useful
//!   for callers that want to disable tiling manually.
//! * **[`dispatch_tiled`]** / **[`TiledDispatcher`]** (mid-level):
//!   inherit [`tile_ranges`] semantics — `max_tile == 0` produces a
//!   single dispatch over the whole input.
//! * **[`dispatch_map_with_tiling`]** (high-level entry with CPU
//!   fallback): treats 0 as "VRAM budget too small to fit even one
//!   element — use CPU". This is because [`max_elements_per_tile`]
//!   returns 0 precisely when the budget can't fit a single element.
//!
//! No `unwrap`/`expect`/`panic!`/`todo!` in non-test code. No
//! [`std::collections::HashMap`] / [`std::collections::HashSet`]
//! (project hard rule).

use crate::error::RuntimeError;
use crate::mock_gpu::GpuBackend;

/// Compute the contiguous `(start, end)` ranges that tile `0..total_len`
/// into chunks of at most `max_tile` elements.
///
/// # Behaviour
///
/// * `total_len == 0` → empty `Vec` (no tiles needed).
/// * `max_tile == 0` → single tile covering the whole input
///   `[(0, total_len)]` (if `total_len > 0`), else empty. Documented as
///   "no tiling" — lets callers pass 0 to disable tiling.
/// * `total_len <= max_tile` → single tile `[(0, total_len)]`.
/// * Otherwise → `ceil(total_len / max_tile)` tiles, each of size
///   `max_tile` except the last which is `total_len % max_tile` (or
///   `max_tile` if `total_len` is an exact multiple).
///
/// # QA case
///
/// ```
/// use buff_lang_runtime::tile_ranges;
/// assert_eq!(tile_ranges(250, 100), vec![(0, 100), (100, 200), (200, 250)]);
/// ```
///
/// 3 tiles for 250 elements at `max_tile=100` — matches the T46 QA
/// spec verbatim.
///
/// # Determinism
///
/// Pure function — no allocation beyond the returned `Vec`, no hashing,
/// no floats. Same inputs → byte-identical output.
#[must_use]
pub fn tile_ranges(total_len: usize, max_tile: usize) -> Vec<(usize, usize)> {
    if total_len == 0 {
        return Vec::new();
    }
    if max_tile == 0 {
        return vec![(0, total_len)];
    }

    // Pre-allocate the exact capacity: ceil(total_len / max_tile).
    // On overflow (max_tile == 0 already handled above; total_len / 1 is
    // total_len which can't overflow usize::MAX), this is sound.
    let capacity = total_len.div_ceil(max_tile);
    let mut ranges = Vec::with_capacity(capacity);
    let mut start = 0;
    while start < total_len {
        let end = (start + max_tile).min(total_len);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

/// Maximum number of elements per tile, given a VRAM budget in bytes and
/// the per-element byte size.
///
/// # Formula
///
/// `max_elements_per_tile = vram_budget_bytes / (3 * bytes_per_element)`
///
/// The factor of 3 reserves headroom for the three buffers each dispatch
/// uses (input storage + output storage + host-visible staging). Each
/// individual buffer is `tile_size * bytes_per_element` bytes, so the
/// total VRAM consumed per tile is `3 * tile_size * bytes_per_element`.
///
/// # Edge cases
///
/// * `bytes_per_element == 0` → returns 0 (avoid division by zero).
/// * `vram_budget_bytes < 3 * bytes_per_element` → returns 0 (can't fit
///   even one element; caller should fall back to CPU).
/// * Multiplication overflow (`3 * bpe` overflows `u64`) → returns 0.
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::max_elements_per_tile;
/// // 1200 bytes budget, 4 bpe: 1200 / (3*4) = 100 elements per tile.
/// assert_eq!(max_elements_per_tile(1200, 4), 100);
/// // 11 bytes budget, 4 bpe: 11 < 12, can't fit one element.
/// assert_eq!(max_elements_per_tile(11, 4), 0);
/// ```
#[must_use]
pub fn max_elements_per_tile(vram_budget_bytes: u64, bytes_per_element: u64) -> usize {
    if bytes_per_element == 0 {
        return 0;
    }
    // 3 * bpe — saturating so a huge bpe doesn't overflow into a small
    // number that would falsely satisfy the `vram_budget < per_element_total`
    // check below.
    let per_element_total = 3u64.saturating_mul(bytes_per_element);
    if per_element_total == 0 || vram_budget_bytes < per_element_total {
        return 0;
    }
    // u64 / u64 → u64. Cast to usize at the end. On 64-bit platforms
    // (everywhere Buff runs in practice) this is lossless; on 32-bit
    // platforms the saturating cast to usize::MAX is a defensible
    // fallback (a tile that large couldn't be allocated anyway).
    let elements = vram_budget_bytes / per_element_total;
    usize::try_from(elements).unwrap_or(usize::MAX)
}

/// Query the practical per-tile VRAM budget (in bytes) from a wgpu device.
///
/// wgpu 26 does not expose total VRAM. The binding constraints on a
/// single dispatch are:
///
/// * `max_storage_buffer_binding_size` — largest buffer binding the
///   device accepts (u32; always ≤ 4 GiB on current hardware).
/// * `max_buffer_size` — largest buffer the device can allocate
///   (u64 in wgpu 26 to accommodate > 4 GiB allocations on discrete GPUs).
///
/// We return the **min** of the two as the practical per-tile budget:
/// each tile's input/output/staging buffers must satisfy BOTH limits.
/// In practice `max_storage_buffer_binding_size <= max_buffer_size`
/// (you can't bind more bytes than the buffer holds), so the min usually
/// equals `max_storage_buffer_binding_size` — but the min is taken
/// defensively in case a future wgpu version inverts this.
///
/// # Example
///
/// ```ignore
/// use buff_lang_runtime::{vram_budget_from_device, max_elements_per_tile, WgpuBackend};
///
/// let backend = WgpuBackend::new()?;
/// let device = backend.context().device()?;
/// let budget = vram_budget_from_device(device);
/// let max_tile = max_elements_per_tile(budget, 4);
/// // max_tile is now the largest f32-tile that fits this GPU's bindings.
/// ```
#[must_use]
pub fn vram_budget_from_device(device: &wgpu::Device) -> u64 {
    let limits = device.limits();
    let binding_cap: u64 = u64::from(limits.max_storage_buffer_binding_size);
    let buffer_cap: u64 = limits.max_buffer_size;
    binding_cap.min(buffer_cap)
}

/// Run a tiled GPU dispatch over `input`, splitting into chunks of at
/// most `max_tile_elements` and dispatching each chunk through `backend`.
///
/// Each tile's output is concatenated to the result `Vec` in input order.
/// The final `Vec` has the same length as `input` (the kernel is an
/// element-wise map — one output per input).
///
/// # Errors
///
/// * [`RuntimeError::GpuUnavailable`] — backend has no GPU adapter.
/// * [`RuntimeError::GpuInit`] — individual tile dispatch failure
///   (shader compile error, buffer allocation failure, map_async error).
/// * [`RuntimeError::Unsupported`] — a tile's output length doesn't
///   match its input length (broken shader that didn't honor the
///   element-wise map contract).
///
/// # `max_tile_elements == 0`
///
/// Treated as "no tiling" — a single tile covers the whole input
/// (delegates to [`tile_ranges`]). The high-level
/// [`dispatch_map_with_tiling`] interprets 0 differently (CPU fallback);
/// pick the API that matches your intent.
///
/// # Empty input
///
/// Returns `Ok(Vec::new())` without touching the backend.
pub fn dispatch_tiled(
    backend: &dyn GpuBackend,
    shader_wgsl: &str,
    input: &[f32],
    max_tile_elements: usize,
) -> Result<Vec<f32>, RuntimeError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let ranges = tile_ranges(input.len(), max_tile_elements);
    // Pre-allocate the result. For an element-wise map, output.len() ==
    // input.len(), so we can reserve the exact capacity and avoid any
    // reallocation as tiles are appended.
    let mut output = Vec::with_capacity(input.len());

    for (start, end) in ranges {
        let tile = &input[start..end];
        let tile_out = backend.dispatch_map(shader_wgsl, tile)?;
        // Defensive: a correct element-wise map produces exactly
        // tile.len() outputs. If the shader broke that contract, surface
        // it as a structured error instead of returning a malformed Vec.
        if tile_out.len() != tile.len() {
            return Err(RuntimeError::Unsupported {
                detail: format!(
                    "tiled dispatch: tile @ [{start}, {end}) produced {} outputs, expected {}",
                    tile_out.len(),
                    tile.len()
                ),
                span: None,
            });
        }
        output.extend(tile_out);
    }

    Ok(output)
}

/// Fluent wrapper around [`dispatch_tiled`] that remembers the backend
/// and max-tile so callers don't have to pass them on every call.
///
/// Construct with [`TiledDispatcher::new`], then call [`Self::dispatch`]
/// for each input. The dispatcher borrows the backend immutably —
/// multiple dispatchers can share one backend via `&` references.
///
/// # Example
///
/// ```ignore
/// use buff_lang_runtime::{TiledDispatcher, WgpuBackend};
///
/// let backend = WgpuBackend::new()?;
/// let dispatcher = TiledDispatcher::new(&backend, 4096);
/// let out1 = dispatcher.dispatch(SHADER, &[1.0, 2.0, 3.0])?;
/// let out2 = dispatcher.dispatch(SHADER, &[4.0, 5.0])?;
/// ```
///
/// # Send + Sync
///
/// `TiledDispatcher<'a>` is `Send + Sync` because `&dyn GpuBackend` is
/// (the trait requires `Send + Sync` as supertraits).
#[derive(Debug)]
pub struct TiledDispatcher<'a> {
    /// Borrowed GPU backend (real `WgpuBackend` or test `MockGpuBackend`).
    backend: &'a dyn GpuBackend,
    /// Max elements per tile. See [`tile_ranges`] for `0` semantics.
    max_tile_elements: usize,
}

impl<'a> TiledDispatcher<'a> {
    /// Construct a dispatcher that will split inputs into tiles of at
    /// most `max_tile_elements` elements per dispatch.
    #[must_use]
    pub fn new(backend: &'a dyn GpuBackend, max_tile_elements: usize) -> Self {
        Self {
            backend,
            max_tile_elements,
        }
    }

    /// Run a tiled dispatch over `input`. See [`dispatch_tiled`] for the
    /// per-tile semantics.
    ///
    /// # Errors
    ///
    /// Same error conditions as [`dispatch_tiled`].
    pub fn dispatch(&self, shader_wgsl: &str, input: &[f32]) -> Result<Vec<f32>, RuntimeError> {
        dispatch_tiled(self.backend, shader_wgsl, input, self.max_tile_elements)
    }

    /// Borrow the underlying backend (observational only — exists for
    /// diagnostic assertions in tests).
    #[must_use]
    pub fn backend(&self) -> &dyn GpuBackend {
        self.backend
    }

    /// The max-elements-per-tile this dispatcher was constructed with.
    #[must_use]
    pub fn max_tile_elements(&self) -> usize {
        self.max_tile_elements
    }
}

/// Top-level dispatch entry: decides GPU-tiled vs CPU fallback and
/// always returns the correct `Vec<f32>`.
///
/// # Decision tree
///
/// 1. **Empty input** → return empty `Vec` directly (no work).
/// 2. **`gpu_backend == None`** → no GPU available → run
///    `cpu_oracle(input)`.
/// 3. **`max_tile_elements == 0`** → VRAM budget too small to fit even
///    one element (i.e. [`max_elements_per_tile`] returned 0) → run
///    `cpu_oracle(input)`.
/// 4. **Otherwise** → attempt [`dispatch_tiled`]. On ANY error
///    (`GpuUnavailable`, `GpuInit`, etc), fall back to
///    `cpu_oracle(input)`.
///
/// The CPU oracle is typically T38b's [`crate::cpu_fallback_map`]
/// (sequential, deterministic) or T39's `CpuDispatcher::par_map`
/// (parallel, rayon-backed).
///
/// # Why "always returns `Vec<f32>`" (not `Result`)
///
/// The CPU oracle is infallible — both [`crate::cpu_fallback_map`] and
/// `par_map` return `Vec<f32>` directly, not `Result`. So this function
/// can promise to always return a value: GPU failure is invisible to
/// the caller, masked by the CPU fallback. This is the contract the
/// T46 task spec demands ("always returns the correct `Vec<f32>`").
///
/// # Example
///
/// ```
/// use buff_lang_runtime::{cpu_fallback_map, dispatch_map_with_tiling};
///
/// let input = vec![1.0_f32, 2.0, 3.0, 4.0, 5.0];
/// let out = dispatch_map_with_tiling(
///     None,                    // no GPU → straight to CPU
///     "@compute ...",          // shader source (unused on CPU path)
///     &input,
///     100,                     // max_tile_elements (irrelevant on CPU path)
///     |input| cpu_fallback_map(input, |x| x * 2.0),
/// );
/// assert_eq!(out, vec![2.0, 4.0, 6.0, 8.0, 10.0]);
/// ```
#[must_use]
pub fn dispatch_map_with_tiling<F>(
    gpu_backend: Option<&dyn GpuBackend>,
    shader_wgsl: &str,
    input: &[f32],
    max_tile_elements: usize,
    cpu_oracle: F,
) -> Vec<f32>
where
    F: Fn(&[f32]) -> Vec<f32>,
{
    if input.is_empty() {
        return Vec::new();
    }

    let Some(backend) = gpu_backend else {
        // No GPU available — straight to CPU.
        return cpu_oracle(input);
    };

    // max_tile_elements == 0 from max_elements_per_tile() means VRAM
    // budget can't fit a single element. Tiling is impossible; CPU.
    // (If the caller explicitly wants "no tiling" via tile_ranges, they
    // should call dispatch_tiled directly with max_tile=0.)
    if max_tile_elements == 0 {
        return cpu_oracle(input);
    }

    match dispatch_tiled(backend, shader_wgsl, input, max_tile_elements) {
        Ok(out) => out,
        Err(err) => {
            // T70: BUFF_FAIL_LOUD_GPU — development debugging tool.
            // When set (any non-empty value), GPU dispatch failure
            // panics with a diagnostic message instead of silently
            // falling back to CPU. Zero overhead when absent.
            if let Ok(val) = std::env::var("BUFF_FAIL_LOUD_GPU") {
                if !val.is_empty() {
                    panic!(
                        "BUFF_FAIL_LOUD_GPU: GPU dispatch failed in \
                         dispatch_map_with_tiling (input.len={}, max_tile={}): {}",
                        input.len(),
                        max_tile_elements,
                        err,
                    );
                }
            }
            // Defensive: any GPU-side failure → CPU. Includes GpuUnavailable
            // (which we couldn't pre-check without querying the backend) and
            // GpuInit (device init, shader compile, buffer alloc, etc).
            // The CPU oracle is infallible, so this fallback always succeeds.
            cpu_oracle(input)
        }
    }
}

#[cfg(test)]
mod tests {
    //! Inline unit tests for the pure helpers. All test names contain
    //! `tiling` so the QA filter `cargo test -p buff-lang-runtime tiling`
    //! matches them via the module path `tiling::tests::*`.

    use super::*;

    // ----- tile_ranges ----------------------------------------------------

    #[test]
    fn tiling_tile_ranges_qa_250_at_100_yields_3_tiles() {
        assert_eq!(
            tile_ranges(250, 100),
            vec![(0, 100), (100, 200), (200, 250)]
        );
    }

    #[test]
    fn tiling_tile_ranges_empty_input_yields_no_tiles() {
        assert!(tile_ranges(0, 100).is_empty());
    }

    #[test]
    fn tiling_tile_ranges_input_le_max_yields_single_tile() {
        assert_eq!(tile_ranges(50, 100), vec![(0, 50)]);
        assert_eq!(tile_ranges(100, 100), vec![(0, 100)]);
    }

    #[test]
    fn tiling_tile_ranges_singleton_input() {
        assert_eq!(tile_ranges(1, 100), vec![(0, 1)]);
    }

    #[test]
    fn tiling_tile_ranges_max_tile_one_yields_n_tiles() {
        assert_eq!(tile_ranges(3, 1), vec![(0, 1), (1, 2), (2, 3)]);
    }

    #[test]
    fn tiling_tile_ranges_max_tile_zero_disables_tiling() {
        assert_eq!(tile_ranges(250, 0), vec![(0, 250)]);
        // Combined with empty input: still empty.
        assert!(tile_ranges(0, 0).is_empty());
    }

    #[test]
    fn tiling_tile_ranges_exact_multiple_no_partial_tile() {
        assert_eq!(tile_ranges(200, 100), vec![(0, 100), (100, 200)]);
        assert_eq!(
            tile_ranges(300, 100),
            vec![(0, 100), (100, 200), (200, 300)]
        );
    }

    // ----- max_elements_per_tile ------------------------------------------

    #[test]
    fn tiling_max_elements_per_tile_basic_formula() {
        // 1200 / (3*4) = 100
        assert_eq!(max_elements_per_tile(1200, 4), 100);
        // 2400 / (3*8) = 100
        assert_eq!(max_elements_per_tile(2400, 8), 100);
    }

    #[test]
    fn tiling_max_elements_per_tile_zero_budget() {
        assert_eq!(max_elements_per_tile(0, 4), 0);
    }

    #[test]
    fn tiling_max_elements_per_tile_cant_fit_one_element() {
        // 11 < 12 (= 3 * 4)
        assert_eq!(max_elements_per_tile(11, 4), 0);
        // exactly 12 → fits 1 element
        assert_eq!(max_elements_per_tile(12, 4), 1);
    }

    #[test]
    fn tiling_max_elements_per_tile_zero_bpe_returns_zero() {
        // Avoid divide-by-zero: return 0.
        assert_eq!(max_elements_per_tile(1200, 0), 0);
    }
}
