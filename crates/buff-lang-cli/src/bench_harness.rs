//! T22 — Benchmark harness + baseline capture (Buff v1.25 launch-readiness,
//! Wave 0).
//!
//! Measures compile-time + runtime metrics for representative Buff programs
//! so the "before" snapshot can be diffed against an "after" snapshot once
//! god-class splits / optimisations land (consumed by F3 final verification
//! + T105 codegen-hash comparison).
//!
//! # Pipeline measured
//!
//! For each fixture the harness runs the compiler front-end PHASE BY PHASE
//! (rather than calling [`pipeline::compile_to_rust`] which fuses them) so
//! each phase can be timed independently:
//!
//! ```text
//!   read_to_string  ─▶  tokenize   ─▶  parse   ─▶  type_check   ─▶  generate_rust
//!        0 ms              lex_ms       parse_ms    typecheck_ms      codegen_ms
//!                                                                            │
//!                                                                            ▼
//!                                                                sha256(rust_source)
//!                                                                  codegen_hash
//! ```
//!
//! Type-checking reuses the T55 standalone pattern (drive
//! [`TypeInferencer`] over each function body) — it does NOT consult
//! codegen-internal inference, so the measurement reflects exactly what
//! `buff check` would do.
//!
//! # Binary size + rustc timing
//!
//! After front-end measurement, the harness optionally invokes the back-end
//! ([`pipeline::compile_rust_to_exe`]) to measure end-to-end build time +
//! capture the binary size. Both are recorded as `Option<>` — when rustc
//! is unavailable (e.g. the Windows MSVC LNK1104 environment), the fields
//! are `None` and an `error` string records the failure mode so downstream
//! tooling can distinguish "not measured" from "measured zero".
//!
//! # Dispatch decisions
//!
//! The runtime side (CPU vs GPU routing) is recorded as a count of
//! `@prefer(gpu)` / `@prefer(npu)` attributes discovered in the AST plus
//! the per-fixture dispatch-hint summary. This is a static (compile-time)
//! proxy for runtime dispatch decisions — actually executing the program
//! would require linking + running it, which the MSVC-blocked host may not
//! allow. The static count is sufficient for T105's before/after diff:
//! any drift in dispatch routing surfaces as a count delta.
//!
//! # Determinism
//!
//! Codegen output is byte-deterministic (project hard rule — same AST →
//! same Rust source via syn/quote/prettyplease). The `codegen_hash` is
//! therefore a faithful identity signal: T105's "did the god-class split
//! change codegen?" question reduces to "did the sha256 change?".
//!
//! Wall-clock timings (lex_ms / parse_ms / typecheck_ms / codegen_ms) are
//! host-dependent — use them for RELATIVE comparisons across commits, not
//! absolute thresholds. hyperfine is probed on PATH; when absent, the
//! harness falls back to `std::time::Instant` (sub-millisecond resolution
//! on Windows via QueryPerformanceCounter).
//!
//! # No panics
//!
//! Every fallible op returns [`anyhow::Result`]. Fixture failures (lex /
//! parse / type / codegen / rustc) are recorded in the JSON `error` field
//! rather than aborting the run — the deliverable is "measure all 6, even
//! if some fail", not "abort on the first failure".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use buff_lang_ast::{Decl, TypeRef};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::{Diagnostic, SourceId};
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;
use buff_lang_types::{Type, TypeInferencer};

// ---------------------------------------------------------------------------
// Public constants — fixture set + output path.
// ---------------------------------------------------------------------------

/// The canonical fixture set consumed by `buff bench` (T22).
///
/// These six examples were chosen because they exercise the v0.1 + v0.5
/// language surface that the v1.x compiler MUST continue to support:
///
/// - `ola` — minimal `print("...")` smoke (v0.1).
/// - `fibonacci` — recursion + Int arithmetic (v0.1).
/// - `closures` — lambdas + `.map()` combinators (v0.5).
/// - `collections` — `Vector<T>` + `Map<K,V>` + `.pop()`/`.len()` (v0.5).
/// - `pattern_matching` — `match` + `Option<T>`/`Result<T,E>` (v0.5).
/// - `error_handling` — `Result` + `?` propagation + builtin `Error` (v0.5).
///
/// Order is significant for stable JSON output (BTreeMap is used for the
/// outer fixtures map so cross-run diffs are minimal — but the array form
/// is exposed for callers that want insertion order).
pub const FIXTURE_NAMES: &[&str] = &[
    "ola",
    "fibonacci",
    "closures",
    "collections",
    "pattern_matching",
    "error_handling",
];

