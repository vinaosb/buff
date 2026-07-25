//! Execution handler - extracted from `kernel.rs` (T106 mechanical split).
//!
//! handle_execute_request (T129b/T129c) and handle_introspection
//! (?/?? magic). Per-request execution arms dispatched by Kernel::run.

use super::*;

impl<T: ZmqTransport + Unpin> Kernel<T> {
    /// T129b/T129c: handle an `execute_request` end-to-end.
    ///
    /// Sequence:
    /// 1. Increment `execution_count`.
    /// 2. iopub `status` busy.
    /// 3. Extract `code` from request content.
    /// 4. T129c: detect `?name` / `??name` introspection magic. If
    ///    matched, emit a single `execute_result` text/plain with the
    ///    name's type (and value for `??`) — skip normal evaluation.
    /// 5. T129c: detect Vector/Matrix literal (`[...]`-shaped
    ///    bare expression whose resolved type is `Vector<_>` /
    ///    `Matrix<_>`). If matched, emit `execute_result` with
    ///    `text/html` + `text/plain` MIME bundle — skip normal
    ///    evaluation (Buff's `print(vec)` fails to compile today
    ///    because `Vec<T>` lacks `Display`; rendering from source
    ///    avoids spawning `rustc` for a known-broken path).
    /// 6. Otherwise: evaluate via `Evaluator::eval_line` (blocking —
    ///    spawns rustc + the compiled program). On success with a
    ///    Vector/Matrix value, emit a MIME bundle (text/html +
    ///    text/plain) using the captured value. Emit iopub outputs
    ///    based on [`EvalResult`]:
    ///    - On diagnostic: iopub `error` (ename/evalue/traceback).
    ///    - Else: iopub `stream` (stdout if value is None, stderr
    ///      always when non-empty) and `execute_result` if value
    ///      is Some.
    /// 7. iopub `status` idle.
    /// 8. shell `execute_reply` (status ok OR error).
    ///
    /// The kernel NEVER returns an error from this method — even when
    /// the cell surfaces a diagnostic, the reply carries the error
    /// shape and the loop continues serving subsequent cells.
    pub(super) async fn handle_execute_request(
        &mut self,
        parsed: &WireMessage,
    ) -> JupyterResult<()> {
        let execution_count = {
            let mut g = self.execution_count.lock().await;
            *g += 1;
            *g
        };

        // iopub: status busy.
        let busy = self.build_status_message(parsed, "busy")?;
        self.send_iopub(&busy).await?;

        // Extract the cell source from the request content. Jupyter
        // sends `code` as a string; missing / wrong-type → empty
        // string (the evaluator classifies empty input as a no-op).
        let code_raw = parsed
            .content
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // Normalize: ensure the source ends with `\n`. Buff's offside-
        // rule lexer emits Indent/Dedent tokens based on line
        // structure; a multi-line block input (`func f():\n    ...`)
        // WITHOUT a trailing newline won't emit the final Dedent,
        // causing the parser to reject the block. Single-line inputs
        // are unaffected (the extra `\n` is a no-op for expressions
        // and simple statements). Mirrors buff-repl's `handle_action`.
        let mut code = code_raw.to_string();
        if !code.ends_with('\n') {
            code.push('\n');
        }

        // T129c: pre-compute the resolved type of the trimmed code.
        // Used for both introspection (skip), rich-display detection
        // (Vector/Matrix), and is cheap (type inference only — no
        // rustc spawn).
        let expr_src = code.trim();
        let pre_type = self.evaluator.type_of(expr_src);

        // Branch order: introspection magic → Vector/Matrix literal
        // rich display → normal eval. Each branch builds the reply
        // WireMessage (and emits its own iopub outputs) so the
        // trailing idle + send_wire can be shared.
        let reply = if let Some(intro) = parse_introspection(&code) {
            // T129c: `?name` / `??name`. Build a single execute_result
            // text/plain carrying the name's type (and value for ??).
            let text = self.handle_introspection(intro);
            let exec_result = self.build_execute_result(parsed, execution_count, &text)?;
            self.send_iopub(&exec_result).await?;
            self.build_execute_reply_ok(parsed, execution_count)?
        } else if is_rich_display_literal(pre_type.as_ref(), expr_src) {
            // T129c: Vector/Matrix literal — render from source as an
            // HTML table + plain-text fallback. Skip normal eval (the
            // codegen's `print(vec)` fails to compile today because
            // Vec<T> lacks Display; rendering from source is the
            // workaround that doesn't waste a rustc spawn).
            let html = format_rich_html(expr_src);
            let exec_result =
                self.build_rich_execute_result(parsed, execution_count, &html, expr_src)?;
            self.send_iopub(&exec_result).await?;
            self.build_execute_reply_ok(parsed, execution_count)?
        } else {
            // Normal T129b evaluation path.
            //
            // Evaluate (blocking). The evaluator accumulates `let` /
            // `func` state across calls so subsequent cells see the
            // session's accumulated bindings.
            let result = self.evaluator.eval_line(&code);

            // Build + emit iopub outputs. Branch on diagnostic presence
            // first (error path) so the success path can short-circuit
            // to the simpler stream/execute_result shape.
            if result.diagnostic.is_some() || result.exit_code != Some(0) {
                // Error path. Build the error payload ONCE so the iopub
                // `error` and the shell `execute_reply` carry the same
                // ename/evalue/traceback triple (front-ends may render
                // either first; the shapes must agree).
                let (evalue, traceback) = build_error_payload(&result);
                let err_msg = self.build_error_message(parsed, &evalue, traceback.clone())?;
                self.send_iopub(&err_msg).await?;
                self.build_execute_reply_error(parsed, execution_count, &evalue, traceback)?
            } else {
                // Success path. Emit stream messages for any captured
                // stdout/stderr, then execute_result if there's a value.
                //
                // Duplication rule: when the evaluator returns a value
                // (bare-expression cell), the spawned program's stdout
                // already contains that value (the wrapper `print(expr)`
                // wrote it). We suppress the stdout stream in that case so
                // the notebook doesn't render the value twice (once as
                // stdout, once as Out[N]).
                if result.value.is_none() && !result.stdout.is_empty() {
                    let stream =
                        self.build_stream_message(parsed, StreamOutput::stdout(&result.stdout))?;
                    self.send_iopub(&stream).await?;
                }
                if !result.stderr.is_empty() {
                    let stream =
                        self.build_stream_message(parsed, StreamOutput::stderr(&result.stderr))?;
                    self.send_iopub(&stream).await?;
                }
                if let Some(value) = &result.value {
                    // T129c: if the captured value's resolved type is
                    // Vector/Matrix, emit a text/html + text/plain
                    // MIME bundle (HTML <table>) instead of plain text.
                    let exec_result = if let Some(ref t) = pre_type {
                        if is_matrix_or_vector(t) {
                            let html = format_rich_html(value);
                            self.build_rich_execute_result(parsed, execution_count, &html, value)?
                        } else {
                            self.build_execute_result(parsed, execution_count, value)?
                        }
                    } else {
                        self.build_execute_result(parsed, execution_count, value)?
                    };
                    self.send_iopub(&exec_result).await?;
                }
                self.build_execute_reply_ok(parsed, execution_count)?
            }
        };

        // iopub: status idle.
        let idle = self.build_status_message(parsed, "idle")?;
        self.send_iopub(&idle).await?;

        // shell: execute_reply (status ok OR error).
        self.send_wire(&reply).await?;

        Ok(())
    }

