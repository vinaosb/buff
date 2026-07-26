#!/bin/bash
for f in \
    crates/buff-lang-buffhtml-parser/selfhost/buffhtml_parser.buff \
    crates/buff-lang-parser/selfhost/parser.buff \
    crates/buff-lang-debug-info/selfhost/debug_info.buff; do
    echo -n "$f: "
    rm -rf target/buff-cache
    output=$(./target/release/buff run "$f" 2>/dev/null)
    rc=$?
    if [ $rc -eq 0 ]; then
        echo "PASS"
    else
        err=$(./target/release/buff run "$f" 2>&1 | grep -m1 "Error:" | head -1)
        echo "FAIL: $err"
    fi
done