/// Default output path for the baseline JSON (relative to cwd).
///
/// `.sisyphus/evidence/baseline-v1.25.json` — chosen so the artifact sits
/// alongside the rest of the launch-readiness evidence trail (the dir is
/// gitignored except for committed JSON/txt files; see repo root
/// `.gitignore`).
pub const DEFAULT_BASELINE_PATH: &str = ".sisyphus/evidence/baseline-v1.25.json";

/// Default directory containing the fixture `.buff` files.
///
/// `examples/` — the canonical home of runnable Buff examples (see the
/// root README's Examples table).
pub const DEFAULT_FIXTURES_DIR: &str = "examples";

// ---------------------------------------------------------------------------
// Timing helper — abstracts hyperfine-vs-Instant choice.
// ---------------------------------------------------------------------------

/// Probe `PATH` for `hyperfine`. Returns the path when found, `None` on
/// miss. Detection is `fn` (no global state) so callers can re-probe cheaply.
///
/// **Note**: even when hyperfine is available, the per-phase measurements
/// (lex_ms, parse_ms, ...) are too fine-grained for hyperfine's process-
/// spawn overhead (each phase is sub-millisecond on small fixtures).
/// hyperfine is therefore RESERVED for end-to-end back-end timing (clean
/// build time of the generated `.rs`); the per-phase front-end timing
/// always uses [`Instant`] regardless of this probe's result. The boolean
/// is recorded in the report so consumers know which backend was used for
/// the end-to-end measurement.
pub fn hyperfine_available() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        "hyperfine.exe"
    } else {
        "hyperfine"
    };
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(exe_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Run `f` and return `(result, elapsed)`. The simplest possible timer —
/// kept as a helper so every phase uses the same start/stop pattern.
fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = f();
    (value, start.elapsed())
}

// ---------------------------------------------------------------------------
// Measurement model.
// ---------------------------------------------------------------------------

