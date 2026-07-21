//! Integration tests for the `buff-cli` crate.
//!
//! Covers all 16 public functions per the T32 spec:
//! - App (10): new, about, version, flag, option, arg, command,
//!   parse, parse_or_exit (via separate-process only — skipped),
//!   help_text.
//! - ParsedArgs (6): subcommand, subcommand_args, flag, option, arg,
//!   args.
//!
//! Plus 3 insta snapshots (App debug, ParsedArgs debug, help_text).

use buff_cli::{App, CliError};

fn argv(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

#[test]
fn app_new_initializes() {
    let app = App::new("mytool".to_string());
    let display = format!("{app}");
    assert_eq!(display, "App(mytool)");
}

#[test]
fn app_flag_parses_long_form() {
    let app = App::new("tool".to_string())
        .flag("verbose".to_string(), "v".to_string(), "verbose".to_string());
    let parsed = app.parse(argv(&["tool", "--verbose"])).expect("parse");
    assert!(parsed.flag("verbose"));
    assert!(!parsed.flag("missing"));
}

#[test]
fn app_flag_parses_short_form() {
    let app = App::new("tool".to_string())
        .flag("verbose".to_string(), "v".to_string(), "verbose".to_string());
    let parsed = app.parse(argv(&["tool", "-v"])).expect("parse");
    assert!(parsed.flag("verbose"));
}

#[test]
fn app_flag_absent_returns_false() {
    let app = App::new("tool".to_string())
        .flag("verbose".to_string(), "v".to_string(), "verbose".to_string());
    let parsed = app.parse(argv(&["tool"])).expect("parse");
    assert!(!parsed.flag("verbose"));
}

#[test]
fn app_option_parses_value() {
    let app = App::new("tool".to_string()).option(
        "name".to_string(),
        "n".to_string(),
        "name".to_string(),
    );
    let parsed = app
        .parse(argv(&["tool", "--name", "alice"]))
        .expect("parse");
    assert_eq!(parsed.option("name"), Some("alice".to_string()));
}

#[test]
fn app_option_short_form() {
    let app = App::new("tool".to_string()).option(
        "name".to_string(),
        "n".to_string(),
        "name".to_string(),
    );
    let parsed = app.parse(argv(&["tool", "-n", "bob"])).expect("parse");
    assert_eq!(parsed.option("name"), Some("bob".to_string()));
}

#[test]
fn app_arg_positional() {
    let app = App::new("tool".to_string())
        .arg("path".to_string(), "path to file".to_string());
    let parsed = app.parse(argv(&["tool", "/tmp/x"])).expect("parse");
    assert_eq!(parsed.arg("path"), Some("/tmp/x".to_string()));
    assert_eq!(parsed.args(), vec!["/tmp/x".to_string()]);
}

#[test]
fn app_multiple_positionals_preserve_order() {
    let app = App::new("cp".to_string())
        .arg("src".to_string(), "source".to_string())
        .arg("dst".to_string(), "destination".to_string());
    let parsed = app
        .parse(argv(&["cp", "a.txt", "b.txt"]))
        .expect("parse");
    assert_eq!(parsed.arg("src"), Some("a.txt".to_string()));
    assert_eq!(parsed.arg("dst"), Some("b.txt".to_string()));
    assert_eq!(
        parsed.args(),
        vec!["a.txt".to_string(), "b.txt".to_string()]
    );
}

#[test]
fn app_subcommand_dispatch() {
    let app = App::new("multi".to_string()).about("demo".to_string());
    let greet = app.command("greet".to_string(), "say hi".to_string());
    greet.option(
        "name".to_string(),
        "n".to_string(),
        "name".to_string(),
    );

    let parsed = app.parse(argv(&["multi", "greet", "-n", "alice"])).expect("parse");
    assert_eq!(parsed.subcommand(), Some("greet".to_string()));
    let sub = parsed.subcommand_args();
    assert_eq!(sub.option("name"), Some("alice".to_string()));
}

#[test]
fn app_subcommand_none_when_not_matched() {
    let app = App::new("multi".to_string());
    let _ = app.command("greet".to_string(), "say hi".to_string());
    let parsed = app.parse(argv(&["multi"])).expect("parse");
    assert_eq!(parsed.subcommand(), None);
}

#[test]
fn app_help_text_contains_name_and_about() {
    let app = App::new("mytool".to_string())
        .about("does useful things".to_string())
        .flag("verbose".to_string(), "v".to_string(), "verbose mode".to_string());
    let help = app.help_text();
    assert!(help.contains("mytool"), "help should contain app name");
    assert!(
        help.contains("does useful things"),
        "help should contain about text"
    );
    assert!(help.contains("--verbose"), "help should list flag");
}

#[test]
fn app_unknown_flag_returns_parse_error() {
    let app = App::new("tool".to_string());
    let err = app.parse(argv(&["tool", "--bogus"])).unwrap_err();
    assert!(matches!(err, CliError::Parse(_)));
}

#[test]
fn app_unknown_flag_getter_returns_false_safely() {
    let app = App::new("tool".to_string());
    let parsed = app.parse(argv(&["tool"])).expect("parse");
    assert!(!parsed.flag("does-not-exist"));
    assert_eq!(parsed.option("does-not-exist"), None);
    assert_eq!(parsed.arg("does-not-exist"), None);
}

#[test]
fn app_version_flag_triggers_parse_error() {
    // clap exits on --version by default; we surface as Parse error.
    let app = App::new("tool".to_string()).version("1.2.3".to_string());
    let err = app.parse(argv(&["tool", "--version"])).unwrap_err();
    match err {
        CliError::Parse(msg) => assert!(msg.contains("1.2.3"), "version shown in error: {msg}"),
        other => panic!("expected Parse error, got {other:?}"),
    }
}

#[test]
fn app_command_returns_child_visible_to_parent() {
    // Mutations on the child App returned by command() are visible
    // to the parent's build_command — this is the core design invariant
    // of buff-cli (Arc<Mutex<Node>> shared via clone).
    let app = App::new("parent".to_string());
    let child = app.command("child".to_string(), "sub".to_string());
    child.flag("verbose".to_string(), "v".to_string(), "verbose".to_string());
    let parsed = app
        .parse(argv(&["parent", "child", "--verbose"]))
        .expect("parse");
    assert_eq!(parsed.subcommand(), Some("child".to_string()));
    let sub = parsed.subcommand_args();
    assert!(sub.flag("verbose"), "child flag should be parsed");
}

#[test]
fn app_empty_short_disables_short_form() {
    let app = App::new("tool".to_string()).flag(
        "verbose".to_string(),
        String::new(),
        "verbose".to_string(),
    );
    let parsed = app.parse(argv(&["tool", "--verbose"])).expect("parse");
    assert!(parsed.flag("verbose"));
}

// ---- Insta snapshots -------------------------------------------------------

#[test]
fn snapshot_app_display() {
    let app = App::new("snapshot-tool".to_string())
        .about("snapshot test".to_string())
        .version("0.1.0".to_string());
    insta::assert_snapshot!("app_display", format!("{app}"));
}

#[test]
fn snapshot_help_text_basic() {
    let app = App::new("snap".to_string())
        .about("snap about".to_string())
        .flag("verbose".to_string(), "v".to_string(), "verbose mode".to_string())
        .option(
            "name".to_string(),
            "n".to_string(),
            "set name".to_string(),
        )
        .arg("path".to_string(), "input path".to_string());
    insta::assert_snapshot!("help_text_basic", app.help_text());
}

#[test]
fn snapshot_parsed_args_debug() {
    let app = App::new("snap".to_string())
        .arg("first".to_string(), "first positional".to_string())
        .arg("second".to_string(), "second positional".to_string());
    let parsed = app
        .parse(argv(&["snap", "a", "b"]))
        .expect("parse");
    insta::assert_snapshot!("parsed_args_debug", format!("{parsed:?}"));
}
