//! Connection-file parsing — the JSON document Jupyter writes to disk
//! per kernel launch to communicate the transport, ports, and signing
//! key to the kernel process.
//!
//! Per the Jupyter client spec, the connection file shape is:
//!
//! ```json,ignore
//! {
//!   "transport": "tcp",
//!   "ip": "127.0.0.1",
//!   "shell_port": 53718,
//!   "iopub_port": 53719,
//!   "stdin_port": 53720,
//!   "control_port": 53721,
//!   "hb_port": 53722,
//!   "signature_scheme": "hmac-sha256",
//!   "key": "a0123456-7890-abcd-ef01-234567890abc"
//! }
//! ```
//!
//! The kernel reads this file on boot (path passed via
//! `--connection-file`), binds the 5 ZMQ sockets to the listed ports,
//! and uses `key` + `signature_scheme` to sign / verify every wire
//! message. T129a supports TCP transport and `hmac-sha256` only —
//! inproc / ipc / pgm transports and other signature schemes surface
//! [`JupyterError::UnsupportedConnectionValue`](crate::JupyterError::UnsupportedConnectionValue).

use serde::{Deserialize, Serialize};

use crate::error::{JupyterError, JupyterResult};

/// The connection-file JSON shape written by Jupyter and read by the
/// kernel on boot.
///
/// All fields are required (no `Option<...>`) because Jupyter always
/// writes a complete file — a missing field means the file was hand-
/// edited incorrectly and the kernel should refuse to boot rather than
/// silently defaulting to a wrong port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionFile {
    /// Transport: `"tcp"` (T129a-supported), `"ipc"`, `"inproc"`, etc.
    /// Only `"tcp"` is wired up in this scaffold.
    pub transport: String,
    /// Bind IP. `"127.0.0.1"` (loopback, default) or `"*"` / `"0.0.0.0"`.
    pub ip: String,
    /// Shell ROUTER socket port.
    pub shell_port: u16,
    /// IOPub PUB socket port.
    pub iopub_port: u16,
    /// Stdin ROUTER socket port.
    pub stdin_port: u16,
    /// Control ROUTER socket port.
    pub control_port: u16,
    /// Heartbeat REP socket port.
    pub hb_port: u16,
    /// Signature scheme: `"hmac-sha256"` (T129a-supported) or empty
    /// string for unsigned. Other schemes (`hmac-sha512`, etc.) are
    /// rejected.
    pub signature_scheme: String,
    /// The HMAC signing key (a UUID-style string in Jupyter's default
    /// launcher). May be empty when `signature_scheme` is empty.
    pub key: String,
}

/// The default signature scheme used by Jupyter (and the only scheme
/// T129a wires up signing for). Mirrors `jupyter_client.connect`.
pub const DEFAULT_SIGNATURE_SCHEME: &str = "hmac-sha256";

impl ConnectionFile {
    /// Parse a connection-file JSON document.
    ///
    /// Does NOT validate the transport / scheme — callers that need
    /// runtime validation (the kernel loop) call [`Self::validate`]
    /// after parse. Tests can parse a known-bad fixture without the
    /// validator tripping.
    ///
    /// # Errors
    ///
    /// Returns [`JupyterError::FrameDeserialize`] wrapped via
    /// `serde_json::Error` if the JSON is malformed OR a required
    /// field is missing / wrong type.
    pub fn parse(json: &str) -> JupyterResult<Self> {
        let parsed: Self = serde_json::from_str(json)?;
        Ok(parsed)
    }

    /// Read + parse a connection file from disk.
    ///
    /// # Errors
    ///
    /// Returns [`JupyterError::ConnectionFileRead`] on read / parse
    /// failure. The path string is preserved in the error for
    /// user-facing diagnostics.
    pub fn from_path(path: &std::path::Path) -> JupyterResult<Self> {
        let path_str = path.display().to_string();
        let bytes = std::fs::read(path).map_err(|e| JupyterError::ConnectionFileRead {
            path: path_str.clone(),
            message: e.to_string(),
        })?;
        let json = String::from_utf8(bytes).map_err(|e| JupyterError::ConnectionFileRead {
            path: path_str.clone(),
            message: format!("file is not valid UTF-8: {e}"),
        })?;
        Self::parse(&json).map_err(|e| JupyterError::ConnectionFileRead {
            path: path_str,
            message: e.to_string(),
        })
    }

