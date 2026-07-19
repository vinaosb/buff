/* ==========================================================================
   Buff Website — link builder for "Try this" playground links
   ==========================================================================
   Reads data-buff-source attributes from .try-link anchors and generates
   the correct #s=<base64> URL fragment that the playground decodes.

   Encoding matches playground/app.js encodeBase64() exactly:
     TextEncoder → UTF-8 bytes → binary string → btoa()
   ========================================================================== */

(function () {
    "use strict";

    /**
     * UTF-8-safe base64 encode. Identical to playground/app.js encodeBase64().
     */
    function encodeBase64(text) {
        var bytes = new TextEncoder().encode(text);
        var binary = "";
        var chunkSize = 0x8000;
        for (var i = 0; i < bytes.length; i += chunkSize) {
            var chunk = bytes.subarray(i, Math.min(i + chunkSize, bytes.length));
            binary += String.fromCharCode.apply(null, chunk);
        }
        return btoa(binary);
    }

    /**
     * Wire up all .try-link elements whose href ends with "#s=".
     * Reads the Buff source from data-buff-source and appends the encoded
     * fragment so the link resolves in the playground with code pre-loaded.
     */
    function wireTryLinks() {
        var links = document.querySelectorAll("a.try-link");
        if (!links.length) return;

        for (var i = 0; i < links.length; i++) {
            var link = links[i];
            var source = link.getAttribute("data-buff-source");
            if (!source) continue;

            // The HTML attribute already contains the decoded Buff source
            // (entities like &#10; are resolved by the browser by the time
            // we read the attribute).
            try {
                var encoded = encodeBase64(source);
                link.href = "../playground/index.html#s=" + encoded;
            } catch (err) {
                // Silently degrade: link just goes to playground without code.
                link.href = "../playground/index.html";
            }
        }
    }

    // Run on DOM ready. No framework dependency.
    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", wireTryLinks);
    } else {
        wireTryLinks();
    }
})();
