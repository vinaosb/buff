# decisions — buff-post-v10-tooling


## [T116] 2026-07-19T16:57:57

- **No JS framework**: Plain HTML/CSS/vanilla JS per task spec. No build step, no bundler. Files can be served directly by any static host.
- **data-buff-source on anchors**: Buff source stored as HTML attributes (with entities for newlines). JS reads at runtime, encodes, and sets href. This avoids hardcoding base64 in HTML (which would be fragile and unreadable).
- **3-column grid for examples**: Rust (red tint) | Buff (green tint) | Why easier (blue tint). Each column has a distinct background color for visual distinction. Stacks to single column below 960px.
- **Quick start shows cargo install (local path)**: Not cargo install buff-cli since crates.io publishing isn't done yet. Matches current README install instructions.
- **Features strip**: A separate section showing 'what Buff removes' (strikethrough + note) reinforces the pitch without bloating examples.
- **Port 8093 for website tests**: Avoids port collision with playground on 8092. Both playwright configs use reuseExistingServer: true.
