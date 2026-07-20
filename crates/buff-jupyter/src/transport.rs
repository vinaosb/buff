//! ZMQ transport layer for the Jupyter kernel.
//!
//! Defines a small [`ZmqTransport`] trait that abstracts the multipart
//! send / recv surface the kernel needs. The kernel's message loop
//! ([`crate::kernel::Kernel::run`]) is generic over this trait so the
//! protocol logic is unit-testable without binding real sockets — a
//! mock transport in `tests/` can drive the dispatch loop end-to-end
//! without touching the network.
//!
//! ## Why a trait?
//!
//! Two reasons:
//!
//! 1. **Testability.** The protocol/HMAC/dispatch logic is the bulk of
//!    T129a's testable surface; the socket binding is glue. Putting
//!    the sockets behind a trait means unit tests can construct a
//!    mock transport and exercise the dispatch loop without a live
//!    ZMQ endpoint.
//! 2. **Future swap-out.** The pure-Rust `zeromq = "0.4"` crate is
//!    verified to build on this Windows host (see root `Cargo.toml`),
//!    but if a future bump pulls in a dep that breaks Windows, the
//!    kernel loop stays usable via a mock or alt transport — the
//!    trait is the swappable boundary.
//!
//! The default impl [`ZmqSocketSet`] binds the 5 canonical sockets via
//! `zeromq` (ROUTER × 3 for shell / stdin / control, PUB for iopub,
//! REP for heartbeat).

use std::sync::Arc;
use tokio::sync::Mutex;
use zeromq::{
    prelude::{Socket, SocketRecv, SocketSend},
    PubSocket, RepSocket, RouterSocket, ZmqMessage,
};

use crate::error::{JupyterError, JupyterResult};

/// A received multipart ZMQ message — a Vec of frames, each a `Vec<u8>`.
pub type Multipart = Vec<Vec<u8>>;
/// The transport-side abstraction the kernel's loop depends on.
///
/// Implementations:
/// - [`ZmqSocketSet`] (production): real `zeromq` sockets bound to
///   the connection file's endpoints.
/// - (tests may construct a mock transport — T129a ships only the
///   production impl here; the trait surface is what enables future
///   test scaffolding.)
#[allow(clippy::len_without_is_empty)]
pub trait ZmqTransport: Send {
    /// Block until a multipart message arrives on the SHELL socket.
    fn recv_shell(&mut self) -> impl std::future::Future<Output = JupyterResult<Multipart>> + Send;
    /// Send a multipart message on the SHELL socket (the routing
    /// identities are encoded as leading frames in `msg`).
    fn send_shell(
        &mut self,
        msg: Multipart,
    ) -> impl std::future::Future<Output = JupyterResult<()>> + Send;

    /// Send a multipart message on the IOPUB socket (no routing
    /// identities — PUB is broadcast).
    fn send_iopub(
        &mut self,
        msg: Multipart,
    ) -> impl std::future::Future<Output = JupyterResult<()>> + Send;

    /// Block until a multipart message arrives on the CONTROL socket.
    fn recv_control(
        &mut self,
    ) -> impl std::future::Future<Output = JupyterResult<Multipart>> + Send;
    /// Send a multipart message on the CONTROL socket.
    fn send_control(
        &mut self,
        msg: Multipart,
    ) -> impl std::future::Future<Output = JupyterResult<()>> + Send;

    /// Block until a multipart message arrives on the HEARTBEAT socket
    /// (a REP socket — reply with the same bytes).
    fn recv_hb(&mut self) -> impl std::future::Future<Output = JupyterResult<Multipart>> + Send;
    /// Send a multipart message on the HEARTBEAT socket (echo the
    /// received bytes).
    fn send_hb(
        &mut self,
        msg: Multipart,
    ) -> impl std::future::Future<Output = JupyterResult<()>> + Send;
}

/// The production transport — 5 real `zeromq` sockets bound to the
/// connection file's endpoints.
///
/// Constructed via [`ZmqSocketSet::bind`] which takes the parsed
/// [`ConnectionFile`](crate::ConnectionFile) and binds each socket to
/// its endpoint, returning once all 5 are listening.
///
/// All sockets are wrapped in a `tokio::sync::Mutex` because zeromq's
/// socket handles are NOT `Sync` (they own internal state that must
/// not be concurrently accessed). The kernel loop is single-threaded
/// per socket so the mutex never contends — but holding it across
/// `.await` would block another task trying to use the same socket.
/// This is fine because the kernel's loop is the only task that
/// touches a given socket at a time. Clippy's `await_holding_lock`
/// lint would fire only if we held a `std::sync::Mutex` guard across
/// an `.await`; we use `tokio::sync::Mutex` whose guard is `Send`.
pub struct ZmqSocketSet {
    shell: Arc<Mutex<RouterSocket>>,
    iopub: Arc<Mutex<PubSocket>>,
    control: Arc<Mutex<RouterSocket>>,
    hb: Arc<Mutex<RepSocket>>,
}

