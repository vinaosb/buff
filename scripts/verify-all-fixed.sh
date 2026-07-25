#!/bin/bash
# Verify all previously-broken crates
CRATES="buff-chat buff-crypto-extras buff-dataframe buff-db buff-fake buff-fuzz buff-game buff-geo buff-http-client buff-jobs buff-lsp buff-mcp buff-protobuf buff-scrape buff-audit buff-validate buff-web3"
pass=0
fail=0
for c in $CRATES; do
    result=$(cargo check -p "$c" --lib 2>&1)
    if echo "$result" | tail -3 | grep -q "Finished"; then
        echo "PASS: $c"
        pass=$((pass+1))
    else
        errs=$(echo "$result" | grep "^error" | wc -l)
        echo "FAIL ($errs): $c"
        fail=$((fail+1))
    fi
done
echo "=== $pass PASS, $fail FAIL ==="