    /// Validate that the transport + signature scheme are supported by
    /// this kernel build.
    ///
    /// T129a supports:
    /// - `transport == "tcp"`
    /// - `signature_scheme` is `"hmac-sha256"` OR empty (unsigned)
    ///
    /// Anything else returns
    /// [`JupyterError::UnsupportedConnectionValue`].
    ///
    /// # Errors
    ///
    /// See above.
    pub fn validate(&self) -> JupyterResult<()> {
        if self.transport != "tcp" {
            return Err(JupyterError::UnsupportedConnectionValue {
                field: "transport".to_string(),
                value: self.transport.clone(),
            });
        }
        if !self.signature_scheme.is_empty() && self.signature_scheme != DEFAULT_SIGNATURE_SCHEME {
            return Err(JupyterError::UnsupportedConnectionValue {
                field: "signature_scheme".to_string(),
                value: self.signature_scheme.clone(),
            });
        }
        Ok(())
    }

    /// Build the ZMQ endpoint string for a port, e.g. `tcp://127.0.0.1:53718`.
    #[must_use]
    pub fn endpoint(&self, port: u16) -> String {
        format!("{}://{}:{}", self.transport, self.ip, port)
    }

    /// Convenience: shell socket endpoint.
    #[must_use]
    pub fn shell_endpoint(&self) -> String {
        self.endpoint(self.shell_port)
    }
    /// Convenience: iopub socket endpoint.
    #[must_use]
    pub fn iopub_endpoint(&self) -> String {
        self.endpoint(self.iopub_port)
    }
    /// Convenience: stdin socket endpoint.
    #[must_use]
    pub fn stdin_endpoint(&self) -> String {
        self.endpoint(self.stdin_port)
    }
    /// Convenience: control socket endpoint.
    #[must_use]
    pub fn control_endpoint(&self) -> String {
        self.endpoint(self.control_port)
    }
    /// Convenience: heartbeat socket endpoint.
    #[must_use]
    pub fn hb_endpoint(&self) -> String {
        self.endpoint(self.hb_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_JSON: &str = r#"{
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
    fn parse_canonical_connection_file() {
        let cf = ConnectionFile::parse(SAMPLE_JSON).expect("parse sample");
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
    fn parse_rejects_missing_field() {
        let broken = r#"{ "transport": "tcp" }"#;
        let err = ConnectionFile::parse(broken).unwrap_err();
        assert!(
            matches!(err, JupyterError::Json(_)),
            "expected Json error, got {err:?}"
        );
    }

    #[test]
    fn validate_accepts_tcp_hmac_sha256() {
        let cf = ConnectionFile::parse(SAMPLE_JSON).expect("parse");
        assert!(cf.validate().is_ok());
    }

    #[test]
    fn validate_accepts_unsigned_empty_scheme() {
        let json = r#"{
            "transport": "tcp",
            "ip": "127.0.0.1",
            "shell_port": 1,
            "iopub_port": 2,
            "stdin_port": 3,
            "control_port": 4,
            "hb_port": 5,
            "signature_scheme": "",
            "key": ""
        }"#;
        let cf = ConnectionFile::parse(json).expect("parse");
        assert!(cf.validate().is_ok());
    }

    #[test]
    fn validate_rejects_non_tcp_transport() {
        let cf = ConnectionFile {
            transport: "ipc".to_string(),
            ip: "127.0.0.1".to_string(),
            shell_port: 1,
            iopub_port: 2,
            stdin_port: 3,
            control_port: 4,
            hb_port: 5,
            signature_scheme: "hmac-sha256".to_string(),
            key: "k".to_string(),
        };
        let err = cf.validate().unwrap_err();
        assert!(
            matches!(err, JupyterError::UnsupportedConnectionValue { ref field, .. } if field == "transport"),
            "expected transport rejection, got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_unknown_signature_scheme() {
        let cf = ConnectionFile {
            transport: "tcp".to_string(),
            ip: "127.0.0.1".to_string(),
            shell_port: 1,
            iopub_port: 2,
            stdin_port: 3,
            control_port: 4,
            hb_port: 5,
            signature_scheme: "hmac-sha512".to_string(),
            key: "k".to_string(),
        };
        let err = cf.validate().unwrap_err();
        assert!(
            matches!(err, JupyterError::UnsupportedConnectionValue { ref field, .. } if field == "signature_scheme"),
            "expected scheme rejection, got {err:?}"
        );
    }

    #[test]
    fn endpoint_formats_canonical_string() {
        let cf = ConnectionFile::parse(SAMPLE_JSON).expect("parse");
        assert_eq!(cf.shell_endpoint(), "tcp://127.0.0.1:53718");
        assert_eq!(cf.iopub_endpoint(), "tcp://127.0.0.1:53719");
        assert_eq!(cf.stdin_endpoint(), "tcp://127.0.0.1:53720");
        assert_eq!(cf.control_endpoint(), "tcp://127.0.0.1:53721");
        assert_eq!(cf.hb_endpoint(), "tcp://127.0.0.1:53722");
    }
}
