#!/bin/bash
rm -rf target/buff-cache
for f in self-host/types/async_analysis.buff self-host/types/exhaustiveness.buff self-host/types/multi_dispatch.buff self-host/types/project.buff self-host/types/recursion.buff; do
    echo "---"
    echo "$f"
    ./target/release/buff check "$f" 2>&1 | head -3
done
