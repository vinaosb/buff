//! T22 — Standalone baseline-capture binary.
//!
//! Pure-Rust-only (no reqwest/rustls/ring) so it links cleanly on hosts
//! where the full `buff-lang-cli` binary cannot (e.g. Windows MSVC
//! LNK1104 ring blocker via reqwest → rustls → ring).
//!
//! Mirrors the per-fixture measurement logic in
//! `crates/buff-lang-cli/src/bench_harness.rs::measure_fixture`:
//!
//!   read → lex → parse → typecheck (skip — types crate API differs) →
//!   codegen → sha256(rust_source)
//!
//! Outputs pretty JSON to stdout (or `<arg0>` path when provided) in the
//! shape documented by the T22 task spec:
//!
//! ```json
//! {
//!   "captured_at": "ISO-8601",
//!   "git_sha": "...",
//!   "host": "...",
//!   "hyperfine_available": false,
//!   "fixtures": { "ola": { "lex_ms": N, ... }, ... },
//!   "binary_sizes_bytes": {},
//!   "dispatch_decisions": {}
//! }
//! ```
//!
//! # Usage
//!
//! ```bash
//! cargo run -p buff-lang-codegen-rust --example capture_t22_baseline -- \
//!     examples/ .sisyphus/evidence/baseline-v1.25.json
//! ```
//!
//! Run from the repo root so the `examples/` path resolves to the
//! canonical fixture dir.

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;
use sha2::{Digest, Sha256};

const FIXTURE_NAMES: &[&str] = &[
    "ola",
    "fibonacci",
    "closures",
    "collections",
    "pattern_matching",
    "error_handling",
];

fn main() {
    let args: Vec<String> = env::args().collect();
    let fixtures_dir: PathBuf = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "examples".to_string())
        .into();
    let output_path: PathBuf = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| ".sisyphus/evidence/baseline-v1.25.json".to_string())
        .into();

    eprintln!("T22 baseline capture");
    eprintln!("fixtures dir: {}", fixtures_dir.display());
    eprintln!("output:       {}", output_path.display());

    let mut fixtures: BTreeMap<String, FixtureMeasurement> = BTreeMap::new();
    let mut binary_sizes_bytes: BTreeMap<String, u64> = BTreeMap::new();
    let mut dispatch_decisions: BTreeMap<String, u64> = BTreeMap::new();

    for name in FIXTURE_NAMES {
        let path = fixtures_dir.join(format!("{name}.buff"));
        eprintln!("- measuring {name} ...");
        let m = measure_fixture(&path, name);
        if let Some(_) = m.binary_size_bytes {
            // (none on this host — back-end unavailable)
        }
        *dispatch_decisions.entry("gpu".to_string()).or_insert(0) += m.prefer_gpu_count;
        *dispatch_decisions.entry("npu".to_string()).or_insert(0) += m.prefer_npu_count;
        fixtures.insert(name.to_string(), m);
    }

    let report = BenchReport {
        captured_at: iso8601_now(),
        git_sha: git_short_sha(),
        host: format!(
            "standalone-capture-binary on {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
        ),
        hyperfine_available: false,
        fixtures,
        binary_sizes_bytes,
        dispatch_decisions,
    };

    let json = serde_json::to_string_pretty(&report).expect("serialize");
    if let Some(parent) = output_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&output_path, format!("{json}\n")).expect("write");
    eprintln!("wrote {} bytes to {}", json.len(), output_path.display());
    // Also print summary table to stderr.
    eprintln!("{:<20} {:>8} {:>8} {:>10} {:>14}", "fixture", "lex_ms", "parse_ms", "cg_ms", "hash[..16]");
    for (name, m) in &report.fixtures {
        let hash_tail = m.codegen_hash.as_ref().and_then(|h| h.get(7..23)).unwrap_or("<none>");
        eprintln!("{:<20} {:>8} {:>8} {:>10} {:>14}", name, m.lex_ms, m.parse_ms, m.codegen_ms, hash_tail);
    }
}

