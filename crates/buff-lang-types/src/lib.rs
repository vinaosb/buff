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
//! v1.0 ships primitives, collections (Vector/Map/Matrix), user-defined types
//! (struct/enum), traits, full type inference, exhaustiveness checking, and
//! recursion detection.

// T31: async call-graph propagation (fixpoint algorithm). Re-exported at
// the crate root so the codegen pass (and downstream tools) can call
// `buff_lang_types::analyze_async(...)` without a long module path.
pub mod async_analysis;
// T53: Zig-style comptime interpreter. Evaluates `comptime { ... }`
// blocks during type checking, producing constants the codegen splices
// into generated Rust source. Re-exported at the crate root so the
// codegen pass + `buff check` can call `analyze_program(...)` directly.
pub mod comptime;
pub mod env;
pub mod exhaustiveness;
pub mod infer;
// T58: multiple-dispatch (Julia-inspired) registry + resolver. Re-exported
// at the crate root so the type inferencer + Rust codegen consume the table
// by short path. Compile-time-only dispatch on ALL argument types; single-
// dispatch is the special case (group size 1) and bypasses the table
// entirely, so all pre-T58 code keeps compiling unmangled.
pub mod modules;
pub mod multi_dispatch;
// T33: ownership analysis (Copy classification, Arc-across-spawn detection,
// CoW mutation detection). Re-exported at crate root so the codegen pass
// can call `buff_lang_types::analyze_ownership(func)` without a long path.
pub mod ownership;
pub mod prelude;
// T124b: prelude-types registry (DateTime, Date, Time, Duration, Instant +
// their associated functions and instance methods). Re-exported at the
// crate root so the type inferencer + Rust codegen consume the registry by
// short path. This is the GENERAL, extensible mechanism every future v1.4
// stdlib task (Regex, Math, URL, Hash, ...) extends.
pub mod prelude_types;
// T105b: impl PreludeType metadata extracted from prelude_types.rs
// (God Class split). Re-exported via prelude_types so the public API
// is unchanged.
pub mod prelude_type_metadata;
// T105b: PreludeAssocFn enum + impl + lookup extracted from prelude_types.rs.
pub mod prelude_assoc_fn_impl;
// T105b: PreludeAssocConst + impl + lookups extracted from prelude_types.rs.
pub mod prelude_assoc_const_impl;
// T105b: PreludeInstanceFn + impl + lookup extracted from prelude_types.rs.
pub mod prelude_instance_fn_impl;
// T105b: return-type inference functions extracted from prelude_types.rs.
pub mod prelude_return_types;
// T1: project-level parse entry point + span-aware error formatting.
pub mod project;
// T1: cross-file symbol resolution table.
pub mod cross_file;
pub mod promote;
// T48: recursion detection (call-graph cycle detection). Re-exported at
// the crate root so the codegen pass + T49 hint-driven codegen can call
// `buff_lang_types::analyze_recursion(decls)` without a long module path.
// Deterministic (BTreeMap/BTreeSet based) — same AST → byte-identical
// cpu_only set every time (the T29 flaky-test lesson).
pub mod range_analysis;
pub mod recursion;
pub mod ty;

