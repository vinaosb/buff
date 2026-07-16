//! Buff Types crate — type representation and local type inference for Buff.
//!
//! This crate is the semantic-analysis stage that sits between parsing
//! (`buff-lang-parser`) and code generation (`buff-codegen-*`). It defines:
//!
//! - the resolved [`Type`] representation ([`ty`]),
//! - numeric promotion rules ([`promote`]),
//! - a flat symbol table ([`env`]),
//! - the **standard library prelude** ([`prelude`]) — implicit built-in
//!   functions available without `import`,
//! - a local [`TypeInferencer`] that walks the AST ([`infer`]).
//!
//! v0.1 supports only primitive types; v0.5 will add collections and
//! user-defined types.

pub mod env;
pub mod exhaustiveness;
pub mod infer;
pub mod prelude;
pub mod promote;
pub mod range_analysis;
pub mod ty;

pub use env::TypeEnv;
// T27: exhaustiveness checker for `match` expressions. Re-exported at the
// crate root so downstream tools (CLI, LSP, snapshot tests) can call
// `check_program`, `check_match_coverage`, `build_enum_registry`, and
// `check_match_expr` without a long module path.
pub use exhaustiveness::{
    build_enum_registry, check_match_coverage, check_match_expr, check_program, EnumRegistry,
};
pub use infer::TypeInferencer;
pub use promote::{assignable_to, promote_binary};
// T96: standard-library prelude. Re-exported at the crate root so the
// type inferencer and downstream crates (codegen, CLI) can call
// `is_prelude` / `prelude::return_type` without a long path.
pub use prelude::{category_of, is_prelude, lookup, PreludeCategory, PreludeFn};
// T22: pure range-analysis primitives (flexible-mode Int width inference,
// auto-width collection helper). Re-exported at crate root for convenience;
// the module path `range_analysis::` is the canonical location.
pub use range_analysis::{collection_int_width, smallest_int_width, IntRange};
pub use ty::{FloatWidth, IntWidth, Type};

// Re-export `Span` from `buff-lang-error` for downstream convenience.
pub use buff_lang_error::Span;
