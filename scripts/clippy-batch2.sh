#!/bin/bash
for c in buff-chat buff-dataframe buff-db buff-game buff-geo buff-http-client buff-lsp buff-mcp buff-protobuf buff-scrape buff-audit buff-validate; do
    echo -n "Clippy $c... "
    result=$(cargo clippy -p "$c" --lib -- -D warnings 2>&1)
    if echo "$result" | tail -3 | grep -q "Finished"; then
        echo "PASS"
    else
        errs=$(echo "$result" | grep "^error" | wc -l)
        first=$(echo "$result" | grep "^error:" | head -1)
        echo "FAIL ($errs): $first"
    fi
done