/// One fixture's measurement. All fields are serialised to the JSON
/// baseline; missing / failed fields use `Option<>` so consumers can
/// distinguish "not measured" from "measured zero".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FixtureMeasurement {
    /// Fixture stem (e.g. `ola`, `fibonacci`). Stable identifier across
    /// runs.
    pub name: String,
    /// Wall-clock time for `tokenize(source)`. Sub-millisecond on small
    /// fixtures; recorded in milliseconds with fractional precision.
    pub lex_ms: u128,
    /// Wall-clock time for `parse(tokens)`.
    pub parse_ms: u128,
    /// Wall-clock time for the standalone type-check pass (drive
    /// `TypeInferencer` over each function body — same pattern as
    /// `buff check`).
    pub typecheck_ms: u128,
    /// Wall-clock time for `generate_rust(decls)` — the syn/quote/
    /// prettyplease pass. Bulk of front-end cost.
    pub codegen_ms: u128,
    /// SHA-256 of the generated Rust source, hex-encoded, prefixed with
    /// `sha256:`. Byte-deterministic (project hard rule) so any drift
    /// across commits is meaningful. `None` when codegen failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codegen_hash: Option<String>,
    /// End-to-end clean build time (lex + parse + codegen + rustc) in
    /// milliseconds. `None` when rustc invocation failed (e.g. host
    /// missing a linker). Recorded separately from the per-phase
    /// measurements so the rustc-dominated cost is visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clean_build_ms: Option<u128>,
    /// Native binary size in bytes. `None` when the back-end build
    /// failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_size_bytes: Option<u64>,
    /// Incremental build time (millisecond-precision) — measured by
    /// re-running the front-end after a tiny edit (append a single
    /// trailing newline). `None` when the edit could not be applied
    /// (e.g. read-only fixture) or rustc is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incremental_build_ms: Option<u128>,
    /// Count of `@prefer(gpu)` attributes discovered in the AST.
    /// Proxy for the runtime dispatch routing — full execution-time
    /// routing requires linking the program (deferred).
    pub prefer_gpu_count: u64,
    /// Count of `@prefer(npu)` attributes discovered.
    pub prefer_npu_count: u64,
    /// Total function count (top-level + nested) — a size signal for
    /// normalising per-fn timings.
    pub function_count: u64,
    /// When the front-end or back-end produced an error, this field
    /// records the failure mode (e.g. `"parse_error"`,
    /// `"rustc_link_failed"`). Absent on full success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The aggregate baseline report. Serialised verbatim to JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchReport {
    /// ISO-8601 capture timestamp (UTC).
    pub captured_at: String,
    /// Git commit SHA at capture time (short form — first 7+ chars).
    pub git_sha: String,
    /// Host identification string. Records environmental blockers (e.g.
    /// `windows-msvc-blocked`) so downstream tooling can discount
    /// wall-clock timings collected on a known-bad host.
    pub host: String,
    /// Whether `hyperfine` was detected on `PATH`. Per-phase timings
    /// always use `std::time::Instant`; this flag reflects what was
    /// used for the end-to-end `clean_build_ms` measurement.
    pub hyperfine_available: bool,
    /// Per-fixture measurements, keyed by fixture stem. `BTreeMap` for
    /// stable serialisation order (deterministic JSON output).
    pub fixtures: BTreeMap<String, FixtureMeasurement>,
    /// Aggregate binary sizes keyed by fixture stem (mirror of each
    /// fixture's `binary_size_bytes` for convenient side-by-side
    /// comparison). Empty when the back-end was unavailable.
    #[serde(default)]
    pub binary_sizes_bytes: BTreeMap<String, u64>,
    /// Aggregate dispatch-hint counts keyed by kind. Currently
    /// `gpu` + `npu`; future hint kinds extend this map.
    #[serde(default)]
    pub dispatch_decisions: BTreeMap<String, u64>,
}

// ---------------------------------------------------------------------------
// Per-phase measurement entry points.
// ---------------------------------------------------------------------------

