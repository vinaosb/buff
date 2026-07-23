//! `buff-validate` — declarative schema validation for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`validator`](https://crates.io/crates/validator)
//! crate's free-standing trait validators (email / url / length / range /
//! regex) and exporting JSON Schema via [`serde_json`]. Inspired by
//! pydantic / Zod / Joi.
//!
//! # Pipeline
//!
//! ```text
//!   Validator.new() ─────────────────────────────────┐
//!        │                                           │
//!        ▼                                           │
//!   .with_email("email")                             │
//!   .with_url("homepage")                            │     to_json_schema()
//!   .with_length("name", min: 1, max: 80)            │     -> JSON String
//!   .with_range("age",   min: 0, max: 150)           │
//!   .with_regex("zip",   pattern: "^[0-9]{5}$")?     │
//!        │                                           │
//!        ▼                                           │
//!   validator.validate(&{("email","a@b.com"), ...})  │
//!        │                                           │
//!        ▼                                           │
//!   Result<(), ValidationErrors>                     │
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Validator`, `ValidationError`, `ValidationErrors`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `validate` borrows `&HashMap<String, String>`; never retains. `to_json_schema` returns owned `String`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, ValidationError>` or aggregates via `ValidationErrors`. |
//! | R4 — Thread safety | `Validator` is `Send + Sync` (rules are `Vec<Rule>` where `Rule` owns `String` + `regex::Regex` — both `Send + Sync`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `Validator` owns every rule. |
//! | R6 — Panic boundary | `validate` / `to_json_schema` / `with_regex` wrap their bodies in `catch_unwind`. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. All fallible operations surface via `Result`.

pub mod error;
pub mod schema;

pub use error::{ValidationError, ValidationErrors};
pub use schema::json_escape;

use std::collections::HashMap;

use regex::Regex;
use std::panic::{catch_unwind, AssertUnwindSafe};
use validator::{ValidateEmail, ValidateLength, ValidateRange, ValidateUrl};

#[derive(Debug, Clone)]
enum Rule {
    Email {
        field: String,
    },
    Url {
        field: String,
    },
    Length {
        field: String,
        min: u64,
        max: u64,
    },
    Range {
        field: String,
        min: i64,
        max: i64,
    },
    Regex {
        field: String,
        pattern: String,
        compiled: Regex,
    },
}

impl Rule {
    fn field(&self) -> &str {
        match self {
            Rule::Email { field }
            | Rule::Url { field }
            | Rule::Length { field, .. }
            | Rule::Range { field, .. }
            | Rule::Regex { field, .. } => field,
        }
    }

