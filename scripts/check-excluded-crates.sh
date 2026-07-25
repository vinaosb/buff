#!/bin/bash
# Check which excluded framework crates actually compile
EXCLUDED=(
    buff-audio buff-auth buff-chat buff-config buff-crypto-extras
    buff-dataframe buff-db buff-email buff-fake buff-fuzz
    buff-game buff-geo buff-http-client buff-i18n buff-image
    buff-jobs buff-lsp buff-mcp buff-nlp buff-observe
    buff-plugins buff-protobuf buff-reactive buff-resilience buff-scrape
    buff-archive buff-audit buff-template buff-validate buff-web buff-web3
)

pass=0
fail=0
for crate in "${EXCLUDED[@]}"; do
    echo -n "Checking $crate... "
    if cargo check -p "$crate" --lib 2>&1 | tail -1 | grep -q "Finished\|Compiling"; then
        echo "PASS"
        pass=$((pass+1))
    else
        result=$(cargo check -p "$crate" --lib 2>&1 | grep "^error" | head -1)
        echo "FAIL: $result"
        fail=$((fail+1))
    fi
done

echo ""
echo "=== Summary: $pass PASS, $fail FAIL ==="
