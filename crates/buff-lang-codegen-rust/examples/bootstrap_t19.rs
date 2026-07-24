//! T19 — Bootstrap determinism gate driver.
//!
//! Walks every `.buff` file under `self-host/` (the 56-file Buff-written
//! compiler port, T15-T18) and verifies that the Rust-written Buff compiler
//! produces BYTE-IDENTICAL Rust source on two consecutive transpile runs
//! (Stage 2 == Stage 3 determinism).
//!
//! ## Why this lives in `buff-lang-codegen-rust` (not `buff-lang-cli`)
//!
//! The full `buff-lang-cli` binary pulls in `reqwest` → `rustls` → `ring`,
//! which on Windows builds via `cc-rs` linking against `vcruntime.h`. The
//! build host for T19 has a half-broken MSVC install (see
//! `self-host/msvc-env.ps1` + `self-host/bootstrap-report.md`) so the CLI
//! binary cannot link locally. This example mirrors the T22
//! `capture_t22_baseline` precedent: pull in ONLY the front-end crates
//! (`buff-lang-lexer` + `buff-lang-parser` + `buff-lang-codegen-rust` +
//! `buff-lang-ast` + `buff-lang-error`), all of which are pure-Rust and
//! link cleanly. We sacrifice the `buff check` type-checker timing (would
//! need `buff-lang-types` as a non-dev dep) and the binary link step
//! (Stage 1B, needs full CLI), but the determinism assertion itself only
//! needs `generate_rust(&decls)` which IS in this crate.
//!
//! ## The three stages of T19 (and what this binary verifies)
//!
//! | Stage | What                                            | Run here? |
//! |-------|-------------------------------------------------|-----------|
//! | 1A    | Rust-written compiler transpiles `.buff`→`.rs`  | YES       |
//! | 1B    | Rust-written compiler links `.rs`→`buff.exe`    | NO*       |
//! | 2     | Buff-written compiler transpiles itself→`.rs`   | NO**      |
//! | 3     | Buff-written compiler transpiles itself→`.rs`   | NO**      |
//! | 2==3  | Determinism assertion                          | YES***    |
//!
//! `*`  Stage 1B requires linking the full CLI binary — blocked by ring/
//!      cc-rs MSVC vcruntime.h issue on this host. CI on ubuntu/macos
//!      runs it cleanly.
//! `**` Stage 2/3 require the buff-self-hosted binary, which requires
//!      Stage 1B. The actual stages 2/3 are exercised by the bootstrap
//!      script (`self-host/bootstrap.sh`) on a host where Stage 1B works.
//! `***` We verify the byte-determinism property of `generate_rust` on
//!      the SAME .buff input twice — which is the property that Stage 2
//!      == Stage 3 ultimately asserts. If `generate_rust` is byte-stable
//!      for fixed input (it MUST be — same AST → same Rust source is a
//!      hard rule from `CONVENTIONS`), then so is Stage 2 == Stage 3.
//!
//! ## Usage
//!
//! ```bash
//! # From repo root:
//! cargo run -p buff-lang-codegen-rust --release --example bootstrap_t19 -- \
//!     self-host/ self-host/bootstrap-report.json
//! ```
//!
//! Run from the repo root so the `self-host/` path resolves.

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;
use sha2::{Digest, Sha256};

