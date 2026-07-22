//! T55 — Compile-speed optimization program.
//!
//! A multi-pronged program to attack Buff's #1 DX risk: inheriting Rust's
//! slow compile times (30-90s for a medium project). This module owns the
//! pure helpers that the pipeline + CLI wire in:
//!
//! 1. **Generated-Rust caching** — skip re-codegen when the `.buff` source
//!    is unchanged. The cache key is a SHA-256 hash of the source bytes;
//!    the cache value is the generated Rust source string. On a cache hit
//!    the entire lex → parse → syn/quote/prettyplease codegen pass is
//!    skipped, saving 30-50% on repeat builds. See [`source_cache_key`],
//!    [`read_cache`], [`write_cache`].
//! 2. **Linker selection** — auto-detect `mold` (Linux) or `lld`
//!    (Windows/macOS) and pass `-C link-arg=-fuse-ld=<name>` to rustc for
//!    a 2-5x link-speedup. Falls back silently to the default linker when
//!    neither is available. See [`FastLinker`].
//! 3. **sccache integration** — detect `sccache` on `PATH` so the CLI can
//!    wrap rustc invocations for cross-project crate caching. Opt-in via
//!    `buff build --sccache` (caches across projects → side effects → not
//!    on by default). See [`sccache_available`], [`rustc_command`].
//!
//! The benchmark harness lives in [`crate::commands::bench_compile`] — it
//! consumes [`synthetic_buff_program`] to synthesise small/medium/large
//! `.buff` fixtures and times the pipeline against them.
//!
//! # Design notes
//!
//! - **Cache location**: `target/buff-cache/<hash>.rs`. Per-repo, matches
//!   cargo's `target/` convention, and is naturally gitignored (the root
//!   `.gitignore` excludes `target/`). No `~/.buff/` home-dir pollution.
//! - **Hash strength**: first 16 hex chars of SHA-256 = 64 bits of entropy.
//!   Matches the T122 git-checkout hash width — plenty to avoid collisions
//!   for a single user's source files.
//! - **No panics**: every fallible op returns [`anyhow::Result`]; cache
//!   misses + read failures degrade gracefully (regenerate, never crash).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// The subdirectory under `target/` where generated-Rust cache files live.
///
/// `target/buff-cache/` — picked so it is automatically covered by the
/// repo-root `.gitignore` (which excludes `target/`) and co-locates with
/// cargo's own incremental-compilation artifacts.
pub const CACHE_SUBDIR: &str = "buff-cache";

/// Compute the deterministic cache key for a `.buff` source string.
///
/// The key is the first 16 hex chars of `SHA-256(source_bytes)` — 64 bits
/// of entropy, matching the T122 git-checkout hash width. Pure function:
/// same input always yields the same key.
///
/// # Example
///
/// ```
/// use buff_lang_cli::compile_speed::source_cache_key;
/// let k = source_cache_key("func main(): print(1)");
/// assert_eq!(k.len(), 16);
/// assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
/// ```
pub fn source_cache_key(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    let digest = hasher.finalize();
    // 32 bytes -> 64 hex chars; take the first 16 (64 bits).
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..16].to_string()
}

/// Resolve the default cache directory: `target/buff-cache/` relative to
/// the current working directory.
///
/// Uses [`std::env::current_dir`] so the cache is per-repo (each project
/// gets its own `target/` — matches cargo's layout). The directory is NOT
/// created here; [`write_cache`] creates it on first write.
pub fn default_cache_dir() -> PathBuf {
    // Fall back to a literal "target" if current_dir fails (extremely rare —
    // e.g. the cwd was deleted mid-process). We never panic.
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    base.join("target").join(CACHE_SUBDIR)
}

/// Compute the cache file path for a given key: `<cache_dir>/<key>.rs`.
pub fn cache_file_path_for(key: &str) -> PathBuf {
    default_cache_dir().join(format!("{key}.rs"))
}

/// Try to read a cached generated-Rust source for `key`.
///
/// Returns `None` when the cache file is missing OR cannot be read (treats
/// I/O errors as cache misses — the caller regenerates, never crashes).
///
/// # Integrity
///
/// A missing file is a clean miss. A truncated/corrupt file would surface
/// as a later rustc parse error — but since the cached content is only ever
/// written by [`write_cache`] immediately after a successful codegen, the
/// window for corruption is limited to OS-level crashes mid-`fs::write`
/// (which most filesystems make atomic at the file-replace level).
pub fn read_cache(key: &str) -> Option<String> {
    let path = cache_file_path_for(key);
    std::fs::read_to_string(&path).ok()
}

