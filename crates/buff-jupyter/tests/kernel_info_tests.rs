//! Integration tests for the `kernel_info_reply` content struct —
//! verifies the Jupyter-mandated JSON shape + the Buff-specific
//! fields (implementation name, language_info).

use buff_jupyter::messages::{KernelInfoReply, LanguageInfo, BANNER, IMPLEMENTATION_NAME};
use buff_jupyter::wire::PROTOCOL_VERSION;
use serde_json::Value;

#[test]
fn kernel_info_reply_advertises_canonical_protocol_version() {
    let r = KernelInfoReply::buff();
    assert_eq!(r.protocol_version, "5.3");
    assert_eq!(r.protocol_version, PROTOCOL_VERSION);
}

#[test]
fn kernel_info_reply_implementation_name_is_buff() {
    let r = KernelInfoReply::buff();
    assert_eq!(r.implementation, IMPLEMENTATION_NAME);
    assert_eq!(r.implementation, "buff");
}

#[test]
fn kernel_info_reply_language_info_targets_buff() {
    let r = KernelInfoReply::buff();
    let li: &LanguageInfo = &r.language_info;
    assert_eq!(li.name, "buff");
    assert_eq!(li.file_extension, ".buff");
    assert_eq!(li.mimetype, "text/x-buff");
    assert!(li.pygments_lexer.as_deref().unwrap_or("").contains("buff"));
}

#[test]
fn kernel_info_reply_banner_mentions_t129a_scaffold_status() {
    let r = KernelInfoReply::buff();
    assert!(!r.banner.is_empty());
    assert_eq!(r.banner, BANNER);
    // Banner must honestly disclose that execution is NOT yet wired
    // up — Jupyter consoles print this verbatim on connect, so users
    // see it before they try `1+1` and see the stub reply.
    assert!(
        r.banner.to_lowercase().contains("t129a") || r.banner.to_lowercase().contains("scaffold"),
        "banner must mention scaffold status: {}",
        r.banner
    );
}

#[test]
fn kernel_info_reply_round_trips_serde() {
    let r = KernelInfoReply::buff();
    let json = serde_json::to_string(&r).expect("serialize");
    let r2: KernelInfoReply = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(r, r2);
}

#[test]
fn kernel_info_reply_has_all_required_json_fields() {
    // The Jupyter messaging spec mandates these top-level keys on
    // every kernel_info_reply content. Missing any causes jupyter_client
    // to raise a KeyError on connect.
    let r = KernelInfoReply::buff();
    let v: Value = serde_json::to_value(&r).expect("to_value");
    let obj = v.as_object().expect("object");
    for key in [
        "protocol_version",
        "implementation",
        "implementation_version",
        "language_info",
        "banner",
        "help_links",
    ] {
        assert!(obj.contains_key(key), "kernel_info_reply missing key {key}");
    }
    let li = obj["language_info"]
        .as_object()
        .expect("language_info object");
    for key in ["name", "version", "mimetype", "file_extension"] {
        assert!(li.contains_key(key), "language_info missing key {key}");
    }
}

#[test]
fn language_info_buff_constructor_returns_canonical_values() {
    let li = LanguageInfo::buff();
    assert_eq!(li.name, "buff");
    assert_eq!(li.file_extension, ".buff");
    assert_eq!(li.mimetype, "text/x-buff");
    assert!(li.pygments_lexer.is_some());
    assert!(li.codemirror_mode.is_some());
}
