#!/bin/bash
# Check clippy on the 13 new CI crates
CRATES="buff-audio buff-config buff-email buff-i18n buff-image buff-nlp buff-observe buff-plugins buff-reactive buff-resilience buff-archive buff-template buff-web"

for crate in $CRATES; do
    echo -n "Clippy $crate... "
    result=$(cargo clippy -p "$crate" --lib -- -D warnings 2>&1)
    if echo "$result" | tail -3 | grep -q "Finished"; then
        echo "PASS"
    else
        errs=$(echo "$result" | grep "^error" | wc -l)
        warns=$(echo "$result" | grep "^warning" | wc -l)
        first=$(echo "$result" | grep "^error\|^warning: unused\|^warning: dead" | head -1)
        echo "FAIL ($errs errors, $warns warns): $first"
    fi
done
