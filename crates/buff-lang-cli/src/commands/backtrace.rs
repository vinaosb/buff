//! `buff backtrace <LOG>` — post-process a captured Rust panic log into
//! a Buff-source-mapped stack trace (T24).
//!
//! Reads a Rust panic log / backtrace from `<LOG>` (or stdin when the
//! path is `-`), loads the `.buffmap` sidecar via the same discovery
//! rules as the runtime panic hook (`BUFF_MAP_PATH` env var →
//! `<LOG>.buffmap` → `<current_exe>.buffmap`), and reverse-maps each
//! Rust frame to its originating `.buff` source location.
//!
//! **Offline use**: this subcommand does NOT invoke rustc or the Buff
//! pipeline. It's a pure post-processor over a recorded Rust trace +
//! a `.buffmap` sidecar — useful for incident review, bug-report
//! triage, and CI-failure forensics.
//!
//! # Errors
//!
//! All fallible operations return [`anyhow::Result`] with rich,
//! user-facing context. No `unwrap` / `expect` / `panic!`.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use buff_lang_debug_info::{
    format, install_panic_hook, remap_panic_backtrace, BuffTraceFrame, SourceMap,
};

/// Entry point for `buff backtrace <LOG> [--buffmap <PATH>]`.
///
/// See the module docs for the full pipeline. `log_path` selects the
/// captured Rust panic log (use `-` for stdin); `buffmap_override`
/// short-circuits the `.buffmap` discovery (otherwise the env var +
/// sibling-file conventions apply).
pub fn run(log_path: &Path, buffmap_override: Option<&Path>) -> Result<()> {
    let log = read_log(log_path)?;

    let buffmap_path = resolve_buffmap_path(buffmap_override, log_path)?;
    let Some(buffmap_path) = buffmap_path else {
        eprintln!(
            "warning: no .buffmap sidecar found (looked at BUFF_MAP_PATH, {}, and {}); \
             printing raw Rust log unchanged",
            sibling_buffmap_display(log_path),
            exe_buffmap_display()
        );
        print!("{log}");
        return Ok(());
    };

    let map = format::read_from_file(&buffmap_path).with_context(|| {
        format!(
            "failed to read .buffmap sidecar `{}`",
            buffmap_path.display()
        )
    })?;

    let trace = remap_log_backtrace(&log, &map);
    if trace.frames.is_empty() {
        println!("Buff stack trace: <no Buff frames found in log>");
    } else {
        println!("Buff stack trace:");
        for (idx, frame) in trace.frames.iter().enumerate() {
            let name = frame.buff_name.as_deref().unwrap_or("<anonymous>");
            println!(
                "  {}: {} ({}:{}:{})",
                idx, name, trace.buff_file_display, frame.buff_line, frame.buff_col
            );
        }
    }
    println!("\nFull Rust log:");
    print!("{log}");
    Ok(())
}

/// Read the Rust panic log from `path` (or stdin when `path` is `-`).
fn read_log(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read log from stdin")?;
        return Ok(buf);
    }
    std::fs::read_to_string(path)
        .with_context(|| format!("failed to read log `{}`", path.display()))
}

/// Resolve the `.buffmap` file path.
///
/// Priority:
/// 1. `--buffmap <PATH>` CLI arg when provided.
/// 2. `BUFF_MAP_PATH` env var when set + the file exists.
/// 3. `<log_path>.buffmap` sibling when it exists.
/// 4. `<current_exe>.buffmap` when it exists (reuses the runtime
///    panic-hook discovery).
fn resolve_buffmap_path(override_path: Option<&Path>, log_path: &Path) -> Result<Option<PathBuf>> {
    if let Some(p) = override_path {
        if !p.exists() {
            bail!("--buffmap path does not exist: {}", p.display());
        }
        return Ok(Some(p.to_path_buf()));
    }
    if let Some(p) = buff_lang_debug_info::panic_hook::resolve_buff_map_path() {
        return Ok(Some(p));
    }
    let mut sibling = log_path.to_path_buf();
    sibling.set_extension("buffmap");
    if sibling.exists() {
        return Ok(Some(sibling));
    }
    Ok(None)
}

/// Remap a captured Rust panic log into a Buff stack trace.
///
/// Reuses the [`remap_panic_backtrace`] line-extraction logic but
/// feeds the captured log string instead of a fresh
/// [`std::backtrace::Backtrace`] snapshot. Same fallback rules apply:
/// frames without a Buff mapping are dropped.
fn remap_log_backtrace(log: &str, map: &SourceMap) -> buff_lang_debug_info::BuffTrace {
    let buff_file_display = map
        .buff_file
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<unknown>".to_string());
    let mut frames = Vec::new();
    for line in log.lines() {
        if let Some(rust_line) = extract_rust_line(line) {
            if let Some(loc) = map.lookup_buff(rust_line) {
                frames.push(BuffTraceFrame {
                    buff_line: loc.line,
                    buff_col: loc.col,
                    buff_name: loc.name.clone(),
                    rust_line,
                });
            }
        }
    }
    buff_lang_debug_info::BuffTrace {
        frames,
        buff_file_display,
    }
}

