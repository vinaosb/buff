#!/bin/bash
cat > /workspace/crates/buff-lang-debug-info/selfhost/debug_info.buff << 'BUFFEOF'
// buff-lang-debug-info self-host port
struct SourceMap {
    source_name: String,
    line_count: Int,
}
struct BuffLocation {
    file: String,
    line: Int,
    column: Int,
}
struct FunctionAnchor {
    function_name: String,
    span_start: Int,
    span_end: Int,
}
func source_map_new(name: String, lines: Int) -> SourceMap:
    return SourceMap { source_name: name, line_count: lines }
func buff_location_new(f: String, l: Int, c: Int) -> BuffLocation:
    return BuffLocation { file: f, line: l, column: c }
func function_anchor_new(name: String, s: Int, e: Int) -> FunctionAnchor:
    return FunctionAnchor { function_name: name, span_start: s, span_end: e }
func main():
    let sm = source_map_new(name: "main.buff", lines: 42)
    print(sm.source_name)
    print(sm.line_count)
    let loc = buff_location_new(f: "test.buff", l: 10, c: 5)
    print(loc.file)
    print(loc.line)
    print(loc.column)
    let anchor = function_anchor_new(name: "main", s: 0, e: 100)
    print(anchor.function_name)
    print(anchor.span_start)
    print(anchor.span_end)
BUFFEOF
rm -rf /workspace/target/buff-cache
/workspace/target/release/buff run /workspace/crates/buff-lang-debug-info/selfhost/debug_info.buff 2>/dev/null
