//! Registry drift alarm (ITER-55): cross-checks the DUAL prelude
//! registries. `buff-lang-types` declares the stdlib surface
//! (`PreludeType` / `PreludeAssocFn` enums); `buff-lang-codegen-rust`
//! lowers them. The codegen dispatch ends in a `_` wildcard, so a
//! registry entry added to types WITHOUT a codegen arm is silently
//! absorbed into "unsupported: prelude type+method combination" at
//! `buff build` time (the api-compat v2.0 drift class - see
//! .sisyphus/decisions/api-compat-v20.md).
//!
//! This test makes that drift mechanical: every variant declared in the
//! types enums must be referenced by at least one codegen lowering file.
//! New registry growth must land with the matching lowering arm (or an
//! explicit, reviewed DEFERRED entry below referencing the tracking
//! decision doc).
//!
//! The source-file scan is deliberately lexical (include_str! + name-set
//! comparison): it needs no new enumeration APIs and stays correct even
//! as the registries grow.

use std::collections::BTreeSet;

const TYPES_REGISTRY: &str = include_str!("../../buff-lang-types/src/prelude_types.rs");
const TYPES_ASSOC_REGISTRY: &str =
    include_str!("../../buff-lang-types/src/prelude_assoc_fn_impl.rs");
const CODEGEN_DISPATCH: &str = include_str!("../src/rust_codegen/prelude_types.rs");
const CODEGEN_FRAMEWORK: &str = include_str!("../src/rust_codegen/prelude_types/framework.rs");

/// Variants declared in `pub enum <name> { ... }` (skips docs/attrs).
fn declared_variants(src: &str, enum_name: &str) -> BTreeSet<String> {
    let mut in_enum = false;
    let mut out = BTreeSet::new();
    for line in src.lines() {
        if line.starts_with("pub enum ") && line.contains(enum_name) {
            in_enum = true;
            continue;
        }
        if !in_enum {
            continue;
        }
        let trimmed = line.trim();
        if trimmed == "}" {
            in_enum = false;
            continue;
        }
        // Variant declarations: `Name,` or `Name(..)` — single leading
        // indent inside the enum body; doc comments and attributes are
        // skipped by the anchored pattern.
        let indent4 = line.starts_with("    ") && !line.starts_with("     ");
        if indent4 {
            let first = trimmed.split([',', '(']).next().unwrap_or("");
            if !first.is_empty()
                && first.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && first.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                out.insert(first.to_string());
            }
        }
    }
    out
}

/// Identifiers referenced as `Prefix::Name` in the codegen lowering
/// sources (`T::DateTime`, `A::Now`, ...).
fn referenced_variants(src: &str, prefix: &str) -> BTreeSet<String> {
    let marker = format!("{prefix}::");
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let mut rest = line;
        while let Some(pos) = rest.find(&marker) {
            let after = &rest[pos + marker.len()..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let name_len = name.len();
            if !name.is_empty() && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                out.insert(name);
            }
            rest = &after[name_len.min(after.len())..];
        }
    }
    out
}

/// Deliberately-unlowered types, each with the tracking decision doc.
/// These type-check (`buff check` passes) but lower to "unsupported" -
/// the api-compat v2.0 coordinated backlog (ITER-56 worklist). Mostly
/// framework crates whose value lowering is deferred; `Time` and `Range`
/// look like genuine core gaps to assess there too.
const DEFERRED_TYPES: &[&str] = &[
    "ActorRef",
    "ActorSystem",
    "ChildSpec",
    "ConnectedWallet",
    "ContractMethod",
    "Element",
    "Entity",
    "Language",
    "OAuth2Client",
    "Observe",
    "Platform",
    "Range",
    "Rbac",
    "RestartStrategy",
    "RsaKeypair",
    "Signal",
    "Spectrum",
    "StemAlgorithm",
    "Supervisor",
    "Time",
    "World",
];

/// Deliberately-unlowered assoc fns, each with the tracking decision doc.
/// A variant listed here type-checks (`buff check` passes) but lowers to
/// "unsupported" - the api-compat v2.0 coordinated backlog: metrics
/// (Counter/Gauge/Histogram), OAuth (AuthorizationUrl/ExchangeCode), RBAC
/// (Enforce), misc (Bootstrap/Filled/Span). Tensor construction
/// (Zeros/Ones/FromVec) landed in ITER-56; DSP windows
/// (Blackman/Hamming/Hann) landed in ITER-56C.
const DEFERRED_ASSOC_FNS: &[&str] = &[
    "AuthorizationUrl",
    "Bootstrap",
    "Counter",
    "Enforce",
    "ExchangeCode",
    "Filled",
    "Gauge",
    "Histogram",
    "Span",
];

#[test]
fn every_prelude_type_has_a_codegen_reference() {
    let declared = declared_variants(TYPES_REGISTRY, "PreludeType");
    assert!(
        !declared.is_empty(),
        "PreludeType enum not found - registry moved? update the include paths"
    );
    let mut lowered: BTreeSet<String> = referenced_variants(CODEGEN_DISPATCH, "T");
    lowered.extend(referenced_variants(CODEGEN_FRAMEWORK, "T"));
    lowered.extend(DEFERRED_TYPES.iter().map(|s| s.to_string()));
    let missing: Vec<_> = declared.difference(&lowered).collect();
    assert!(
        missing.is_empty(),
        "PreludeType variants with NO codegen lowering reference: {missing:?}. \
         Add the lowering arm, or a reviewed DEFERRED entry citing the \
         tracking decision doc."
    );
}

#[test]
fn every_prelude_assoc_fn_has_a_codegen_reference() {
    let declared = declared_variants(TYPES_ASSOC_REGISTRY, "PreludeAssocFn");
    assert!(
        !declared.is_empty(),
        "PreludeAssocFn enum not found - registry moved? update the include paths"
    );
    let mut lowered: BTreeSet<String> = referenced_variants(CODEGEN_DISPATCH, "A");
    lowered.extend(referenced_variants(CODEGEN_FRAMEWORK, "A"));
    lowered.extend(DEFERRED_ASSOC_FNS.iter().map(|s| s.to_string()));
    let missing: Vec<_> = declared.difference(&lowered).collect();
    assert!(
        missing.is_empty(),
        "PreludeAssocFn variants with NO codegen lowering reference: {missing:?}. \
         These type-check but lower to 'unsupported' - the api-compat v2.0 \
         drift class. Add the arm or a reviewed DEFERRED entry."
    );
}
