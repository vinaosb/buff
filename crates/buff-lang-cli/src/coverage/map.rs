//! Rust-line → Buff-line translation (T137 core mapping step).
//!
//! Consumes the T60 [`SourceMap`](buff_lang_error::SourceMap)
//! (populated during codegen) + a flat list of [`RustLineHit`]s, and
//! emits a flat list of [`BuffLineHit`]s.
//!
//! # Mapping semantics
//!
//! [`SourceMap::lookup_buff`](buff_lang_error::SourceMap::lookup_buff)
//! resolves a Rust line number to a Buff [`Span`]. The span's byte
//! offsets are then resolved to a 1-based `(line, col)` via
//! [`SourceMap::lookup`](buff_lang_error::SourceMap::lookup), which
//! walks the [`SourceFile`](buff_lang_error::SourceFile)'s cached
//! line-start offsets.
//!
//! - Rust lines that don't appear in the map (e.g. codegen-emitted
//!   boilerplate, macros, or items outside the user's `.buff` source)
//!   are **dropped** from the Buff coverage — they have no Buff
//!   equivalent to credit.
//! - Multiple Rust lines may collapse onto the same Buff line
//!   (multi-line Buff expression lowered to several Rust statements).
//!   Each survives as an individual [`BuffLineHit`]; downstream
//!   [`BuffCoverage::aggregate`](super::model::BuffCoverage::aggregate)
//!   sums them.
//!
//! # Why callers must pass an `id → path` table
//!
//! [`SourceMap`] doesn't expose a path-only getter for its registered
//! [`SourceFile`]s (it's an internal `HashMap`). The CLI therefore
//! keeps a small side-table of `(SourceId, PathBuf)` pairs — populated
//! alongside `SourceMap::add_source` during codegen wiring — and
//! passes it here so we can emit fully-resolved
//! [`BuffLineHit::buff_file`] paths.

use std::path::PathBuf;

use buff_lang_error::{SourceId, SourceMap, Span};

use super::model::{BuffLineHit, RustLineHit};

/// Translate Rust-level coverage hits to Buff-level hits using the T60
/// [`SourceMap`] + a side-table of registered Buff file paths.
///
/// `source_map` must be populated (via `SourceMap::add_mapping` during
/// codegen) AND must have the Buff source files registered (via
/// `SourceMap::add_source`) so that span byte-offsets can be resolved
/// to line numbers. `paths` is the `(id, path)` side-table populated
/// alongside `add_source` — the order does not matter, lookup is linear
/// (the typical Buff project registers only 1–2 files per compile
/// unit).
///
/// Returns the per-hit translations in input order. Rust lines that
/// fail to map are silently dropped (see module docs).
pub fn map_rust_to_buff(
    hits: &[RustLineHit],
    source_map: &SourceMap,
    paths: &[(SourceId, PathBuf)],
) -> Vec<BuffLineHit> {
    hits.iter()
        .filter_map(|hit| translate_one(hit, source_map, paths))
        .collect()
}

/// Translate a single [`RustLineHit`].
///
/// Returns `None` when:
/// - `lookup_buff` finds no mapping for the rust_line (line was never
///   registered by codegen — typically codegen-emitted boilerplate).
/// - The returned span's `source_id` is not in `paths` (the source
///   file wasn't registered with the matching side-table entry).
/// - The span's start offset can't be resolved to a line (offset past
///   EOF — defensive).
fn translate_one(
    hit: &RustLineHit,
    source_map: &SourceMap,
    paths: &[(SourceId, PathBuf)],
) -> Option<BuffLineHit> {
    // llvm-cov uses 1-based line numbers, matching SourceMap's convention.
    let span: Span = source_map.lookup_buff(hit.rust_line)?;
    let (buff_line, _col) = source_map.lookup(span.source_id, span.start)?;
    let buff_file = lookup_path(span.source_id, paths)?;
    Some(BuffLineHit {
        buff_file,
        buff_line,
        count: hit.count,
    })
}

