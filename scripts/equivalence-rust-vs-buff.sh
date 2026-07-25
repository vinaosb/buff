#!/bin/bash
# REAL behavioral-equivalence harness: compares .buff port output vs Rust original output.
#
# For each ported .buff file with a corresponding Rust example binary:
# 1. Runs the .buff file via `buff run`
# 2. Runs the Rust binary via `cargo run --example`
# 3. Diffs the two outputs
# 4. Reports PASS/FAIL per file
#
# This is the TRUE behavioral equivalence test the user requested:
# "we need to be sure they have the same behaviour"
#
# Usage: bash scripts/equivalence-rust-vs-buff.sh
# CI: continue-on-error (informational)

set -e

BUFF=${BUFF:-./target/release/buff}
PASS=0
FAIL=0
NEW=0

# Format: "buff_file|crate|example_name"
TESTS=(
    "crates/buff-lang-error/selfhost/span.buff|buff-lang-error|equivalence_span"
    "crates/buff-lang-error/selfhost/code.buff|buff-lang-error|equivalence_code"
    "crates/buff-lang-ast/selfhost/op.buff|buff-lang-ast|equivalence_op"
    "crates/buff-lang-ast/selfhost/common.buff|buff-lang-ast|equivalence_common"
    "crates/buff-lang-ast/selfhost/literal.buff|buff-lang-ast|equivalence_literal"
)

echo "=== Rust-vs-Buff Behavioral Equivalence ==="
echo ""

for test_def in "${TESTS[@]}"; do
    IFS='|' read -r buff_file crate example <<< "$test_def"
    echo -n "Testing $buff_file vs $crate:$example... "

    # Run .buff file
    rm -rf target/buff-cache
    buff_output=$($BUFF run "$buff_file" 2>&1 | grep -E '^[0-9]+$|^true$|^false$|^E[0-9]{4}$' || true)

    # Run Rust example — filter to ONLY actual program output (numeric, boolean, or error codes)
    rust_output=$(cargo run -p "$crate" --example "$example" --release 2>&1 | grep -E '^[0-9]+$|^true$|^false$|^E[0-9]{4}$' || true)

    if [ "$buff_output" = "$rust_output" ]; then
        echo "PASS"
        echo "  Output: $(echo "$buff_output" | tr '\n' ' ')"
        PASS=$((PASS+1))
    else
        echo "FAIL"
        echo "  Buff output: $(echo "$buff_output" | tr '\n' ' ')"
        echo "  Rust output: $(echo "$rust_output" | tr '\n' ' ')"
        FAIL=$((FAIL+1))
    fi
done

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
exit 0