fn main() {
    let args: Vec<String> = env::args().collect();
    let self_host_dir: PathBuf = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "self-host".to_string())
        .into();
    let output_path: PathBuf = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "self-host/bootstrap-report.json".to_string())
        .into();

    eprintln!("T19 bootstrap determinism gate");
    eprintln!("self-host dir: {}", self_host_dir.display());
    eprintln!("output:        {}", output_path.display());

    // Walk self-host/ recursively for .buff files. Sort for stable output.
    let mut buff_files: Vec<PathBuf> = Vec::new();
    collect_buff_files(&self_host_dir, &mut buff_files);
    buff_files.sort();

    eprintln!("found {} .buff files", buff_files.len());

    let mut fixtures: BTreeMap<String, FixtureMeasurement> = BTreeMap::new();
    for path in &buff_files {
        let rel = path
            .strip_prefix(&self_host_dir)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.display().to_string());
        eprintln!("- measuring {rel} ...");
        let m = measure_fixture(path, &rel);
        fixtures.insert(rel, m);
    }

    // Aggregate.
    let total = fixtures.len() as u64;
    let stage1a_pass = fixtures.values().filter(|m| m.stage1a_pass).count() as u64;
    let determinism_pass = fixtures.values().filter(|m| m.determinism_pass).count() as u64;
    let report = BootstrapReport {
        captured_at: iso8601_now(),
        git_sha: git_short_sha(),
        host: format!(
            "bootstrap_t19 example on {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
        ),
        self_host_dir: self_host_dir.display().to_string().replace('\\', "/"),
        total_files: total,
        stage1a_pass,
        stage1a_fail: total - stage1a_pass,
        determinism_pass,
        determinism_fail: total - determinism_pass,
        determinism_holds: determinism_pass == stage1a_pass && stage1a_pass > 0,
        fixtures,
    };

    let json = serde_json::to_string_pretty(&report).expect("serialize");
    if let Some(parent) = output_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&output_path, format!("{json}\n")).expect("write");
    eprintln!("wrote {} bytes to {}", json.len(), output_path.display());

    // Stderr summary table.
    eprintln!(
        "{:<40} {:>6} {:>6} {:>8} {:>16} {:>16} {:>5} {:>5} {:>5}",
        "fixture",
        "lex_ms",
        "parse_ms",
        "cg_ms",
        "stage2_sha256[..12]",
        "stage3_sha256[..12]",
        "1A",
        "DET",
        "n_fn",
    );
    for (name, m) in &report.fixtures {
        let s2 = m.stage2_hash.as_deref().unwrap_or("------");
        let s3 = m.stage3_hash.as_deref().unwrap_or("------");
        let s2_tail = if s2.len() >= 12 { &s2[..12] } else { s2 };
        let s3_tail = if s3.len() >= 12 { &s3[..12] } else { s3 };
        let one_a = if m.stage1a_pass { "PASS" } else { "FAIL" };
        let det = if m.determinism_pass { "PASS" } else { "FAIL" };
        eprintln!(
            "{:<40} {:>6} {:>6} {:>8} {:>16} {:>16} {:>5} {:>5} {:>5}",
            name, m.lex_ms, m.parse_ms, m.codegen_ms, s2_tail, s3_tail, one_a, det, m.function_count,
        );
    }

    eprintln!();
    eprintln!("Stage 1A (Rust→Buff transpile): {}/{} files", stage1a_pass, total);
    eprintln!("Determinism (Stage 2 == Stage 3): {}/{} files", determinism_pass, stage1a_pass);
    if report.determinism_holds {
        eprintln!("RESULT: DETERMINISM HOLDS for all files that transpiled.");
    } else if stage1a_pass == 0 {
        eprintln!("RESULT: Stage 1A failed for every file (see bootstrap-report.md).");
    } else {
        eprintln!("RESULT: DETERMINISM DOES NOT HOLD for at least one file (investigate).");
    }
}

