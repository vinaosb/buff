//! Behavioral equivalence test: Rust original vs Buff port (buffhtml_parser.buff).
//!
//! Mirrors the stdout of
//! `crates/buff-lang-buffhtml-parser/selfhost/buffhtml_parser.buff` exactly.
//! Exercises every `BuffHtmlTokenKind` variant, `BuffHtmlToken`, and
//! `BuffHtmlParseError`.
//!
//! Run: `cargo run -p buff-lang-buffhtml-parser --example smoke_buffhtml --release`

use buff_lang_buffhtml_parser::{BuffHtmlParseError, BuffHtmlToken, BuffHtmlTokenKind};
use buff_lang_error::{SourceId, Span};

/// Stable numeric ID for every `BuffHtmlTokenKind` variant (matches the
/// buffhtml_parser.buff port's `buff_html_token_kind_num`). Numbering follows
/// the SOURCE-ORDER convention from the .buff port (NOT the Rust enum
/// declaration order — `AttrSpread` is grouped with attribute tokens at 12
/// in the .buff port but appears after `AwaitClose` in the Rust enum).
fn kind_num(kind: &BuffHtmlTokenKind) -> i64 {
    match kind {
        BuffHtmlTokenKind::Text(_) => 1,
        BuffHtmlTokenKind::OpenTagStart(_) => 2,
        BuffHtmlTokenKind::CloseTag(_) => 3,
        BuffHtmlTokenKind::TagEnd => 4,
        BuffHtmlTokenKind::TagSelfClose => 5,
        BuffHtmlTokenKind::FragmentOpen => 6,
        BuffHtmlTokenKind::FragmentClose => 7,
        BuffHtmlTokenKind::AttrName(_) => 8,
        BuffHtmlTokenKind::AttrEq => 9,
        BuffHtmlTokenKind::AttrColon => 10,
        BuffHtmlTokenKind::AttrStrLit(_) => 11,
        BuffHtmlTokenKind::AttrSpread(_) => 12,
        BuffHtmlTokenKind::Interp(_) => 13,
        BuffHtmlTokenKind::EachOpen { .. } => 14,
        BuffHtmlTokenKind::EachClose => 15,
        BuffHtmlTokenKind::IfOpen(_) => 16,
        BuffHtmlTokenKind::ElseIf(_) => 17,
        BuffHtmlTokenKind::Else => 18,
        BuffHtmlTokenKind::IfClose => 19,
        BuffHtmlTokenKind::HtmlComment(_) => 20,
        BuffHtmlTokenKind::BuffComment(_) => 21,
        BuffHtmlTokenKind::HtmlEscape(_) => 22,
        BuffHtmlTokenKind::AwaitOpen(_) => 23,
        BuffHtmlTokenKind::AwaitThen(_) => 24,
        BuffHtmlTokenKind::AwaitCatch(_) => 25,
        BuffHtmlTokenKind::AwaitClose => 26,
        BuffHtmlTokenKind::SlotOpen => 27,
        BuffHtmlTokenKind::ScriptOpen { .. } => 28,
        BuffHtmlTokenKind::ScriptText(_) => 29,
        BuffHtmlTokenKind::ScriptClose => 30,
        BuffHtmlTokenKind::Eof => 31,
    }
}

