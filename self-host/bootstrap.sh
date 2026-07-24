#!/usr/bin/env bash
#
# self-host/bootstrap.sh — T19 bootstrap determinism gate (Wave 4).
#
# Runs the three-stage self-hosting verification on a POSIX host where the
# Rust-written Buff compiler can fully link (Linux / macOS / WSL with the
# `ring` C shim installed).
#
# Stages (per task spec):
#   1.  Rust-written compiler compiles Buff-written compiler → buff-self-hosted
#   2.  ./buff-self-hosted build self-host/ → stage2 output (hashed)
#   3.  ./buff-self-hosted build self-host/ → stage3 output (hashed)
#   ✓  if sha256(stage2) == sha256(stage3) → DETERMINISM HOLDS
#
# Run from the repo root so relative `self-host/`, `target/`, and
# `examples/` paths resolve.
#
# Usage:
#   ./self-host/bootstrap.sh              # full pipeline; exit 0 if gate holds
#   ./self-host/bootstrap.sh --skip-stage1  # re-use existing buff-self-hosted
#   ./self-host/bootstrap.sh --help
#
# Exit codes:
#   0  Stage 2 == Stage 3 (determinism gate holds)
#   1  Stage 1 failed (Rust compiler cannot compile the .buff sources)
#   2  Stage 2 failed (buff-self-hosted cannot recompile itself)
#   3  Stage 3 failed
#   4  Stage 2 != Stage 3 (NON-DETERMINISM — investigate)
#   5  Missing prerequisites

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SELF_HOST_DIR="$REPO_ROOT/self-host"
STAGE_DIR="$REPO_ROOT/target/bootstrap"
STAGE1_BIN="$STAGE_DIR/buff-self-hosted"
STAGE2_OUT="$STAGE_DIR/stage2.rs"
STAGE3_OUT="$STAGE_DIR/stage3.rs"
SKIP_STAGE1=0

# --- arg parsing -----------------------------------------------------------

while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-stage1) SKIP_STAGE1=1; shift ;;
        -h|--help)
            sed -n '1,40p' "$0"
            exit 0
            ;;
        *)
            echo "bootstrap.sh: unknown arg '$1' (try --help)" >&2
            exit 5
            ;;
    esac
done

# --- helpers ---------------------------------------------------------------

log()  { printf '\033[1m[bootstrap]\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32m[ok]\033[0m      %s\n' "$*"; }
fail() { printf '\033[1;31m[fail]\033[0m    %s\n' "$*"; }
note() { printf '\033[1;33m[note]\033[0m    %s\n' "$*"; }

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        echo "bootstrap.sh: no sha256sum / shasum available" >&2
        return 1
    fi
}

# --- prerequisites ---------------------------------------------------------

mkdir -p "$STAGE_DIR"

if ! command -v cargo >/dev/null 2>&1; then
    fail "cargo not on PATH"
    exit 5
fi

# --- Stage 1: Rust-written compiler compiles Buff-written compiler ---------
#
# Two sub-steps:
#   1a. Build the Rust-written `buff` binary via cargo.
#   1b. Use `buff build` on a synthetic entrypoint .buff file that imports
#       every self-host .buff module, producing buff-self-hosted[.exe].
#
# NOTE: on hosts where Stage 1b is blocked (e.g. MSVC LNK1104 vcruntime.h
# blocker — see bootstrap-report.md), the T19 driver example in
# `crates/buff-lang-codegen-rust/examples/bootstrap_t19.rs` runs a
# parallel verification of the byte-determinism property using only the
# pure-Rust front-end crates. That example does NOT replace Stage 2/3;
# it provides the determinism evidence when Stage 1b cannot run.