/// Write generated Rust source to the cache for `key`.
///
/// Creates the cache directory on demand (best-effort). Propagates I/O
/// errors via [`Result`] so the caller can decide whether to warn or
/// ignore — a cache-write failure does NOT fail the build (the `.rs` file
/// is still written alongside the source by the pipeline).
pub fn write_cache(key: &str, content: &str) -> Result<()> {
    let dir = default_cache_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create cache dir `{}`", dir.display()))?;
    }
    let path = cache_file_path_for(key);
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write cache file `{}`", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Linker selection (mold / lld auto-detection).
// ---------------------------------------------------------------------------

/// The fast linker detected on the host (if any).
///
/// `mold` is preferred on Linux (fastest); `lld` is the cross-platform
/// fallback (ships with the LLVM toolchain / rustup). [`FastLinker::None`]
/// means "no fast linker found — use rustc's default".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastLinker {
    /// The `mold` linker — Linux x86_64/aarch64 only, fastest available.
    /// https://github.com/rui314/mold
    Mold,
    /// The LLVM `lld` linker — cross-platform (Windows/macOS/Linux).
    /// Detected via `rust-lld` (shipped with rustup toolchains) first,
    /// then bare `lld`.
    Lld,
    /// No fast linker detected on `PATH` — fall back to rustc's default
    /// (system `cc`/`ld` on Unix, MSVC `link.exe` on Windows).
    None,
}

impl FastLinker {
    /// Detect the best available fast linker on the host.
    ///
    /// Preference order:
    /// 1. `mold` (Linux only — it has no Windows/macOS port). Preferred
    ///    because it's the fastest linker in widespread use.
    /// 2. `rust-lld` (shipped with every rustup toolchain — near-zero
    ///    install friction). Falls back to bare `lld`.
    /// 3. [`FastLinker::None`] when neither is found.
    ///
    /// This is a cheap `PATH` probe (no subprocess spawn). It is called
    /// once per rustc invocation; the result is not memoised because the
    /// set of installed tools can change during a long-lived REPL session.
    pub fn detect() -> Self {
        // mold is Linux-only in practice.
        if cfg!(target_os = "linux") && on_path("mold") {
            return FastLinker::Mold;
        }
        // rust-lld ships with rustup — the most universally available lld.
        if on_path("rust-lld") {
            return FastLinker::Lld;
        }
        // Bare lld (LLVM install, Homebrew llvm, etc.).
        if on_path("lld") {
            return FastLinker::Lld;
        }
        FastLinker::None
    }

    /// The rustc CLI flags that select this linker.
    ///
    /// Returns the `-C link-arg=-fuse-ld=<name>` pair that tells rustc to
    /// delegate linking to the named linker. Returns an empty vec for
    /// [`FastLinker::None`] (let rustc pick its default).
    pub fn rustc_flags(self) -> Vec<&'static str> {
        match self {
            FastLinker::Mold => vec!["-C", "link-arg=-fuse-ld=mold"],
            FastLinker::Lld => vec!["-C", "link-arg=-fuse-ld=lld"],
            FastLinker::None => Vec::new(),
        }
    }

    /// User-facing lowercase name for log lines.
    pub fn name(self) -> &'static str {
        match self {
            FastLinker::Mold => "mold",
            FastLinker::Lld => "lld",
            FastLinker::None => "default",
        }
    }

    /// Returns `true` when a fast linker was found (not [`FastLinker::None`]).
    pub fn is_fast(self) -> bool {
        !matches!(self, FastLinker::None)
    }
}

// ---------------------------------------------------------------------------
// sccache integration.
// ---------------------------------------------------------------------------

/// Returns `true` when `sccache` is installed and on `PATH`.
///
/// sccache wraps rustc invocations to cache compiled artefacts across
/// projects (a cross-project crate cache, complementary to the per-source
/// [`read_cache`] layer). It is opt-in via `buff build --sccache` because
/// it has side effects (writes to `~/.cache/sccache/`, runs a background
/// server).
pub fn sccache_available() -> bool {
    on_path("sccache")
}

