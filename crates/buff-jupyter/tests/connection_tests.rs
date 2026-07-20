//! Integration tests for the connection-file parser — exercises the
//! canonical Jupyter JSON shape, validation, and endpoint formatting.

use buff_jupyter::connection::ConnectionFile;
use buff_jupyter::error::JupyterError;

const CANONICAL_JSON: &str = r#"{
    "transport": "tcp",
    "ip": "127.0.0.1",
    "shell_port": 53718,
    "iopub_port": 53719,
    "stdin_port": 53720,
    "control_port": 53721,
    "hb_port": 53722,
    "signature_scheme": "hmac-sha256",
    "key": "a0123456-7890-abcd-ef01-234567890abc"
}"#;

#[test]
fn parses_canonical_jupyter_connection_file() {
    let cf = ConnectionFile::parse(CANONICAL_JSON).expect("parse");
    assert_eq!(cf.transport, "tcp");
    assert_eq!(cf.ip, "127.0.0.1");
    assert_eq!(cf.shell_port, 53718);
    assert_eq!(cf.iopub_port, 53719);
    assert_eq!(cf.stdin_port, 53720);
    assert_eq!(cf.control_port, 53721);
    assert_eq!(cf.hb_port, 53722);
    assert_eq!(cf.signature_scheme, "hmac-sha256");
    assert_eq!(cf.key, "a0123456-7890-abcd-ef01-234567890abc");
}

#[test]
fn round_trips_via_serde() {
    let cf = ConnectionFile::parse(CANONICAL_JSON).expect("parse");
    let json = serde_json::to_string(&cf).expect("serialize");
    let cf2: ConnectionFile = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(cf, cf2);
}

#[test]
fn rejects_malformed_json() {
    let err = ConnectionFile::parse("{ not valid json").unwrap_err();
    assert!(matches!(err, JupyterError::Json(_)));
}

#[test]
fn rejects_missing_required_field() {
    let err = ConnectionFile::parse(r#"{"transport":"tcp"}"#).unwrap_err();
    assert!(matches!(err, JupyterError::Json(_)));
}

#[test]
fn rejects_wrong_port_type() {
    let bad = r#"{
        "transport": "tcp", "ip": "127.0.0.1",
        "shell_port": "not-a-number",
        "iopub_port": 2, "stdin_port": 3,
        "control_port": 4, "hb_port": 5,
        "signature_scheme": "hmac-sha256", "key": "k"
    }"#;
    let err = ConnectionFile::parse(bad).unwrap_err();
    assert!(matches!(err, JupyterError::Json(_)));
}

#[test]
fn validate_accepts_tcp_hmac_sha256() {
    let cf = ConnectionFile::parse(CANONICAL_JSON).expect("parse");
    assert!(cf.validate().is_ok());
}

#[test]
fn validate_accepts_unsigned_when_scheme_and_key_both_empty() {
    let cf = ConnectionFile {
        transport: "tcp".to_string(),
        ip: "127.0.0.1".to_string(),
        shell_port: 1,
        iopub_port: 2,
        stdin_port: 3,
        control_port: 4,
        hb_port: 5,
        signature_scheme: String::new(),
        key: String::new(),
    };
    assert!(cf.validate().is_ok());
}

#[test]
fn validate_rejects_non_tcp_transport() {
    let mut cf = ConnectionFile::parse(CANONICAL_JSON).expect("parse");
    cf.transport = "ipc".to_string();
    let err = cf.validate().unwrap_err();
    assert!(matches!(
        err,
        JupyterError::UnsupportedConnectionValue { ref field, .. } if field == "transport"
    ));
}

#[test]
fn validate_rejects_unsupported_signature_scheme() {
    let mut cf = ConnectionFile::parse(CANONICAL_JSON).expect("parse");
    cf.signature_scheme = "hmac-sha512".to_string();
    let err = cf.validate().unwrap_err();
    assert!(matches!(
        err,
        JupyterError::UnsupportedConnectionValue { ref field, .. } if field == "signature_scheme"
    ));
}

#[test]
fn endpoint_formats_canonical_tcp_uri() {
    let cf = ConnectionFile::parse(CANONICAL_JSON).expect("parse");
    assert_eq!(cf.shell_endpoint(), "tcp://127.0.0.1:53718");
    assert_eq!(cf.iopub_endpoint(), "tcp://127.0.0.1:53719");
    assert_eq!(cf.stdin_endpoint(), "tcp://127.0.0.1:53720");
    assert_eq!(cf.control_endpoint(), "tcp://127.0.0.1:53721");
    assert_eq!(cf.hb_endpoint(), "tcp://127.0.0.1:53722");
}

#[test]
fn from_path_returns_typed_error_on_missing_file() {
    let path = std::path::Path::new("/nonexistent/path/that/should/not/exist.json");
    let err = ConnectionFile::from_path(path).unwrap_err();
    assert!(matches!(err, JupyterError::ConnectionFileRead { .. }));
}
