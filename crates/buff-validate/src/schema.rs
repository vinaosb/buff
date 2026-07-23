//! JSON Schema export for [`crate::Validator`].
//!
//! Serializes a validator's registered rules into a JSON Schema
//! (Draft 2020-12) object describing the validated shape. Each rule
//! becomes a property constraint on the top-level `object` schema:
//!
//! - `email` rule -> `{ "format": "email" }`
//! - `url`   rule -> `{ "format": "uri" }`
//! - `length(min, max)` rule -> `{ "minLength": min, "maxLength": max }`
//! - `range(min, max)` rule -> `{ "type": "integer", "minimum": min, "maximum": max }`
//! - `regex(pattern)` rule -> `{ "pattern": pattern }`
//!
//! Rules targeting the same field merge their constraints into a
//! single property entry (later rules extend the same object).

use serde_json::{json, Map, Value};

use crate::Rule;

pub(crate) fn serialize_schema(rules: &[Rule]) -> String {
    let mut properties: Map<String, Value> = Map::new();
    let mut required: Vec<String> = Vec::new();
    for rule in rules {
        let field = rule.field().to_string();
        if !required.contains(&field) {
            required.push(field.clone());
        }
        let entry = properties.entry(field.clone()).or_insert_with(|| json!({}));
        if !entry.is_object() {
            *entry = json!({});
        }
        let obj = match entry.as_object_mut() {
            Some(o) => o,
            None => continue,
        };
        match rule {
            Rule::Email { .. } => {
                obj.insert("type".to_string(), json!("string"));
                obj.insert("format".to_string(), json!("email"));
            }
            Rule::Url { .. } => {
                obj.insert("type".to_string(), json!("string"));
                obj.insert("format".to_string(), json!("uri"));
            }
            Rule::Length { min, max, .. } => {
                obj.insert("type".to_string(), json!("string"));
                obj.insert("minLength".to_string(), json!(min));
                obj.insert("maxLength".to_string(), json!(max));
            }
            Rule::Range { min, max, .. } => {
                obj.insert("type".to_string(), json!("integer"));
                obj.insert("minimum".to_string(), json!(min));
                obj.insert("maximum".to_string(), json!(max));
            }
            Rule::Regex { pattern, .. } => {
                obj.insert("type".to_string(), json!("string"));
                obj.insert("pattern".to_string(), json!(pattern));
            }
        }
    }
    required.sort();
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": true,
    });
    serde_json::to_string(&schema).unwrap_or_else(|_| "{}".to_string())
}

/// Escape a string for inclusion in a JSON string literal.
///
/// This is exposed as a pub helper so the codegen-lowered Buff code
/// and integration tests can build expected JSON values without
/// re-implementing the escape rules.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}
