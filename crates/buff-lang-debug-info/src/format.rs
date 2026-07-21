//! `.buffmap` JSON format — forward + reverse lookup tables.
//!
//! The sidecar file is a single JSON object carrying everything an offline
//! consumer needs to translate Rust locations ↔ Buff locations:
//!
//! - The originating `.buff` file path + the generated `.rs` file path.
//! - A `function_mappings` list (Buff function name → Buff span + Rust
//!   line range) — used by `buff backtrace` and the DAP server to resolve
//!   which Buff function a Rust frame is inside.
//! - A `line_mappings` list (Rust line → Buff `(line, col, span)`), sorted
//!   by Rust line. Used by the panic hook for per-frame remapping.
//!
//! Both lookup directions are derived from the same data — the format is
//! just the JSON projection of [`crate::SourceMap`]. Reverse lookup (Buff →
//! Rust) is computed on demand by walking the sorted line list.
//!
//! # Versioning
//!
//! The format carries a [`MAP_FORMAT_VERSION`] so consumers can detect
//! future schema changes. The version is bumped only on breaking schema
//! changes (renamed keys, removed fields, restructured objects) — additive
//! changes stay forward-compatible.

use std::path::Path;

use serde::{Deserialize, Serialize};

use buff_lang_error::{SourceId, Span};

use crate::{BuffLocation, FunctionAnchor, SourceMap};

/// The `.buffmap` JSON schema version.
///
/// Bump only on breaking schema changes. Additive changes stay
/// forward-compatible — consumers must ignore unknown keys (default
/// `serde` behaviour when no `#[serde(deny_unknown_fields)]` is set).
pub const MAP_FORMAT_VERSION: u32 = 1;

/// Top-level JSON object written to `<binary>.buffmap`.
///
/// Both `function_mappings` + `line_mappings` lists are sorted at
/// serialize time so the same [`SourceMap`] always produces byte-identical
/// JSON (project hard rule — see root AGENTS.md "Determinism").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuffMapFile {
    pub version: u32,
    pub buff_file: Option<String>,
    pub rust_file: Option<String>,
    pub source_id: u32,
    pub function_mappings: Vec<FunctionMapping>,
    pub line_mappings: Vec<LineMapping>,
}

/// JSON projection of a [`FunctionAnchor`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionMapping {
    pub buff_name: String,
    pub buff_span_start: usize,
    pub buff_span_end: usize,
    pub buff_source_id: u32,
    pub buff_line: usize,
    pub buff_col: usize,
    pub rust_start_line: usize,
    pub rust_end_line: usize,
}

/// JSON projection of one entry of [`SourceMap::rust_to_buff`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineMapping {
    pub rust_line: usize,
    pub buff_line: usize,
    pub buff_col: usize,
    pub buff_span_start: usize,
    pub buff_span_end: usize,
    pub buff_source_id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buff_name: Option<String>,
}

/// Serialise a [`SourceMap`] into a pretty-printed `.buffmap` JSON string.
///
/// Both `function_mappings` and `line_mappings` are sorted at serialize
/// time so JSON output is byte-identical across runs.
pub fn serialize_to_string(map: &SourceMap) -> Result<String, serde_json::Error> {
    let file = map.to_buff_map_file();
    serde_json::to_string_pretty(&file)
}

/// Deserialise a `.buffmap` JSON string back into a [`SourceMap`].
///
/// Unknown keys are silently ignored (forwards-compat with future
/// schema versions).
pub fn deserialize(json: &str) -> Result<SourceMap, serde_json::Error> {
    let file: BuffMapFile = serde_json::from_str(json)?;
    Ok(SourceMap::from_buff_map_file(file))
}

/// Write a [`SourceMap`] to a `.buffmap` file at `path`.
pub fn write_to_file(map: &SourceMap, path: &Path) -> Result<(), std::io::Error> {
    let json = serialize_to_string(map)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    std::fs::write(path, json)
}

