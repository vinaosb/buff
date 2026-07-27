//! CLI surface snapshot tests.
//!
//! Asserts that `--help` and `--version` output stay byte-stable.
//! Uses `insta` so changes surface as diffable `.snap.new` files
//! during `cargo insta review`.

use buffup::cli::{Cli, Command};
use clap::Parser;

#[test]
fn parses_install() {
    let cli = Cli::try_parse_from(["buffup", "install", "1.0.0"]).expect("parse install");
    match cli.command {
        Command::Install { version, .. } => assert_eq!(version, "1.0.0"),
        other => panic!("expected Install, got {other:?}"),
    }
}

#[test]
fn parses_default() {
    let cli = Cli::try_parse_from(["buffup", "default", "1.2.0"]).expect("parse default");
    match cli.command {
        Command::Default { version } => assert_eq!(version, "1.2.0"),
        other => panic!("expected Default, got {other:?}"),
    }
}

#[test]
fn parses_list() {
    let cli = Cli::try_parse_from(["buffup", "list"]).expect("parse list");
    assert!(matches!(cli.command, Command::List));
}

#[test]
fn parses_update() {
    let cli = Cli::try_parse_from(["buffup", "update"]).expect("parse update");
    assert!(matches!(cli.command, Command::Update));
}

#[test]
fn help_snapshot() {
    // clap renders `--help` as a ClapError of kind DisplayHelp (exit
    // code 0 semantically, but `try_parse_from` returns Err).
    let err = Cli::try_parse_from(["buffup", "--help"]).expect_err("--help returns Err");
    insta::assert_snapshot!("help", err.to_string());
}

#[test]
fn version_snapshot() {
    let err = Cli::try_parse_from(["buffup", "--version"]).expect_err("--version returns Err");
    insta::assert_snapshot!("version", err.to_string());
}

#[test]
fn no_subcommand_errors() {
    let err = Cli::try_parse_from(["buffup"]).expect_err("no subcommand returns Err");
    // Stable prefix: clap prints "error:" on parse failures (not on
    // help/version which print "buffup ..." to stdout-format).
    let msg = err.to_string();
    assert!(
        msg.contains("Usage") || msg.contains("usage"),
        "expected usage text in error, got: {msg}"
    );
}
