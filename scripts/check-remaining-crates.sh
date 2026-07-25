#!/bin/bash
# Check remaining excluded framework crates (deps already cached)
EXCLUDED=(
    buff-lsp buff-mcp buff-nlp buff-observe
    buff-plugins buff-protobuf buff-reactive buff-resilience buff-scrape
    buff-archive buff-audit buff-template buff-validate buff-web buff-web3
)

pass=0
fail=0
for crate in "${EXCLUDED[@]}"; do
    echo -n "Checking $crate... "
    result=$(cargo check -p "$crate" --lib 2>&1)
    if echo "$result" | tail -3 | grep -q "Finished"; then
        echo "PASS"
        pass=$((pass+1))
    else
        err=$(echo "$result" | grep "^error" | head -1)
        echo "FAIL: $err"
        fail=$((fail+1))
    fi
done
echo "=== Summary: $pass PASS, $fail FAIL ==="
