//! `buff ssr <FILE>` — Server-Side Render a `.buffhtml` Single-File
//! Component to HTML (T135).
//!
//! ## Pipeline
//!
//! 1. Parse + codegen the `.buffhtml` via [`pipeline::compile_buffhtml_to_rust`]
//!    (T133) → `syn::File` with a `#[component] fn <Name>() -> Element`
//!    item + a post-format [`SpanMap`].
//! 2. Splice in an SSR driver `fn main()` that:
//!    - imports the wrapper crate's SSR surface
//!      (`use buff_ui_dioxus::*; use buff_ui_dioxus::dioxus::prelude::*;`)
//!    - calls `buff_ui_dioxus::render_to_string(<Name>)` and writes the
//!      rendered HTML to stdout via `print!`.
//! 3. Write the assembled `.rs` to a temporary file alongside the input.
//! 4. Compile via `rustc --edition 2021` (host target; debug or release
//!    based on `--release`). Reuses [`pipeline::compile_buffhtml_rust_to_exe`]
//!    so rustc diagnostics are reverse-mapped to `.buffhtml` line:col via
//!    the [`SpanMap`].
//! 5. Run the binary; capture stdout (the rendered HTML).
//! 6. Forward stdout to the user's stdout, or write to `--output <file>`
//!    when provided.
//! 7. Clean up the temporary driver `.rs` and the binary (mirrors
//!    `buff run`'s cleanup-on-exit stance).
//!
//! ## Event handlers
//!
//! `onclick` / `oninput` / ... handlers are **ignored** during SSR (they
//! do not fire and do not appear in the output HTML). Initial `use_signal`
//! values ARE rendered. See [`buff_ui_dioxus::render_to_string`] for the
//! underlying helper.
//!
//! ## Errors
//!
//! Returns [`anyhow::Error`] on any failure surfaced by the compile
//! pipeline (file-read, buffhtml-parse, codegen, rustc invocation, binary
//! execution, or `--output` write). rustc diagnostics are translated to
//! `.buffhtml` line:col via the [`SpanMap`] side-table.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::pipeline::{
    compile_buffhtml_rust_to_exe, compile_buffhtml_to_rust, with_exe_extension, BuildMode,
};

/// SSR driver source template.
///
/// This is the `fn main()` wrapper spliced onto the codegen-buffhtml
/// output so the resulting binary, when run, prints the rendered HTML
/// for the named component. The template intentionally uses
/// fully-qualified `buff_ui_dioxus::` paths so a missing `use` import
/// never breaks the SSR driver (mirrors the T133 codegen philosophy of
/// not polluting the wrapper crate's namespace).
///
/// Two `use` lines are needed:
///
/// - `buff_ui_dioxus::*` brings in the wrapper crate's own exports
///   (`Element`, `component`, `render_to_string`, `VirtualDom`).
/// - `buff_ui_dioxus::dioxus::prelude::*` brings in the rsx!-macro
///   INTERNAL crate aliases (`dioxus_signals::{self, *}`,
///   `dioxus_elements`, `pub use dioxus_core`) — without these, the
///   `rsx!{}` macro inside `<Name>`'s body fails to expand (mirrors
///   `examples/counter.rs`'s dual `use` import).
const SSR_DRIVER_TEMPLATE: &str = r#"use buff_ui_dioxus::*;
use buff_ui_dioxus::dioxus::prelude::*;

fn main() {{
    let html = buff_ui_dioxus::render_to_string({component});
    print!("{{html}}");
}}
"#;

