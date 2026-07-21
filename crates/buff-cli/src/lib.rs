//! `buff-cli` — CLI framework for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`clap`](https://crates.io/crates/clap)
//! crate. Exposes clap's builder API to USER programs (the `buff`
//! compiler binary already uses clap internally via
//! `crates/buff-lang-cli`; this crate makes clap available to Buff
//! programs as a Click / Commander / Cobra equivalent).
//!
//! # Pipeline
//!
//! ```text
//!   App.new(name) ──▶ app.flag(name, short, desc) ──▶ app.option(...)
//!        │                       │
//!        ▼                       ▼
//!   app.command(child, about) ──▶ app.parse(args) -> Result<ParsedArgs, CliError>
//!                                       │
//!                                       ▼
//!                              ParsedArgs.subcommand() -> Option<String>
//!                              ParsedArgs.flag(name)   -> bool
//!                              ParsedArgs.option(name) -> Option<String>
//!                              ParsedArgs.arg(name)    -> Option<String>
//!                              ParsedArgs.args()       -> Vec<String>
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `App`, `ParsedArgs`, `CliError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | `parse` consumes `self`; returns owned `ParsedArgs`. All getters return owned `String` / `bool` / `Vec<String>`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, CliError>`. `clap::Error` mapped via `From`. |
//! | R4 — Thread safety | `App` is `Send + Sync` (wraps `Arc<Mutex<Node>>`). CLI parsing is synchronous in `main()` — not idiomatic to cross `spawn` boundaries. |
//! | R5 — Lifetime hiding | No public lifetime parameters. All arguments are owned `String`. |
//! | R6 — Panic boundary | `parse` / `parse_or_exit` wrap their bodies in `catch_unwind` (per FFI guide §6). |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. All fallible ops return `Result`. Boolean / Option
//! getters return `bool` / `Option<T>` — never panic.

pub mod error;

pub use error::CliError;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

/// Internal builder state. Shared via `Arc<Mutex<Node>>` so that
/// `App::command(name, about)` can return a child `App` whose later
/// mutations are visible to the parent (the child shares its `Node`
/// with the entry stored in the parent's `subcommands` vector).
/// Mutex poisoning is treated as a no-op (the builder falls silent
/// — the next `parse` returns `CliError::Panic`).
#[derive(Debug, Clone)]
struct Node {
    name: String,
    about: Option<String>,
    version: Option<String>,
    flags: Vec<(String, char, String)>,
    options: Vec<(String, char, String)>,
    args: Vec<(String, String)>,
    subcommands: Vec<App>,
}

impl Node {
    fn new(name: String) -> Self {
        Node {
            name,
            about: None,
            version: None,
            flags: Vec::new(),
            options: Vec::new(),
            args: Vec::new(),
            subcommands: Vec::new(),
        }
    }
}

/// A CLI application built on `clap::Command`. Construct via
/// [`App::new`], configure via the builder methods (`flag` / `option`
/// / `arg` / `command`), then call [`App::parse`] to turn argv into
/// [`ParsedArgs`].
///
/// `App` is `Send + Sync` (wraps `Arc<Mutex<Node>>`). Holding an
/// `App` across a `spawn` boundary is supported but not idiomatic —
/// CLI parsing is synchronous in `main()` and the lock is held only
/// during the builder calls themselves.
#[derive(Debug, Clone)]
pub struct App {
    inner: Arc<Mutex<Node>>,
}

impl App {
    /// Create a new CLI app with the given binary name. The name
    /// appears in the auto-generated help text and version output.
    pub fn new(name: String) -> Self {
        App {
            inner: Arc::new(Mutex::new(Node::new(name))),
        }
    }

    /// Set the short description shown in `--help`. Mirrors
    /// `clap::Command::about`. Returns `self` for chaining.
    pub fn about(self, about: String) -> Self {
        if let Ok(mut node) = self.inner.lock() {
            node.about = Some(about);
        }
        self
    }

    /// Set the version string printed by `--version`. Mirrors
    /// `clap::Command::version`. Returns `self` for chaining.
    pub fn version(self, version: String) -> Self {
        if let Ok(mut node) = self.inner.lock() {
            node.version = Some(version);
        }
        self
    }

    /// Register a boolean flag (no value). At parse time, presence
    /// of `--<name>` (and `-<short>` when non-empty) sets the flag
    /// to `true`. `ParsedArgs::flag(name)` reads the resulting bool.
    /// Returns `self` for chaining.
    ///
    /// `short` accepts a single-character string (e.g. `"v"`). An
    /// empty `short` disables the short form. Only the first
    /// character of a multi-char string is used.
    pub fn flag(self, name: String, short: String, description: String) -> Self {
        if let Ok(mut node) = self.inner.lock() {
            node.flags.push((name, first_char(&short), description));
        }
        self
    }

    /// Register a value option. At parse time, `--<name> <value>`
    /// (and `-<short> <value>` when non-empty) populates the option.
    /// `ParsedArgs::option(name)` reads the resulting `Option<String>`.
    /// Returns `self` for chaining.
    ///
    /// `short` accepts a single-character string; empty disables the
    /// short form. Only the first character of a multi-char string
    /// is used.
    pub fn option(self, name: String, short: String, description: String) -> Self {
        if let Ok(mut node) = self.inner.lock() {
            node.options.push((name, first_char(&short), description));
        }
        self
    }

