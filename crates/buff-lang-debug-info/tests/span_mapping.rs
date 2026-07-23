//! Integration tests for `buff-lang-debug-info` — end-to-end span mapping
//! correctness (T24).
//!
//! These tests verify the round-trip `SourceMap` → JSON → `SourceMap` +
//! the panic-hook line-remapping logic with realistic inputs. They DO
//! NOT invoke rustc (the host linker is missing); instead they build
//! `SourceMap` instances directly and feed synthetic Rust sources /
//! backtrace strings through the public API.

use std::path::{Path, PathBuf};

use buff_lang_ast::{common::Ident, decl::FuncDecl, Decl};
use buff_lang_error::{SourceId, Span};

use buff_lang_debug_info::{
    build_source_map, deserialize, install_panic_hook, remap_panic_backtrace, serialize_to_string,
    BuffLocation, BuffMapFile, FunctionAnchor, FunctionMapping, LineMapping, SourceMap,
    MAP_FORMAT_VERSION,
};
use buff_lang_error::SourceFile;

fn make_func(name: &str, span_start: usize, span_end: usize) -> FuncDecl {
    FuncDecl {
        name: Ident::new(name, Span::dummy()),
        params: Vec::new(),
        return_type: None,
        body: buff_lang_ast::common::Block::empty(Span::dummy()),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: Span::new(span_start, span_end, SourceId(0)),
    }
}

fn make_decl(name: &str, span_start: usize, span_end: usize) -> Decl {
    Decl::FuncDecl(make_func(name, span_start, span_end))
}

#[test]
fn round_trip_preserves_function_mappings_order() {
    let buff = "func helper():\n    return 1\n\nfunc main():\n    return helper()\n";
    let rust = "fn helper() {\n    1\n}\nfn main() {\n    helper()\n}\n";
    let helper_start = buff.find("func helper").unwrap_or(0);
    let helper_end = helper_start + "func helper():\n    return 1".len();
    let main_start = buff.find("func main").unwrap_or(0);
    let main_end = main_start + "func main():\n    return helper()".len();
    let decls = vec![
        make_decl("helper", helper_start, helper_end),
        make_decl("main", main_start, main_end),
    ];
    let map = build_source_map(&decls, rust, Path::new("test.buff"), buff);
    let json = serialize_to_string(&map).expect("serialize");
    let back = deserialize(&json).expect("deserialize");
    assert_eq!(back.functions.len(), 2);
    assert_eq!(back.functions[0].name, "helper");
    assert_eq!(back.functions[1].name, "main");
    assert!(back.functions[0].rust_start_line <= back.functions[1].rust_start_line);
}

#[test]
fn lookup_buff_returns_exact_match_first() {
    let mut map = SourceMap::new();
    let span = Span::new(0, 10, SourceId(0));
    map.add_line_mapping(
        5,
        BuffLocation {
            line: 1,
            col: 1,
            span,
            name: Some("exact".to_string()),
        },
    );
    let result = map.lookup_buff(5);
    assert!(result.is_some());
    assert_eq!(result.unwrap().line, 1);
    assert_eq!(result.unwrap().name.as_deref(), Some("exact"));
}

#[test]
fn lookup_buff_falls_back_to_nearest_below() {
    let mut map = SourceMap::new();
    let span = Span::new(0, 10, SourceId(0));
    map.add_line_mapping(
        3,
        BuffLocation {
            line: 1,
            col: 1,
            span,
            name: Some("near_below".to_string()),
        },
    );
    let result = map.lookup_buff(7);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name.as_deref(), Some("near_below"));
}

#[test]
fn lookup_buff_uses_function_containment_as_final_fallback() {
    let mut map = SourceMap::new();
    let span = Span::new(0, 10, SourceId(0));
    map.add_function(FunctionAnchor {
        name: "helper".to_string(),
        buff_span: span,
        buff_line: 1,
        buff_col: 1,
        rust_start_line: 5,
        rust_end_line: 10,
        buff_location: Some(BuffLocation {
            line: 1,
            col: 1,
            span,
            name: Some("helper".to_string()),
        }),
    });
    let result = map.lookup_buff(7);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name.as_deref(), Some("helper"));
}

