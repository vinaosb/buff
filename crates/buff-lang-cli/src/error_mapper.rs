//! Error mapper — translates `rustc` diagnostics and runtime panics from the
//! intermediate `.rs` file back to the original `.buff` source location.
//!
//! ## v0.1 strategy
//!
//! Exact Buff line translation requires a populated
//! [`SourceMap`](buff_lang_error::SourceMap) (Buff span ↔ Rust line), which in turn
//! requires a post-`prettyplease` line scan during codegen. For v0.1 we use the
//! simpler **filename translation** approach:
//!
//! - [`translate_rustc_errors`] — replaces the intermediate `.rs` path with the
//!   original `.buff` path in `rustc`'s stderr.
//! - [`translate_panic`] — replaces the `.rs` path embedded in the panic
//!   location string (e.g. `thread 'main' panicked at …, foo.rs:3:5`) with the
//!   `.buff` path. If a [`SourceMap`](buff_lang_error::SourceMap) with line mappings
//!   is available, the Rust line number is also translated to the closest Buff
//!   line; otherwise the Rust line is preserved (still an improvement over
//!   showing the `.rs` filename).
//! - [`filter_backtrace`] — removes Rust-stdlib frames so the user only sees
//!   frames originating from their Buff program.

use std::path::Path;

use buff_lang_error::SourceMap;

/// Replace every occurrence of the `rust_file` path in `stderr` with the
/// `buff_file` path.
///
/// `rustc` diagnostics reference the `.rs` file we generated (e.g.
/// `error[E0384]: … --> temp.rs:3:5`). After translation the same message
/// points at the user's `.buff` source instead.
///
/// Both forward-slash and backslash variants are replaced so the function
/// works on both Unix and Windows `rustc` output.
pub fn translate_rustc_errors(stderr: &str, buff_file: &Path, rust_file: &Path) -> String {
    let rust_str = rust_file.to_string_lossy();
    let buff_str = buff_file.to_string_lossy();
    // Replace both the native path and any slash-variant rustc may emit.
    let mut result = stderr.replace(rust_str.as_ref(), buff_str.as_ref());
    // Also handle the case where rustc normalizes to forward slashes.
    let rust_fwd = rust_str.replace('\\', "/");
    let buff_fwd = buff_str.replace('\\', "/");
    if rust_fwd != rust_str.as_ref() {
        result = result.replace(rust_fwd.as_str(), buff_fwd.as_str());
    }
    result
}

/// Translate a runtime panic message, replacing the `.rs` file reference with
/// the `.buff` file reference.
///
/// If `source_map` contains Rust-line → Buff-span mappings, the Rust line
/// number in the panic location is additionally translated to the closest Buff
/// source line (extracted from the span via the map's front-end
/// [`SourceMap::lookup`]). When the map is empty (v0.1 default), only the
/// filename is translated and the line number is preserved.
///
/// **Example** (v0.1, empty map):
///
/// ```text
/// thread 'main' panicked at 'attempt to divide by zero', prog.rs:2:15
/// ```
/// becomes:
/// ```text
/// thread 'main' panicked at 'attempt to divide by zero', prog.buff:2:15
/// ```
pub fn translate_panic(
    panic_msg: &str,
    rust_file: &Path,
    buff_file: &Path,
    source_map: &SourceMap,
) -> String {
    // Step 1: filename replacement (always).
    let rust_str = rust_file.to_string_lossy();
    let buff_str = buff_file.to_string_lossy();
    let mut result = panic_msg.replace(rust_str.as_ref(), buff_str.as_ref());
    // Also handle forward-slash normalization.
    let rust_fwd = rust_str.replace('\\', "/");
    let buff_fwd = buff_str.replace('\\', "/");
    if rust_fwd != rust_str.as_ref() {
        result = result.replace(rust_fwd.as_str(), buff_fwd.as_str());
    }

    // Step 2: if the source map has line mappings, translate the line number.
    // We look for the pattern "<buff_file>:LINE:COL" that we just produced and
    // replace LINE with the best Buff line. This is best-effort.
    if !source_map.is_line_map_empty() {
        result = translate_panic_line_numbers(&result, buff_file, source_map);
    }

    result
}

/// Given a panic string that already has the `.buff` filename substituted in,
/// find `<buff_file>:RUST_LINE:COL` patterns and replace `RUST_LINE` with the
/// closest Buff line from the source map.
fn translate_panic_line_numbers(msg: &str, buff_file: &Path, source_map: &SourceMap) -> String {
    let buff_str = buff_file.to_string_lossy();
    let mut result = String::with_capacity(msg.len());
    let mut rest = msg;

    while let Some(pos) = rest.find(buff_str.as_ref()) {
        // Copy everything up to and including the filename.
        result.push_str(&rest[..pos + buff_str.len()]);
        rest = &rest[pos + buff_str.len()..];

        // Expect a ':' followed by digits (the line number).
        if let Some(colon_pos) = rest.find(':') {
            let after_colon = &rest[colon_pos + 1..];
            // Parse the run of ASCII digits.
            let digit_end = after_colon
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_digit())
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);

            if digit_end > 0 {
                let line_str = &after_colon[..digit_end];
                if let Ok(rust_line) = line_str.parse::<usize>() {
                    if let Some(buff_span) = source_map.lookup_buff(rust_line) {
                        // Look up the Buff line from the span's source_id.
                        if let Some((buff_line, _)) =
                            source_map.lookup(buff_span.source_id, buff_span.start)
                        {
                            result.push(':');
                            result.push_str(&buff_line.to_string());
                            // Skip past the original digits.
                            rest = &after_colon[digit_end..];
                            continue;
                        }
                    }
                }
            }
            // Line parse failed — copy the ':' verbatim and continue.
            result.push(':');
            rest = &rest[colon_pos + 1..];
        } else {
            // No ':' after filename — nothing more to translate.
            result.push_str(rest);
            return result;
        }
    }
    // Copy any remaining text after the last filename occurrence.
    result.push_str(rest);
    result
}