    /// Register a positional argument. Positional args are matched
    /// in declaration order. `ParsedArgs::arg(name)` reads the
    /// resulting `Option<String>` by declared name. Returns `self`
    /// for chaining.
    pub fn arg(self, name: String, description: String) -> Self {
        if let Ok(mut node) = self.inner.lock() {
            node.args.push((name, description));
        }
        self
    }

    /// Register a subcommand on `self`. Returns a NEW `App`
    /// representing the subcommand — further builder calls on the
    /// returned value configure the subcommand (NOT the parent).
    /// The mutations are visible to `self` because the child shares
    /// its internal `Node` with the entry stored in `self.subcommands`.
    ///
    /// Takes `&self` (NOT `self`) so the caller can register multiple
    /// subcommands and then call `parse` on the original `app` (the
    /// chained-builder pattern does not work here because each
    /// `command()` returns a different child, not the parent).
    ///
    /// At parse time, if argv starts with `<binary> <name> [args]`,
    /// `ParsedArgs::subcommand()` returns `Some(name)` and the
    /// remaining args are matched against the subcommand's grammar.
    /// Use `ParsedArgs::subcommand_args()` to inspect them.
    ///
    /// Mirrors `clap::Command::subcommand`. Subcommands may nest
    /// arbitrarily deep (a subcommand can register its own
    /// subcommands via the same `command()` call).
    pub fn command(&self, name: String, about: String) -> App {
        let sub = App::new(name).about(about);
        if let Ok(mut node) = self.inner.lock() {
            node.subcommands.push(sub.clone());
        }
        sub
    }

    /// Parse argv into [`ParsedArgs`]. Returns `Err(CliError::Parse)`
    /// for clap-level rejections (unknown flag, missing required arg,
    /// `--help` / `--version` invocations, etc.) and
    /// `Err(CliError::Panic)` if the parser panics (per T4 FFI guide
    /// R6). Use [`App::parse_or_exit`] for the standard
    /// exit-0-on-help behaviour.
    pub fn parse(self, args: Vec<String>) -> Result<ParsedArgs, CliError> {
        let positional_names = self.collect_positional_names();
        let cmd = self.build_command();
        let result = catch_unwind(AssertUnwindSafe(|| {
            cmd.try_get_matches_from(args)
        }));
        match result {
            Ok(Ok(matches)) => Ok(ParsedArgs {
                matches,
                positional_names,
            }),
            Ok(Err(e)) => Err(CliError::from(e)),
            Err(_) => Err(CliError::Panic),
        }
    }

    /// Parse argv, printing the error and exiting with status 1 on
    /// failure (mirrors `clap::Command::get_matches_from`). On
    /// `--help` / `--version`, clap prints and exits with status 0.
    ///
    /// Wraps the parse body in `catch_unwind` per T4 FFI guide R6.
    /// If the parser panics, the process exits with status 1 and a
    /// diagnostic on stderr.
    pub fn parse_or_exit(self, args: Vec<String>) -> ParsedArgs {
        let positional_names = self.collect_positional_names();
        let cmd = self.build_command();
        let result = catch_unwind(AssertUnwindSafe(|| {
            cmd.try_get_matches_from(args)
        }));
        match result {
            Ok(Ok(matches)) => ParsedArgs {
                matches,
                positional_names,
            },
            Ok(Err(e)) => {
                e.print().ok();
                std::process::exit(e.exit_code());
            }
            Err(_) => {
                eprintln!("internal error: CLI parser panicked");
                std::process::exit(1);
            }
        }
    }

    /// Render the auto-generated help text (the same text clap
    /// prints on `--help`). Mirrors `clap::Command::render_help`.
    pub fn help_text(&self) -> String {
        self.build_command().render_help().to_string()
    }

    fn build_command(&self) -> clap::Command {
        let node = match self.inner.lock() {
            Ok(n) => n,
            Err(e) => {
                let msg = e.to_string();
                return clap::Command::new(msg.as_str());
            }
        };
        let mut cmd = clap::Command::new(node.name.as_str());
        if let Some(ref about) = node.about {
            cmd = cmd.about(about.as_str());
        }
        if let Some(ref version) = node.version {
            cmd = cmd.version(version.as_str());
        }
        for (name, short, desc) in &node.flags {
            let mut arg = clap::Arg::new(name.as_str())
                .long(name.as_str())
                .help(desc.as_str())
                .action(clap::ArgAction::SetTrue);
            if *short != '\0' {
                arg = arg.short(*short);
            }
            cmd = cmd.arg(arg);
        }
        for (name, short, desc) in &node.options {
            let mut arg = clap::Arg::new(name.as_str())
                .long(name.as_str())
                .help(desc.as_str())
                .action(clap::ArgAction::Set)
                .num_args(1);
            if *short != '\0' {
                arg = arg.short(*short);
            }
            cmd = cmd.arg(arg);
        }
        for (name, desc) in &node.args {
            let arg = clap::Arg::new(name.as_str())
                .help(desc.as_str())
                .num_args(1);
            cmd = cmd.arg(arg);
        }
        for sub in &node.subcommands {
            cmd = cmd.subcommand(sub.build_command());
        }
        cmd
    }

