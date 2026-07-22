//! RBAC policy — `(role, resource, action)` triples + wildcard match.

use std::collections::BTreeSet;

use crate::error::AuthError;

/// One RBAC permission entry: a role may perform `action` on `resource`.
///
/// Stored as `(role, resource, action)` owned String triples so the
/// struct is `Send + 'static` per FFI guide R4 + lifetime-free per R5.
/// Each field may be the literal `"*"` wildcard meaning "any".
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RbacRule {
    pub role: String,
    pub resource: String,
    pub action: String,
}

/// An RBAC policy table.
///
/// Owned `BTreeSet<RbacRule>` for deterministic iteration (mirrors the
/// workspace convention that all state collections are BTreeMap /
/// BTreeSet, never HashMap / HashSet — the snapshot tests rely on
/// deterministic output). Constructed via [`Rbac::new`] + repeated
/// [`Rbac::add`] calls.
#[derive(Debug, Clone, Default)]
pub struct Rbac {
    rules: BTreeSet<RbacRule>,
}

impl Rbac {
    /// Build an empty RBAC policy. Mutate via [`Rbac::add`].
    pub fn new() -> Self {
        Rbac {
            rules: BTreeSet::new(),
        }
    }

    /// Add a `(role, resource, action)` rule to the policy.
    ///
    /// Duplicate adds are silently deduplicated (BTreeSet semantics).
    /// Empty fields are rejected as malformed (returns
    /// `Err(AuthError::Rbac(_))`).
    pub fn add(&mut self, role: &str, resource: &str, action: &str) -> Result<(), AuthError> {
        if role.is_empty() || resource.is_empty() || action.is_empty() {
            return Err(AuthError::Rbac(format!(
                "rule fields must be non-empty (got ({role:?}, {resource:?}, {action:?}))"
            )));
        }
        self.rules.insert(RbacRule {
            role: role.to_string(),
            resource: resource.to_string(),
            action: action.to_string(),
        });
        Ok(())
    }

    /// Number of rules in the policy (after dedup).
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the policy has zero rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Decide whether `roles` collectively may perform `action` on
    /// `resource`.
    ///
    /// Match is exact OR wildcard (`"*"`) on each field:
    /// - a rule with `role="*"` matches any role set;
    /// - a rule with `resource="*"` matches any resource;
    /// - a rule with `action="*"` matches any action.
    ///
    /// Returns `true` iff at least one rule matches at least one of the
    /// supplied `roles`. NEVER panics — empty `roles` set returns
    /// `false` (no rule can match).
    pub fn enforce(&self, roles: &[String], resource: &str, action: &str) -> bool {
        for rule in &self.rules {
            if !field_matches(&rule.resource, resource) {
                continue;
            }
            if !field_matches(&rule.action, action) {
                continue;
            }
            if rule.role == "*" {
                return true;
            }
            for role in roles {
                if rule.role == *role {
                    return true;
                }
            }
        }
        false
    }

    /// Snapshot of all rules, sorted deterministically (BTreeSet order).
    /// Useful for tests + `Rbac` introspection in user code.
    pub fn rules(&self) -> &BTreeSet<RbacRule> {
        &self.rules
    }
}

fn field_matches(rule_value: &str, requested: &str) -> bool {
    rule_value == "*" || rule_value == requested
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    fn make_policy() -> Rbac {
        let mut p = Rbac::new();
        p.add("admin", "users", "delete").expect("admin rule");
        p.add("admin", "*", "read").expect("admin read any");
        p.add("user", "posts", "read").expect("user read posts");
        p.add("*", "health", "read").expect("anon health");
        p
    }

    #[test]
    fn enforce_exact_match_admin() {
        let p = make_policy();
        assert!(p.enforce(&["admin".to_string()], "users", "delete"));
    }

    #[test]
    fn enforce_wildcard_resource_admin() {
        let p = make_policy();
        assert!(p.enforce(&["admin".to_string()], "anything", "read"));
        assert!(p.enforce(&["admin".to_string()], "posts", "read"));
    }

    #[test]
    fn enforce_wildcard_role_for_health() {
        let p = make_policy();
        assert!(p.enforce(&[], "health", "read"));
        assert!(p.enforce(&["nobody".to_string()], "health", "read"));
    }

    #[test]
    fn enforce_rejects_when_no_rule_matches() {
        let p = make_policy();
        assert!(!p.enforce(&["user".to_string()], "users", "delete"));
        assert!(!p.enforce(&["user".to_string()], "posts", "delete"));
    }

    #[test]
    fn add_rejects_empty_fields() {
        let mut p = Rbac::new();
        assert!(p.add("", "x", "y").is_err());
        assert!(p.add("x", "", "y").is_err());
        assert!(p.add("x", "y", "").is_err());
    }

    #[test]
    fn add_dedups_identical_rules() {
        let mut p = Rbac::new();
        p.add("admin", "users", "delete").expect("first");
        p.add("admin", "users", "delete").expect("second");
        assert_eq!(p.len(), 1);
    }
}