/// Short variant-stem label (matches the .buff port's `kind_label`).
fn kind_label(kind: &BuffHtmlTokenKind) -> &'static str {
    match kind {
        BuffHtmlTokenKind::Text(_) => "Text",
        BuffHtmlTokenKind::OpenTagStart(_) => "OpenTagStart",
        BuffHtmlTokenKind::CloseTag(_) => "CloseTag",
        BuffHtmlTokenKind::TagEnd => "TagEnd",
        BuffHtmlTokenKind::TagSelfClose => "TagSelfClose",
        BuffHtmlTokenKind::FragmentOpen => "FragmentOpen",
        BuffHtmlTokenKind::FragmentClose => "FragmentClose",
        BuffHtmlTokenKind::AttrName(_) => "AttrName",
        BuffHtmlTokenKind::AttrEq => "AttrEq",
        BuffHtmlTokenKind::AttrColon => "AttrColon",
        BuffHtmlTokenKind::AttrStrLit(_) => "AttrStrLit",
        BuffHtmlTokenKind::AttrSpread(_) => "AttrSpread",
        BuffHtmlTokenKind::Interp(_) => "Interp",
        BuffHtmlTokenKind::EachOpen { .. } => "EachOpen",
        BuffHtmlTokenKind::EachClose => "EachClose",
        BuffHtmlTokenKind::IfOpen(_) => "IfOpen",
        BuffHtmlTokenKind::ElseIf(_) => "ElseIf",
        BuffHtmlTokenKind::Else => "Else",
        BuffHtmlTokenKind::IfClose => "IfClose",
        BuffHtmlTokenKind::HtmlComment(_) => "HtmlComment",
        BuffHtmlTokenKind::BuffComment(_) => "BuffComment",
        BuffHtmlTokenKind::HtmlEscape(_) => "HtmlEscape",
        BuffHtmlTokenKind::AwaitOpen(_) => "AwaitOpen",
        BuffHtmlTokenKind::AwaitThen(_) => "AwaitThen",
        BuffHtmlTokenKind::AwaitCatch(_) => "AwaitCatch",
        BuffHtmlTokenKind::AwaitClose => "AwaitClose",
        BuffHtmlTokenKind::SlotOpen => "SlotOpen",
        BuffHtmlTokenKind::ScriptOpen { .. } => "ScriptOpen",
        BuffHtmlTokenKind::ScriptText(_) => "ScriptText",
        BuffHtmlTokenKind::ScriptClose => "ScriptClose",
        BuffHtmlTokenKind::Eof => "Eof",
    }
}

/// Extract the human-readable message from a `BuffHtmlParseError`.
fn error_message(err: &BuffHtmlParseError) -> String {
    match err {
        BuffHtmlParseError::Lex { message, .. } => message.clone(),
        BuffHtmlParseError::Parse { message, .. } => message.clone(),
    }
}

/// Combined num + label print (the .buff port routes through `print_kind_info`
/// to sidestep move-on-call semantics; in Rust we borrow).
fn print_kind_info(kind: &BuffHtmlTokenKind) {
    println!("{}", kind_num(kind));
    println!("{}", kind_label(kind));
}

/// Combined accessor print for a `BuffHtmlToken` (num + label).
fn print_token_info(token: &BuffHtmlToken) {
    println!("{}", kind_num(&token.kind));
    println!("{}", kind_label(&token.kind));
}

/// Print `Some(s)` or `0` for `None` (matches the .buff port's
/// `print_opt_string`).
fn print_opt_string(opt: &Option<String>) {
    match opt {
        Some(s) => println!("{}", s),
        None => println!("0"),
    }
}

/// Round-trip the four fields of an `EachOpen` token.
fn print_each_open_slots(
    iterable: &str,
    binding: &str,
    index: &Option<String>,
    key: &Option<String>,
) {
    println!("{}", iterable);
    println!("{}", binding);
    print_opt_string(index);
    print_opt_string(key);
}

/// Round-trip the two fields of a `ScriptOpen` token.
fn print_script_open_slots(lang: &str, props: &Option<String>) {
    println!("{}", lang);
    print_opt_string(props);
}

/// Round-trip (message, span.start) of an error.
fn print_error_slots(message: &str, span: Span) {
    println!("{}", message);
    println!("{}", span.start);
}