/// Read a `.buffmap` file from `path` into a [`SourceMap`].
pub fn read_from_file(path: &Path) -> Result<SourceMap, std::io::Error> {
    let json = std::fs::read_to_string(path)?;
    deserialize(&json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
}

impl SourceMap {
    /// Project this [`SourceMap`] into the JSON-friendly [`BuffMapFile`].
    pub(crate) fn to_buff_map_file(&self) -> BuffMapFile {
        let mut function_mappings: Vec<FunctionMapping> =
            self.functions.iter().map(anchor_to_mapping).collect();
        function_mappings.sort_by_key(|m| (m.rust_start_line, m.buff_span_start));

        let mut line_mappings: Vec<LineMapping> = self
            .rust_to_buff
            .iter()
            .map(|(rust_line, loc)| LineMapping {
                rust_line: *rust_line,
                buff_line: loc.line,
                buff_col: loc.col,
                buff_span_start: loc.span.start,
                buff_span_end: loc.span.end,
                buff_source_id: loc.span.source_id.0,
                buff_name: loc.name.clone(),
            })
            .collect();
        line_mappings.sort_by_key(|m| m.rust_line);

        BuffMapFile {
            version: MAP_FORMAT_VERSION,
            buff_file: self
                .buff_file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            rust_file: self
                .rust_file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            source_id: self.source_id.0,
            function_mappings,
            line_mappings,
        }
    }

    /// Rebuild a [`SourceMap`] from a deserialised [`BuffMapFile`].
    pub(crate) fn from_buff_map_file(file: BuffMapFile) -> Self {
        let mut map = SourceMap::new().with_buff_file(
            file.buff_file.unwrap_or_default().into(),
            SourceId(file.source_id),
        );
        if let Some(rust_file) = file.rust_file {
            map = map.with_rust_file(rust_file.into());
        }
        for fm in file.function_mappings {
            let span = Span::new(fm.buff_span_start, fm.buff_span_end, SourceId(fm.buff_source_id));
            map.add_function(FunctionAnchor {
                name: fm.buff_name,
                buff_span: span,
                buff_line: fm.buff_line,
                buff_col: fm.buff_col,
                rust_start_line: fm.rust_start_line,
                rust_end_line: fm.rust_end_line,
                buff_location: Some(BuffLocation {
                    line: fm.buff_line,
                    col: fm.buff_col,
                    span,
                    name: None,
                }),
            });
        }
        for lm in file.line_mappings {
            map.add_line_mapping(
                lm.rust_line,
                BuffLocation {
                    line: lm.buff_line,
                    col: lm.buff_col,
                    span: Span::new(
                        lm.buff_span_start,
                        lm.buff_span_end,
                        SourceId(lm.buff_source_id),
                    ),
                    name: lm.buff_name,
                },
            );
        }
        map
    }
}

fn anchor_to_mapping(anchor: &FunctionAnchor) -> FunctionMapping {
    FunctionMapping {
        buff_name: anchor.name.clone(),
        buff_span_start: anchor.buff_span.start,
        buff_span_end: anchor.buff_span.end,
        buff_source_id: anchor.buff_span.source_id.0,
        buff_line: anchor.buff_line,
        buff_col: anchor.buff_col,
        rust_start_line: anchor.rust_start_line,
        rust_end_line: anchor.rust_end_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_map_round_trips() {
        let map = SourceMap::new();
        let json = serialize_to_string(&map).expect("serialize");
        let back = deserialize(&json).expect("deserialize");
        assert_eq!(map.functions.len(), back.functions.len());
        assert_eq!(map.rust_to_buff.len(), back.rust_to_buff.len());
        assert_eq!(back.source_id, SourceId(0));
    }

    #[test]
    fn buff_map_file_has_current_version() {
        let map = SourceMap::new();
        let file = map.to_buff_map_file();
        assert_eq!(file.version, MAP_FORMAT_VERSION);
        assert_eq!(file.version, 1);
    }

    #[test]
    fn serialize_then_deserialize_preserves_function_anchor() {
        let mut map = SourceMap::new();
        let span = Span::new(10, 50, SourceId(0));
        map.add_function(FunctionAnchor {
            name: "helper".to_string(),
            buff_span: span,
            buff_line: 1,
            buff_col: 1,
            rust_start_line: 3,
            rust_end_line: 5,
            buff_location: None,
        });
        let json = serialize_to_string(&map).expect("serialize");
        let back = deserialize(&json).expect("deserialize");
        assert_eq!(back.functions.len(), 1);
        assert_eq!(back.functions[0].name, "helper");
        assert_eq!(back.functions[0].buff_span.start, 10);
        assert_eq!(back.functions[0].buff_span.end, 50);
        assert_eq!(back.functions[0].rust_start_line, 3);
        assert_eq!(back.functions[0].rust_end_line, 5);
    }

    #[test]
    fn deserialize_rejects_malformed_json() {
        let result = deserialize("{not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn unknown_keys_are_ignored_for_forwards_compat() {
        let json = r#"{
            "version": 1,
            "buff_file": null,
            "rust_file": null,
            "source_id": 0,
            "function_mappings": [],
            "line_mappings": [],
            "future_field": ["some", "new", "data"]
        }"#;
        let result = deserialize(json);
        assert!(result.is_ok(), "forwards-compat: unknown keys ignored");
    }
}
