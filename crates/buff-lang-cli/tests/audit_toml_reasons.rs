//! W5/T31 — audit.toml is the single ignore source of truth.
//!
//! Every advisory ID in the `[advisories] ignore` list must carry a
//! non-empty comment line IMMEDIATELY ABOVE its entry, explaining why it
//! is ignored. The TOML parse drops comments, so the comment assertion
//! scans the raw text; parseability is asserted via the `toml` crate
//! (cargo-audit's own schema: `ignore: Vec<Id>` — string IDs only).

use std::path::PathBuf;

#[test]
fn audit_toml_parses_and_every_ignored_id_has_comment_above() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let audit_path = manifest.join("../../.cargo/audit.toml");
    let raw = std::fs::read_to_string(&audit_path)
        .unwrap_or_else(|e| panic!("must read {}: {e}", audit_path.display()));

    let value: toml::Value = raw
        .parse()
        .expect("audit.toml must parse as TOML (cargo-audit schema)");
    let ignore = value
        .get("advisories")
        .and_then(|a| a.get("ignore"))
        .and_then(|i| i.as_array())
        .expect("[advisories] ignore must be an array of string IDs");
    assert!(
        !ignore.is_empty(),
        "advisories.ignore must not be empty (the ignore list is load-bearing)"
    );

    let lines: Vec<&str> = raw.lines().collect();
    for entry in ignore {
        let id = entry
            .as_str()
            .expect("ignore entries must be plain string IDs (cargo-audit Vec<Id>)");
        let needle = format!("\"{id}\"");
        let line_idx = lines
            .iter()
            .position(|l| l.contains(&needle))
            .unwrap_or_else(|| {
                panic!("ignore entry {id} not found as a quoted line in audit.toml")
            });
        let above = lines.get(line_idx.saturating_sub(1)).copied().unwrap_or("");
        let comment = above.trim();
        assert!(
            comment.starts_with('#') && comment.len() > 1,
            "ignore entry {id} must have a non-empty comment line immediately above its entry; found: {comment:?}"
        );
    }
}
