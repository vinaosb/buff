//! T134 — prop type pre-checker.
//!
//! A diagnostic pass that runs AFTER parse + codegen, BEFORE rustc.
//! For each `<Component prop: value />` usage in a parent `.buffhtml`,
//! validates against the child component's declared Props interface:
//!
//! - Required props are all provided.
//! - No unknown props.
//! - (Stretch) Simple literal-type matching.
//!
//! Diagnostics point at `.buffhtml` spans via the existing [`SpanMap`]
//! infrastructure (or directly via [`Span`] attached to each
//! diagnostic — the caller chooses the rendering path).
//!
//! # Architecture
//!
//! 1. The CLI (or unit test) builds a [`PropInterfaceRegistry`] by
//!    walking every `.buffhtml` file in scope + calling
//!    [`extract_interface`] on each.
//! 2. For each parent template, the CLI calls [`check_props`] with the
//!    parent's AST + the registry.
//! 3. The returned `Vec<PropCheckDiagnostic>` is rendered via the
//!    existing error-mapper (spans map 1:1 to `.buffhtml` positions).
//!
//! If a component has NO declared interface (no `props="..."` on its
//! `<script>` block, or the named struct is missing), the pre-checker
//! skips that component (backward-compat with T133 floor components
//! — they have no interface to check against).

use std::collections::HashMap;

use buff_lang_ast_rsx::{RsxAttributeKind, RsxNode, RsxTemplateFile};
use buff_lang_error::Span;

// ---------------------------------------------------------------------------
// Data types.
// ---------------------------------------------------------------------------

/// One field in a component's declared Props interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropField {
    /// Field identifier (e.g. `name`, `count`).
    pub name: String,
    /// Type as written in the source (`String`, `i32`, `bool`).
    pub type_name: String,
    /// `true` when the field has no default value (i.e. is a required
    /// prop). For T134 we treat every declared field as required —
    /// the `Option<T>` opt-out is T135+.
    pub required: bool,
}

/// A component's declared interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentInterface {
    /// Component tag name as used in templates (`Greeting`, `Card`).
    pub tag_name: String,
    /// Declared struct name (`Props`, or whatever `props="..."` names).
    pub struct_name: String,
    pub fields: Vec<PropField>,
}

/// Registry mapping component tag name → declared interface.
///
/// Built by the caller (CLI / test harness). Lookup is case-sensitive
/// (matching the Svelte/JSX uppercase-first-letter convention).
#[derive(Debug, Clone, Default)]
pub struct PropInterfaceRegistry {
    interfaces: HashMap<String, ComponentInterface>,
}

impl PropInterfaceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, interface: ComponentInterface) {
        self.interfaces
            .insert(interface.tag_name.clone(), interface);
    }

    pub fn lookup(&self, tag_name: &str) -> Option<&ComponentInterface> {
        self.interfaces.get(tag_name)
    }

    pub fn len(&self) -> usize {
        self.interfaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.interfaces.is_empty()
    }
}

/// Category of pre-checker finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropCheckKind {
    /// Required prop is missing from the invocation.
    MissingRequired,
    /// Invocation passes a prop not declared on the component.
    UnknownProp,
    /// Best-effort literal-type mismatch (e.g. `name: 42` against
    /// `String`).
    TypeMismatch,
}

/// One pre-checker diagnostic. Spans point at the `.buffhtml` source
/// position of the offending construct (either the missing attribute
/// location — defaults to the component tag's span — or the offending
/// attribute's own span).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropCheckDiagnostic {
    pub kind: PropCheckKind,
    pub message: String,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Interface extraction (run on each component .buffhtml).
// ---------------------------------------------------------------------------

/// Extract a component's declared interface from its parsed template.
///
/// Returns `None` when:
/// - The template has no `<script>` block.
/// - The script has no `props="..."` attribute.
/// - The named struct is not present in the script body.
///
/// When `Some`, the returned [`ComponentInterface`] carries every
/// named field of the declared struct (each marked `required: true`
/// per the T134 contract — `Option<T>` opt-out is T135+).
pub fn extract_interface(
    template: &RsxTemplateFile,
    component_tag_name: &str,
) -> Option<ComponentInterface> {
    let script = template.script.as_ref()?;
    let props_name = script.props.as_ref()?;
    let fields = parse_struct_fields(&script.source, props_name)?;
    Some(ComponentInterface {
        tag_name: component_tag_name.to_string(),
        struct_name: props_name.clone(),
        fields,
    })
}

