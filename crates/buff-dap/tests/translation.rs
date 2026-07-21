//! Integration tests for the `buff-dap` translation layer.
//!
//! These tests exercise the **pure** translation surface — they
//! build a synthetic [`SourceMap`] via the T60 public API
//! (`add_source` + `add_mapping`) and assert that:
//!
//! - `setBreakpoints` direction (`buff_line` → `rust_line`) returns
//!   the expected translated values for known mappings.
//! - `stackTrace` direction (`rust_line` → `buff_line` + `buff_col`)
//!   returns the expected translated values for known mappings.
//! - Identity fallback works when no mapping exists.
//! - Empty source map degrades to identity for every breakpoint.
//! - Multiple breakpoints translate in batch.
//!
//! The tests are deliberately self-contained — they do NOT spawn a
//! backend subprocess or open stdio. The transport + lifecycle
//! handshake are exercised via type-checking only (the `run_session`
//! fn signature is the contract; spawning a real lldb-dap is a USER
//! ACTION documented in `task-136-debugger-USER-ACTION.txt`).

use std::path::{Path, PathBuf};

use buff_dap::{
    translate_breakpoints_buff_to_rust, translate_stack_frame_rust_to_buff,
    translate_stack_trace_rust_to_buff, Backend, DapError, DapResult, Message, MessageKind,
    ServerConfig, TranslatedBreakpoint, TranslatedStackFrame,
};
use buff_lang_error::{SourceId, SourceMap, Span};

// -----------------------------------------------------------------
// Fixture builders.
// -----------------------------------------------------------------

/// Build a synthetic source map populated with a known mapping:
/// `buff_span(start=B, end=E, source_id=ID)` ↔ `rust_line=R`.
fn build_map(entries: &[(usize, usize, SourceId, usize)]) -> SourceMap {
    let mut sm = SourceMap::new();
    for &(start, end, source_id, rust_line) in entries {
        sm.add_mapping(Span::new(start, end, source_id), rust_line);
    }
    sm
}

/// Fixture: a 5-line buff source with deterministic line starts.
///
/// `"aaa\nbbb\nccc\nddd\neee\n"`
///  line_starts = [0, 4, 8, 12, 16, 20]
///  line 1 = "aaa"  (offset 0)
///  line 2 = "bbb"  (offset 4)
///  line 3 = "ccc"  (offset 8)
///  line 4 = "ddd"  (offset 12)
///  line 5 = "eee"  (offset 16)
const FIXTURE_SOURCE: &str = "aaa\nbbb\nccc\nddd\neee\n";

fn fixture_source_id() -> SourceId {
    SourceId(42)
}

fn fixture_buff_file() -> PathBuf {
    PathBuf::from("/workspace/proj/src/main.buff")
}

fn fixture_rust_file() -> PathBuf {
    PathBuf::from("/workspace/proj/src/main.rs")
}

/// Build a ServerConfig for tests that exercise the server-level
/// translation helpers. Maps each buff line to a rust line offset
/// by +100 (line 1 → 101, line 2 → 102, etc.) so translations are
/// easy to verify.
fn fixture_config() -> ServerConfig {
    let id = fixture_source_id();
    let mut sm = SourceMap::new();
    sm.add_source(id, fixture_buff_file(), FIXTURE_SOURCE.to_string());
    // Map every buff line to a rust line offset by +100.
    for (line_idx, byte_offset) in [0usize, 4, 8, 12, 16].iter().enumerate() {
        let buff_line = line_idx + 1;
        let rust_line = buff_line + 100;
        // Span end = next line start (or EOF); mirrors real codegen.
        let end = byte_offset + 3;
        sm.add_mapping(Span::new(*byte_offset, end, id), rust_line);
    }
    ServerConfig {
        backend: Backend::LldbDap,
        buff_file: fixture_buff_file(),
        rust_file: fixture_rust_file(),
        buff_source: FIXTURE_SOURCE.to_string(),
        source_map: sm,
        buff_source_id: id,
    }
}

// -----------------------------------------------------------------
// setBreakpoints direction (buff → rust).
// -----------------------------------------------------------------

#[test]
fn breakpoint_translates_known_line() {
    let id = fixture_source_id();
    let sm = build_map(&[(0, 3, id, 101), (4, 7, id, 102)]);
    let result = translate_breakpoints_buff_to_rust(&[1, 2], &sm, id, FIXTURE_SOURCE);
    assert_eq!(result.len(), 2);
    assert_eq!(
        result[0],
        TranslatedBreakpoint {
            buff_line: 1,
            rust_line: 101,
            translated: true,
        }
    );
    assert_eq!(
        result[1],
        TranslatedBreakpoint {
            buff_line: 2,
            rust_line: 102,
            translated: true,
        }
    );
}

#[test]
fn breakpoint_falls_back_to_identity_when_unmapped() {
    let id = fixture_source_id();
    // Map only line 1; line 5 is unmapped.
    let sm = build_map(&[(0, 3, id, 101)]);
    let result = translate_breakpoints_buff_to_rust(&[5], &sm, id, FIXTURE_SOURCE);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].buff_line, 5);
    assert_eq!(result[0].rust_line, 5); // identity
    assert!(!result[0].translated);
}