#[test]
fn lookup_buff_returns_none_when_no_mapping_anywhere() {
    let map = SourceMap::new();
    let result = map.lookup_buff(42);
    assert!(result.is_none());
}

#[test]
fn lookup_name_returns_none_for_unnamed_location() {
    let mut map = SourceMap::new();
    map.add_line_mapping(
        3,
        BuffLocation {
            line: 1,
            col: 1,
            span: Span::new(0, 10, SourceId(0)),
            name: None,
        },
    );
    assert!(map.lookup_name(3).is_none());
}

#[test]
fn is_empty_distinguishes_populated_from_empty_map() {
    let empty = SourceMap::new();
    assert!(empty.is_empty());
    let mut populated = SourceMap::new();
    populated.add_function(FunctionAnchor {
        name: "x".to_string(),
        buff_span: Span::new(0, 1, SourceId(0)),
        buff_line: 1,
        buff_col: 1,
        rust_start_line: 1,
        rust_end_line: 2,
        buff_location: None,
    });
    assert!(!populated.is_empty());
}

#[test]
fn serialize_to_string_is_deterministic_across_calls() {
    let mut map = SourceMap::new().with_buff_file(PathBuf::from("test.buff"), SourceId(0));
    map.add_function(FunctionAnchor {
        name: "f".to_string(),
        buff_span: Span::new(0, 10, SourceId(0)),
        buff_line: 1,
        buff_col: 1,
        rust_start_line: 1,
        rust_end_line: 3,
        buff_location: None,
    });
    let json1 = serialize_to_string(&map).expect("serialize");
    let json2 = serialize_to_string(&map).expect("serialize");
    assert_eq!(json1, json2, "deterministic JSON output");
}

#[test]
fn deserialize_picks_up_buff_file_path() {
    let mut map = SourceMap::new()
        .with_buff_file(PathBuf::from("examples/debug/panic_demo.buff"), SourceId(0));
    map.add_line_mapping(
        1,
        BuffLocation {
            line: 1,
            col: 1,
            span: Span::new(0, 10, SourceId(0)),
            name: Some("main".to_string()),
        },
    );
    let json = serialize_to_string(&map).expect("serialize");
    let back = deserialize(&json).expect("deserialize");
    assert_eq!(
        back.buff_file
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        Some("examples/debug/panic_demo.buff".to_string())
    );
}

#[test]
fn round_trip_preserves_line_mapping_with_name() {
    let mut map = SourceMap::new();
    map.add_line_mapping(
        5,
        BuffLocation {
            line: 2,
            col: 3,
            span: Span::new(20, 30, SourceId(0)),
            name: Some("helper".to_string()),
        },
    );
    let json = serialize_to_string(&map).expect("serialize");
    let back = deserialize(&json).expect("deserialize");
    assert_eq!(back.rust_to_buff.len(), 1);
    let loc = back.rust_to_buff.get(&5).expect("entry present");
    assert_eq!(loc.line, 2);
    assert_eq!(loc.col, 3);
    assert_eq!(loc.name.as_deref(), Some("helper"));
}

#[test]
fn buff_map_file_struct_round_trips_via_json() {
    let file = BuffMapFile {
        version: MAP_FORMAT_VERSION,
        buff_file: Some("test.buff".to_string()),
        rust_file: Some("test.rs".to_string()),
        source_id: 0,
        function_mappings: vec![FunctionMapping {
            buff_name: "main".to_string(),
            buff_span_start: 0,
            buff_span_end: 100,
            buff_source_id: 0,
            buff_line: 1,
            buff_col: 1,
            rust_start_line: 1,
            rust_end_line: 5,
        }],
        line_mappings: vec![LineMapping {
            rust_line: 1,
            buff_line: 1,
            buff_col: 1,
            buff_span_start: 0,
            buff_span_end: 100,
            buff_source_id: 0,
            buff_name: Some("main".to_string()),
        }],
    };
    let json = serde_json::to_string_pretty(&file).expect("serialize");
    let back: BuffMapFile = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, file);
}

#[test]
fn remap_panic_backtrace_returns_empty_for_empty_map() {
    let map = SourceMap::new();
    let trace = remap_panic_backtrace(Some(&map));
    assert!(trace.frames.is_empty());
}

