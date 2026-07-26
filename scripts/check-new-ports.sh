#!/bin/bash
# Check all new .buff ports
PASS=0
FAIL=0
for f in \
    crates/buff-lang-lexer/selfhost/lexer.buff \
    crates/buff-template/selfhost/template.buff \
    crates/buff-lang-debug-info/selfhost/debug_info.buff \
    crates/buff-eval/selfhost/eval.buff; do
    echo -n "Testing $f... "
    rm -rf target/buff-cache
    output=$(./target/release/buff run "$f" 2>&1)
    if echo "$output" | grep -q "^Error"; then
        err=$(echo "$output" | head -1)
        echo "FAIL: $err"
        FAIL=$((FAIL+1))
    else
        echo "PASS"
        PASS=$((PASS+1))
    fi
done
echo "=== $PASS PASS, $FAIL FAIL ==="
