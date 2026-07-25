//! Message builders - extracted from `kernel.rs` (T106 mechanical split).
//!
//! The build_* family of WireMessage constructors for Jupyter protocol replies.

use super::*;

impl<T: ZmqTransport + Unpin> Kernel<T> {
    /// Build a `kernel_info_reply` WireMessage in response to a
    /// `kernel_info_request`.
    pub(super) fn build_kernel_info_reply(
        &self,
        parent: &WireMessage,
    ) -> JupyterResult<WireMessage> {
        let content = serde_json::to_value(KernelInfoReply::buff())?;
        Ok(WireMessage::new_reply(
            "kernel_info_reply",
            parent,
            content,
            &now_iso(),
            &self.fresh_msg_id(),
        ))
    }

    /// Build an `execute_reply` WireMessage with status=ok.
    pub(super) fn build_execute_reply_ok(
        &self,
        parent: &WireMessage,
        execution_count: u64,
    ) -> JupyterResult<WireMessage> {
        let content = serde_json::to_value(ExecuteReply::ok(execution_count))?;
        Ok(WireMessage::new_reply(
            "execute_reply",
            parent,
            content,
            &now_iso(),
            &self.fresh_msg_id(),
        ))
    }

    /// Build an `execute_reply` WireMessage with status=error
    /// carrying the same ename/evalue/traceback as the iopub `error`.
    pub(super) fn build_execute_reply_error(
        &self,
        parent: &WireMessage,
        execution_count: u64,
        evalue: &str,
        traceback: Vec<String>,
    ) -> JupyterResult<WireMessage> {
        let content = serde_json::to_value(ExecuteReply::error(
            execution_count,
            EXEC_ERROR_ENAME,
            evalue,
            traceback,
        ))?;
        Ok(WireMessage::new_reply(
            "execute_reply",
            parent,
            content,
            &now_iso(),
            &self.fresh_msg_id(),
        ))
    }

    /// Build an iopub `execute_result` WireMessage carrying a single
    /// `text/plain` MIME entry for the bare-expression value.
    pub(super) fn build_execute_result(
        &self,
        parent: &WireMessage,
        execution_count: u64,
        value: &str,
    ) -> JupyterResult<WireMessage> {
        let content = serde_json::to_value(ExecuteResult::text(execution_count, value))?;
        Ok(WireMessage::new_reply(
            "execute_result",
            parent,
            content,
            &now_iso(),
            &self.fresh_msg_id(),
        ))
    }

    /// T129c: build an iopub `execute_result` WireMessage carrying a
    /// rich-display MIME bundle — both `text/html` and `text/plain`.
    ///
    /// Used for Vector/Matrix values where the HTML representation is
    /// an `<table>` and the plain-text fallback is the source literal
    /// (e.g. `[1, 2, 3]`). Mirrors [`build_execute_result`] in shape
    /// — only the content payload differs.
    pub(super) fn build_rich_execute_result(
        &self,
        parent: &WireMessage,
        execution_count: u64,
        html: &str,
        plain: &str,
    ) -> JupyterResult<WireMessage> {
        let content =
            serde_json::to_value(ExecuteResult::html_with_plain(execution_count, html, plain))?;
        Ok(WireMessage::new_reply(
            "execute_result",
            parent,
            content,
            &now_iso(),
            &self.fresh_msg_id(),
        ))
    }

    /// Build an iopub `stream` WireMessage (stdout or stderr).
    pub(super) fn build_stream_message(
        &self,
        parent: &WireMessage,
        stream: StreamOutput,
    ) -> JupyterResult<WireMessage> {
        let content = serde_json::to_value(stream)?;
        Ok(WireMessage::new_reply(
            "stream",
            parent,
            content,
            &now_iso(),
            &self.fresh_msg_id(),
        ))
    }

    /// Build an iopub `error` WireMessage carrying ename/evalue/traceback.
    pub(super) fn build_error_message(
        &self,
        parent: &WireMessage,
        evalue: &str,
        traceback: Vec<String>,
    ) -> JupyterResult<WireMessage> {
        let content = serde_json::to_value(ErrorOutput::new(EXEC_ERROR_ENAME, evalue, traceback))?;
        Ok(WireMessage::new_reply(
            "error",
            parent,
            content,
            &now_iso(),
            &self.fresh_msg_id(),
        ))
    }

    /// Build an iopub `status` WireMessage (`busy` or `idle`).
    pub(super) fn build_status_message(
        &self,
        parent: &WireMessage,
        state: &str,
    ) -> JupyterResult<WireMessage> {
        let content = serde_json::json!({
            "execution_state": state,
        });
        Ok(WireMessage::new_reply(
            "status",
            parent,
            content,
            &now_iso(),
            &self.fresh_msg_id(),
        ))
    }

    /// Build a `shutdown_reply` WireMessage.
    pub(super) fn build_shutdown_reply(
        &self,
        parent: &WireMessage,
        restart: bool,
    ) -> JupyterResult<WireMessage> {
        let content = serde_json::to_value(ShutdownReply::ok(restart))?;
        Ok(WireMessage::new_reply(
            "shutdown_reply",
            parent,
            content,
            &now_iso(),
            &self.fresh_msg_id(),
        ))
    }

    /// Build an `interrupt_reply` WireMessage (T129a acknowledges but
    /// does not honor the interrupt).
    pub(super) fn build_interrupt_reply(&self, parent: &WireMessage) -> JupyterResult<WireMessage> {
        let content = serde_json::json!({ "status": "ok" });
        Ok(WireMessage::new_reply(
            "interrupt_reply",
            parent,
            content,
            &now_iso(),
            &self.fresh_msg_id(),
        ))
    }
}