#[test]
fn remap_panic_backtrace_returns_empty_when_map_none() {
    let trace = remap_panic_backtrace(None);
    assert!(trace.frames.is_empty());
    assert!(trace.buff_file_display.is_empty() || trace.buff_file_display == "<unknown>");
}

#[test]
fn install_panic_hook_does_not_panic_when_no_buffmap() {
    std::env::remove_var("BUFF_MAP_PATH");
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("__nonexistent__"));
    let mut sibling_buffmap = exe.clone();
    sibling_buffmap.set_extension("buffmap");
    let _ = std::fs::remove_file(&sibling_buffmap);
    install_panic_hook();
}

#[test]
fn build_source_map_realistic_panic_demo() {
    let buff = "func helper():\n    let x = 1 / 0\n    print(x)\n\nfunc main():\n    helper()\n";
    let rust = "fn helper() {\n    let x = 1 / 0;\n    println!(\"{}\", x);\n}\nfn main() {\n    helper();\n}\n";
    let helper_start = buff.find("func helper").unwrap_or(0);
    let helper_end = buff.find("func main").unwrap_or(buff.len());
    let main_start = helper_end;
    let main_end = buff.len();
    let decls = vec![
        make_decl("helper", helper_start, helper_end),
        make_decl("main", main_start, main_end),
    ];
    let path = Path::new("examples/debug/panic_demo.buff");
    let map = build_source_map(&decls, rust, path, buff);
    assert_eq!(map.functions.len(), 2);
    assert_eq!(map.functions[0].name, "helper");
    assert_eq!(map.functions[0].rust_start_line, 1);
    assert_eq!(map.functions[0].rust_end_line, 4);
    assert_eq!(map.functions[1].name, "main");
    assert_eq!(map.functions[1].rust_start_line, 5);
    assert_eq!(
        map.buff_file.as_ref().map(|p| p.to_path_buf()),
        Some(path.to_path_buf())
    );
}

#[test]
fn lookup_buff_handles_function_end_line_inclusive() {
    let mut map = SourceMap::new();
    let span = Span::new(0, 10, SourceId(0));
    map.add_function(FunctionAnchor {
        name: "f".to_string(),
        buff_span: span,
        buff_line: 1,
        buff_col: 1,
        rust_start_line: 5,
        rust_end_line: 10,
        buff_location: Some(BuffLocation {
            line: 1,
            col: 1,
            span,
            name: Some("f".to_string()),
        }),
    });
    assert!(map.lookup_buff(5).is_some(), "start line inclusive");
    assert!(map.lookup_buff(10).is_some(), "end line inclusive");
    assert!(map.lookup_buff(4).is_none(), "before start");
    assert!(map.lookup_buff(11).is_none(), "after end");
}

#[test]
fn source_file_lookup_returns_correct_line_col() {
    let sf = SourceFile::new(
        PathBuf::from("test.buff"),
        "first\nsecond\nthird".to_string(),
    );
    assert_eq!(sf.lookup(0), Some((1, 1)));
    assert_eq!(sf.lookup(6), Some((2, 1)));
    assert_eq!(sf.lookup(13), Some((3, 1)));
    assert_eq!(sf.lookup(99), None);
}

#[test]
fn deserialize_handles_missing_optional_fields() {
    let json = r#"{
        "version": 1,
        "buff_file": null,
        "rust_file": null,
        "source_id": 0,
        "function_mappings": [],
        "line_mappings": []
    }"#;
    let result = deserialize(json);
    assert!(result.is_ok());
    let map = result.unwrap();
    assert!(map.is_empty());
}

#[test]
fn serialize_omits_none_names_in_line_mappings() {
    let mut map = SourceMap::new();
    map.add_line_mapping(
        5,
        BuffLocation {
            line: 1,
            col: 1,
            span: Span::new(0, 10, SourceId(0)),
            name: None,
        },
    );
    let json = serialize_to_string(&map).expect("serialize");
    assert!(
        !json.contains("\"buff_name\""),
        "None names should be omitted via skip_serializing_if, got: {json}"
    );
}
