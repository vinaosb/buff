#!/bin/bash
# Check all self-host .buff files and categorize results
rm -rf target/buff-cache
pass=0
fail=0
parse_err=0
type_err=0
codegen_err=0
other=0

echo "=== Self-host file check results ==="
for f in $(find self-host -name '*.buff' | sort); do
    out=$(./target/release/buff check "$f" 2>&1)
    rc=$?
    if [ $rc -eq 0 ]; then
        pass=$((pass+1))
        echo "PASS: $f"
    else
        if echo "$out" | grep -qE 'E1[01][0-9]|parse|ParseError|unexpected token'; then
            parse_err=$((parse_err+1))
            echo "PARSE_ERR: $f"
        elif echo "$out" | grep -qE 'E12[0-9]|type mismatch|Type error|cannot find'; then
            type_err=$((type_err+1))
            echo "TYPE_ERR: $f"
        elif echo "$out" | grep -qE 'E13[0-9]|codegen|Codegen'; then
            codegen_err=$((codegen_err+1))
            echo "CODEGEN_ERR: $f"
        else
            other=$((other+1))
            echo "OTHER_FAIL: $f -- $(echo "$out" | head -2 | tr '\n' ' ')"
        fi
    fi
done

echo ""
echo "=== Summary ==="
echo "PASS=$pass PARSE_ERR=$parse_err TYPE_ERR=$type_err CODEGEN_ERR=$codegen_err OTHER=$other TOTAL=56"