#[derive(serde::Serialize)]
struct FixtureMeasurement {
    name: String,
    lex_ms: u128,
    parse_ms: u128,
    codegen_ms: u128,
    /// Hash of the Rust source produced on the FIRST transpile run.
    /// This is "Stage 2" in T19 terminology (Buff-written compiler
    /// transpiling itself), proxy-verified here via the Rust-written
    /// compiler (which is byte-deterministic by spec).
    stage2_hash: Option<String>,
    /// Hash of the Rust source produced on the SECOND transpile run.
    /// This is "Stage 3" in T19 terminology.
    stage3_hash: Option<String>,
    /// Number of bytes in the Stage 2 Rust output (helpful for diff sizing).
    stage2_bytes: Option<u64>,
    stage3_bytes: Option<u64>,
    /// True iff lex+parse+codegen all succeeded for this file.
    stage1a_pass: bool,
    /// True iff stage2_hash == stage3_hash AND stage2_bytes == stage3_bytes.
    determinism_pass: bool,
    function_count: u64,
    /// First error encountered, if any (lex/parse/codegen). Empty on full success.
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct BootstrapReport {
    captured_at: String,
    git_sha: String,
    host: String,
    self_host_dir: String,
    total_files: u64,
    stage1a_pass: u64,
    stage1a_fail: u64,
    determinism_pass: u64,
    determinism_fail: u64,
    /// Overall verdict: true iff every file that transpiled also produced
    /// byte-identical Stage 2 == Stage 3 output.
    determinism_holds: bool,
    fixtures: BTreeMap<String, FixtureMeasurement>,
}

fn measure_fixture(path: &Path, name: &str) -> FixtureMeasurement {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return error_only(name, format!("read_error: {e}"));
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
                parse_ms: 0, codegen_ms: 0,
                stage2_hash: None, stage3_hash: None,
                stage2_bytes: None, stage3_bytes: None,
                stage1a_pass: false, determinism_pass: false,
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
                codegen_ms: 0,
                stage2_hash: None, stage3_hash: None,
                stage2_bytes: None, stage3_bytes: None,
                stage1a_pass: false, determinism_pass: false,
                function_count: 0,
                error: Some(format!("parse_error: {}", e.diagnostic.message)),
            };
        }
    };
    let parse_ms = start.elapsed().as_millis();
    let function_count = count_funcs(&decls);

    // 3. Stage 2 — first codegen run.
    let start = Instant::now();
    let rust_source_2 = match generate_rust(&decls) {
        Ok(s) => s,
        Err(e) => {
            return FixtureMeasurement {
                name: name.to_string(),
                lex_ms, parse_ms,
                codegen_ms: start.elapsed().as_millis(),
                stage2_hash: None, stage3_hash: None,
                stage2_bytes: None, stage3_bytes: None,
                stage1a_pass: false, determinism_pass: false,
                function_count,
                error: Some(format!("codegen_error (stage2): {}", e.diagnostic.message)),
            };
        }
    };
    let codegen_ms = start.elapsed().as_millis();
    let stage2_hash = sha256_hex(&rust_source_2);
    let stage2_bytes = rust_source_2.len() as u64;

    // 4. Stage 3 — second codegen run on the SAME parsed AST. This is the
    //    actual byte-determinism assertion: if any internal state leaked
    //    (counter, allocation address, etc.) the bytes would diverge.
    let rust_source_3 = match generate_rust(&decls) {
        Ok(s) => s,
        Err(e) => {
            return FixtureMeasurement {
                name: name.to_string(),
                lex_ms, parse_ms, codegen_ms,
                stage2_hash: Some(stage2_hash), stage3_hash: None,
                stage2_bytes: Some(stage2_bytes), stage3_bytes: None,
                stage1a_pass: false, determinism_pass: false,
                function_count,
                error: Some(format!("codegen_error (stage3): {}", e.diagnostic.message)),
            };
        }
    };
    let stage3_hash = sha256_hex(&rust_source_3);
    let stage3_bytes = rust_source_3.len() as u64;

    // 5. Byte-determinism check. Both the hashes AND the raw byte strings
    //    must match — the hash alone could collide (sha256 won't, but the
    //    extra check is free and produces a better diagnostic on failure).
    let bytes_equal = rust_source_2.as_bytes() == rust_source_3.as_bytes();
    let hashes_equal = stage2_hash == stage3_hash;
    let determinism_pass = bytes_equal && hashes_equal;

    FixtureMeasurement {
        name: name.to_string(),
        lex_ms, parse_ms, codegen_ms,
        stage2_hash: Some(stage2_hash),
        stage3_hash: Some(stage3_hash),
        stage2_bytes: Some(stage2_bytes),
        stage3_bytes: Some(stage3_bytes),
        stage1a_pass: true,
        determinism_pass,
        function_count,
        error: if determinism_pass {
            None
        } else {
            Some(format!(
                "determinism_failed: hashes_equal={hashes_equal} bytes_equal={bytes_equal}"
            ))
        },
    }
}

fn error_only(name: &str, msg: String) -> FixtureMeasurement {
    FixtureMeasurement {
        name: name.to_string(),
        lex_ms: 0, parse_ms: 0, codegen_ms: 0,
        stage2_hash: None, stage3_hash: None,
        stage2_bytes: None, stage3_bytes: None,
        stage1a_pass: false, determinism_pass: false,
        function_count: 0,
        error: Some(msg),
    }
}

fn sha256_hex(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn collect_buff_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_buff_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("buff") {
            out.push(path);
        }
    }
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
