#!/bin/bash
for d in crates/*/; do
    pkg=$(basename "$d")
    cargo fmt -p "$pkg" 2>/dev/null
done
echo "FMT DONE"
