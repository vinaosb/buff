#!/bin/bash
# REAL behavioral-equivalence harness: compares .buff port stdout vs Rust original stdout.
#
# For each ported .buff file with a corresponding Rust example binary:
# 1. Runs the .buff file via `buff run` (captures STDOUT ONLY)
# 2. Runs the Rust binary via `cargo run --example` (captures STDOUT ONLY)
# 3. Diffs the two raw stdout outputs
# 4. Reports PASS/FAIL per file
#
# This is the TRUE behavioral equivalence test the user requested:
# "we need to be sure they have the same behaviour"
#
# NO OUTPUT FILTERING — raw stdout comparison.
# FAILS on any error (no `|| true` swallowing).
#
# Usage: bash scripts/equivalence-rust-vs-buff.sh
# Exit code: 0 if all pass, 1 if any fail

BUFF=${BUFF:-./target/release/buff}
PASS=0
FAIL=0

# Format: "buff_file|crate|example_name"
TESTS=(
    "crates/buff-lang-error/selfhost/span.buff|buff-lang-error|equivalence_span"
    "crates/buff-lang-error/selfhost/code.buff|buff-lang-error|equivalence_code"
    "crates/buff-lang-ast/selfhost/op.buff|buff-lang-ast|equivalence_op"
    "crates/buff-lang-ast/selfhost/common.buff|buff-lang-ast|equivalence_common"
    "crates/buff-lang-ast/selfhost/literal.buff|buff-lang-ast|equivalence_literal"
    "crates/buff-pubsub/selfhost/event.buff|buff-pubsub|equivalence_event"
    "crates/buff-fsm/selfhost/transition.buff|buff-fsm|equivalence_transition"
    "crates/buff-lang-ffi-guide/selfhost/ffi_guide.buff|buff-lang-ffi-guide|equivalence_ffi_guide"
    "crates/buff-lang-parser/selfhost/parser.buff|buff-lang-parser|equivalence_parser"
    "crates/buff-lang-debug-info/selfhost/debug_info.buff|buff-lang-debug-info|equivalence_debug_info"
    "crates/buff-lang-lexer/selfhost/lexer.buff|buff-lang-lexer|equivalence_lexer"
    "crates/buff-eval/selfhost/eval.buff|buff-eval|equivalence_eval"
    "crates/buff-lang-buffhtml-parser/selfhost/buffhtml_parser.buff|buff-lang-buffhtml-parser|equivalence_buffhtml"
)

echo "=== Rust-vs-Buff Behavioral Equivalence (raw stdout) ==="
echo ""

for test_def in "${TESTS[@]}"; do
    IFS='|' read -r buff_file crate example <<< "$test_def"
    echo -n "Testing $buff_file vs $crate:$example... "

    # Run .buff file — capture STDOUT ONLY (stderr has rustup/compilation noise)
    # NO error swallowing — if buff run fails, the test FAILS
    rm -rf target/buff-cache
    buff_output=$($BUFF run "$buff_file" 2>/dev/null)
    buff_rc=$?

    if [ $buff_rc -ne 0 ]; then
        echo "FAIL (buff run exited $buff_rc)"
        FAIL=$((FAIL+1))
        continue
    fi

    # Run Rust example — capture STDOUT ONLY
    # NO error swallowing — if cargo run fails, the test FAILS
    rust_output=$(cargo run -p "$crate" --example "$example" --release 2>/dev/null)
    rust_rc=$?

    if [ $rust_rc -ne 0 ]; then
        echo "FAIL (cargo run exited $rust_rc)"
        FAIL=$((FAIL+1))
        continue
    fi

    if [ "$buff_output" = "$rust_output" ]; then
        echo "PASS"
        echo "  Output: $(echo "$buff_output" | tr '\n' ' ')"
        PASS=$((PASS+1))
    else
        echo "FAIL (output mismatch)"
        echo "  Buff output: [$buff_output]"
        echo "  Rust output: [$rust_output]"
        FAIL=$((FAIL+1))
    fi
done

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [ $FAIL -gt 0 ]; then
    exit 1
fi
exit 0
