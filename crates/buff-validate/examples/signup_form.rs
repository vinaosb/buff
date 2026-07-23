// T29 example: validate a user-signup form (email + length + range).
//
// Demonstrates the three most common rule kinds: email format, string
// length bounds, and numeric range. The validator is built up via the
// `with_*` builder methods, then applied to a HashMap representing a
// form submission. The example shows both a passing submission and a
// failing submission (multi-rule aggregate error report).

use buff_validate::{ValidationErrors, Validator};
use std::collections::HashMap;

fn main() -> Result<(), ValidationErrors> {
    let validator = Validator::new()
        .with_email("email")
        .with_length("name", 1, 80)
        .with_range("age", 0, 150)?;

    let mut good: HashMap<String, String> = HashMap::new();
    good.insert("email".to_string(), "alice@example.com".to_string());
    good.insert("name".to_string(), "Alice".to_string());
    good.insert("age".to_string(), "30".to_string());
    validator.validate(&good)?;
    println!("good submission passed validation");

    let mut bad: HashMap<String, String> = HashMap::new();
    bad.insert("email".to_string(), "not-an-email".to_string());
    bad.insert("name".to_string(), "".to_string());
    bad.insert("age".to_string(), "200".to_string());
    match validator.validate(&bad) {
        Ok(()) => println!("unexpected pass"),
        Err(errs) => println!("bad submission failed with {errs}"),
    }

    println!(
        "JSON schema for this validator:\n{}",
        validator.to_json_schema()
    );
    Ok(())
}