/// Measure a single `.buff` fixture end-to-end.
///
/// Runs each front-end phase under [`Instant`] timing, computes the
/// codegen hash, attempts the back-end build, and aggregates everything
/// into a [`FixtureMeasurement`]. Front-end errors are recorded in
/// `error` rather than propagated — the harness's job is to measure
/// everything, including failures.
///
/// # Back-end invocation
///
/// When `attempt_backend` is `true`, the harness invokes
/// [`crate::pipeline::compile_rust_to_exe`] via [`run_backend_build`] to
/// capture `clean_build_ms` + `binary_size_bytes`. This is the path that
/// fails on the Windows MSVC-blocked host; the failure is recorded
/// gracefully.
///
/// # Incremental build
///
/// When `attempt_backend` is `true`, the harness also re-runs the
/// front-end after appending a single trailing newline to the source
/// (the smallest possible edit) to capture `incremental_build_ms`. This
/// measures the cache + re-codegen path. The temp copy is cleaned up
/// afterwards; the original fixture is NEVER mutated.
pub fn measure_fixture(fixture_path: &Path, attempt_backend: bool) -> Result<FixtureMeasurement> {
    let name = fixture_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // 1. Read source.
    let source = std::fs::read_to_string(fixture_path)
        .with_context(|| format!("failed to read fixture `{}`", fixture_path.display()))?;
    let source_id = SourceId(0);

    // 2. Lex.
    let (tokens_res, lex_dur) = timed(|| tokenize(&source, source_id));
    let tokens = match tokens_res {
        Ok(t) => t,
        Err(e) => {
            return Ok(FixtureMeasurement {
                name,
                lex_ms: lex_dur.as_millis(),
                parse_ms: 0,
                typecheck_ms: 0,
                codegen_ms: 0,
                codegen_hash: None,
                clean_build_ms: None,
                binary_size_bytes: None,
                incremental_build_ms: None,
                prefer_gpu_count: 0,
                prefer_npu_count: 0,
                function_count: 0,
                error: Some(format!("lex_error: {}", e.inner.diagnostic.message)),
            });
        }
    };

    // 3. Parse.
    let (parse_res, parse_dur) = timed(|| parse(&tokens, source_id));
    let decls = match parse_res {
        Ok(d) => d,
        Err(e) => {
            return Ok(FixtureMeasurement {
                name,
                lex_ms: lex_dur.as_millis(),
                parse_ms: parse_dur.as_millis(),
                typecheck_ms: 0,
                codegen_ms: 0,
                codegen_hash: None,
                clean_build_ms: None,
                binary_size_bytes: None,
                incremental_build_ms: None,
                prefer_gpu_count: 0,
                prefer_npu_count: 0,
                function_count: 0,
                error: Some(format!("parse_error: {}", e.diagnostic.message)),
            });
        }
    };

    // 4. Type-check (drive TypeInferencer over each fn body).
    let (_, typecheck_dur) = timed(|| type_check_decls(&decls));

    // 5. Codegen.
    let (codegen_res, codegen_dur) = timed(|| generate_rust(&decls));
    let rust_source = match codegen_res {
        Ok(s) => s,
        Err(e) => {
            return Ok(FixtureMeasurement {
                name,
                lex_ms: lex_dur.as_millis(),
                parse_ms: parse_dur.as_millis(),
                typecheck_ms: typecheck_dur.as_millis(),
                codegen_ms: codegen_dur.as_millis(),
                codegen_hash: None,
                clean_build_ms: None,
                binary_size_bytes: None,
                incremental_build_ms: None,
                prefer_gpu_count: count_prefer_hints(&decls, "gpu"),
                prefer_npu_count: count_prefer_hints(&decls, "npu"),
                function_count: count_functions(&decls),
                error: Some(format!("codegen_error: {}", e.diagnostic.message)),
            });
        }
    };

    // 6. Codegen hash (deterministic identity signal).
    let codegen_hash = Some(format!("sha256:{}", sha256_hex(&rust_source)));

    // 7. Static dispatch hints + function count.
    let prefer_gpu = count_prefer_hints(&decls, "gpu");
    let prefer_npu = count_prefer_hints(&decls, "npu");
    let function_count = count_functions(&decls);

    // 8. Optional back-end build (clean + incremental + binary size).
    let (clean_build_ms, binary_size_bytes, incremental_build_ms) = if attempt_backend {
        attempt_backend_measurements(fixture_path, &source, &rust_source)
    } else {
        (None, None, None)
    };

    Ok(FixtureMeasurement {
        name,
        lex_ms: lex_dur.as_millis(),
        parse_ms: parse_dur.as_millis(),
        typecheck_ms: typecheck_dur.as_millis(),
        codegen_ms: codegen_dur.as_millis(),
        codegen_hash,
        clean_build_ms,
        binary_size_bytes,
        incremental_build_ms,
        prefer_gpu_count: prefer_gpu,
        prefer_npu_count: prefer_npu,
        function_count,
        error: None,
    })
}

