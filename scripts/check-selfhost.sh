#!/bin/bash
rm -rf target/buff-cache
PASS=0
FAIL=0
ERRORS=""
for f in self-host/lexer/*.buff self-host/parser/*.buff self-host/types/*.buff self-host/codegen/*.buff; do
    if [ ! -f "$f" ]; then continue; fi
    sed -i 's/\r$//' "$f" 2>/dev/null
    rm -rf target/buff-cache
    if ./target/release/buff check "$f" 2>/dev/null | head -1 | grep -q "no issues"; then
        PASS=$((PASS+1))
    else
        FAIL=$((FAIL+1))
        ERRORS="$ERRORS $f"
    fi
done
echo "PASS: $PASS / $((PASS+FAIL))"
echo "FAIL: $FAIL"
echo "Failed:$ERRORS"