if [[ "$SKIP_STAGE1" -eq 0 ]]; then
    log "Stage 1a: building Rust-written buff compiler (cargo build --release -p buff-lang-cli)"
    if cargo build --release -p buff-lang-cli; then
        ok "Stage 1a: buff binary built at target/release/buff"
    else
        fail "Stage 1a: cargo build failed"
        exit 1
    fi

    log "Stage 1b: Rust-written compiler transpiling + linking Buff-written compiler"
    BUFF_BIN="$REPO_ROOT/target/release/buff"
    # Build every .buff file in self-host/. The CLI's project pipeline
    # (T1 multi-file project compilation) handles the import graph.
    if "$BUFF_BIN" build "$SELF_HOST_DIR" --output "$STAGE1_BIN"; then
        ok "Stage 1b: buff-self-hosted built at $STAGE1_BIN"
    else
        fail "Stage 1b: buff build failed"
        note "This is EXPECTED on the first bootstrap attempt — the .buff"
        note "ports (T15-T18) may have lex/parse/codegen gaps. See"
        note "bootstrap-report.md for the gap inventory."
        note "Falling back to the determinism driver example..."
        if cargo run -p buff-lang-codegen-rust --release --example bootstrap_t19 -- \
               "$SELF_HOST_DIR" "$STAGE_DIR/bootstrap-report.json"; then
            ok "Stage 1b fallback: determinism driver ran successfully"
            note "See bootstrap-report.json for per-file pass/fail."
            exit 1  # still exit-non-zero: Stage 1 did not complete as spec'd
        else
            fail "Stage 1b fallback also failed"
            exit 1
        fi
    fi
fi

if [[ ! -x "$STAGE1_BIN" ]]; then
    fail "buff-self-hosted binary missing at $STAGE1_BIN"
    note "Run without --skip-stage1, or copy a pre-built binary into place."
    exit 2
fi

# --- Stage 2: buff-self-hosted compiles itself (first run) -----------------

log "Stage 2: $STAGE1_BIN build $SELF_HOST_DIR -> $STAGE2_OUT"
if "$STAGE1_BIN" build "$SELF_HOST_DIR" --emit-rust "$STAGE2_OUT"; then
    HASH2="$(sha256_of "$STAGE2_OUT")"
    ok "Stage 2: complete  sha256=$HASH2"
else
    fail "Stage 2: buff-self-hosted could not recompile self-host/"
    exit 2
fi

# --- Stage 3: buff-self-hosted compiles itself (second run) ----------------

log "Stage 3: $STAGE1_BIN build $SELF_HOST_DIR -> $STAGE3_OUT"
if "$STAGE1_BIN" build "$SELF_HOST_DIR" --emit-rust "$STAGE3_OUT"; then
    HASH3="$(sha256_of "$STAGE3_OUT")"
    ok "Stage 3: complete  sha256=$HASH3"
else
    fail "Stage 3: buff-self-hosted could not recompile self-host/"
    exit 3
fi

# --- Determinism assertion -------------------------------------------------

log "Comparing Stage 2 vs Stage 3 ..."
if [[ "$HASH2" == "$HASH3" ]] && diff -q "$STAGE2_OUT" "$STAGE3_OUT" >/dev/null; then
    ok "DETERMINISM HOLDS: Stage 2 == Stage 3 byte-identical"
    echo "         sha256(stage2.rs) = $HASH2"
    echo "         sha256(stage3.rs) = $HASH3"
    exit 0
else
    fail "NON-DETERMINISM: Stage 2 != Stage 3"
    echo "         sha256(stage2.rs) = $HASH2"
    echo "         sha256(stage3.rs) = $HASH3"
    note "Diff (first 200 lines):"
    diff -u "$STAGE2_OUT" "$STAGE3_OUT" | head -n 200 || true
    note "Probable causes:"
    note "  1. HashMap/HashSet iteration order leaking into codegen"
    note "     (CONVENTIONS mandate BTreeMap/BTreeSet — check the failing"
    note "     file's lowering)."
    note "  2. Timestamp / process-id / random embedded in output."
    note "  3. Race in parallel codegen (rayon) producing non-deterministic"
    note "     splice order."
    exit 4
fi