/// Entry point for `buff ssr <FILE> [--output <PATH>] [--release]`.
///
/// Orchestrates the SSR pipeline as documented at the crate-root. All
/// fallible operations return [`anyhow::Result`] with rich, user-facing
/// context; no `unwrap` / `expect` / `panic!`.
pub fn run(file: &Path, output: Option<&Path>, release: bool) -> Result<()> {
    // 1. Parse + codegen the .buffhtml (T133 path).
    let buffhtml_out = compile_buffhtml_to_rust(file)?;

    // 2. Derive the component fn name from the file stem (matches the
    //    T133 `derive_component_name` rule: PascalCased stem, falls
    //    back to `App` for dotfiles / non-alphabetic stems). We re-
    //    extract it from the generated source so the CLI + codegen
    //    never drift on naming.
    let component_name = extract_component_name(&buffhtml_out.rust_source)
        .context("failed to locate the #[component] fn name in the generated Rust source")?;

    // 3. Build the SSR driver source by appending the main() wrapper.
    let driver_source = format_driver_source(&buffhtml_out.rust_source, &component_name);

    // 4. Write the driver .rs to a unique-per-invocation temp path so
    //    concurrent `buff ssr` invocations don't clobber each other.
    let driver_rs_path = make_driver_path(file);
    std::fs::write(&driver_rs_path, &driver_source)
        .with_context(|| format!("failed to write SSR driver `{}`", driver_rs_path.display()))?;

    // 5. Compile the driver via rustc (host target). Reuse the span-
    //    aware `compile_buffhtml_rust_to_exe` so rustc diagnostics are
    //    reverse-mapped to .buffhtml line:col via the SpanMap.
    let mode = BuildMode::from_release_flag(release);
    // Driver binary path: replace the `.rs` extension with the host's
    // platform executable extension (`.exe` on Windows, no extension on
    // Unix). The driver `.rs` already encodes process+thread IDs so
    // concurrent invocations stay isolated at the binary level too.
    let exe_path =
        with_exe_extension(&driver_rs_path.with_extension(std::env::consts::EXE_EXTENSION));
    let compile_result = compile_buffhtml_rust_to_exe(
        &driver_rs_path,
        &exe_path,
        file,
        mode,
        &buffhtml_out.span_map,
        // The driver file is a re-rendering of the buffhtml source; pass
        // the original source for span lookup in error messages.
        &read_buffhtml_source_for_diagnostics(file)?,
    );

    // 6. Always clean up the driver .rs (the binary is cleaned below).
    let _ = std::fs::remove_file(&driver_rs_path);

    let exe_actual = compile_result?;

    // 7. Run the binary, capture stdout.
    let output_result = Command::new(&exe_actual)
        .output()
        .with_context(|| format!("failed to invoke SSR binary `{}`", exe_actual.display()))?;

    // 8. Clean up the binary (mirrors `buff run`'s cleanup stance).
    let _ = std::fs::remove_file(&exe_actual);

    if !output_result.status.success() {
        let stderr = String::from_utf8_lossy(&output_result.stderr);
        bail!(
            "SSR binary exited with status {}: {}",
            output_result.status,
            stderr.trim()
        );
    }

    // 9. Forward the rendered HTML.
    let html = String::from_utf8_lossy(&output_result.stdout);
    match output {
        Some(out_path) => {
            std::fs::write(out_path, html.as_bytes())
                .with_context(|| format!("failed to write SSR output `{}`", out_path.display()))?;
            eprintln!(
                "Wrote {} bytes of HTML to {}",
                html.len(),
                out_path.display()
            );
        }
        None => {
            // Write directly to stdout so users can pipe `buff ssr foo.buffhtml > out.html`.
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle
                .write_all(html.as_bytes())
                .context("failed to write SSR HTML to stdout")?;
        }
    }

    Ok(())
}

/// Build the SSR driver source by appending the main() wrapper to the
/// generated component file.
///
/// The component fn (`fn <Name>() -> Element { ... }`) is already
/// present in the codegen-buffhtml output; we append the driver `fn
/// main()` that imports the wrapper crate's SSR surface and invokes
/// [`buff_ui_dioxus::render_to_string`].
pub fn format_driver_source(component_source: &str, component_name: &str) -> String {
    let driver = SSR_DRIVER_TEMPLATE.replace("{component}", component_name);
    format!("{component_source}\n\n{driver}")
}

/// Extract the `fn <Name>` component identifier from the codegen-buffhtml
/// output.
///
/// The codegen emits exactly one `#[component] fn <Name>() -> Element`
/// item; we walk past the `#[component]` attribute and the `fn` keyword
/// to find the first identifier. Falls back to [`None`] when no
/// `#[component] fn` pattern is found (which indicates a bug in the
/// codegen path — surface it as a user-facing error).
pub fn extract_component_name(rust_source: &str) -> Option<String> {
    let mut tokens = rust_source.split_whitespace();
    while let Some(tok) = tokens.next() {
        if tok != "#[component]" && !tok.starts_with("#[component") {
            continue;
        }
        // Skip any closing `]` if the attribute was tokenised as
        // `#[component]` (the `]` may be in the same token) — then
        // look ahead for `fn <Name>`.
        if let Some(rest) = tok.strip_prefix("#[component") {
            if !rest.starts_with(']') {
                continue;
            }
        }
        // Walk forward to the next `fn` keyword.
        for ahead in tokens.by_ref() {
            if ahead == "fn" {
                if let Some(name_tok) = tokens.next() {
                    // Strip any trailing `(`, `<`, `(` from the identifier.
                    let clean: String = name_tok
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !clean.is_empty() {
                        return Some(clean);
                    }
                }
                break;
            }
        }
    }
    None
}