/// Parse `struct <Name> { field1: Type1, field2: Type2 }` from a
/// Rust source string. Returns `None` when the named struct cannot
/// be found (rustc will surface the type-not-found error separately).
fn parse_struct_fields(script_source: &str, struct_name: &str) -> Option<Vec<PropField>> {
    let wrapped = format!("{{{script_source}}}");
    let block: syn::Block = syn::parse_str(&wrapped).ok()?;
    let target = syn::Ident::new(struct_name, proc_macro2::Span::call_site());
    for stmt in block.stmts {
        if let syn::Stmt::Item(syn::Item::Struct(s)) = stmt {
            if s.ident == target {
                return Some(
                    s.fields
                        .iter()
                        .filter_map(|f| {
                            let name = f.ident.as_ref()?.to_string();
                            let type_name = quote::ToTokens::to_token_stream(&f.ty)
                                .to_string()
                                .replace(' ', "");
                            Some(PropField {
                                name,
                                type_name,
                                required: true,
                            })
                        })
                        .collect(),
                );
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Pre-checker (run on each PARENT template).
// ---------------------------------------------------------------------------

/// Validate every `<Component ... />` invocation in `parent` against
/// the registry.
///
/// Returns a `Vec<PropCheckDiagnostic>` (empty when clean). Order is
/// deterministic: depth-first, source-order.
pub fn check_props(
    parent: &RsxTemplateFile,
    registry: &PropInterfaceRegistry,
) -> Vec<PropCheckDiagnostic> {
    let mut out = Vec::new();
    for node in &parent.root {
        walk_node(node, registry, &mut out);
    }
    out
}

fn walk_node(node: &RsxNode, registry: &PropInterfaceRegistry, out: &mut Vec<PropCheckDiagnostic>) {
    match node {
        RsxNode::Element(e) => {
            if e.is_component {
                check_component_invocation(e, registry, out);
            }
            for child in &e.children {
                walk_node(child, registry, out);
            }
        }
        RsxNode::Fragment(f) => {
            for child in &f.children {
                walk_node(child, registry, out);
            }
        }
        RsxNode::If(i) => {
            for b in &i.branches {
                for n in &b.body {
                    walk_node(n, registry, out);
                }
            }
            if let Some(else_body) = &i.else_branch {
                for n in else_body {
                    walk_node(n, registry, out);
                }
            }
        }
        RsxNode::Each(e) => {
            for n in &e.body {
                walk_node(n, registry, out);
            }
            if let Some(else_body) = &e.else_branch {
                for n in else_body {
                    walk_node(n, registry, out);
                }
            }
        }
        _ => { /* text / interp / comment / slot / script / raw_html / await: no children */ }
    }
}

fn check_component_invocation(
    e: &buff_lang_ast_rsx::RsxElement,
    registry: &PropInterfaceRegistry,
    out: &mut Vec<PropCheckDiagnostic>,
) {
    let Some(iface) = registry.lookup(&e.tag) else {
        // Backward-compat: component has no declared interface → skip.
        return;
    };
    // Collect the names of props the caller provided. `{...spread}`
    // is treated as "covers all required props" because we can't
    // statically know what's inside (the rustc-side typecheck will
    // catch concrete mismatches).
    let mut provided: HashMap<String, ProvidedProp> = HashMap::new();
    let mut has_spread = false;
    for a in &e.attributes {
        match &a.kind {
            RsxAttributeKind::NamedProp {
                name,
                value,
                value_span,
            } => {
                provided.insert(
                    name.clone(),
                    ProvidedProp {
                        value: value.clone(),
                        span: *value_span,
                    },
                );
            }
            RsxAttributeKind::Literal { name, value } => {
                provided.insert(
                    name.clone(),
                    ProvidedProp {
                        value: value.clone(),
                        span: a.span,
                    },
                );
            }
            RsxAttributeKind::Expression {
                name,
                expr,
                expr_span,
            } => {
                provided.insert(
                    name.clone(),
                    ProvidedProp {
                        value: expr.clone(),
                        span: *expr_span,
                    },
                );
            }
            RsxAttributeKind::Boolean { name } => {
                provided.insert(
                    name.clone(),
                    ProvidedProp {
                        value: "true".to_string(),
                        span: a.span,
                    },
                );
            }
            RsxAttributeKind::Spread { .. } => {
                // Spread covers everything — opt out of unknown-prop
                // AND missing-required checks for this invocation.
                has_spread = true;
            }
            RsxAttributeKind::Event { .. } | RsxAttributeKind::Bind { .. } => {
                // Event handlers and bind: directives are not props.
            }
        }
    }
    if has_spread {
        return;
    }

    // 1. Missing-required check.
    for f in &iface.fields {
        if f.required && !provided.contains_key(&f.name) {
            out.push(PropCheckDiagnostic {
                kind: PropCheckKind::MissingRequired,
                message: format!(
                    "component `{}` is missing required prop `{}`",
                    iface.tag_name, f.name
                ),
                // Point at the component tag itself — the missing
                // attribute has no location of its own.
                span: e.span,
            });
        }
    }

    // 2. Unknown-prop check.
    for (name, pp) in &provided {
        if !iface.fields.iter().any(|f| &f.name == name) {
            out.push(PropCheckDiagnostic {
                kind: PropCheckKind::UnknownProp,
                message: format!(
                    "component `{}` has no prop `{}` (declared interface: `{}`)",
                    iface.tag_name, name, iface.struct_name
                ),
                span: pp.span,
            });
        }
    }

    // 3. Stretch: literal-type matching.
    for f in &iface.fields {
        if let Some(pp) = provided.get(&f.name) {
            if let Some(lit_kind) = classify_literal(&pp.value) {
                if let Some(mismatch) = type_mismatch(lit_kind, &f.type_name) {
                    out.push(PropCheckDiagnostic {
                        kind: PropCheckKind::TypeMismatch,
                        message: format!(
                            "prop `{}` expects `{}` but received {} literal (`{}`)",
                            f.name, f.type_name, mismatch, pp.value
                        ),
                        span: pp.span,
                    });
                }
            }
        }
    }
}

/// One caller-provided prop value (with its source span).
struct ProvidedProp {
    value: String,
    span: Span,
}

/// Best-effort classification of a prop value as a Rust literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiteralKind {
    String,
    Integer,
    Boolean,
}

/// Classify a raw prop-value source string as a literal kind, when
/// unambiguous. Non-literal expressions (identifiers, calls) return
/// `None` — they're not statically checkable.
fn classify_literal(value: &str) -> Option<LiteralKind> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
        return Some(LiteralKind::String);
    }
    if v == "true" || v == "false" {
        return Some(LiteralKind::Boolean);
    }
    // Integer: optional sign, digits only (no float / hex suffix —
    // stretch keeps the matcher simple).
    let bytes = v.as_bytes();
    let start = if bytes[0] == b'-' || bytes[0] == b'+' {
        1
    } else {
        0
    };
    if start < bytes.len() && bytes[start..].iter().all(|b| b.is_ascii_digit()) {
        return Some(LiteralKind::Integer);
    }
    None
}

/// Returns `Some(description)` when a literal kind definitely does
/// not match a declared type-name. Conservative: returns `None`
/// (no mismatch) for any non-obvious case.
fn type_mismatch(lit: LiteralKind, declared_type: &str) -> Option<&'static str> {
    // Normalize the declared type-name for matching.
    let t = declared_type.replace(' ', "");
    match lit {
        LiteralKind::String => match t.as_str() {
            "String" | "&str" | "&'staticstr" | "str" | "char" => None,
            _ => Some("string"),
        },
        LiteralKind::Integer => match t.as_str() {
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
            | "u128" | "usize" | "Int" | "UInt" => None,
            _ => Some("integer"),
        },
        LiteralKind::Boolean => match t.as_str() {
            "bool" | "Bool" => None,
            _ => Some("boolean"),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast_rsx::{RsxAttribute, ScriptBlock};
    use buff_lang_error::SourceId;

    fn span(start: usize, end: usize) -> Span {
        Span::new(start, end, SourceId(0))
    }

    fn make_interface(tag: &str, struct_name: &str, fields: &[(&str, &str)]) -> ComponentInterface {
        ComponentInterface {
            tag_name: tag.to_string(),
            struct_name: struct_name.to_string(),
            fields: fields
                .iter()
                .map(|(n, t)| PropField {
                    name: n.to_string(),
                    type_name: t.to_string(),
                    required: true,
                })
                .collect(),
        }
    }

    // ----- extract_interface -----

    #[test]
    fn extract_interface_returns_none_without_script() {
        let tpl = RsxTemplateFile {
            script: None,
            root: vec![],
            span: span(0, 0),
        };
        assert!(extract_interface(&tpl, "Greeting").is_none());
    }

    #[test]
    fn extract_interface_returns_none_without_props_attribute() {
        let tpl = RsxTemplateFile {
            script: Some(ScriptBlock::new("buff", "struct X { a: i32 }", span(0, 10))),
            root: vec![],
            span: span(0, 0),
        };
        assert!(extract_interface(&tpl, "Greeting").is_none());
    }

    #[test]
    fn extract_interface_returns_none_when_struct_missing() {
        let tpl = RsxTemplateFile {
            script: Some(ScriptBlock::with_props(
                "buff",
                "Props",
                "struct Other { a: i32 }",
                span(0, 10),
            )),
            root: vec![],
            span: span(0, 0),
        };
        assert!(extract_interface(&tpl, "Greeting").is_none());
    }

    #[test]
    fn extract_interface_finds_named_struct_fields() {
        let src = "struct Props { name: String, count: i32, active: bool }";
        let tpl = RsxTemplateFile {
            script: Some(ScriptBlock::with_props("buff", "Props", src, span(0, 10))),
            root: vec![],
            span: span(0, 0),
        };
        let iface = extract_interface(&tpl, "Greeting").expect("interface must extract");
        assert_eq!(iface.tag_name, "Greeting");
        assert_eq!(iface.struct_name, "Props");
        assert_eq!(iface.fields.len(), 3);
        assert_eq!(iface.fields[0].name, "name");
        assert_eq!(iface.fields[0].type_name, "String");
        assert_eq!(iface.fields[1].name, "count");
        assert_eq!(iface.fields[1].type_name, "i32");
        assert_eq!(iface.fields[2].name, "active");
        assert!(iface.fields.iter().all(|f| f.required));
    }

    // ----- literal classification -----

    #[test]
    fn classify_string_literal() {
        assert_eq!(classify_literal("\"hello\""), Some(LiteralKind::String));
        assert_eq!(classify_literal("\"\""), Some(LiteralKind::String));
    }

    #[test]
    fn classify_integer_literal() {
        assert_eq!(classify_literal("42"), Some(LiteralKind::Integer));
        assert_eq!(classify_literal("-7"), Some(LiteralKind::Integer));
        assert_eq!(classify_literal("+0"), Some(LiteralKind::Integer));
    }

    #[test]
    fn classify_boolean_literal() {
        assert_eq!(classify_literal("true"), Some(LiteralKind::Boolean));
        assert_eq!(classify_literal("false"), Some(LiteralKind::Boolean));
    }

    #[test]
    fn classify_non_literal_returns_none() {
        assert_eq!(classify_literal("foo"), None);
        assert_eq!(classify_literal("a + b"), None);
        assert_eq!(classify_literal(""), None);
        assert_eq!(classify_literal("name.to_string()"), None);
    }

    // ----- type_mismatch -----

    #[test]
    fn mismatch_string_to_int() {
        assert_eq!(type_mismatch(LiteralKind::String, "i32"), Some("string"));
    }

    #[test]
    fn mismatch_int_to_string() {
        assert_eq!(
            type_mismatch(LiteralKind::Integer, "String"),
            Some("integer")
        );
    }

    #[test]
    fn no_mismatch_string_to_string() {
        assert_eq!(type_mismatch(LiteralKind::String, "String"), None);
    }

    #[test]
    fn no_mismatch_int_to_alias() {
        // `Int` is a Buff alias for `i32` — both accept integer literals.
        assert_eq!(type_mismatch(LiteralKind::Integer, "Int"), None);
        assert_eq!(type_mismatch(LiteralKind::Integer, "usize"), None);
    }

    // ----- check_props end-to-end -----

    fn build_template(root_nodes: Vec<RsxNode>) -> RsxTemplateFile {
        RsxTemplateFile {
            script: None,
            root: root_nodes,
            span: span(0, 0),
        }
    }

    fn make_component_invocation(tag: &str, attrs: Vec<(RsxAttributeKind, Span)>) -> RsxNode {
        RsxNode::Element(buff_lang_ast_rsx::RsxElement {
            tag: tag.to_string(),
            is_component: true,
            attributes: attrs
                .into_iter()
                .map(|(kind, sp)| RsxAttribute { kind, span: sp })
                .collect(),
            children: vec![],
            self_closing: true,
            span: span(10, 30),
        })
    }

    #[test]
    fn check_props_no_diagnostics_when_interface_unknown() {
        // Component invocation against a registry that doesn't know
        // about it → skipped (backward-compat).
        let tpl = build_template(vec![make_component_invocation(
            "Unknown",
            vec![(
                RsxAttributeKind::NamedProp {
                    name: "x".to_string(),
                    value: "1".to_string(),
                    value_span: span(15, 16),
                },
                span(10, 30),
            )],
        )]);
        let reg = PropInterfaceRegistry::new();
        assert!(check_props(&tpl, &reg).is_empty());
    }

    #[test]
    fn check_props_missing_required_reports_one_per_field() {
        let mut reg = PropInterfaceRegistry::new();
        reg.register(make_interface(
            "Greeting",
            "Props",
            &[("name", "String"), ("count", "i32")],
        ));
        // Caller provides NEITHER prop.
        let tpl = build_template(vec![make_component_invocation("Greeting", vec![])]);
        let diags = check_props(&tpl, &reg);
        assert_eq!(diags.len(), 2);
        assert!(diags
            .iter()
            .all(|d| d.kind == PropCheckKind::MissingRequired));
    }

    #[test]
    fn check_props_unknown_prop_reports_with_offending_attr_span() {
        let mut reg = PropInterfaceRegistry::new();
        reg.register(make_interface("Greeting", "Props", &[("name", "String")]));
        // Caller passes the right prop PLUS an unknown one.
        let unknown_span = span(40, 50);
        let tpl = build_template(vec![make_component_invocation(
            "Greeting",
            vec![
                (
                    RsxAttributeKind::NamedProp {
                        name: "name".to_string(),
                        value: "\"Alice\"".to_string(),
                        value_span: span(20, 28),
                    },
                    span(10, 30),
                ),
                (
                    RsxAttributeKind::NamedProp {
                        name: "age".to_string(),
                        value: "30".to_string(),
                        value_span: unknown_span,
                    },
                    span(10, 30),
                ),
            ],
        )]);
        let diags = check_props(&tpl, &reg);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, PropCheckKind::UnknownProp);
        assert_eq!(diags[0].span, unknown_span);
        assert!(diags[0].message.contains("age"));
    }

    #[test]
    fn check_props_clean_when_all_required_provided() {
        let mut reg = PropInterfaceRegistry::new();
        reg.register(make_interface(
            "Greeting",
            "Props",
            &[("name", "String"), ("count", "i32")],
        ));
        let tpl = build_template(vec![make_component_invocation(
            "Greeting",
            vec![
                (
                    RsxAttributeKind::NamedProp {
                        name: "name".to_string(),
                        value: "\"Alice\"".to_string(),
                        value_span: span(20, 28),
                    },
                    span(10, 30),
                ),
                (
                    RsxAttributeKind::NamedProp {
                        name: "count".to_string(),
                        value: "5".to_string(),
                        value_span: span(35, 36),
                    },
                    span(10, 30),
                ),
            ],
        )]);
        let diags = check_props(&tpl, &reg);
        assert!(diags.is_empty(), "expected no diagnostics: {diags:?}");
    }

    #[test]
    fn check_props_stretch_type_mismatch_integer_to_string() {
        let mut reg = PropInterfaceRegistry::new();
        reg.register(make_interface("Greeting", "Props", &[("name", "String")]));
        let tpl = build_template(vec![make_component_invocation(
            "Greeting",
            vec![(
                RsxAttributeKind::NamedProp {
                    name: "name".to_string(),
                    value: "42".to_string(),
                    value_span: span(20, 22),
                },
                span(10, 30),
            )],
        )]);
        let diags = check_props(&tpl, &reg);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, PropCheckKind::TypeMismatch);
        // value `42` is an integer literal; declared type is String →
        // mismatch description says "received integer literal".
        assert!(
            diags[0].message.contains("integer"),
            "expected `integer` in message: {}",
            diags[0].message
        );
    }

    #[test]
    fn check_props_stretch_type_mismatch_string_to_int() {
        let mut reg = PropInterfaceRegistry::new();
        reg.register(make_interface("Greeting", "Props", &[("count", "i32")]));
        let tpl = build_template(vec![make_component_invocation(
            "Greeting",
            vec![(
                RsxAttributeKind::NamedProp {
                    name: "count".to_string(),
                    value: "\"five\"".to_string(),
                    value_span: span(20, 26),
                },
                span(10, 30),
            )],
        )]);
        let diags = check_props(&tpl, &reg);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, PropCheckKind::TypeMismatch);
        assert!(
            diags[0].message.contains("string"),
            "expected `string` in message: {}",
            diags[0].message
        );
    }

    #[test]
    fn check_props_spread_skips_all_checks() {
        let mut reg = PropInterfaceRegistry::new();
        reg.register(make_interface("Greeting", "Props", &[("name", "String")]));
        let tpl = build_template(vec![make_component_invocation(
            "Greeting",
            vec![(
                RsxAttributeKind::Spread {
                    ident: "rest".to_string(),
                },
                span(10, 30),
            )],
        )]);
        let diags = check_props(&tpl, &reg);
        assert!(
            diags.is_empty(),
            "spread props must bypass pre-checker: {diags:?}"
        );
    }

    #[test]
    fn check_props_walks_nested_elements_and_if_branches() {
        let mut reg = PropInterfaceRegistry::new();
        reg.register(make_interface("Greeting", "Props", &[("name", "String")]));
        // Outer element wraps a fragment that wraps an if-branch
        // containing a bad Greeting invocation.
        let bad = make_component_invocation("Greeting", vec![]);
        let tpl = build_template(vec![RsxNode::Fragment(buff_lang_ast_rsx::RsxFragment {
            children: vec![RsxNode::If(buff_lang_ast_rsx::RsxIf {
                branches: vec![buff_lang_ast_rsx::RsxIfBranch {
                    cond: "true".to_string(),
                    cond_span: span(0, 4),
                    body: vec![bad],
                }],
                else_branch: None,
                span: span(0, 50),
            })],
            span: span(0, 50),
        })]);
        let diags = check_props(&tpl, &reg);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, PropCheckKind::MissingRequired);
    }

    #[test]
    fn check_props_event_handler_not_treated_as_prop() {
        // `on:click={...}` is NOT a prop — it must not appear in the
        // provided-props map (so it must not trigger UnknownProp).
        let mut reg = PropInterfaceRegistry::new();
        reg.register(make_interface("Button", "Props", &[("label", "String")]));
        let tpl = build_template(vec![make_component_invocation(
            "Button",
            vec![
                (
                    RsxAttributeKind::NamedProp {
                        name: "label".to_string(),
                        value: "\"Go\"".to_string(),
                        value_span: span(20, 24),
                    },
                    span(10, 30),
                ),
                (
                    RsxAttributeKind::Event {
                        event: "click".to_string(),
                        modifiers: vec![],
                        handler_expr: "handle".to_string(),
                        handler_span: span(40, 50),
                    },
                    span(35, 50),
                ),
            ],
        )]);
        let diags = check_props(&tpl, &reg);
        assert!(diags.is_empty(), "on:click is not a prop: {diags:?}");
    }

    // ----- registry -----

    #[test]
    fn registry_register_and_lookup() {
        let mut reg = PropInterfaceRegistry::new();
        assert!(reg.is_empty());
        reg.register(make_interface("A", "Props", &[("x", "i32")]));
        reg.register(make_interface("B", "Props", &[("y", "String")]));
        assert_eq!(reg.len(), 2);
        assert!(reg.lookup("A").is_some());
        assert!(reg.lookup("B").is_some());
        assert!(reg.lookup("C").is_none());
    }

    #[test]
    fn registry_register_replaces_on_duplicate_tag() {
        let mut reg = PropInterfaceRegistry::new();
        reg.register(make_interface("A", "Props", &[("x", "i32")]));
        reg.register(make_interface("A", "PropsV2", &[("z", "bool")]));
        assert_eq!(reg.len(), 1);
        let iface = reg.lookup("A").expect("present");
        assert_eq!(iface.struct_name, "PropsV2");
    }
}
