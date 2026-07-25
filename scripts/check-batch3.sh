#!/bin/bash
for c in buff-dataframe buff-db buff-lsp buff-mcp; do
    echo "=== $c ==="
    cargo check -p "$c" --lib 2>&1 | tail -3
    echo ""
done
