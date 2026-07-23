// T29 example: regex rule for structured codes (zip code + slug).
//
// Demonstrates the `with_regex` rule kind. The validator enforces
// US 5-digit zip codes and URL-safe slug patterns. A bad regex
// pattern (unbalanced parens) is shown surfacing at rule-registration
// time (NOT deferred until validate), per the T29 panic-free +
// fail-fast contract.

use buff_validate::{ValidationError, Validator};
use std::collections::HashMap;

fn main() -> Result<(), ValidationError> {
    let validator = Validator::new()
        .with_regex("zip", "^[0-9]{5}$")?
        .with_regex("slug", "^[a-z0-9-]+$")?;

    let mut good: HashMap<String, String> = HashMap::new();
    good.insert("zip".to_string(), "94105".to_string());
    good.insert("slug".to_string(), "hello-world".to_string());
    validator
        .validate(&good)
        .map_err(|e| e.into_iter().next().unwrap_or(ValidationError::Panic))?;
    println!("regex-conformant input passed");

    let mut bad: HashMap<String, String> = HashMap::new();
    bad.insert("zip".to_string(), "9410".to_string());
    bad.insert("slug".to_string(), "Hello World!".to_string());
    let errs = validator.validate(&bad).unwrap_err();
    println!("non-conformant input failed {errs}");

    let bad_pattern = Validator::new().with_regex("phone", "(unbalanced");
    match bad_pattern {
        Ok(_) => println!("unexpected: bad pattern compiled"),
        Err(e) => println!("caught bad pattern at registration: {e}"),
    }
    Ok(())
}