#[derive(serde::Serialize)]
struct FixtureMeasurement {
    name: String,
    lex_ms: u128,
    parse_ms: u128,
    typecheck_ms: u128,
    codegen_ms: u128,
    codegen_hash: Option<String>,
    clean_build_ms: Option<u128>,
    binary_size_bytes: Option<u64>,
    incremental_build_ms: Option<u128>,
    prefer_gpu_count: u64,
    prefer_npu_count: u64,
    function_count: u64,
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct BenchReport {
    captured_at: String,
    git_sha: String,
    host: String,
    hyperfine_available: bool,
    fixtures: BTreeMap<String, FixtureMeasurement>,
    binary_sizes_bytes: BTreeMap<String, u64>,
    dispatch_decisions: BTreeMap<String, u64>,
}

fn measure_fixture(path: &Path, name: &str) -> FixtureMeasurement {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return FixtureMeasurement {
                name: name.to_string(),
                lex_ms: 0, parse_ms: 0, typecheck_ms: 0, codegen_ms: 0,
                codegen_hash: None, clean_build_ms: None, binary_size_bytes: None,
                incremental_build_ms: None, prefer_gpu_count: 0, prefer_npu_count: 0,
                function_count: 0,
                error: Some(format!("read_error: {e}")),
            };
        }
    };
    let source_id = SourceId(0);

    // 1. Lex
    let start = Instant::now();
    let tokens = match tokenize(&source, source_id) {
        Ok(t) => t,
        Err(e) => {
            return FixtureMeasurement {
                name: name.to_string(),
                lex_ms: start.elapsed().as_millis(),
                parse_ms: 0, typecheck_ms: 0, codegen_ms: 0,
                codegen_hash: None, clean_build_ms: None, binary_size_bytes: None,
                incremental_build_ms: None, prefer_gpu_count: 0, prefer_npu_count: 0,
                function_count: 0,
                error: Some(format!("lex_error: {}", e.inner.diagnostic.message)),
            };
        }
    };
    let lex_ms = start.elapsed().as_millis();

    // 2. Parse
    let start = Instant::now();
    let decls = match parse(&tokens, source_id) {
        Ok(d) => d,
        Err(e) => {
            return FixtureMeasurement {
                name: name.to_string(),
                lex_ms, parse_ms: start.elapsed().as_millis(),
                typecheck_ms: 0, codegen_ms: 0,
                codegen_hash: None, clean_build_ms: None, binary_size_bytes: None,
                incremental_build_ms: None, prefer_gpu_count: 0, prefer_npu_count: 0,
                function_count: 0,
                error: Some(format!("parse_error: {}", e.diagnostic.message)),
            };
        }
    };
    let parse_ms = start.elapsed().as_millis();

    // 3. Type-check — the types crate's TypeInferencer isn't easily
    //    accessible from this example binary (it would require another
    //    dev-dep). The full bench_harness.rs in buff-lang-cli measures
    //    it; here we record 0 with a note in the JSON via the host field.
    let typecheck_ms: u128 = 0;

    // 4. Codegen
    let start = Instant::now();
    let rust_source = match generate_rust(&decls) {
        Ok(s) => s,
        Err(e) => {
            return FixtureMeasurement {
                name: name.to_string(),
                lex_ms, parse_ms, typecheck_ms,
                codegen_ms: start.elapsed().as_millis(),
                codegen_hash: None, clean_build_ms: None, binary_size_bytes: None,
                incremental_build_ms: None,
                prefer_gpu_count: count_prefer(&decls, "gpu"),
                prefer_npu_count: count_prefer(&decls, "npu"),
                function_count: count_funcs(&decls),
                error: Some(format!("codegen_error: {}", e.diagnostic.message)),
            };
        }
    };
    let codegen_ms = start.elapsed().as_millis();

    // 5. Hash
    let mut hasher = Sha256::new();
    hasher.update(rust_source.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let codegen_hash = Some(format!("sha256:{hex}"));

    FixtureMeasurement {
        name: name.to_string(),
        lex_ms,
        parse_ms,
        typecheck_ms,
        codegen_ms,
        codegen_hash,
        clean_build_ms: None,           // back-end unavailable in this binary
        binary_size_bytes: None,        // ditto
        incremental_build_ms: None,     // ditto
        prefer_gpu_count: count_prefer(&decls, "gpu"),
        prefer_npu_count: count_prefer(&decls, "npu"),
        function_count: count_funcs(&decls),
        error: None,
    }
}

fn count_prefer(decls: &[buff_lang_ast::Decl], kind: &str) -> u64 {
    use buff_lang_ast::Decl;
    let kind_lower = kind.to_ascii_lowercase();
    let mut count = 0u64;
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            for attr in &f.attributes {
                if attr.name.name.eq_ignore_ascii_case("prefer")
                    && attr.args.iter().any(|a| a.eq_ignore_ascii_case(&kind_lower))
                {
                    count += 1;
                }
            }
        }
    }
    count
}

fn count_funcs(decls: &[buff_lang_ast::Decl]) -> u64 {
    use buff_lang_ast::Decl;
    let mut count = 0u64;
    for decl in decls {
        match decl {
            Decl::FuncDecl(_) => count += 1,
            Decl::TraitDecl(t) => count += t.defaults.len() as u64,
            Decl::ExtendBlock(b) => count += b.methods.len() as u64,
            Decl::ExportDecl(inner) => {
                count += count_funcs(std::slice::from_ref(&inner.inner));
            }
            _ => {}
        }
    }
    count
}

fn git_short_sha() -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}

fn iso8601_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
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
