//! The injected client JS snippet + HTML injection logic (T131).
//!
//! On every HTML response (i.e. responses with a `text/html` content
//! type), the dev server appends a small (`<1 KB`) `<script>` block
//! that:
//!
//! 1. Opens a WebSocket to `/__buff_reload__` (the dev server's WS
//!    upgrade endpoint).
//! 2. Listens for JSON messages:
//!    - `{type:"reload"}` — calls `location.reload()` for a full page
//!      refresh (LIVE RELOAD — not state-preserving HMR; that's v1.9+).
//!    - `{type:"error", message:"..."}` — shows a red banner overlay
//!      with the message and a Reconnect button.
//! 3. On WS `close` / `error`, shows a "Dev server disconnected" red
//!    overlay so the user immediately sees that saves are no longer
//!    triggering rebuilds.
//! 4. Auto-reconnects with a 1 s backoff (so a `buff ui dev` restart
//!    reconnects without manual page refresh).
//!
//! The snippet is a compile-time `const &str` — NOT fetched at runtime
//! — so the dev server has zero round-trips before live reload is
//! wired. (See T131 MUST DO: "The injected client JS MUST be a
//! compile-time constant".)

/// The injected client JS, including the wrapping `<script>` tag.
///
/// Kept under 1 KB so it adds negligible weight to served HTML. The
/// snippet is intentionally vanilla JS (no transpile, no framework
/// dependency) so it works against any HTML the user serves —
/// hand-written, generated, or a future T133 RSX-produced page.
///
/// Behaviour contract: opens a WS to `/__buff_reload__`, listens for
/// `{type:"reload"}` and `{type:"error",message:"..."}`, calls
/// `location.reload()` on reload, shows a red banner overlay on
/// error / disconnect (auto-clears on the next message), auto-
/// reconnects with 1 s backoff.
///
/// Minified by hand (single-letter locals, no comments, compact CSS)
/// to fit under the 1 KB budget. Uses `textContent` rather than
/// `innerHTML` for the error message so multi-line compiler output
/// is rendered verbatim (and is XSS-safe against any byte sequence
/// the compiler emits).
pub const RELOAD_CLIENT_SNIPPET: &str = concat!(
    "<script>(function(){",
    // ov(msg) — show red banner with `msg` (textContent, so no XSS).
    r#"function ov(m){var e=document.getElementById('__bo');if(!e){e=document.createElement('div');e.id='__bo';e.style.cssText='position:fixed;top:0;left:0;right:0;z-index:2147483647;padding:6px 10px;font:13px/1.4 monospace;color:#fff;background:#c00;white-space:pre-wrap';document.body.appendChild(e);}e.textContent=m;}"#,
    // cn() — open WS, wire onmessage/onclose/onerror.
    r#"function cn(){var u=(location.protocol==='https:'?'wss:':'ws:')+'//'+location.host+'/__buff_reload__';var w=new WebSocket(u);w.onmessage=function(ev){var m;try{m=JSON.parse(ev.data);}catch(_){return;}if(m.type==='reload'){location.reload();return;}if(m.type==='error'){ov('Buff compile error:\n'+(m.message||''));}};w.onclose=function(){ov('Dev server disconnected — save any .buff file to reconnect.');setTimeout(cn,1000);};w.onerror=function(){w.close();};}"#,
    // boot — wait for DOMContentLoaded if needed.
    r#"if(document.readyState==='loading'){document.addEventListener('DOMContentLoaded',cn);}else{cn();}"#,
    "})();</script>",
);

/// Inject [`RELOAD_CLIENT_SNIPPET`] into an HTML body just before
/// `</body>` (or append at end when `</body>` is absent).
///
/// The injection point is intentional: placing the snippet right
/// before `</body>` guarantees the DOM has been parsed when the
/// snippet runs, so the overlay's `document.body.appendChild` never
/// hits a null body. The fallback (append-at-end) covers hand-written
/// HTML fragments that omit `</body>`.
///
/// # Determinism
///
/// Pure function — same input byte string produces byte-identical
/// output. No randomness, no environment dependence.
pub fn inject_client(html: &str) -> String {
    // Case-insensitive search for `</body>` so user HTML using any
    // casing convention (`</BODY>`, `</Body>`) still gets the snippet
    // injected at the right position.
    let lower = html.to_ascii_lowercase();
    match lower.rfind("</body>") {
        Some(idx) => {
            let mut out = String::with_capacity(html.len() + RELOAD_CLIENT_SNIPPET.len());
            out.push_str(&html[..idx]);
            out.push_str(RELOAD_CLIENT_SNIPPET);
            out.push_str(&html[idx..]);
            out
        }
        None => {
            // No </body> tag — append at end. This is the graceful-
            // degradation path for HTML fragments.
            let mut out = String::with_capacity(html.len() + RELOAD_CLIENT_SNIPPET.len());
            out.push_str(html);
            out.push_str(RELOAD_CLIENT_SNIPPET);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_is_under_1kb() {
        assert!(
            RELOAD_CLIENT_SNIPPET.len() < 1024,
            "client snippet must be <1024 bytes (was {})",
            RELOAD_CLIENT_SNIPPET.len()
        );
    }

    #[test]
    fn snippet_contains_websocket_connect() {
        // Sanity: the snippet must open a WS to the canonical path.
        assert!(RELOAD_CLIENT_SNIPPET.contains("/__buff_reload__"));
        assert!(RELOAD_CLIENT_SNIPPET.contains("WebSocket"));
        assert!(RELOAD_CLIENT_SNIPPET.contains("location.reload"));
    }

    #[test]
    fn inject_inserts_before_body_close() {
        let html = "<html><body><h1>hi</h1></body></html>";
        let injected = inject_client(html);
        assert!(injected.contains("<script>(function(){"));
        // The script MUST come BEFORE `</body>`, not after.
        let script_idx = injected.find("<script>").expect("script tag present");
        let body_close_idx = injected.find("</body>").expect("</body> present");
        assert!(script_idx < body_close_idx, "script must precede </body>");
    }

    #[test]
    fn inject_handles_missing_body_close() {
        let html = "<html><body><h1>hi</h1>"; // no </body>
        let injected = inject_client(html);
        assert!(injected.contains("<script>"));
        // Appended at end.
        let script_idx = injected.find("<script>").expect("script tag present");
        assert!(script_idx > 0);
    }

    #[test]
    fn inject_handles_lowercase_body_tag_only() {
        // We search the lowercased copy but splice into the original,
        // so `</BODY>` is still found and the original-case body
        // closing tag is preserved in the output.
        let html = "<html><BODY><h1>hi</h1></BODY></html>";
        let injected = inject_client(html);
        assert!(injected.contains("</BODY>"));
        assert!(injected.contains("<script>"));
    }

    #[test]
    fn inject_preserves_content_around_insertion_point() {
        // BEFORE-content-body-AFTER with the script injected just
        // before </body> (i.e. AFTER the content but BEFORE the
        // closing body tag).
        let html = "<html><body><span>BEFORE</span><span>AFTER</span></body></html>";
        let injected = inject_client(html);
        let before_idx = injected.find("BEFORE").expect("BEFORE");
        let after_idx = injected.find("AFTER").expect("AFTER");
        let script_idx = injected.find("<script>").expect("script");
        let body_close_idx = injected.find("</body>").expect("</body>");
        // Content order: BEFORE → AFTER → script → </body>.
        assert!(before_idx < after_idx);
        assert!(after_idx < script_idx);
        assert!(script_idx < body_close_idx);
    }
}
