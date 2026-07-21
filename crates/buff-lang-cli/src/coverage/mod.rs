//! Coverage mapping module (T137).
//!
//! Translates Rust-level line coverage data (captured by `cargo llvm-cov`
//! or `cargo-tarpaulin`) back to `.buff` source lines using the T60
//! [`SourceMap`](buff_lang_error::SourceMap) bidirectional Buff ↔ Rust
//! line map.
//!
//! # Pipeline
//!
//! ```text
//!  cargo llvm-cov --json           ─┐
//!  cargo-tarpaulin --out json      │  parse()
//!  (raw Rust-line coverage JSON)   ─┘
//!                                       ▼
//!                                 Vec<RustLineHit>
//!                                       │
//!                                       ▼  map_rust_to_buff()
//!                                 Vec<BuffLineHit>
//!                                       │
//!                                       ▼  aggregate()
//!                                 BuffCoverage
//!                                       │
//!                       ┌─────────────┴─────────────┐
//!                       ▼                           ▼
//!                 render_lcov()               render_html()
//!                 (LCOV .info)                (HTML report)
//! ```
//!
//! # The line-mapping contract
//!
//! [`buff_lang_error::SourceMap`] records a many-to-one `rust_line →
//! buff_span` mapping populated during codegen. The CLI then resolves
//! `buff_span` to a 1-based `(file, line)` pair via
//! [`SourceMap::lookup`](buff_lang_error::SourceMap::lookup) +
//! [`SourceFile`](buff_lang_error::SourceFile)'s byte-offset-to-line
//! lookup.
//!
//! Buff's CLI compiles one `.buff` translation unit at a time (the
//! `compile_to_rust` pipeline is single-file). All generated Rust lines
//! therefore map back into a single `.buff` source. Multi-file coverage
//! is a post-v1.10 concern — see `task-137-coverage.txt` GAP-1.
//!
//! # Local vs. live runs
//!
//! The mapping + report-emission layer is **pure + local-buildable**:
//! unit tests feed in a synthetic `SourceMap` + sample `cargo-llvm-cov`
//! JSON and assert the translated `.buff` coverage. The CLI's actual
//! llvm-cov / tarpaulin invocation requires the tool to be installed
//! on the host — see `task-137-coverage-USER-ACTION.txt` for the
//! PowerShell install + run recipe.

pub mod html;
pub mod lcov;
pub mod map;
pub mod model;
pub mod parse;

pub use html::render_html;
pub use lcov::render_lcov;
pub use map::map_rust_to_buff;
pub use model::{BuffCoverage, BuffFileCoverage, BuffLineHit, RustLineHit};
pub use parse::{parse_llvm_cov_json, LlvmCovError};