/// Filter a Rust backtrace to hide stdlib frames.
///
/// Removes lines that reference Rust's standard library or toolchain paths,
/// which are noise from the Buff user's perspective. The patterns filtered:
///
/// - `rustc/` (toolchain root, e.g. `…/rust/toolchains/…/rustc/…`)
/// - `rustlib` (rustlib sysroot)
/// - `/std/` and `\std\` (standard library source/modules)
/// - `/core/` and `\core\` (core library)
/// - `/alloc/` and `\alloc\` (alloc library)
///
/// Frames from the user's code (or the generated `.rs` file) are preserved.
pub fn filter_backtrace(backtrace: &str) -> String {
    backtrace
        .lines()
        .filter(|line| {
            !line.contains("rustc")
                && !line.contains("rustlib")
                && !line.contains("/std/")
                && !line.contains("\\std\\")
                && !line.contains("/core/")
                && !line.contains("\\core\\")
                && !line.contains("/alloc/")
                && !line.contains("\\alloc\\")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn rust_path() -> PathBuf {
        PathBuf::from("/tmp/prog.rs")
    }

    fn buff_path() -> PathBuf {
        PathBuf::from("/tmp/prog.buff")
    }

    #[test]
    fn translate_rustc_errors_replaces_path() {
        let stderr = "error[E0384]: cannot assign\n  --> /tmp/prog.rs:15:5\n   |\n15 |     x = 2;\n   |     ^^^^^";
        let result = translate_rustc_errors(stderr, &buff_path(), &rust_path());
        assert!(
            result.contains("prog.buff:15:5"),
            "expected buff path in result, got: {result}"
        );
        assert!(
            !result.contains("prog.rs:"),
            "should not contain .rs path, got: {result}"
        );
    }

    #[test]
    fn translate_panic_replaces_path_empty_map() {
        let sm = SourceMap::new();
        let panic = "thread 'main' panicked at 'boom', /tmp/prog.rs:2:15\nnote: run with `RUST_BACKTRACE=1`";
        let result = translate_panic(panic, &rust_path(), &buff_path(), &sm);
        assert!(
            result.contains("prog.buff:2:15"),
            "expected buff path + original line, got: {result}"
        );
        assert!(
            !result.contains("prog.rs:"),
            "should not contain .rs path, got: {result}"
        );
    }

    #[test]
    fn filter_backtrace_hides_stdlib() {
        let bt = "  0: std::panicking::begin_panic\n  1: prog::main\n             at /rustc/abc/lib/std/panicking.rs:5\n  2: core::fmt::write\n             at C:\\rust\\toolchains\\stable\\rustlib\\src\\core\\fmt.rs:10\n  3: prog::helper\n             at /tmp/prog.rs:5";
        let filtered = filter_backtrace(bt);
        assert!(
            !filtered.contains("rustc"),
            "should filter rustc frames: {filtered}"
        );
        assert!(
            !filtered.contains("rustlib"),
            "should filter rustlib frames: {filtered}"
        );
        assert!(
            filtered.contains("prog::main"),
            "should keep user frame prog::main: {filtered}"
        );
        assert!(
            filtered.contains("prog::helper"),
            "should keep user frame prog::helper: {filtered}"
        );
    }

    #[test]
    fn translate_rustc_errors_no_match_returns_unchanged() {
        let stderr = "some message without a path";
        let result = translate_rustc_errors(stderr, &buff_path(), &rust_path());
        assert_eq!(result, stderr, "unchanged when no path present");
    }

    #[test]
    fn translate_panic_translates_line_with_map() {
        // Build a source map: rust line 5 → buff span at byte 10 (line 2).
        let mut sm = SourceMap::new();
        let sid = buff_lang_error::SourceId(0);
        sm.add_source(
            sid,
            PathBuf::from("prog.buff"),
            "line1\nline2\nline3\nline4\nline5\n".to_string(),
        );
        // Byte offset 6 = start of "line2" = line 2.
        let buff_span = buff_lang_error::Span::new(6, 11, sid);
        sm.add_mapping(buff_span, 5);

        let panic = "thread 'main' panicked at 'boom', prog.rs:5:10";
        let result = translate_panic(panic, Path::new("prog.rs"), Path::new("prog.buff"), &sm);
        // The rust line 5 should map to buff line 2.
        assert!(
            result.contains("prog.buff:2:"),
            "expected line 2 (translated from rust 5), got: {result}"
        );
    }

    #[test]
    fn filter_backtrace_empty_input() {
        let filtered = filter_backtrace("");
        assert_eq!(filtered, "");
    }

    #[test]
    fn filter_backtrace_preserves_all_user_frames() {
        let bt = "  0: myapp::main\n  1: myapp::helper\n  2: myapp::foo";
        let filtered = filter_backtrace(bt);
        assert_eq!(filtered, bt, "all user frames preserved");
    }
}