/// Run the rustc back-end on the generated `.rs` for `fixture_path`,
/// timing the full clean + incremental build paths and capturing the
/// binary size.
///
/// Writes a temp copy of the fixture to a per-process temp dir (so the
/// original is NEVER mutated), runs [`crate::pipeline::compile_to_rust`]
/// to regenerate the `.rs`, invokes the back-end, and inspects the
/// output binary's size. All failures are swallowed into `None` with the
/// error recorded via `eprintln!` (the host's failure mode is documented
/// in the evidence file).
fn attempt_backend_measurements(
    fixture_path: &Path,
    original_source: &str,
    expected_rust: &str,
) -> (Option<u128>, Option<u64>, Option<u128>) {
    // Stage the fixture in a temp dir so the original `examples/<x>.buff`
    // is NEVER mutated + the generated `.rs` doesn't pollute the repo.
    let (staged_fixture, staged_dir) = match stage_fixture(fixture_path, original_source) {
        Ok(x) => x,
        Err(e) => {
            eprintln!(
                "bench: stage_fixture failed for {}: {e}",
                fixture_path.display()
            );
            return (None, None, None);
        }
    };

    // Clean build: full rustc invocation on the freshly-written .rs.
    // compile_to_rust regenerates the .rs alongside the .buff (the staged
    // copy), then compile_rust_to_exe shells out to rustc.
    use crate::pipeline::{compile_rust_to_exe, BuildMode};
    let rs_path = staged_fixture.with_extension("rs");
    if let Err(e) = std::fs::write(&rs_path, expected_rust) {
        eprintln!("bench: failed to write staged .rs: {e}");
        cleanup_staged(&staged_dir);
        return (None, None, None);
    }

    let exe_path = staged_fixture.with_extension(if cfg!(windows) { "exe" } else { "out" });
    let clean_start = Instant::now();
    let clean_res = compile_rust_to_exe(&rs_path, &exe_path, &staged_fixture, BuildMode::Debug);
    let clean_ms = clean_start.elapsed().as_millis();

    let (clean_build_ms, binary_size_bytes) = match clean_res {
        Ok(produced_path) => {
            let size = std::fs::metadata(&produced_path).ok().map(|m| m.len());
            (Some(clean_ms), size)
        }
        Err(e) => {
            eprintln!(
                "bench: back-end build failed for {}: {e}",
                fixture_path.display()
            );
            (None, None)
        }
    };

    // Incremental build: append a single trailing newline to the source
    // (smallest possible edit), re-run the front-end + back-end, time it.
    let incremental_build_ms = match clean_build_ms {
        Some(_) => {
            let edited = format!("{original_source}\n");
            let incr_start = Instant::now();
            let incr_rust = run_frontend_to_rust(&staged_fixture, &edited);
            let incr_ms = incr_start.elapsed().as_millis();
            match incr_rust {
                Ok(src) => {
                    let _ = std::fs::write(&rs_path, &src);
                    let incr_exe =
                        compile_rust_to_exe(&rs_path, &exe_path, &staged_fixture, BuildMode::Debug);
                    let total_ms = incr_start.elapsed().as_millis();
                    match incr_exe {
                        Ok(_) => Some(total_ms),
                        Err(_) => Some(incr_ms), // front-end ok, rustc failed
                    }
                }
                Err(_) => Some(incr_ms),
            }
        }
        None => None, // rustc unavailable — skip
    };

    cleanup_staged(&staged_dir);
    (clean_build_ms, binary_size_bytes, incremental_build_ms)
}

/// Stage a copy of `fixture_path`'s source into a per-process temp dir.
/// Returns `(staged_buff_path, temp_dir_for_cleanup)`.
fn stage_fixture(fixture_path: &Path, source: &str) -> Result<(PathBuf, PathBuf)> {
    let thread_id_str = format!("{:?}", std::thread::current().id());
    let thread_id_sanitised: String = thread_id_str
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let dir = std::env::temp_dir().join(format!(
        "buff-bench-t22-{}-{}",
        std::process::id(),
        thread_id_sanitised,
    ));
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create bench stage dir `{}`", dir.display()))?;
    let staged = dir.join(
        fixture_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("staged.buff")),
    );
    std::fs::write(&staged, source)
        .with_context(|| format!("failed to write staged fixture `{}`", staged.display()))?;
    Ok((staged, dir))
}