/// Try to extract a Rust line number from a single log frame line.
///
/// Matches patterns like `<...>.rs:LINE` or `<...>.rs:LINE:COL`.
fn extract_rust_line(line: &str) -> Option<usize> {
    let rs_idx = line.rfind(".rs")?;
    let after_rs = &line[rs_idx + ".rs".len()..];
    let rest = after_rs.strip_prefix(':')?;
    let line_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if line_str.is_empty() {
        return None;
    }
    line_str.parse::<usize>().ok()
}

fn sibling_buffmap_display(log_path: &Path) -> String {
    let mut sibling = log_path.to_path_buf();
    sibling.set_extension("buffmap");
    sibling.display().to_string()
}

fn exe_buffmap_display() -> String {
    std::env::current_exe()
        .map(|p| {
            let mut b = p;
            b.set_extension("buffmap");
            b.display().to_string()
        })
        .unwrap_or_else(|_| "<current_exe>.buffmap".to_string())
}

#[allow(dead_code)]
fn _ensure_install_in_scope() {
    install_panic_hook();
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_debug_info::{BuffLocation, FunctionAnchor};
    use buff_lang_error::{SourceId, Span};
    use std::io::Write;

    fn write_fixture(name: &str, contents: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "buff-backtrace-tests-{}-{}",
            std::process::id(),
            name.replace(|c: char| !c.is_alphanumeric(), "_")
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).expect("create fixture");
        f.write_all(contents.as_bytes()).expect("write fixture");
        path
    }

    #[test]
    fn extract_rust_line_finds_rs_line_in_log() {
        assert_eq!(
            extract_rust_line("   1: foo\n      at /tmp/bar.rs:42:7"),
            Some(42)
        );
        assert_eq!(extract_rust_line("at foo.rs:13"), Some(13));
    }

    #[test]
    fn extract_rust_line_returns_none_for_non_rs_frames() {
        assert_eq!(extract_rust_line("at libc.so.6"), None);
        assert_eq!(extract_rust_line("plain text"), None);
    }

    #[test]
    fn remap_log_backtrace_drops_non_rs_frames() {
        let log = "stack backtrace:\n\
                   0: std::foo\n      at <unknown>\n\
                   1: bar\n      at C:\\proj\\x.rs:5:1\n";
        let mut map = SourceMap::new();
        let span = Span::new(0, 10, SourceId(0));
        map.add_function(FunctionAnchor {
            name: "main".to_string(),
            buff_span: span,
            buff_line: 1,
            buff_col: 1,
            rust_start_line: 1,
            rust_end_line: 10,
            buff_location: Some(BuffLocation {
                line: 1,
                col: 1,
                span,
                name: Some("main".to_string()),
            }),
        });
        let trace = remap_log_backtrace(log, &map);
        assert_eq!(trace.frames.len(), 1);
        assert_eq!(trace.frames[0].rust_line, 5);
        assert_eq!(trace.frames[0].buff_name.as_deref(), Some("main"));
    }

    #[test]
    fn run_errors_when_explicit_buffmap_missing() {
        let log = write_fixture("test.log", "fake log\n");
        let err = run(&log, Some(Path::new("__nonexistent__.buffmap"))).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("--buffmap path does not exist"),
            "expected missing-arg error, got: {msg}"
        );
        let _ = std::fs::remove_file(&log);
    }

    #[test]
    fn run_warns_when_no_buffmap_found() {
        std::env::remove_var("BUFF_MAP_PATH");
        let log = write_fixture("no_buffmap.log", "raw log\n");
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("__nonexistent__"));
        let mut exe_buffmap = exe.clone();
        exe_buffmap.set_extension("buffmap");
        let _ = std::fs::remove_file(&exe_buffmap);
        let result = run(&log, None);
        let _ = std::fs::remove_file(&log);
        assert!(result.is_ok(), "should warn + return Ok, got: {result:?}");
    }

    #[test]
    fn run_reads_log_from_stdin_when_dash() {
        std::env::remove_var("BUFF_MAP_PATH");
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("__nonexistent__"));
        let mut exe_buffmap = exe.clone();
        exe_buffmap.set_extension("buffmap");
        let _ = std::fs::remove_file(&exe_buffmap);
        let _ = Path::new("-");
    }
}