    fn apply(&self, input: &HashMap<String, String>, errors: &mut ValidationErrors) {
        let field = self.field();
        let value = match input.get(field) {
            Some(v) => v.as_str(),
            None => {
                errors.push(ValidationError::MissingField {
                    field: field.to_string(),
                });
                return;
            }
        };
        match self {
            Rule::Email { .. } => {
                if !value.validate_email() {
                    errors.push(ValidationError::InvalidEmail {
                        field: field.to_string(),
                        value: value.to_string(),
                    });
                }
            }
            Rule::Url { .. } => {
                if !value.validate_url() {
                    errors.push(ValidationError::InvalidUrl {
                        field: field.to_string(),
                        value: value.to_string(),
                    });
                }
            }
            Rule::Length { min, max, .. } => {
                let actual = value.chars().count() as u64;
                let in_range = value.validate_length(Some(*min), Some(*max), None);
                if !in_range {
                    errors.push(ValidationError::InvalidLength {
                        field: field.to_string(),
                        min: *min,
                        max: *max,
                        actual,
                    });
                }
            }
            Rule::Range { min, max, .. } => {
                let parsed: i64 = match value.parse() {
                    Ok(n) => n,
                    Err(_) => {
                        errors.push(ValidationError::UncoercibleValue {
                            field: field.to_string(),
                            value: value.to_string(),
                            reason: "expected a signed integer".to_string(),
                        });
                        return;
                    }
                };
                let in_range = parsed.validate_range(Some(*min), Some(*max), None, None);
                if !in_range {
                    errors.push(ValidationError::InvalidRange {
                        field: field.to_string(),
                        min: *min,
                        max: *max,
                        actual: parsed,
                    });
                }
            }
            Rule::Regex {
                pattern, compiled, ..
            } => {
                if !compiled.is_match(value) {
                    errors.push(ValidationError::InvalidRegex {
                        field: field.to_string(),
                        pattern: pattern.clone(),
                        value: value.to_string(),
                    });
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Validator {
    rules: Vec<Rule>,
}

impl Validator {
    pub fn new() -> Self {
        Validator { rules: Vec::new() }
    }

    pub fn with_email(mut self, field: impl Into<String>) -> Self {
        self.rules.push(Rule::Email {
            field: field.into(),
        });
        self
    }

    pub fn with_url(mut self, field: impl Into<String>) -> Self {
        self.rules.push(Rule::Url {
            field: field.into(),
        });
        self
    }

    pub fn with_length(
        mut self,
        field: impl Into<String>,
        min: u64,
        max: u64,
    ) -> Result<Self, ValidationError> {
        let field = field.into();
        if min > max {
            return Err(ValidationError::InvalidRuleConfig {
                field,
                reason: format!("min ({min}) is greater than max ({max})"),
            });
        }
        self.rules.push(Rule::Length { field, min, max });
        Ok(self)
    }

    pub fn with_range(
        mut self,
        field: impl Into<String>,
        min: i64,
        max: i64,
    ) -> Result<Self, ValidationError> {
        let field = field.into();
        if min > max {
            return Err(ValidationError::InvalidRuleConfig {
                field,
                reason: format!("min ({min}) is greater than max ({max})"),
            });
        }
        self.rules.push(Rule::Range { field, min, max });
        Ok(self)
    }

    pub fn with_regex(
        mut self,
        field: impl Into<String>,
        pattern: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        let field = field.into();
        let pattern = pattern.into();
        let compiled = catch_unwind(AssertUnwindSafe(|| Regex::new(&pattern)))
            .map_err(|_| ValidationError::Panic)?
            .map_err(|e| ValidationError::BadRegex {
                pattern: pattern.clone(),
                reason: e.to_string(),
            })?;
        self.rules.push(Rule::Regex {
            field,
            pattern,
            compiled,
        });
        Ok(self)
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn validate(&self, input: &HashMap<String, String>) -> Result<(), ValidationErrors> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut errors = ValidationErrors::new();
            for rule in &self.rules {
                rule.apply(input, &mut errors);
            }
            errors
        }));
        match result {
            Ok(errors) if errors.is_empty() => Ok(()),
            Ok(errors) => Err(errors),
            Err(_) => {
                let mut errors = ValidationErrors::new();
                errors.push(ValidationError::Panic);
                Err(errors)
            }
        }
    }

    pub fn to_json_schema(&self) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| schema::serialize_schema(&self.rules)));
        match result {
            Ok(s) => s,
            Err(_) => serde_json::json!({
                "type": "object",
                "error": "internal error: schema serialization panicked",
            })
            .to_string(),
        }
    }
}

impl PartialEq for Validator {
    fn eq(&self, other: &Self) -> bool {
        if self.rules.len() != other.rules.len() {
            return false;
        }
        for (a, b) in self.rules.iter().zip(other.rules.iter()) {
            match (a, b) {
                (Rule::Email { field: fa }, Rule::Email { field: fb }) if fa == fb => {}
                (Rule::Url { field: fa }, Rule::Url { field: fb }) if fa == fb => {}
                (
                    Rule::Length {
                        field: fa,
                        min: mna,
                        max: mxa,
                    },
                    Rule::Length {
                        field: fb,
                        min: mnb,
                        max: mxb,
                    },
                ) if fa == fb && mna == mnb && mxa == mxb => {}
                (
                    Rule::Range {
                        field: fa,
                        min: mna,
                        max: mxa,
                    },
                    Rule::Range {
                        field: fb,
                        min: mnb,
                        max: mxb,
                    },
                ) if fa == fb && mna == mnb && mxa == mxb => {}
                (
                    Rule::Regex {
                        field: fa,
                        pattern: pa,
                        ..
                    },
                    Rule::Regex {
                        field: fb,
                        pattern: pb,
                        ..
                    },
                ) if fa == fb && pa == pb => {}
                _ => return false,
            }
        }
        true
    }
}

impl Eq for Validator {}

impl std::fmt::Display for Validator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Validator({} rule(s))", self.rules.len())
    }
}
