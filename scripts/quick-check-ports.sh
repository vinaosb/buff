#!/bin/bash
for f in crates/buff-template/selfhost/template.buff crates/buff-eval/selfhost/eval.buff crates/buff-lang-debug-info/selfhost/debug_info.buff crates/buff-lang-lexer/selfhost/lexer.buff; do
    echo -n "$f: "
    rm -rf target/buff-cache
    output=$(./target/release/buff run "$f" 2>/dev/null)
    rc=$?
    if [ $rc -eq 0 ]; then
        echo "PASS"
    else
        echo "FAIL (rc=$rc)"
    fi
done
