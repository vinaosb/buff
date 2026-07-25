#!/bin/bash
# Behavioral-equivalence test harness for self-host .buff ports.
# For each .buff file with a .expected file, runs `buff run` and compares output.
# If no .expected exists, runs once and creates it (baseline).
#
# Usage: bash scripts/equivalence-check.sh
# CI: continue-on-error (informational)

set -e

BUFF=${BUFF:-./target/release/buff}
PASS=0
FAIL=0
NEW=0

for f in $(find crates -path '*/selfhost/*.buff' | sort); do
    expected="${f}.expected"

    # Run the .buff file
    rm -rf target/buff-cache
    actual=$($BUFF run "$f" 2>&1 | grep -v "^info:" | grep -v "^$" || true)

    if [ ! -f "$expected" ]; then
        # No baseline — create one
        echo "$actual" > "$expected"
        echo "NEW (baseline created): $f"
        NEW=$((NEW+1))
        continue
    fi

    expected_content=$(cat "$expected")
    if [ "$actual" = "$expected_content" ]; then
        echo "PASS: $f"
        PASS=$((PASS+1))
    else
        echo "FAIL: $f"
        echo "  Expected: $expected_content"
        echo "  Actual:   $actual"
        FAIL=$((FAIL+1))
    fi
done

echo ""
echo "Results: $PASS passed, $FAIL failed, $NEW new baselines"
exit 0
