#!/bin/bash
# Find all clippy-broken crates
cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c "import json,sys; [print(p['name']) for p in json.load(sys.stdin)['packages']]" | sort | while read pkg; do
    result=$(cargo clippy -p "$pkg" --lib -- -D warnings 2>&1)
    if echo "$result" | grep -q "error:"; then
        errs=$(echo "$result" | grep "error:" | wc -l)
        echo "BROKEN ($errs): $pkg"
    fi
done