#[test]
fn breakpoint_empty_source_map_returns_all_identity() {
    let id = fixture_source_id();
    let sm = SourceMap::new(); // empty
    let result = translate_breakpoints_buff_to_rust(&[1, 2, 3, 99], &sm, id, FIXTURE_SOURCE);
    assert_eq!(result.len(), 4);
    for (i, &bl) in [1u32, 2, 3, 99].iter().enumerate() {
        assert_eq!(result[i].buff_line, bl);
        assert_eq!(result[i].rust_line, bl);
        assert!(!result[i].translated);
    }
}

#[test]
fn breakpoint_translates_empty_request() {
    let id = fixture_source_id();
    let sm = build_map(&[(0, 3, id, 101)]);
    let result = translate_breakpoints_buff_to_rust(&[], &sm, id, FIXTURE_SOURCE);
    assert!(result.is_empty());
}

#[test]
fn breakpoint_translates_large_batch() {
    let id = fixture_source_id();
    let mut entries = Vec::new();
    for (line_idx, byte_offset) in [0usize, 4, 8, 12, 16].iter().enumerate() {
        let buff_line = line_idx + 1;
        let rust_line = buff_line * 10;
        entries.push((*byte_offset, byte_offset + 3, id, rust_line));
    }
    let sm = build_map(&entries);
    let buff_lines: Vec<u32> = (1..=5).collect();
    let result = translate_breakpoints_buff_to_rust(&buff_lines, &sm, id, FIXTURE_SOURCE);
    assert_eq!(result.len(), 5);
    for (i, &bl) in buff_lines.iter().enumerate() {
        assert_eq!(result[i].buff_line, bl);
        assert_eq!(result[i].rust_line, bl * 10);
        assert!(result[i].translated);
    }
}

// -----------------------------------------------------------------
// stackTrace direction (rust → buff).
// -----------------------------------------------------------------

#[test]
fn stack_frame_translates_known_rust_line() {
    let id = fixture_source_id();
    let mut sm = build_map(&[(0, 3, id, 101), (4, 7, id, 102)]);
    sm.add_source(id, fixture_buff_file(), FIXTURE_SOURCE.to_string());
    let result = translate_stack_frame_rust_to_buff(101, &sm, id, &fixture_buff_file());
    assert!(result.translated);
    assert_eq!(result.rust_line, 101);
    assert_eq!(result.buff_line, 1);
    assert_eq!(result.buff_col, 1);
    assert_eq!(result.buff_file, fixture_buff_file());
}

#[test]
fn stack_frame_falls_back_to_identity_when_unmapped() {
    // T60 lookup_buff returns None only when the source map is
    // EMPTY. When non-empty, it returns the closest-below span
    // (so rust_line=9999 with mapping at 101 returns the span
    // at 101). Identity fallback only triggers on empty map.
    let id = fixture_source_id();
    let mut sm = SourceMap::new(); // empty
    sm.add_source(id, fixture_buff_file(), FIXTURE_SOURCE.to_string());
    let result = translate_stack_frame_rust_to_buff(9999, &sm, id, &fixture_buff_file());
    assert!(!result.translated);
    assert_eq!(result.buff_line, 9999);
    assert_eq!(result.buff_col, 1);
}

#[test]
fn stack_frame_uses_closest_below_fallback() {
    // SourceMap::lookup_buff returns the closest rust_line at or
    // below the requested one. So rust_line 105 (between mapped
    // 101 + 102) returns the span for 102 (closest below).
    let id = fixture_source_id();
    let mut sm = build_map(&[(0, 3, id, 101), (4, 7, id, 102)]);
    sm.add_source(id, fixture_buff_file(), FIXTURE_SOURCE.to_string());
    let result = translate_stack_frame_rust_to_buff(105, &sm, id, &fixture_buff_file());
    assert!(result.translated);
    assert_eq!(result.buff_line, 2); // closest-below (102 → line 2)
}

#[test]
fn stack_trace_batch_translates_all_frames() {
    let id = fixture_source_id();
    let mut sm = build_map(&[(0, 3, id, 101), (4, 7, id, 102), (8, 11, id, 103)]);
    sm.add_source(id, fixture_buff_file(), FIXTURE_SOURCE.to_string());
    // T60 closest-below: rust_line=9999 → span at 103 → buff line 3.
    let result =
        translate_stack_trace_rust_to_buff(&[101, 102, 103, 9999], &sm, id, &fixture_buff_file());
    assert_eq!(result.len(), 4);
    assert_eq!(result[0].buff_line, 1);
    assert_eq!(result[1].buff_line, 2);
    assert_eq!(result[2].buff_line, 3);
    // 9999 is past all mappings → closest-below returns 103's
    // span → buff line 3 (translated=true per T60 semantics).
    assert!(result[3].translated);
    assert_eq!(result[3].buff_line, 3);
}

