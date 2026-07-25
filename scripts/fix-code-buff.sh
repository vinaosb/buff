#!/bin/bash
# Fix code.buff: replace String.from("...") with just "..."
sed -i 's/String\.from(("\([^"]*\)"))/\1/g; s/String\.from("\([^"]*\)")/\1/g' crates/buff-lang-error/selfhost/code.buff
echo "=== After fix ==="
grep '=>' crates/buff-lang-error/selfhost/code.buff | head -5