pub use env::TypeEnv;
// T53: comptime interpreter + program-level analysis entry point.
// Re-exported at crate root so the codegen pass + `buff check` can call
// `buff_lang_types::analyze_program(...)` without a long module path.
pub use comptime::{
    analyze_program, ComptimeError, ComptimeFacts, ComptimeInterpreter, ComptimeValue,
    COMPTIME_MAX_DEPTH,
};
// T31: async call-graph propagation. Re-exported at crate root so callers
// (codegen, CLI, snapshot tests) can use `analyze_async`, `build_call_graph`,
// `propagate_async`, etc. without a long path. Deterministic (BTreeMap/
// BTreeSet based) — same AST → byte-identical async set every time.
pub use async_analysis::{
    analyze_async, build_call_graph, is_async_after_propagation, propagate_async, AsyncSet,
    CallGraph,
};
// T27: exhaustiveness checker for `match` expressions. Re-exported at the
// crate root so downstream tools (CLI, LSP, snapshot tests) can call
// `check_program`, `check_match_coverage`, `build_enum_registry`, and
// `check_match_expr` without a long module path.
// T28: `build_enum_registry_with_prelude` seeds the built-in Option enum.
pub use exhaustiveness::{
    build_enum_registry, build_enum_registry_with_prelude, check_match_coverage, check_match_expr,
    check_program, EnumRegistry,
};
pub use infer::TypeInferencer;
// T58: multiple-dispatch table (Julia-inspired). Re-exported at crate root
// so the type inferencer + Rust codegen consume it by short path.
pub use multi_dispatch::{MultiDispatchMethod, MultiDispatchTable};
// T29: module-system graph. Re-exported at crate root for the CLI/LSP
// so callers can `buff_lang_types::build_graph(...)` without a long path.
pub use modules::{
    build_graph, resolve_path, FsLoader, MemoryLoader, Module, ModuleGraph, ModuleLoader,
};
// T1: project-level parse entry point + span-aware error type.
pub use project::{parse_project, parse_project_with_loader, ParsedProject, ProjectError};
// T1: cross-file symbol resolution table.
pub use cross_file::{CrossFileSymbolTable, SymbolKind, SymbolSignature};
pub use promote::{assignable_to, promote_binary};
// T48: recursion detection (call-graph cycle detection). Re-exported at
// crate root for the codegen pass + T49 hint-driven codegen so callers can
// use `buff_lang_types::RecursionFacts` / `analyze_recursion` /
// `is_cpu_only_after_recursion_analysis` without a long module path.
// Deterministic (BTreeSet based) — same AST → byte-identical cpu_only set
// every time (the T29 flaky-test lesson).
pub use recursion::{analyze_recursion, detect_cycles, has_prefer_gpu_attr, RecursionFacts};
// T33: ownership analysis (Copy/Arc/CoW facts). Re-exported at crate root
// for the codegen pass (and snapshot tests) so callers can use
// `buff_lang_types::OwnershipFacts` / `analyze_ownership` without a long
// module path. Deterministic (BTreeSet based) — same AST → byte-identical
// facts every time (the T29 flaky-test lesson).
pub use ownership::{analyze_func as analyze_ownership, closure_captures, OwnershipFacts};
// T96: standard-library prelude. Re-exported at the crate root so the
// type inferencer and downstream crates (codegen, CLI) can call
// `is_prelude` / `prelude::return_type` without a long path.
pub use prelude::{category_of, is_prelude, lookup, PreludeCategory, PreludeFn};
// T124b: prelude-types registry (DateTime / Date / Time / Duration /
// Instant + their associated fns + instance methods). Re-exported at the
// crate root so the inferencer + codegen consume the registry by short
// path. Future v1.4 stdlib tasks (Regex, Math, URL, Hash, ...) extend
// this registry rather than rewriting the inferencer or codegen.
// T124f: associated-CONST registry (`Math.PI` / `Math.E`) added for
// the Math utility module.
pub use prelude_types::{
    assoc_const_lookup, assoc_const_return_type, assoc_fn_lookup, assoc_fn_return_type,
    instance_fn_lookup, instance_fn_return_type, is_prelude_type, prelude_type_lookup,
    PreludeAssocConst, PreludeAssocFn, PreludeInstanceFn, PreludeType,
};
// T22: pure range-analysis primitives (flexible-mode Int width inference,
// auto-width collection helper). Re-exported at crate root for convenience;
// the module path `range_analysis::` is the canonical location.
pub use range_analysis::{collection_int_width, smallest_int_width, IntRange};
pub use ty::{FloatWidth, IntWidth, Type};

// Re-export `Span` from `buff-lang-error` for downstream convenience.
pub use buff_lang_error::Span;