#[test]
fn stack_frame_returns_buff_col_for_non_zero_offset() {
    // Place a span starting at byte 5 (line 2, col 2 in "aaa\nbbb\n...").
    let id = fixture_source_id();
    let mut sm = build_map(&[(5, 7, id, 200)]); // span starts at offset 5
    sm.add_source(id, fixture_buff_file(), FIXTURE_SOURCE.to_string());
    let result = translate_stack_frame_rust_to_buff(200, &sm, id, &fixture_buff_file());
    assert!(result.translated);
    assert_eq!(result.buff_line, 2);
    assert_eq!(result.buff_col, 2); // 'b' at offset 5 is col 2 of line 2
}

// -----------------------------------------------------------------
// ServerConfig smoke (no live session).
// -----------------------------------------------------------------

#[test]
fn server_config_fixture_is_well_formed() {
    let cfg = fixture_config();
    assert_eq!(cfg.backend, Backend::LldbDap);
    assert!(cfg.buff_file.ends_with("main.buff"));
    assert!(cfg.rust_file.ends_with("main.rs"));
    assert!(!cfg.source_map.is_line_map_empty());
    assert_eq!(cfg.buff_source_id, fixture_source_id());
}

// -----------------------------------------------------------------
// Protocol round-trip smoke.
// -----------------------------------------------------------------

#[test]
fn protocol_event_message_roundtrips() {
    let msg = Message::event(1, "initialized", None);
    let bytes = buff_dap::encode(&msg).expect("encode");
    let (decoded, consumed) = buff_dap::decode(&bytes).expect("decode");
    assert_eq!(consumed, bytes.len());
    assert_eq!(decoded.kind(), Ok(MessageKind::Event));
    assert_eq!(decoded.event_name(), Some("initialized"));
}

#[test]
fn protocol_response_roundtrips() {
    let body = serde_json::json!({"supportsConfigurationDoneRequest": true});
    let msg = Message::response(10, 20, Some(body.clone()));
    let bytes = buff_dap::encode(&msg).expect("encode");
    let (decoded, _) = buff_dap::decode(&bytes).expect("decode");
    assert_eq!(decoded.kind(), Ok(MessageKind::Response));
    assert_eq!(decoded.request_seq, Some(10));
    assert_eq!(decoded.success, Some(true));
    assert_eq!(decoded.body, Some(body));
}

// -----------------------------------------------------------------
// Error types are reachable.
// -----------------------------------------------------------------

#[test]
fn dap_error_no_backend_renders_user_facing_message() {
    let err = DapError::NoBackend;
    let msg = format!("{err}");
    assert!(msg.contains("lldb-dap"));
    assert!(msg.contains("USER-ACTION"));
}

#[test]
fn dap_error_backend_exited_carries_status() {
    let err = DapError::BackendExited {
        status: "exit code: 1".into(),
        stderr_excerpt: "lldb-dap: error".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("exit code: 1"));
    assert!(msg.contains("lldb-dap: error"));
}

#[test]
fn dap_result_alias_compiles() {
    let ok: DapResult<()> = Ok(());
    assert!(ok.is_ok());
}

// -----------------------------------------------------------------
// Backend detection surface.
// -----------------------------------------------------------------

#[test]
fn backend_all_lists_three_options_in_preference_order() {
    let all = Backend::all();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0], Backend::LldbDap);
    assert_eq!(all[1], Backend::Codelldb);
    assert_eq!(all[2], Backend::VscodeLldb);
}

#[test]
fn backend_from_str_supports_all_three() {
    assert_eq!(Backend::from_name("lldb-dap"), Some(Backend::LldbDap));
    assert_eq!(Backend::from_name("codelldb"), Some(Backend::Codelldb));
    assert_eq!(Backend::from_name("vscode-lldb"), Some(Backend::VscodeLldb));
    assert!(Backend::from_name("unknown").is_none());
}

// -----------------------------------------------------------------
// Type-level smoke: ensure derives satisfy workspace rule.
// -----------------------------------------------------------------

#[test]
fn translated_breakpoint_satisfies_derive_defaults() {
    let bp = TranslatedBreakpoint {
        buff_line: 1,
        rust_line: 2,
        translated: true,
    };
    let _ = bp.clone();
    let _ = format!("{bp:?}");
    assert_eq!(bp, bp.clone());
}

#[test]
fn translated_stack_frame_satisfies_derive_defaults() {
    let f = TranslatedStackFrame {
        rust_line: 10,
        buff_file: PathBuf::from("x.buff"),
        buff_line: 5,
        buff_col: 3,
        translated: true,
    };
    let _ = f.clone();
    let _ = format!("{f:?}");
    assert_eq!(f, f.clone());
}

#[test]
fn server_config_clones_without_panicking() {
    let cfg = fixture_config();
    let _ = cfg.clone();
}

#[test]
fn path_compare_smoke() {
    // Just verify Path comparisons don't panic — the helper is
    // tested directly in the unit module.
    let _ = Path::new("a").exists();
}
