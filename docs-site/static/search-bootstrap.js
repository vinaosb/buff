// Search bootstrap for the Buff docs site.
// Wired into base.html only when config.build_search_index = true.
// Zola emits /search_index.<lang>.js (the search index) at build time;
// elasticlunr itself is bundled by Zola as /elasticlunr.min.js.

(function () {
  var input = document.getElementById("search-input");
  var resultsList = document.getElementById("search-results");
  if (!input || !resultsList) return;
  if (typeof window.elasticlunr === "undefined") return;

  var index = null;

  function loadIndex() {
    if (index) return;
    var script = document.createElement("script");
    script.src = "/search_index.en.js";
    script.onload = function () {
      if (typeof window.searchIndex !== "undefined") {
        index = elasticlunr.Index.load(window.searchIndex);
      }
    };
    document.body.appendChild(script);
  }

  function render(hits) {
    resultsList.innerHTML = "";
    if (hits.length === 0) {
      resultsList.hidden = true;
      return;
    }
    hits.slice(0, 10).forEach(function (hit) {
      var li = document.createElement("li");
      var a = document.createElement("a");
      a.href = hit.ref;
      a.textContent = (hit.doc && hit.doc.title) || hit.ref;
      li.appendChild(a);
      resultsList.appendChild(li);
    });
    resultsList.hidden = false;
  }

  input.addEventListener("focus", loadIndex);
  input.addEventListener("input", function () {
    if (!index) {
      loadIndex();
      return;
    }
    var query = input.value.trim();
    if (!query) {
      resultsList.hidden = true;
      return;
    }
    var hits = index.search(query, { expand: true });
    render(hits);
  });

  document.addEventListener("click", function (e) {
    if (!resultsList.contains(e.target) && e.target !== input) {
      resultsList.hidden = true;
    }
  });
})();