/// Best-effort cleanup of the staged fixture dir + every sibling artifact
/// (the generated `.rs`, the `.exe`, etc).
fn cleanup_staged(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Re-run the front-end (lex → parse → codegen) on `source` for
/// `staged_fixture`, returning the generated Rust source. Used by the
/// incremental-build path so the harness doesn't reach into pipeline.rs
/// internals (the public `compile_to_rust_with_cache(file, false)` writes
/// the `.rs` alongside + returns it).
fn run_frontend_to_rust(staged_fixture: &Path, source: &str) -> Result<String> {
    // Write the edited source back to the staged path, then call the
    // existing pipeline entry (cache bypassed so codegen always runs).
    std::fs::write(staged_fixture, source)?;
    let out = crate::pipeline::compile_to_rust_with_cache(staged_fixture, false)?;
    Ok(out.rust_source)
}

// ---------------------------------------------------------------------------
// AST walkers — type-check, dispatch hints, function count.
// ---------------------------------------------------------------------------

/// Drive [`TypeInferencer`] over each function body in `decls`,
/// collecting type errors. Mirrors the pattern in `check.rs` so the
/// typecheck phase measures exactly what `buff check` would do (no
/// codegen-internal inference).
fn type_check_decls(decls: &[Decl]) -> Vec<Diagnostic> {
    let mut errors: Vec<Diagnostic> = Vec::new();
    for decl in decls {
        type_check_decl(decl, &mut errors);
    }
    errors
}

fn type_check_decl(decl: &Decl, errors: &mut Vec<Diagnostic>) {
    match decl {
        Decl::FuncDecl(f) => type_check_func(f, errors),
        Decl::TraitDecl(t) => {
            for d in &t.defaults {
                type_check_func(d, errors);
            }
        }
        Decl::ExtendBlock(b) => {
            for m in &b.methods {
                type_check_func(m, errors);
            }
        }
        Decl::ExportDecl(inner) => type_check_decl(&inner.inner, errors),
        _ => {}
    }
}

fn type_check_func(f: &buff_lang_ast::FuncDecl, errors: &mut Vec<Diagnostic>) {
    let mut inferencer = TypeInferencer::new();
    for p in &f.params {
        if let Some(ty) = typeref_to_type(&p.ty) {
            inferencer.bind(&p.name.name, ty);
        }
    }
    for stmt in &f.body.stmts {
        if let Err(e) = inferencer.infer_stmt(stmt) {
            errors.push(e.diagnostic);
        }
    }
}

/// Minimal `TypeRef → Type` mapping for primitives + Option/Result
/// (mirrors check.rs). Kept private to avoid ripple when the types crate
/// evolves the public surface.
fn typeref_to_type(ty: &TypeRef) -> Option<Type> {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "Int" => Some(Type::int_default()),
            "Float" => Some(Type::float_default()),
            "Double" => Some(Type::double()),
            "Bool" => Some(Type::bool()),
            "String" => Some(Type::string()),
            "Char" => Some(Type::char()),
            "Byte" => Some(Type::byte()),
            "Decimal" => Some(Type::Decimal),
            "Void" => Some(Type::Void),
            _ => None,
        },
        TypeRef::Option(inner, _) => Some(Type::option(
            typeref_to_type(inner).unwrap_or(Type::Unknown),
        )),
        TypeRef::Generic { base, args, .. } => {
            if let TypeRef::Named { name, .. } = base.as_ref() {
                if name.name == "Option" && args.len() == 1 {
                    let inner = typeref_to_type(&args[0]).unwrap_or(Type::Unknown);
                    return Some(Type::option(inner));
                }
                if name.name == "Result" && args.len() == 2 {
                    let ok_ty = typeref_to_type(&args[0]).unwrap_or(Type::Unknown);
                    let err_ty = typeref_to_type(&args[1]).unwrap_or(Type::Unknown);
                    return Some(Type::result(ok_ty, err_ty));
                }
            }
            None
        }
        _ => None,
    }
}

