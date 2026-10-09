//! `{% %}` control-flow tag translation (ITER-07).
//!
//! Buff templates document a Jinja-style control-flow surface on top of
//! handlebars:
//!
//! - `{% if cond %} ... [{% else %} ...] {% endif %}`
//! - `{% for var in collection %} ... {% endfor %}`
//!
//! handlebars itself only parses `{{#if}}` / `{{#each}}` blocks, so a raw
//! `{% ... %}` tag used to be emitted as literal text. This pass runs in
//! `Template::from_string` BEFORE registration: it rewrites the five known
//! control tags to their handlebars equivalents, copies every other byte
//! through unchanged, and rejects unknown or malformed tags with
//! `TemplateError::Parse`.
//!
//! Tag mapping:
//!
//! | Buff tag | handlebars equivalent |
//! |---|---|
//! | `{% if cond %}` | `{{#if cond}}` |
//! | `{% else %}` | `{{else}}` |
//! | `{% endif %}` | `{{/if}}` |
//! | `{% for item in list %}` | `{{#each list as \|item\|}}` |
//! | `{% endfor %}` | `{{/each}}` |
//!
//! Nesting needs no special handling — the rewrite is purely lexical, and
//! handlebars enforces block structure (an unmatched `{% endfor %}` fails
//! registration with a `Parse` error). Truthiness and loop semantics are
//! handlebars' own.

use std::borrow::Cow;

use crate::error::TemplateError;

/// Translate `{% %}` control tags in `source` to handlebars block syntax.
///
/// Returns the input unchanged (borrowed) when no `{%` appears. Returns
/// `TemplateError::Parse` for an unclosed `{%` or an unknown/malformed tag.
pub(crate) fn translate_control_flow(source: &str) -> Result<Cow<'_, str>, TemplateError> {
    if !source.contains("{%") {
        return Ok(Cow::Borrowed(source));
    }
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("{%") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let end = after_open.find("%}").ok_or_else(|| {
            TemplateError::Parse("unclosed control tag: `{%` without matching `%}`".into())
        })?;
        let inner = after_open[..end].trim();
        out.push_str(&translate_tag(inner)?);
        rest = &after_open[end + 2..];
    }
    out.push_str(rest);
    Ok(Cow::Owned(out))
}

/// Translate one trimmed tag body (`"if ok"`, `"for item in items"`, ...).
fn translate_tag(inner: &str) -> Result<String, TemplateError> {
    let (keyword, args) = match inner.split_once(char::is_whitespace) {
        Some((keyword, rest)) => (keyword, Some(rest.trim())),
        None => (inner, None),
    };
    match keyword {
        "if" => match args {
            Some(cond) if !cond.is_empty() => Ok(format!("{{{{#if {cond}}}}}")),
            _ => Err(TemplateError::Parse(format!(
                "malformed control tag `{{% {inner} %}}`: `if` requires a condition"
            ))),
        },
        "else" => no_args_tag(inner, args, "{{else}}"),
        "endif" => no_args_tag(inner, args, "{{/if}}"),
        "for" => translate_for_tag(inner, args),
        "endfor" => no_args_tag(inner, args, "{{/each}}"),
        _ => Err(TemplateError::Parse(format!(
            "unknown template tag `{{% {inner} %}}`: expected if/else/endif/for/endfor"
        ))),
    }
}

/// Accept an argument-less closing/branch tag (`else` / `endif` / `endfor`).
fn no_args_tag(
    inner: &str,
    args: Option<&str>,
    replacement: &str,
) -> Result<String, TemplateError> {
    match args {
        None => Ok(replacement.to_string()),
        Some(_) => Err(TemplateError::Parse(format!(
            "malformed control tag `{{% {inner} %}}`: `{inner}` takes no arguments"
        ))),
    }
}

/// Translate `{% for var in collection %}` to `{{#each collection as |var|}}`.
///
/// The block parameter makes `{{var}}` resolve to the current element inside
/// the loop body (handlebars' `#each` alone only exposes `{{this}}`).
fn translate_for_tag(inner: &str, args: Option<&str>) -> Result<String, TemplateError> {
    let malformed = || {
        TemplateError::Parse(format!(
            "malformed control tag `{{% {inner} %}}`: expected `for <var> in <collection>`"
        ))
    };
    let args = args.ok_or_else(malformed)?;
    let tokens: Vec<&str> = args.split_whitespace().collect();
    match tokens.as_slice() {
        [var, "in", collection] => Ok(format!("{{{{#each {collection} as |{var}|}}}}")),
        _ => Err(malformed()),
    }
}

#[cfg(test)]
mod tests {
    use super::translate_control_flow;
    use crate::error::TemplateError;

    #[test]
    fn passthrough_when_no_control_tags() {
        assert_eq!(
            translate_control_flow("Hello {{name}}!").expect("passthrough"),
            "Hello {{name}}!"
        );
    }

    #[test]
    fn translates_if_else_and_for_tags() {
        let src = "{% if ok %}yes{% else %}no{% endif %}{% for x in xs %}{{x}}{% endfor %}";
        let translated = translate_control_flow(src).expect("translate");
        assert_eq!(
            translated,
            "{{#if ok}}yes{{else}}no{{/if}}{{#each xs as |x|}}{{x}}{{/each}}"
        );
    }

    #[test]
    fn rejects_unclosed_tag() {
        let err = translate_control_flow("{% if ok").unwrap_err();
        assert!(matches!(err, TemplateError::Parse(_)));
    }

    #[test]
    fn rejects_unknown_and_malformed_tags() {
        assert!(matches!(
            translate_control_flow("{% upper name %}").unwrap_err(),
            TemplateError::Parse(_)
        ));
        assert!(matches!(
            translate_control_flow("{% if %}x{% endif %}").unwrap_err(),
            TemplateError::Parse(_)
        ));
        assert!(matches!(
            translate_control_flow("{% for xs %}x{% endfor %}").unwrap_err(),
            TemplateError::Parse(_)
        ));
    }
}