    /// T129c: resolve an introspection query (`?name` / `??name`) to
    /// the display text emitted as `execute_result` text/plain.
    ///
    /// - `?name` → `"<name>: <Type>"` (or `"<name>: <unknown>"` if the
    ///   name is unbound / type inference fails). Uses only
    ///   [`Evaluator::type_of`] — NO `rustc` spawn.
    /// - `??name` → `"<name>: <Type>\n= <value>"` (best-available
    ///   definition text). Calls [`Evaluator::eval_line`] on the bare
    ///   name to capture its current value via the standard
    ///   compile-and-run path. The original source line is NOT
    ///   surfaced because [`Evaluator`]'s accumulated source is
    ///   private; surfacing the source is post-T129c work.
    ///
    /// Both branches return without panicking on missing types /
    /// values — front-ends always see a well-formed `execute_result`.
    pub(super) fn handle_introspection(&mut self, intro: Introspection) -> String {
        match intro {
            Introspection::Help(name) => match self.evaluator.type_of(&name) {
                Some(ty) => format!("{name}: {ty}"),
                None => format!("{name}: <unknown>"),
            },
            Introspection::Source(name) => {
                // ?? is the "source" path. Best-available definition:
                // type signature + current value. BareExpr evaluation
                // does NOT accumulate state, so this query has no side
                // effects on subsequent cells.
                let ty = self.evaluator.type_of(&name);
                let val = self.evaluator.eval_line(&name).value;
                match (ty, val) {
                    (Some(t), Some(v)) => format!("{name}: {t}\n= {v}"),
                    (Some(t), None) => format!("{name}: {t}"),
                    (None, Some(v)) => format!("{name}\n= {v}"),
                    (None, None) => format!("{name}: <unknown>"),
                }
            }
        }
    }
}