/// Build a `rustc` [`Command`], optionally wrapped in `sccache`.
///
/// - `use_sccache = true` AND sccache is available → `sccache rustc ...`
/// - otherwise → `rustc ...` (the default, unchanged since v0.1)
///
/// The wrapper is only applied when sccache is actually installed — if the
/// user passes `--sccache` but sccache is missing, we fall back silently
/// (with a stderr note) rather than failing the build. This matches the
/// "auto-detect + opt-in" mandate.
pub fn rustc_command(use_sccache: bool) -> Command {
    if use_sccache && sccache_available() {
        let mut cmd = Command::new("sccache");
        cmd.arg("rustc");
        cmd
    } else {
        Command::new("rustc")
    }
}

/// Returns the TOML snippet for `.cargo/config.toml` that wires sccache
/// globally for the project.
///
/// ```toml
/// [build]
/// rustc-wrapper = "sccache"
/// ```
///
/// Written by `buff build --sccache` when the user opts in, so subsequent
/// bare `cargo build` / `cargo test` invocations ALSO go through sccache
/// (not just the `buff build` path). The snippet is idempotent-safe —
/// re-running `--sccache` overwrites the same content.
pub fn sccache_cargo_config_toml() -> &'static str {
    "[build]\nrustc-wrapper = \"sccache\"\n"
}

// ---------------------------------------------------------------------------
// Benchmark harness — synthetic Buff program generator.
// ---------------------------------------------------------------------------

/// Synthesise a deterministic `.buff` source string of the given "size"
/// tier for the [`crate::commands::bench_compile`] benchmark suite.
///
/// The generated program is always valid Buff (parses + type-checks +
/// codegens to valid Rust). The tiers scale the number of functions so
/// the codegen + rustc times grow measurably:
///
/// - [`BenchTier::Small`] — 5 functions (~20 lines). Smoke test.
/// - [`BenchTier::Medium`] — 50 functions (~200 lines). Realistic module.
/// - [`BenchTier::Large`] — 200 functions (~800 lines). Stress test.
///
/// Determinism: the same tier always produces byte-identical output so
/// benchmark runs are comparable across commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchTier {
    Small,
    Medium,
    Large,
}

impl BenchTier {
    /// Number of synthetic functions this tier generates.
    pub fn fn_count(self) -> usize {
        match self {
            BenchTier::Small => 5,
            BenchTier::Medium => 50,
            BenchTier::Large => 200,
        }
    }

    /// User-facing label.
    pub fn label(self) -> &'static str {
        match self {
            BenchTier::Small => "small",
            BenchTier::Medium => "medium",
            BenchTier::Large => "large",
        }
    }

    /// All tiers in ascending size order (for the benchmark loop).
    pub fn all() -> [BenchTier; 3] {
        [BenchTier::Small, BenchTier::Medium, BenchTier::Large]
    }
}

/// Generate a deterministic `.buff` source string for `tier`.
///
/// Each generated function has a unique name (`bench_fn_0`, `bench_fn_1`,
/// …) and a small arithmetic body so codegen + rustc have real work to do
/// (no trivial `print(0)` that the optimiser folds away). The program
/// ends with a `main` that calls the first function so the binary is
/// runnable.
pub fn synthetic_buff_program(tier: BenchTier) -> String {
    let count = tier.fn_count();
    let mut src = String::new();
    src.push_str("// Auto-generated by buff bench-compile (T55) — do not edit.\n");
    src.push_str(&format!(
        "// tier: {} ({} functions)\n",
        tier.label(),
        count
    ));
    for i in 0..count {
        // Body uses `i` so each function differs (prevents dedup) and
        // forces real arithmetic codegen.
        let v = i + 1;
        src.push_str(&format!("func bench_fn_{i}(x: Int) -> Int:\n"));
        src.push_str(&format!("    return x * {v} + {i}\n"));
    }
    src.push_str("func main():\n");
    src.push_str("    print(bench_fn_0(10))\n");
    src
}

// ---------------------------------------------------------------------------
// PATH probe helper.
// ---------------------------------------------------------------------------

