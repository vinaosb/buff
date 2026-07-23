// T29 example: JSON Schema export round-trip.
//
// Demonstrates the `to_json_schema()` method serializing a validator's
// rule set into a JSON Schema (Draft 2020-12) string. The example
// parses the output back through `serde_json` to verify it's valid
// JSON and shows how a downstream tool (form generator, docs site,
// mock-data generator) would consume the schema.

use buff_validate::Validator;

fn main() {
    let validator = Validator::new()
        .with_email("email")
        .with_url("homepage")
        .with_length("name", 1, 80)
        .with_range("age", 0, 150)
        .with_regex("zip", "^[0-9]{5}$")
        .expect("valid rules");

    let schema_str = validator.to_json_schema();
    println!("raw schema string: {schema_str}");

    let schema: serde_json::Value =
        serde_json::from_str(&schema_str).expect("schema is valid JSON");
    println!("pretty-printed:");
    println!(
        "{}",
        serde_json::to_string_pretty(&schema).unwrap_or_default()
    );

    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    println!("{required} required fields in schema");
}