impl ZmqSocketSet {
    /// Bind all 5 sockets to the connection file's endpoints.
    ///
    /// stdin (the 4th ROUTER socket) is NOT bound in T129a — the
    /// kernel does not yet handle `input_request` (deferred to T129b
    /// when interactive `input()` lands). We still consume the
    /// `stdin_port` field from the connection file so the validation
    /// surface stays correct, but no socket binds to it.
    ///
    /// # Errors
    ///
    /// Returns [`JupyterError::Zmq`] if any socket fails to bind
    /// (port already in use, ip not available, etc.).
    pub async fn bind(conn: &crate::ConnectionFile) -> JupyterResult<Self> {
        let mut shell = RouterSocket::new();
        shell
            .bind(&conn.shell_endpoint())
            .await
            .map_err(|e| JupyterError::Zmq(format!("bind shell: {e}")))?;

        let mut iopub = PubSocket::new();
        iopub
            .bind(&conn.iopub_endpoint())
            .await
            .map_err(|e| JupyterError::Zmq(format!("bind iopub: {e}")))?;

        let mut control = RouterSocket::new();
        control
            .bind(&conn.control_endpoint())
            .await
            .map_err(|e| JupyterError::Zmq(format!("bind control: {e}")))?;

        let mut hb = RepSocket::new();
        hb.bind(&conn.hb_endpoint())
            .await
            .map_err(|e| JupyterError::Zmq(format!("bind hb: {e}")))?;

        Ok(Self {
            shell: Arc::new(Mutex::new(shell)),
            iopub: Arc::new(Mutex::new(iopub)),
            control: Arc::new(Mutex::new(control)),
            hb: Arc::new(Mutex::new(hb)),
        })
    }
}

/// Helper: convert a `zeromq::ZmqMessage` (multipart) into a
/// `Vec<Vec<u8>>`. Each frame becomes a `Vec<u8>`; the order is
/// preserved.
fn zmq_message_to_multipart(msg: ZmqMessage) -> Multipart {
    msg.into_vec().into_iter().map(|b| b.to_vec()).collect()
}

/// Helper: convert a `Vec<Vec<u8>>` into a `zeromq::ZmqMessage`.
///
/// Returns an error on empty input (ZMQ disallows empty multipart).
fn multipart_to_zmq_message(mp: Multipart) -> JupyterResult<ZmqMessage> {
    if mp.is_empty() {
        return Err(JupyterError::MalformedWire {
            expected: 1,
            actual: 0,
        });
    }
    let bytes_vec: Vec<bytes::Bytes> = mp.into_iter().map(bytes::Bytes::from).collect();
    ZmqMessage::try_from(bytes_vec).map_err(|e| JupyterError::Zmq(format!("empty message: {e}")))
}

impl ZmqTransport for ZmqSocketSet {
    async fn recv_shell(&mut self) -> JupyterResult<Multipart> {
        let mut guard = self.shell.lock().await;
        let msg = guard
            .recv()
            .await
            .map_err(|e| JupyterError::Zmq(format!("recv shell: {e}")))?;
        Ok(zmq_message_to_multipart(msg))
    }

    async fn send_shell(&mut self, msg: Multipart) -> JupyterResult<()> {
        let zmq = multipart_to_zmq_message(msg)?;
        let mut guard = self.shell.lock().await;
        guard
            .send(zmq)
            .await
            .map_err(|e| JupyterError::Zmq(format!("send shell: {e}")))
    }

    async fn send_iopub(&mut self, msg: Multipart) -> JupyterResult<()> {
        let zmq = multipart_to_zmq_message(msg)?;
        let mut guard = self.iopub.lock().await;
        guard
            .send(zmq)
            .await
            .map_err(|e| JupyterError::Zmq(format!("send iopub: {e}")))
    }

    async fn recv_control(&mut self) -> JupyterResult<Multipart> {
        let mut guard = self.control.lock().await;
        let msg = guard
            .recv()
            .await
            .map_err(|e| JupyterError::Zmq(format!("recv control: {e}")))?;
        Ok(zmq_message_to_multipart(msg))
    }

    async fn send_control(&mut self, msg: Multipart) -> JupyterResult<()> {
        let zmq = multipart_to_zmq_message(msg)?;
        let mut guard = self.control.lock().await;
        guard
            .send(zmq)
            .await
            .map_err(|e| JupyterError::Zmq(format!("send control: {e}")))
    }

    async fn recv_hb(&mut self) -> JupyterResult<Multipart> {
        let mut guard = self.hb.lock().await;
        let msg = guard
            .recv()
            .await
            .map_err(|e| JupyterError::Zmq(format!("recv hb: {e}")))?;
        Ok(zmq_message_to_multipart(msg))
    }

    async fn send_hb(&mut self, msg: Multipart) -> JupyterResult<()> {
        let zmq = multipart_to_zmq_message(msg)?;
        let mut guard = self.hb.lock().await;
        guard
            .send(zmq)
            .await
            .map_err(|e| JupyterError::Zmq(format!("send hb: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_to_zmq_message_round_trips() {
        let mp: Multipart = vec![b"a".to_vec(), b"bb".to_vec(), b"ccc".to_vec()];
        let zmq = multipart_to_zmq_message(mp.clone()).expect("to zmq");
        let back = zmq_message_to_multipart(zmq);
        assert_eq!(back, mp);
    }

    #[test]
    fn multipart_to_zmq_message_rejects_empty() {
        let mp: Multipart = vec![];
        let err = multipart_to_zmq_message(mp).unwrap_err();
        assert!(matches!(err, JupyterError::MalformedWire { .. }));
    }

    #[test]
    fn zmq_message_to_multipart_preserves_frame_order() {
        let mp: Multipart = vec![b"first".to_vec(), b"second".to_vec()];
        let zmq = multipart_to_zmq_message(mp.clone()).expect("convert");
        let back = zmq_message_to_multipart(zmq);
        assert_eq!(back[0], b"first");
        assert_eq!(back[1], b"second");
    }
}