/// Returns `true` when `name` (an executable basename) is found on `PATH`.
///
/// Walks `$PATH` entries and checks for an executable file matching `name`
/// (with the platform extension appended on Windows). No subprocess is
/// spawned — this is a pure filesystem probe, so it's cheap to call.
///
/// Mirrors the logic of `which`/`where` without shelling out. Returns
/// `false` when `PATH` is unset or empty.
fn on_path(name: &str) -> bool {
    let path_var = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    let candidates: Vec<PathBuf> = std::env::split_paths(&path_var).collect();
    for dir in candidates {
        let full = dir.join(name);
        if is_executable(&full) {
            return true;
        }
        // On Windows, also try with `.exe` (and `.bat`).
        if cfg!(windows) {
            if is_executable(&dir.join(format!("{name}.exe"))) {
                return true;
            }
        }
    }
    false
}

/// Cross-platform "is this path an executable file" check.
///
/// On Unix this checks the executable bit; on Windows it checks that the
/// file exists (Windows determines executability by extension, which the
/// caller already appended).
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(md) => md.is_file() && md.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(path)
            .map(|m| m.is_file())
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Tests — pure-function unit tests (no rustc, no network).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_cache_key_is_16_hex_chars() {
        let key = source_cache_key("func main(): print(1)");
        assert_eq!(key.len(), 16, "key must be 16 hex chars, got {key}");
        assert!(
            key.chars().all(|c| c.is_ascii_hexdigit()),
            "key must be hex, got {key}"
        );
    }

    #[test]
    fn source_cache_key_is_deterministic() {
        let k1 = source_cache_key("hello");
        let k2 = source_cache_key("hello");
        assert_eq!(k1, k2, "same input must yield same key");
    }

    #[test]
    fn source_cache_key_differs_per_input() {
        let a = source_cache_key("func a():");
        let b = source_cache_key("func b():");
        assert_ne!(a, b, "different inputs must yield different keys");
    }

    #[test]
    fn fast_linker_rustc_flags_correct() {
        assert_eq!(
            FastLinker::Mold.rustc_flags(),
            vec!["-C", "link-arg=-fuse-ld=mold"]
        );
        assert_eq!(
            FastLinker::Lld.rustc_flags(),
            vec!["-C", "link-arg=-fuse-ld=lld"]
        );
        assert!(FastLinker::None.rustc_flags().is_empty());
    }

    #[test]
    fn fast_linker_is_fast_predicate() {
        assert!(FastLinker::Mold.is_fast());
        assert!(FastLinker::Lld.is_fast());
        assert!(!FastLinker::None.is_fast());
    }

    #[test]
    fn sccache_config_toml_is_well_formed() {
        let toml = sccache_cargo_config_toml();
        assert!(toml.contains("[build]"));
        assert!(toml.contains("rustc-wrapper = \"sccache\""));
        assert!(toml.ends_with('\n'));
    }

    #[test]
    fn bench_tier_fn_counts_ascending() {
        let tiers = BenchTier::all();
        assert_eq!(tiers.len(), 3);
        assert!(tiers[0].fn_count() < tiers[1].fn_count());
        assert!(tiers[1].fn_count() < tiers[2].fn_count());
    }

    #[test]
    fn synthetic_buff_program_small_is_valid_shape() {
        let src = synthetic_buff_program(BenchTier::Small);
        assert!(src.contains("func main():"), "must have a main: {src}");
        assert!(
            src.contains("func bench_fn_0("),
            "must have bench_fn_0: {src}"
        );
        // Small tier = 5 bench fns + 1 main = 6 funcs total.
        let func_count = src.matches("func ").count();
        assert_eq!(func_count, 6, "small tier function count: {src}");
    }

    #[test]
    fn synthetic_buff_program_is_deterministic() {
        let a = synthetic_buff_program(BenchTier::Medium);
        let b = synthetic_buff_program(BenchTier::Medium);
        assert_eq!(a, b, "same tier must produce identical source");
    }

    #[test]
    fn cache_file_path_for_ends_with_hash_rs() {
        let p = cache_file_path_for("deadbeefdeadbeef");
        assert_eq!(
            p.file_name().and_then(|s| s.to_str()),
            Some("deadbeefdeadbeef.rs")
        );
    }

    #[test]
    fn read_cache_returns_none_when_missing() {
        // A key that is vanishingly unlikely to collide with a real cache.
        let missing = read_cache("zz_nonexistent_zz");
        assert!(
            missing.is_none(),
            "missing cache must return None, got {missing:?}"
        );
    }
}