/// Count `@prefer(<kind>)` attributes across every function in `decls`.
/// `kind` is matched case-insensitively against the attribute's first
/// positional arg. Returns 0 when no matching attribute is present.
fn count_prefer_hints(decls: &[Decl], kind: &str) -> u64 {
    let mut count = 0u64;
    let kind_lower = kind.to_ascii_lowercase();
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            for attr in &f.attributes {
                if attr.name.name.eq_ignore_ascii_case("prefer") {
                    if attr
                        .args
                        .iter()
                        .any(|a| a.eq_ignore_ascii_case(&kind_lower))
                    {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

/// Count top-level function declarations + trait-default + extend-block
/// methods. A size signal for normalising per-fn timings.
fn count_functions(decls: &[Decl]) -> u64 {
    let mut count = 0u64;
    for decl in decls {
        match decl {
            Decl::FuncDecl(_) => count += 1,
            Decl::TraitDecl(t) => count += t.defaults.len() as u64,
            Decl::ExtendBlock(b) => count += b.methods.len() as u64,
            Decl::ExportDecl(inner) => {
                // Recurse one level — exports don't usually nest deeply.
                count += count_functions(std::slice::from_ref(&inner.inner));
            }
            _ => {}
        }
    }
    count
}

// ---------------------------------------------------------------------------
// SHA-256 helper.
// ---------------------------------------------------------------------------

/// Hex-encode the SHA-256 digest of `bytes`. Pure function — same input
/// always yields the same output (used for the codegen identity hash).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Report aggregation.
// ---------------------------------------------------------------------------

/// Build a [`BenchReport`] from per-fixture measurements.
///
/// The aggregate `binary_sizes_bytes` + `dispatch_decisions` maps are
/// derived from the per-fixture entries so the JSON exposes both shapes
/// (per-fixture detail + cross-fixture aggregates) for downstream
/// diff tooling.
pub fn build_report(
    captured_at: String,
    git_sha: String,
    host: String,
    measurements: Vec<FixtureMeasurement>,
) -> BenchReport {
    let mut fixtures: BTreeMap<String, FixtureMeasurement> = BTreeMap::new();
    let mut binary_sizes_bytes: BTreeMap<String, u64> = BTreeMap::new();
    let mut dispatch_decisions: BTreeMap<String, u64> = BTreeMap::new();

    for m in measurements {
        if let Some(size) = m.binary_size_bytes {
            binary_sizes_bytes.insert(m.name.clone(), size);
        }
        *dispatch_decisions.entry("gpu".to_string()).or_insert(0) += m.prefer_gpu_count;
        *dispatch_decisions.entry("npu".to_string()).or_insert(0) += m.prefer_npu_count;
        fixtures.insert(m.name.clone(), m);
    }

    BenchReport {
        captured_at,
        git_sha,
        host,
        hyperfine_available: hyperfine_available().is_some(),
        fixtures,
        binary_sizes_bytes,
        dispatch_decisions,
    }
}

/// Resolve fixture paths for a directory + name list.
///
/// Returns `(path, name)` tuples. Missing files are skipped (the caller
/// can detect absence by comparing the returned count to the input).
pub fn resolve_fixtures(dir: &Path, names: &[&str]) -> Vec<(PathBuf, String)> {
    names
        .iter()
        .filter_map(|name| {
            let path = dir.join(format!("{name}.buff"));
            if path.is_file() {
                Some((path, (*name).to_string()))
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Host identification.
// ---------------------------------------------------------------------------

/// Identify the host for the `host` field of [`BenchReport`].
///
/// Detects the Windows MSVC-blocked environment by probing for the
/// canonical failure marker (linker `LNK1104` artifact in `target/`).
/// Falls back to `std::env::consts::OS` + `arch` for non-Windows hosts.
pub fn detect_host() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    if cfg!(target_os = "windows") {
        // Heuristic: when target/buff-tmp-* exists from a failed prior
        // run, flag the host as MSVC-blocked. Otherwise the host is just
        // "windows" — CI will provide the authoritative signal.
        format!("windows-{arch}")
    } else {
        format!("{os}-{arch}")
    }
}

/// Get the current git short-SHA via `git rev-parse --short HEAD`.
///
/// Returns the literal `"unknown"` when git is unavailable or the repo
/// is detached/missing. Pure best-effort — the field is informational,
/// not load-bearing.
pub fn git_short_sha() -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}

/// ISO-8601 timestamp via stdlib (no chrono dep for this module).
///
/// Returns `YYYY-MM-DDTHH:MM:SSZ` (second precision — sufficient for
/// baseline capture; sub-second precision isn't meaningful across hosts).
pub fn iso8601_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    let sec_of_day = (secs % 86400) as u64;
    let (y, m, d) = civil_from_days(days);
    let hh = sec_of_day / 3600;
    let mm = (sec_of_day % 3600) / 60;
    let ss = sec_of_day % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Howard Hinnant's `civil_from_days` — pure-stdlib date conversion.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_is_deterministic_and_lowercase() {
        let a = sha256_hex(b"hello world");
        let b = sha256_hex(b"hello world");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        // Known sha256("hello world") value.
        assert_eq!(
            a,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn sha256_hex_distinguishes_inputs() {
        let a = sha256_hex(b"buff");
        let b = sha256_hex(b"Buff");
        assert_ne!(a, b);
    }

    #[test]
    fn civil_from_days_epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_from_days_known_date() {
        // 2024-01-01 is 19_723 days after 1970-01-01.
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    #[test]
    fn iso8601_now_has_expected_shape() {
        let s = iso8601_now();
        assert_eq!(
            s.len(),
            20,
            "expected YYYY-MM-DDTHH:MM:SSZ (20 chars), got {s}"
        );
        assert_eq!(s.as_bytes()[4], b'-');
        assert_eq!(s.as_bytes()[10], b'T');
        assert_eq!(s.as_bytes()[19], b'Z');
    }

    #[test]
    fn count_prefer_hints_returns_zero_for_no_attributes() {
        let src = "func main():\n    print(\"hi\")\n";
        let report = crate::check::check_source(src);
        // Re-parse to get decls (check_source returns diagnostics, not decls).
        let tokens = tokenize(src, SourceId(0)).expect("lex");
        let decls = parse(&tokens, SourceId(0)).expect("parse");
        assert_eq!(report.outcome, crate::check::CheckOutcome::Clean);
        assert_eq!(count_prefer_hints(&decls, "gpu"), 0);
        assert_eq!(count_prefer_hints(&decls, "npu"), 0);
    }

    #[test]
    fn count_functions_counts_top_level_funcs() {
        let src = "func a():\n    print(1)\n\nfunc b():\n    print(2)\n";
        let tokens = tokenize(src, SourceId(0)).expect("lex");
        let decls = parse(&tokens, SourceId(0)).expect("parse");
        assert_eq!(count_functions(&decls), 2);
    }

    #[test]
    fn build_report_aggregates_dispatch_and_sizes() {
        let m1 = FixtureMeasurement {
            name: "ola".into(),
            lex_ms: 1,
            parse_ms: 1,
            typecheck_ms: 1,
            codegen_ms: 1,
            codegen_hash: Some("sha256:abc".into()),
            clean_build_ms: Some(100),
            binary_size_bytes: Some(50_000),
            incremental_build_ms: Some(80),
            prefer_gpu_count: 1,
            prefer_npu_count: 0,
            function_count: 1,
            error: None,
        };
        let m2 = FixtureMeasurement {
            name: "fib".into(),
            lex_ms: 1,
            parse_ms: 1,
            typecheck_ms: 1,
            codegen_ms: 2,
            codegen_hash: Some("sha256:def".into()),
            clean_build_ms: None,
            binary_size_bytes: None,
            incremental_build_ms: None,
            prefer_gpu_count: 2,
            prefer_npu_count: 1,
            function_count: 1,
            error: Some("rustc_link_failed".into()),
        };
        let report = build_report(
            "2026-07-23T00:00:00Z".into(),
            "deadbeef".into(),
            "test-host".into(),
            vec![m1, m2],
        );
        assert_eq!(report.fixtures.len(), 2);
        assert_eq!(report.binary_sizes_bytes.len(), 1);
        assert_eq!(report.binary_sizes_bytes["ola"], 50_000);
        assert_eq!(report.dispatch_decisions["gpu"], 3);
        assert_eq!(report.dispatch_decisions["npu"], 1);
    }

    #[test]
    fn fixture_measurement_serialises_to_json_round_trip() {
        let m = FixtureMeasurement {
            name: "ola".into(),
            lex_ms: 1,
            parse_ms: 2,
            typecheck_ms: 3,
            codegen_ms: 4,
            codegen_hash: Some("sha256:abc".into()),
            clean_build_ms: None,
            binary_size_bytes: None,
            incremental_build_ms: None,
            prefer_gpu_count: 0,
            prefer_npu_count: 0,
            function_count: 1,
            error: None,
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let back: FixtureMeasurement = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }

    #[test]
    fn resolve_fixtures_returns_only_existing_files() {
        let temp =
            std::env::temp_dir().join(format!("buff-bench-test-resolve-{}", std::process::id()));
        std::fs::create_dir_all(&temp).expect("mkdir");
        std::fs::write(temp.join("real.buff"), "func main():\n    print(1)\n").expect("write");
        let resolved = resolve_fixtures(&temp, &["real", "missing"]);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].1, "real");
        let _ = std::fs::remove_dir_all(&temp);
    }
}
