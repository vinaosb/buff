#!/bin/bash
for f in crates/buff-lang-error/selfhost/code.buff crates/buff-lang-error/selfhost/types.buff crates/buff-lang-ast/selfhost/op.buff crates/buff-lang-ast/selfhost/common.buff crates/buff-lang-ast/selfhost/literal.buff; do
    echo "=== $f ==="
    cat "$f" | sed -n '/func main/,$p'
    echo "---EXPECTED---"
    cat "$f.expected" 2>/dev/null || echo "(no expected)"
    echo ""
done