    /// Walk the declared positional-arg names in declaration order.
    /// Used by [`ParsedArgs::args`] to expose all positional values
    /// as an ordered `Vec<String>` (clap's `ArgMatches` does not
    /// expose positionals by index directly).
    fn collect_positional_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(node) = self.inner.lock() {
            for (name, _) in &node.args {
                out.push(name.clone());
            }
        }
        out
    }
}

impl std::fmt::Display for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self
            .inner
            .lock()
            .map(|n| n.name.clone())
            .unwrap_or_else(|_| "<poisoned>".to_string());
        write!(f, "App({name})")
    }
}

/// The parsed argv values. Constructed by [`App::parse`] (or
/// [`App::parse_or_exit`]). Getters are panic-free: missing values
/// return `false` (flags) or `None` (options, args).
///
/// `ParsedArgs` is NOT `Clone` — it is a one-shot result consumed by
/// the caller's dispatch logic. It is `Send + Sync` because
/// `clap::ArgMatches` is `Send + Sync`.
pub struct ParsedArgs {
    matches: clap::ArgMatches,
    positional_names: Vec<String>,
}

impl ParsedArgs {
    /// The name of the matched subcommand, if any. `None` when
    /// argv did not match any registered subcommand.
    pub fn subcommand(&self) -> Option<String> {
        self.matches.subcommand_name().map(|s| s.to_string())
    }

    /// The parsed args of the matched subcommand. Returns a fresh
    /// `ParsedArgs` whose getters operate on the subcommand's
    /// matches. If no subcommand was matched, returns an empty
    /// `ParsedArgs` (all getters yield defaults).
    pub fn subcommand_args(&self) -> ParsedArgs {
        match self.matches.subcommand() {
            Some((_, sub_matches)) => ParsedArgs {
                matches: sub_matches.clone(),
                positional_names: Vec::new(),
            },
            None => ParsedArgs {
                matches: clap::ArgMatches::default(),
                positional_names: Vec::new(),
            },
        }
    }

    /// Was the boolean flag `name` present on the command line?
    /// Returns `false` for unknown names (no panic).
    pub fn flag(&self, name: &str) -> bool {
        if self.matches.try_contains_id(name).unwrap_or(false) {
            self.matches.get_flag(name)
        } else {
            false
        }
    }

    /// The value of the option `name`. Returns `None` for unknown
    /// names or options that were not provided on the command line.
    pub fn option(&self, name: &str) -> Option<String> {
        if self.matches.try_contains_id(name).unwrap_or(false) {
            self.matches
                .get_one::<String>(name)
                .map(|s| s.to_string())
        } else {
            None
        }
    }

    /// The value of the positional arg `name` (by declared name).
    /// Returns `None` for unknown names or unset positionals.
    pub fn arg(&self, name: &str) -> Option<String> {
        if self.matches.try_contains_id(name).unwrap_or(false) {
            self.matches
                .get_one::<String>(name)
                .map(|s| s.to_string())
        } else {
            None
        }
    }

    /// All positional arg values in declaration order. Positionals
    /// that were not provided are skipped (the returned vector may
    /// be shorter than the number of declared positionals).
    pub fn args(&self) -> Vec<String> {
        let mut out = Vec::new();
        for name in &self.positional_names {
            if let Some(v) = self.arg(name) {
                out.push(v);
            }
        }
        out
    }
}

impl std::fmt::Debug for ParsedArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParsedArgs")
            .field("subcommand", &self.subcommand())
            .field("positionals", &self.positional_names)
            .finish()
    }
}

/// First character of `s`, or `'\0'` if empty. Used as the short-
/// flag character; the build_command code skips the short form when
/// the char is `'\0'`. Never panics.
fn first_char(s: &str) -> char {
    s.chars().next().unwrap_or('\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_char_handles_empty_string() {
        assert_eq!(first_char(""), '\0');
        assert_eq!(first_char("v"), 'v');
        assert_eq!(first_char("verbose"), 'v');
    }

    #[test]
    fn app_new_initializes_with_name() {
        let app = App::new("mytool".to_string());
        let node = app.inner.lock().expect("test lock");
        assert_eq!(node.name, "mytool");
        assert!(node.about.is_none());
        assert!(node.version.is_none());
        assert!(node.flags.is_empty());
    }

    #[test]
    fn app_builder_methods_chain() {
        let app = App::new("tool".to_string())
            .about("does stuff".to_string())
            .version("1.0.0".to_string())
            .flag("verbose".to_string(), "v".to_string(), "verbose".to_string());
        let node = app.inner.lock().expect("test lock");
        assert_eq!(node.about.as_deref(), Some("does stuff"));
        assert_eq!(node.version.as_deref(), Some("1.0.0"));
        assert_eq!(node.flags.len(), 1);
    }
}