/// Linear search the `paths` side-table for the path registered under
/// `id`. Returns `None` when no entry matches.
fn lookup_path(id: SourceId, paths: &[(SourceId, PathBuf)]) -> Option<PathBuf> {
    paths
        .iter()
        .find(|(sid, _)| *sid == id)
        .map(|(_, p)| p.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::SourceFile;

    /// Build a test SourceMap populated with one `.buff` source file
    /// and a few rust_line → span mappings. The mappings are arbitrary
    /// but realistic: a multi-line Buff source lowered to several
    /// Rust lines, with some lines remapped (e.g. line 1 → rust 1+2).
    fn make_fixture() -> (SourceMap, SourceId, PathBuf) {
        let mut map = SourceMap::new();
        let id = SourceId(0);
        let path = PathBuf::from("examples/ola.buff");
        // 3-line Buff source:
        //   line 1 = `print("hi")`     starts at byte 0
        //   line 2 = `func add(a, b):` starts at byte 12
        //   line 3 = `    return a + b` starts at byte 28
        let buff_source = "print(\"hi\")\nfunc add(a, b):\n    return a + b\n";
        map.add_source(id, path.clone(), buff_source.to_string());

        // Map rust lines → Buff spans. Span.start MUST be a line-start
        // byte offset (0, 12, 28) so SourceFile::lookup resolves to
        // the correct 1-based line. Multiple Rust lines may map to the
        // same Buff line (e.g. codegen lowered `print("hi")` into 2
        // Rust statements).
        map.add_mapping(Span::new(0, 11, id), 1); // rust 1 → buff line 1
        map.add_mapping(Span::new(0, 11, id), 2); // rust 2 → buff line 1
        map.add_mapping(Span::new(12, 27, id), 5); // rust 5 → buff line 2
        map.add_mapping(Span::new(28, 44, id), 8); // rust 8 → buff line 3

        (map, id, path)
    }

    fn rust_line(line: usize, count: u64) -> RustLineHit {
        RustLineHit {
            rust_file: PathBuf::from("out.rs"),
            rust_line: line,
            count,
        }
    }

    #[test]
    fn map_translates_each_hit() {
        let (map, id, path) = make_fixture();
        let hits = vec![
            rust_line(1, 3),
            rust_line(2, 1),
            rust_line(5, 2),
            rust_line(8, 0),
        ];
        let out = map_rust_to_buff(&hits, &map, &[(id, path.clone())]);
        assert_eq!(
            out,
            vec![
                BuffLineHit {
                    buff_file: path.clone(),
                    buff_line: 1,
                    count: 3,
                },
                BuffLineHit {
                    buff_file: path.clone(),
                    buff_line: 1,
                    count: 1,
                },
                BuffLineHit {
                    buff_file: path.clone(),
                    buff_line: 2,
                    count: 2,
                },
                BuffLineHit {
                    buff_file: path,
                    buff_line: 3,
                    count: 0,
                },
            ]
        );
    }

    #[test]
    fn map_falls_back_to_closest_below_for_unmapped_lines() {
        // The T60 SourceMap::lookup_buff falls back to the closest
        // recorded line at or below the queried line — this mirrors
        // how rustc diagnostics point at the START of the statement,
        // so the nearest mapped statement above is the best candidate.
        // Lines 3, 4, 6, 7, 100 fall back to their nearest-below
        // mapping; nothing is dropped.
        let (map, id, path) = make_fixture();
        let hits = vec![
            rust_line(3, 5),   // falls back to line 2 → buff line 1
            rust_line(4, 9),   // falls back to line 2 → buff line 1
            rust_line(6, 1),   // falls back to line 5 → buff line 2
            rust_line(7, 2),   // falls back to line 5 → buff line 2
            rust_line(100, 7), // falls back to line 8 → buff line 3
            rust_line(1, 2),   // exact match → buff line 1
        ];
        let out = map_rust_to_buff(&hits, &map, &[(id, path)]);
        assert_eq!(
            out.len(),
            6,
            "T60 fallback maps every line; nothing dropped"
        );
        // Verify each line resolved to its expected buff line
        // (input order preserved):
        //   rust 3 → buff 1, rust 4 → buff 1, rust 6 → buff 2,
        //   rust 7 → buff 2, rust 100 → buff 3, rust 1 → buff 1
        let buff_lines: Vec<_> = out.iter().map(|h| h.buff_line).collect();
        assert_eq!(buff_lines, vec![1, 1, 2, 2, 3, 1]);
    }

    #[test]
    fn map_aggregates_multiple_rust_lines_to_one_buff_line() {
        // Two Rust lines both mapping to buff line 1 should produce
        // two BuffLineHits that aggregate (after BuffCoverage::aggregate)
        // to a single line 1 entry with summed counts.
        let (map, id, path) = make_fixture();
        let hits = vec![
            rust_line(1, 3),
            rust_line(2, 4), // also maps to buff line 1
        ];
        let out = map_rust_to_buff(&hits, &map, &[(id, path.clone())]);
        assert_eq!(out.len(), 2, "two hits before aggregation");
        let cov = super::super::model::BuffCoverage::aggregate(&out);
        let file_cov = cov.files.get(&path).expect("file present");
        assert_eq!(file_cov.lines.get(&1).copied(), Some(7), "3 + 4 = 7");
    }

    #[test]
    fn map_with_no_mappings_returns_empty() {
        let mut map = SourceMap::new();
        // Register the source but DON'T add any rust_line mappings.
        let id = SourceId(0);
        map.add_source(id, PathBuf::from("empty.buff"), "x = 1\n".to_string());
        let hits = vec![rust_line(1, 1)];
        let out = map_rust_to_buff(&hits, &map, &[(id, PathBuf::from("empty.buff"))]);
        assert!(out.is_empty(), "no rust_line mappings → no BuffLineHits");
    }

    #[test]
    fn map_drops_hits_when_source_not_in_paths_table() {
        // Sanity: the side-table lookup is exhaustive — if a source
        // wasn't registered, we should get None rather than panic.
        let (map, _id, _path) = make_fixture();
        let hits = vec![rust_line(1, 1)];
        // Pass an EMPTY side-table — every lookup should miss.
        let out = map_rust_to_buff(&hits, &map, &[]);
        assert!(out.is_empty());
    }

    #[test]
    fn map_supports_multiple_buff_files() {
        // Multi-file Buff project: two .buff files registered under
        // different SourceIds.
        let mut map = SourceMap::new();
        let id_a = SourceId(0);
        let id_b = SourceId(1);
        let path_a = PathBuf::from("a.buff");
        let path_b = PathBuf::from("b.buff");
        map.add_source(id_a, path_a.clone(), "x = 1\n".to_string());
        map.add_source(id_b, path_b.clone(), "y = 2\n".to_string());
        map.add_mapping(Span::new(0, 5, id_a), 1);
        map.add_mapping(Span::new(0, 5, id_b), 5);

        let hits = vec![rust_line(1, 4), rust_line(5, 7)];
        let out = map_rust_to_buff(
            &hits,
            &map,
            &[(id_a, path_a.clone()), (id_b, path_b.clone())],
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].buff_file, path_a);
        assert_eq!(out[0].buff_line, 1);
        assert_eq!(out[1].buff_file, path_b);
        assert_eq!(out[1].buff_line, 1);
    }

    /// Verify that the `SourceFile` line-start computation works for
    /// the multi-byte / multi-line cases we care about.
    #[test]
    fn source_file_lookup_handles_multiline_source() {
        let src = "line1\nline2\nline3\n";
        let sf = SourceFile::new(PathBuf::from("x.buff"), src.to_string());
        // line 1 starts at byte 0, line 2 at byte 6, line 3 at byte 12.
        assert_eq!(sf.lookup(0), Some((1, 1)));
        assert_eq!(sf.lookup(6), Some((2, 1)));
        assert_eq!(sf.lookup(12), Some((3, 1)));
        // offset past EOF returns None.
        assert_eq!(sf.lookup(src.len() + 1), None);
    }
}
