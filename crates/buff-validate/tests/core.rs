//! Integration tests for the `buff-validate` crate.
//!
//! Covers all 8 public functions per the T29 spec:
//! - Constructors: `Validator::new`
//! - Builders: `with_email` / `with_url` / `with_length` / `with_range` / `with_regex`
//! - Action: `validate` / `to_json_schema`
//! - Helper: `json_escape` (from schema module)
//!
//! Plus the error variants in `ValidationError` / `ValidationErrors`.
//!
//! All rule-failure paths verified: missing field, bad email, bad url,
//! out-of-range length, out-of-range number, regex mismatch, bad regex
//! pattern, invalid rule config. Per the T29 acceptance criteria.

use buff_validate::{json_escape, ValidationError, ValidationErrors, Validator};
use std::collections::HashMap;

fn input(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn empty_validator_passes_empty_input() {
    let v = Validator::new();
    let map: HashMap<String, String> = HashMap::new();
    assert!(v.validate(&map).is_ok());
    assert_eq!(v.rule_count(), 0);
}

#[test]
fn email_rule_passes_valid_emails() {
    let v = Validator::new().with_email("email");
    let map = input(&[("email", "alice@example.com")]);
    assert!(v.validate(&map).is_ok());
}

#[test]
fn email_rule_rejects_invalid_emails() {
    let v = Validator::new().with_email("email");
    let map = input(&[("email", "not-an-email")]);
    let errs = v.validate(&map).unwrap_err();
    assert_eq!(errs.len(), 1);
    assert!(matches!(
        errs[0],
        ValidationError::InvalidEmail { ref field, .. } if field == "email"
    ));
}

#[test]
fn url_rule_validates_homepages() {
    let v = Validator::new().with_url("homepage");
    assert!(v.validate(&input(&[("homepage", "https://example.com")])).is_ok());
    let errs = v
        .validate(&input(&[("homepage", "not-a-url")]))
        .unwrap_err();
    assert!(matches!(errs[0], ValidationError::InvalidUrl { .. }));
}

#[test]
fn length_rule_enforces_min_max() {
    let v = Validator::new()
        .with_length("name", 2, 5)
        .expect("valid length rule");
    assert!(v.validate(&input(&[("name", "abcd")])).is_ok());
    let errs = v.validate(&input(&[("name", "a")])).unwrap_err();
    assert!(matches!(
        errs[0],
        ValidationError::InvalidLength { min: 2, max: 5, actual: 1, .. }
    ));
    let errs = v.validate(&input(&[("name", "abcdef")])).unwrap_err();
    assert!(matches!(
        errs[0],
        ValidationError::InvalidLength { actual: 6, .. }
    ));
}

#[test]
fn length_rule_rejects_min_greater_than_max() {
    let err = Validator::new()
        .with_length("name", 10, 5)
        .unwrap_err();
    assert!(matches!(err, ValidationError::InvalidRuleConfig { .. }));
}

#[test]
fn range_rule_enforces_numeric_bounds() {
    let v = Validator::new()
        .with_range("age", 0, 150)
        .expect("valid range rule");
    assert!(v.validate(&input(&[("age", "30")])).is_ok());
    let errs = v.validate(&input(&[("age", "200")])).unwrap_err();
    assert!(matches!(
        errs[0],
        ValidationError::InvalidRange { min: 0, max: 150, actual: 200, .. }
    ));
}

#[test]
fn range_rule_surfaces_uncoercible_value() {
    let v = Validator::new()
        .with_range("age", 0, 150)
        .expect("valid range rule");
    let errs = v.validate(&input(&[("age", "thirty")])).unwrap_err();
    assert!(matches!(
        errs[0],
        ValidationError::UncoercibleValue { ref field, .. } if field == "age"
    ));
}

#[test]
fn regex_rule_matches_patterns() {
    let v = Validator::new()
        .with_regex("zip", "^[0-9]{5}$")
        .expect("valid regex");
    assert!(v.validate(&input(&[("zip", "94105")])).is_ok());
    let errs = v.validate(&input(&[("zip", "9410")])).unwrap_err();
    assert!(matches!(
        errs[0],
        ValidationError::InvalidRegex { ref pattern, .. } if pattern == "^[0-9]{5}$"
    ));
}

#[test]
fn regex_rule_surfaces_bad_pattern_at_registration() {
    let err = Validator::new()
        .with_regex("phone", "(unbalanced")
        .unwrap_err();
    assert!(matches!(err, ValidationError::BadRegex { ref pattern, .. } if pattern == "(unbalanced"));
}

#[test]
fn missing_field_surfaces_in_aggregate() {
    let v = Validator::new()
        .with_email("email")
        .with_length("name", 1, 80)
        .with_range("age", 0, 150)
        .expect("valid rules");
    let errs = v.validate(&input(&[])).unwrap_err();
    assert_eq!(errs.len(), 3);
    let mut fields: Vec<String> = errs
        .iter()
        .filter_map(|e| match e {
            ValidationError::MissingField { field } => Some(field.clone()),
            _ => None,
        })
        .collect();
    fields.sort();
    assert_eq!(fields, vec!["age".to_string(), "email".to_string(), "name".to_string()]);
}

#[test]
fn multiple_rules_aggregate_errors_for_same_field() {
    let v = Validator::new()
        .with_email("email")
        .with_length("email", 5, 10)
        .expect("valid rules");
    let errs = v.validate(&input(&[("email", "x")])).unwrap_err();
    assert_eq!(errs.len(), 2);
}

#[test]
fn json_schema_has_expected_shape_for_each_rule_kind() {
    let v = Validator::new()
        .with_email("email")
        .with_url("homepage")
        .with_length("name", 1, 80)
        .with_range("age", 0, 150)
        .with_regex("zip", "^[0-9]{5}$")
        .expect("valid rules");
    let schema_str = v.to_json_schema();
    let schema: serde_json::Value = serde_json::from_str(&schema_str).expect("valid JSON");
    assert_eq!(schema["$schema"], "https://json-schema.org/draft/2020-12/schema");
    assert_eq!(schema["type"], "object");
    let required = schema["required"].as_array().expect("required is array");
    assert_eq!(required.len(), 5);
    assert_eq!(schema["properties"]["email"]["format"], "email");
    assert_eq!(schema["properties"]["homepage"]["format"], "uri");
    assert_eq!(schema["properties"]["name"]["minLength"], 1);
    assert_eq!(schema["properties"]["name"]["maxLength"], 80);
    assert_eq!(schema["properties"]["age"]["type"], "integer");
    assert_eq!(schema["properties"]["age"]["minimum"], 0);
    assert_eq!(schema["properties"]["age"]["maximum"], 150);
    assert_eq!(schema["properties"]["zip"]["pattern"], "^[0-9]{5}$");
}

#[test]
fn json_schema_merges_multiple_rules_on_same_field() {
    let v = Validator::new()
        .with_email("email")
        .with_length("email", 5, 80)
        .expect("valid rules");
    let schema: serde_json::Value =
        serde_json::from_str(&v.to_json_schema()).expect("valid JSON");
    let email_schema = &schema["properties"]["email"];
    assert_eq!(email_schema["format"], "email");
    assert_eq!(email_schema["minLength"], 5);
    assert_eq!(email_schema["maxLength"], 80);
}

#[test]
fn json_escape_handles_special_chars() {
    assert_eq!(json_escape("hello"), "hello");
    assert_eq!(json_escape("a\"b"), "a\\\"b");
    assert_eq!(json_escape("a\\b"), "a\\\\b");
    assert_eq!(json_escape("a\nb"), "a\\nb");
    assert_eq!(json_escape("\t\r\n"), "\\t\\r\\n");
}

#[test]
fn validation_errors_display_lists_all_failures() {
    let mut errs = ValidationErrors::new();
    errs.push(ValidationError::InvalidEmail {
        field: "email".to_string(),
        value: "x".to_string(),
    });
    errs.push(ValidationError::InvalidRange {
        field: "age".to_string(),
        min: 0,
        max: 150,
        actual: 200,
    });
    let s = format!("{errs}");
    assert!(s.contains("2 validation error(s)"));
    assert!(s.contains("1. field `email`"));
    assert!(s.contains("2. field `age`"));
}

#[test]
fn validation_errors_empty_displays_zero() {
    let errs = ValidationErrors::new();
    assert!(errs.is_empty());
    assert_eq!(format!("{errs}"), "(no validation errors)");
}

#[test]
fn validator_display_shows_rule_count() {
    let v = Validator::new()
        .with_email("email")
        .with_url("homepage");
    assert_eq!(format!("{v}"), "Validator(2 rule(s))");
}

#[test]
fn validator_eq_compares_rule_set() {
    let a = Validator::new().with_email("email").with_url("homepage");
    let b = Validator::new().with_email("email").with_url("homepage");
    let c = Validator::new().with_email("email").with_url("website");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// ---- Insta snapshots --------------------------------------------------------

#[test]
fn snapshot_validation_error_display() {
    let cases = [
        format!(
            "{}",
            ValidationError::InvalidEmail {
                field: "email".to_string(),
                value: "x".to_string(),
            }
        ),
        format!(
            "{}",
            ValidationError::InvalidRange {
                field: "age".to_string(),
                min: 0,
                max: 150,
                actual: 200,
            }
        ),
        format!(
            "{}",
            ValidationError::BadRegex {
                pattern: "(unbalanced".to_string(),
                reason: "unclosed group".to_string(),
            }
        ),
    ];
    insta::assert_snapshot!("validation_error_display", cases.join("\n"));
}

#[test]
fn snapshot_json_schema_for_signup_form() {
    let v = Validator::new()
        .with_email("email")
        .with_length("name", 1, 80)
        .with_range("age", 0, 150)
        .with_regex("zip", "^[0-9]{5}$")
        .expect("valid rules");
    let schema: serde_json::Value =
        serde_json::from_str(&v.to_json_schema()).expect("valid JSON");
    insta::assert_snapshot!(
        "json_schema_signup_form",
        serde_json::to_string_pretty(&schema).unwrap_or_default()
    );
}

#[test]
fn snapshot_validator_debug() {
    let v = Validator::new()
        .with_email("email")
        .with_url("homepage");
    insta::assert_snapshot!("validator_debug", format!("{v}"));
}