fn main() {
    // --- Span stand-in ---
    let sp = Span::new(10, 42, SourceId(7));
    println!("{}", sp.start);
    println!("{}", sp.end);
    println!("{}", sp.source_id.0);

    let dummy = Span::dummy();
    println!("{}", dummy.start);
    println!("{}", dummy.end);
    println!("{}", dummy.source_id.0);

    // --- BuffHtmlParseError: Lex variant ---
    let lex_err = BuffHtmlParseError::lex(
        "unterminated interpolation, missing close brace",
        sp,
    );
    println!("{}", error_message(&lex_err));

    // --- BuffHtmlParseError: Parse variant ---
    let parse_err = BuffHtmlParseError::parse("expected close div, got close span", sp);
    println!("{}", error_message(&parse_err));

    // --- BuffHtmlTokenKind: every variant exercised via print_kind_info ---
    print_kind_info(&BuffHtmlTokenKind::Text("hello world".to_string()));
    print_kind_info(&BuffHtmlTokenKind::OpenTagStart("div".to_string()));
    print_kind_info(&BuffHtmlTokenKind::CloseTag("div".to_string()));
    print_kind_info(&BuffHtmlTokenKind::TagEnd);
    print_kind_info(&BuffHtmlTokenKind::TagSelfClose);
    print_kind_info(&BuffHtmlTokenKind::FragmentOpen);
    print_kind_info(&BuffHtmlTokenKind::FragmentClose);
    print_kind_info(&BuffHtmlTokenKind::AttrName("on:click".to_string()));
    print_kind_info(&BuffHtmlTokenKind::AttrEq);
    print_kind_info(&BuffHtmlTokenKind::AttrColon);
    print_kind_info(&BuffHtmlTokenKind::AttrStrLit("card".to_string()));
    print_kind_info(&BuffHtmlTokenKind::AttrSpread("rest".to_string()));
    print_kind_info(&BuffHtmlTokenKind::Interp("count + 1".to_string()));
    print_kind_info(&BuffHtmlTokenKind::EachOpen {
        iterable: "items.read()".to_string(),
        binding: "item".to_string(),
        index: Some("i".to_string()),
        key: Some("item.id".to_string()),
    });
    print_kind_info(&BuffHtmlTokenKind::EachOpen {
        iterable: "items".to_string(),
        binding: "item".to_string(),
        index: None,
        key: None,
    });
    print_kind_info(&BuffHtmlTokenKind::EachClose);
    print_kind_info(&BuffHtmlTokenKind::IfOpen("count > 0".to_string()));
    print_kind_info(&BuffHtmlTokenKind::ElseIf("count == 0".to_string()));
    print_kind_info(&BuffHtmlTokenKind::Else);
    print_kind_info(&BuffHtmlTokenKind::IfClose);
    print_kind_info(&BuffHtmlTokenKind::HtmlComment("hi".to_string()));
    print_kind_info(&BuffHtmlTokenKind::BuffComment("this is a comment".to_string()));
    print_kind_info(&BuffHtmlTokenKind::HtmlEscape("raw_trusted_html".to_string()));
    print_kind_info(&BuffHtmlTokenKind::AwaitOpen("fetchUser(id)".to_string()));
    print_kind_info(&BuffHtmlTokenKind::AwaitThen("user".to_string()));
    print_kind_info(&BuffHtmlTokenKind::AwaitCatch("err".to_string()));
    print_kind_info(&BuffHtmlTokenKind::AwaitClose);
    print_kind_info(&BuffHtmlTokenKind::SlotOpen);
    print_kind_info(&BuffHtmlTokenKind::ScriptOpen {
        lang: "buff".to_string(),
        props: Some("CounterProps".to_string()),
    });
    print_kind_info(&BuffHtmlTokenKind::ScriptOpen {
        lang: "buff".to_string(),
        props: None,
    });
    print_kind_info(&BuffHtmlTokenKind::ScriptText("print(\"hi\")".to_string()));
    print_kind_info(&BuffHtmlTokenKind::ScriptClose);
    print_kind_info(&BuffHtmlTokenKind::Eof);

    // --- Wrap a few kinds in BuffHtmlToken, exercise accessors ---
    let tok_text = BuffHtmlToken::new(
        BuffHtmlTokenKind::Text("hello world".to_string()),
        sp,
    );
    print_token_info(&tok_text);
    println!("{}", sp.start);
    println!("{}", sp.end);

    let tok_eof = BuffHtmlToken::new(BuffHtmlTokenKind::Eof, sp);
    print_token_info(&tok_eof);

    let tok_each = BuffHtmlToken::new(
        BuffHtmlTokenKind::EachOpen {
            iterable: "items.read()".to_string(),
            binding: "item".to_string(),
            index: Some("i".to_string()),
            key: Some("item.id".to_string()),
        },
        sp,
    );
    print_token_info(&tok_each);

    // --- Field destructuring of multi-payload variants ---
    // EachOpen full form (iterable, binding, Some(index), Some(key))
    let eo_full = BuffHtmlTokenKind::EachOpen {
        iterable: "items.read()".to_string(),
        binding: "item".to_string(),
        index: Some("i".to_string()),
        key: Some("item.id".to_string()),
    };
    if let BuffHtmlTokenKind::EachOpen {
        iterable,
        binding,
        index,
        key,
    } = &eo_full
    {
        print_each_open_slots(iterable, binding, index, key);
    }

    // EachOpen minimal form: index and key are None
    let eo_min = BuffHtmlTokenKind::EachOpen {
        iterable: "items".to_string(),
        binding: "item".to_string(),
        index: None,
        key: None,
    };
    if let BuffHtmlTokenKind::EachOpen {
        iterable,
        binding,
        index,
        key,
    } = &eo_min
    {
        print_each_open_slots(iterable, binding, index, key);
    }

    // ScriptOpen with props
    let so_full = BuffHtmlTokenKind::ScriptOpen {
        lang: "buff".to_string(),
        props: Some("CounterProps".to_string()),
    };
    if let BuffHtmlTokenKind::ScriptOpen { lang, props } = &so_full {
        print_script_open_slots(lang, props);
    }

    // ScriptOpen without props
    let so_min = BuffHtmlTokenKind::ScriptOpen {
        lang: "buff".to_string(),
        props: None,
    };
    if let BuffHtmlTokenKind::ScriptOpen { lang, props } = &so_min {
        print_script_open_slots(lang, props);
    }

    // --- BuffHtmlParseError round-trip via match ---
    let lex_err2 = BuffHtmlParseError::lex("lex boom", sp);
    match &lex_err2 {
        BuffHtmlParseError::Lex { message, span } => print_error_slots(message, *span),
        BuffHtmlParseError::Parse { message, span } => print_error_slots(message, *span),
    }

    let parse_err2 = BuffHtmlParseError::parse("parse boom", sp);
    match &parse_err2 {
        BuffHtmlParseError::Lex { message, span } => print_error_slots(message, *span),
        BuffHtmlParseError::Parse { message, span } => print_error_slots(message, *span),
    }

    // --- Remaining accessors ---
    println!(
        "{}",
        BuffHtmlParseError::lex("x", sp).span().start
    );
    println!(
        "{}",
        matches!(
            BuffHtmlParseError::lex("x", sp),
            BuffHtmlParseError::Lex { .. }
        )
    );
    println!(
        "{}",
        matches!(
            BuffHtmlParseError::lex("x", sp),
            BuffHtmlParseError::Parse { .. }
        )
    );
    println!(
        "{}",
        matches!(
            BuffHtmlParseError::parse("x", sp),
            BuffHtmlParseError::Lex { .. }
        )
    );
    println!(
        "{}",
        matches!(
            BuffHtmlParseError::parse("x", sp),
            BuffHtmlParseError::Parse { .. }
        )
    );

    // --- kind_label accessor (single payload-carrying variant) ---
    println!(
        "{}",
        kind_label(&BuffHtmlTokenKind::Text("payload".to_string()))
    );
    println!("{}", kind_label(&BuffHtmlTokenKind::Eof));

    // --- BuffHtmlToken span accessor ---
    println!(
        "{}",
        BuffHtmlToken::new(BuffHtmlTokenKind::Eof, sp).span.end
    );
}