/// Build a unique-per-invocation path for the SSR driver `.rs` file.
///
/// Lives alongside the input `.buffhtml` (so rustc diagnostics
/// reference a path the user can recognise) but uses a unique suffix
/// derived from process ID + thread ID so concurrent invocations do
/// not clobber each other. The file is cleaned up by [`run`].
fn make_driver_path(buffhtml_file: &Path) -> PathBuf {
    let thread_id_str = format!("{:?}", std::thread::current().id());
    let thread_id_sanitised: String = thread_id_str
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let stem = buffhtml_file
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("buffhtml");
    let mut driver = buffhtml_file.to_path_buf();
    // `set_file_name` (not `set_extension`) so the dotted suffix
    // `ssr.<pid>.<tid>.rs` becomes part of the file name, not collapsed
    // into a single extension segment by `Path::set_extension`.
    driver.set_file_name(format!(
        "{stem}.ssr.{}.{}.rs",
        std::process::id(),
        thread_id_sanitised
    ));
    driver
}

/// Read the `.buffhtml` source for diagnostic span lookup.
///
/// Re-reads the file the user passed (the codegen step also reads it,
/// but we don't carry the source string across the API boundary to
/// `compile_buffhtml_rust_to_exe`). On read failure, returns an empty
/// string so the diagnostic translation falls back to filename-only
/// substitution (line:col lookup degrades gracefully).
fn read_buffhtml_source_for_diagnostics(file: &Path) -> Result<String> {
    Ok(std::fs::read_to_string(file).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_component_name_finds_component_fn() {
        let src = "#[component]\nfn Counter() -> Element {\n    rsx! { div { \"hi\" } }\n}\n";
        assert_eq!(
            extract_component_name(src).as_deref(),
            Some("Counter"),
            "should find Counter"
        );
    }

    #[test]
    fn extract_component_name_handles_typed_props_signature() {
        let src = "#[component]\nfn TodoList(props: TodoListProps) -> Element {\n    rsx! { div { \"hi\" } }\n}\n";
        assert_eq!(
            extract_component_name(src).as_deref(),
            Some("TodoList"),
            "should find TodoList even with props"
        );
    }

    #[test]
    fn extract_component_name_returns_none_when_no_component_attr() {
        let src = "fn Counter() -> Element {\n    rsx! { div { \"hi\" } }\n}\n";
        assert!(
            extract_component_name(src).is_none(),
            "no #[component] → None"
        );
    }

    #[test]
    fn format_driver_source_appends_main() {
        let component_src = "#[component]\nfn Counter() -> Element {\n    rsx! {}\n}\n";
        let driver = format_driver_source(component_src, "Counter");
        assert!(driver.contains("fn Counter()"));
        assert!(driver.contains("fn main()"));
        assert!(driver.contains("buff_ui_dioxus::render_to_string(Counter)"));
        // The component source must come BEFORE the driver main() so the
        // item is in scope when main() references it.
        let main_idx = driver.find("fn main()").expect("main present");
        let comp_idx = driver.find("fn Counter()").expect("component present");
        assert!(comp_idx < main_idx, "component must precede main");
    }

    #[test]
    fn make_driver_path_has_unique_suffix() {
        let p1 = make_driver_path(Path::new("counter.buffhtml"));
        let p2 = make_driver_path(Path::new("counter.buffhtml"));
        assert_eq!(
            p1, p2,
            "same-process + same-thread should yield the same path"
        );
        // The unique suffix (process ID + thread ID) is encoded in the
        // file NAME (not extension), because `Path::set_extension`
        // collapses dotted extensions. Verify by inspecting the full
        // file name string.
        let file_name = p1
            .file_name()
            .and_then(|n| n.to_str())
            .expect("file_name present");
        assert!(
            file_name.contains(&format!("{}", std::process::id())),
            "file name should encode PID for concurrency isolation; got {file_name}"
        );
        assert!(
            file_name.starts_with("counter.ssr."),
            "file name should start with stem + .ssr. prefix; got {file_name}"
        );
        assert!(
            file_name.ends_with(".rs"),
            "file name should end with `.rs` so rustc compiles it; got {file_name}"
        );
    }
}
