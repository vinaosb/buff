//! Integration tests for the `buff-auth` Rbac module.
//!
//! Covers: exact match, wildcard resource / role / action, denial,
//! deduplication, empty-field rejection. 7 tests (counted toward the
//! T34 acceptance 15 tests).

use buff_auth::{AuthError, Rbac};

fn policy() -> Rbac {
    let mut p = Rbac::new();
    p.add("admin", "users", "delete").expect("admin delete users");
    p.add("admin", "*", "read").expect("admin read any");
    p.add("user", "posts", "read").expect("user read posts");
    p.add("*", "health", "read").expect("anon read health");
    p
}

#[test]
fn rbac_enforce_exact_match_admin_delete_users() {
    assert!(policy().enforce(&["admin".to_string()], "users", "delete"));
}

#[test]
fn rbac_enforce_wildcard_resource_admin_read_anything() {
    assert!(policy().enforce(&["admin".to_string()], "anything-at-all", "read"));
    assert!(policy().enforce(&["admin".to_string()], "posts", "read"));
}

#[test]
fn rbac_enforce_wildcard_role_anon_health() {
    assert!(policy().enforce(&[], "health", "read"));
    assert!(policy().enforce(&["nobody-special".to_string()], "health", "read"));
}

#[test]
fn rbac_enforce_denies_when_no_rule_matches() {
    let p = policy();
    assert!(!p.enforce(&["user".to_string()], "users", "delete"));
    assert!(!p.enforce(&["user".to_string()], "posts", "delete"));
    assert!(!p.enforce(&["admin".to_string()], "users", "patch"));
}

#[test]
fn rbac_add_rejects_empty_fields() {
    let mut p = Rbac::new();
    assert!(matches!(p.add("", "x", "y").unwrap_err(), AuthError::Rbac(_)));
    assert!(matches!(p.add("x", "", "y").unwrap_err(), AuthError::Rbac(_)));
    assert!(matches!(p.add("x", "y", "").unwrap_err(), AuthError::Rbac(_)));
}

#[test]
fn rbac_add_dedups_identical_rules() {
    let mut p = Rbac::new();
    p.add("admin", "users", "delete").expect("first");
    p.add("admin", "users", "delete").expect("second");
    assert_eq!(p.len(), 1);
}

#[test]
fn rbac_len_and_is_empty_track_state() {
    let p = policy();
    assert_eq!(p.len(), 4);
    assert!(!p.is_empty());
    let empty = Rbac::new();
    assert!(empty.is_empty());
}
