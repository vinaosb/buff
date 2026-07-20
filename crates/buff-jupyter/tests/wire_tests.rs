//! Integration tests for the Jupyter wire-message serde round-trip
//! and the `MessageHeader` reply construction logic.

use buff_jupyter::wire::{MessageHeader, WireMessage, PROTOCOL_VERSION};
use serde_json::Value;

const SAMPLE_HEADER_JSON: &str = r#"{
    "msg_id": "req-001",
    "session": "session-abc",
    "username": "tester",
    "date": "2026-07-20T12:00:00.000000Z",
    "msg_type": "kernel_info_request",
    "version": "5.3"
}"#;

#[test]
fn header_round_trips_through_serde() {
    let h: MessageHeader = serde_json::from_str(SAMPLE_HEADER_JSON).expect("parse");
    assert_eq!(h.msg_id, "req-001");
    assert_eq!(h.msg_type, "kernel_info_request");
    assert_eq!(h.version, PROTOCOL_VERSION);

    let serialized = serde_json::to_string(&h).expect("serialize");
    let h2: MessageHeader = serde_json::from_str(&serialized).expect("re-parse");
    assert_eq!(h, h2);
}

#[test]
fn header_required_json_keys_are_present() {
    let h: MessageHeader = serde_json::from_str(SAMPLE_HEADER_JSON).expect("parse");
    let v: Value = serde_json::to_value(&h).expect("to_value");
    let obj = v.as_object().expect("object");
    for key in [
        "msg_id", "session", "username", "date", "msg_type", "version",
    ] {
        assert!(obj.contains_key(key), "header missing required key {key}");
    }
}

#[test]
fn header_new_reply_inherits_session_and_username() {
    let parent: MessageHeader = serde_json::from_str(SAMPLE_HEADER_JSON).expect("parse");
    let reply = MessageHeader::new_reply(
        "kernel_info_reply",
        &parent,
        "2026-07-20T12:00:01.000000Z",
        "reply-id",
    );
    assert_eq!(reply.msg_type, "kernel_info_reply");
    assert_eq!(reply.session, parent.session);
    assert_eq!(reply.username, parent.username);
    assert_eq!(reply.msg_id, "reply-id");
    assert_eq!(reply.version, PROTOCOL_VERSION);
}

#[test]
fn wire_message_frames_for_signing_returns_four_frames() {
    let header: MessageHeader = serde_json::from_str(SAMPLE_HEADER_JSON).expect("parse");
    let msg = WireMessage {
        identities: vec![],
        header,
        parent_header: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: serde_json::json!({"foo": "bar"}),
    };
    let frames = msg.frames_for_signing().expect("frames");
    assert_eq!(frames.len(), 4);
    // Header is the first frame and must round-trip back to the
    // original header struct.
    let header_back: MessageHeader = serde_json::from_slice(&frames[0]).expect("header re-parse");
    assert_eq!(header_back, msg.header);
    // Content is the 4th frame and must round-trip back to the
    // original content value.
    let content_back: Value = serde_json::from_slice(&frames[3]).expect("content re-parse");
    assert_eq!(content_back, msg.content);
}

#[test]
fn wire_message_new_reply_constructs_well_formed_message() {
    let header: MessageHeader = serde_json::from_str(SAMPLE_HEADER_JSON).expect("parse");
    let parent = WireMessage {
        identities: vec![b"routing-id".to_vec()],
        header,
        parent_header: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: serde_json::json!({}),
    };
    let content = serde_json::json!({"protocol_version": "5.3"});
    let reply = WireMessage::new_reply(
        "kernel_info_reply",
        &parent,
        content,
        "2026-07-20T12:00:01.000000Z",
        "reply-id",
    );
    assert_eq!(reply.header.msg_type, "kernel_info_reply");
    assert_eq!(reply.identities, parent.identities);
    assert_eq!(reply.metadata, serde_json::json!({}));
}

#[test]
fn protocol_version_advertises_5_3() {
    assert_eq!(PROTOCOL_VERSION, "5.3");
}
