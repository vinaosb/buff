#!/bin/bash
for crate in buff-web3 buff-dataframe buff-db buff-lsp buff-mcp; do
    echo "=== $crate ==="
    cargo check -p "$crate" --lib 2>&1 | grep 'error\[' -A3 | head -12
    echo ""
done
