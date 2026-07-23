//! `buff-lang-debug-info` — Buff-span stack traces via `.buffmap` source-map
//! sidecar + `std::panic` hook.
//!
//! # T24 — Stack Traces with Buff Spans
//!
//! Every modern scripting language that lowers to a lower-level runtime
//! (TypeScript, Python, Dart, …) gives users stack traces in their own
//! language, not the underlying IR. Buff lowers to Rust, so without a
//! translation layer a panicked Buff program shows the user generated
//! `<file>.rs:LINE:COL` paths and Rust-internal frames — useless for
//! debugging the Buff source.
//!
//! This crate ships that translation layer in three pieces:
//!
//! 1. **Capture** ([`capture`]) — during codegen, walk the Buff AST +
//!    formatted Rust source and build a [`SourceMap`] carrying the
//!    bidirectional Rust-line ↔ Buff-(file,line,col,span) mapping plus
//!    the originating Buff identifier names (function names especially).
//! 2. **Format** ([`format`]) — serialise the [`SourceMap`] to a JSON
//!    `.buffmap` sidecar file written next to the compiled binary. Both
//!    forward (Rust → Buff) and reverse (Buff → Rust) lookup tables ship
//!    in the file so the DAP server (T136) and `buff backtrace` can
//!    consume it for offline work.
//! 3. **Panic hook** ([`panic_hook`]) — register a `std::panic::set_hook`
//!    interceptor at runtime that reads the `.buffmap` and walks the
//!    Rust backtrace, remapping each frame to its Buff source location.
//!    `RUST_BACKTRACE=1` is always preserved as an escape hatch (full
//!    Rust trace printed AFTER the Buff trace).
//!
//! # Pipeline wiring
//!
//! ```text
//!   buff-lang-codegen-rust::generate_rust(&[Decl]) -> String
//!        │
//!        ▼  capture::build_source_map(decls, &rust_source, buff_path)
//!   buff-lang-debug-info::SourceMap
//!        │
//!        ▼  format::serialize(&source_map) -> String
//!   <binary>.buffmap   (written alongside <binary>)
//!        │
//!        ▼  panic_hook::install("<binary>.buffmap")
//!   std::panic::set_hook(...)
//!        │
//!        ▼  on panic: panic_hook::remap_panic_backtrace(...) -> Buff trace
//!   stderr
//! ```
//!
//! # Determinism
//!
//! All map/set types in this crate are [`BTreeMap`] / [`BTreeSet`] — never
//! `HashMap`/`HashSet` — so the `.buffmap` JSON output is byte-identical
//! across runs (project hard rule, see root AGENTS.md).

pub mod capture;
pub mod format;
pub mod panic_hook;

pub use capture::build_source_map;
pub use format::{
    deserialize, serialize_to_string, BuffMapFile, FunctionMapping, LineMapping, MAP_FORMAT_VERSION,
};
pub use panic_hook::{install_panic_hook, remap_panic_backtrace, BuffTrace, BuffTraceFrame};

use std::collections::BTreeMap;
use std::path::PathBuf;

use buff_lang_error::{SourceId, Span};

/// The bidirectional Rust-line ↔ Buff-location mapping that backs the
/// `.buffmap` sidecar file.
///
/// Built during codegen by [`capture::build_source_map`]. Forward lookup
/// (Rust → Buff) is consulted by the panic hook at runtime; reverse lookup
/// (Buff → Rust) is consumed by the DAP server (T136) + `buff backtrace`.
///
/// All maps are [`BTreeMap`]s keyed by Rust line number so iteration order
/// is deterministic — the same Buff program produces the same `.buffmap`
/// JSON byte-for-byte across runs (project hard rule).
#[derive(Debug, Clone)]
pub struct SourceMap {
    /// Path of the originating `.buff` source file (recorded into the
    /// `.buffmap` so offline consumers — `buff backtrace`, DAP — know
    /// which file the mappings refer to without extra context).
    pub buff_file: Option<PathBuf>,
    /// Path of the generated `.rs` file (recorded for symmetry + for
    /// offline consumers that need to cross-reference rustc diagnostics).
    pub rust_file: Option<PathBuf>,
    /// Rust line (1-based) → Buff location. Populated during codegen by
    /// scanning the formatted Rust source for Buff identifier anchors
    /// (function names). See [`capture::build_source_map`].
    pub rust_to_buff: BTreeMap<usize, BuffLocation>,
    /// Function-level mapping: Buff function name → `(Buff span, Rust
    /// line range)`. Used by [`panic_hook::remap_panic_backtrace`] to
    /// resolve which Buff function a Rust frame is inside, even when
    /// no per-line mapping exists for the exact Rust line of the frame.
    pub functions: Vec<FunctionAnchor>,
    /// Source ID of the originating `.buff` file. Stored so consumers
    /// that round-trip the [`Span`] back through `buff_lang_error`'s
    /// [`SourceMap`](buff_lang_error::SourceMap) (`lookup(id, offset)`)
    /// can resolve byte offsets to `(line, col)` pairs.
    pub source_id: SourceId,
}

/// A Buff source location: byte span + the resolved 1-based `(line, col)`
/// pair + the originating Buff identifier name (function name, etc.) when
/// known.
///
/// `line` and `col` are pre-computed during capture (via the canonical
/// `buff_lang_error::SourceFile::lookup`) so consumers don't need to
/// re-resolve byte offsets at runtime. The `name` field is the
/// human-friendly identifier shown in stack traces (`helper`, `main`, …);
/// `None` when the location has no associated identifier (e.g. an
/// intermediate statement line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuffLocation {
    /// 1-based line number in the originating `.buff` file.
    pub line: usize,
    /// 1-based column number in the originating `.buff` file.
    pub col: usize,
    /// Byte span in the originating `.buff` file. Round-trips through
    /// [`buff_lang_error::SourceMap`] for diagnostic rendering.
    pub span: Span,
    /// Originating Buff identifier name when known (e.g. `"helper"`,
    /// `"main"`). `None` for locations that don't correspond to a named
    /// declaration.
    pub name: Option<String>,
}

/// Function-level anchor: Buff function name + Buff span + Rust line
/// range. Built by [`capture::build_source_map`] for each top-level
/// Buff function declaration.
///
/// Consulted by [`panic_hook::remap_panic_backtrace`] (via
/// [`SourceMap::lookup_buff`]'s fallback path) when no exact per-line
/// mapping exists for a Rust frame's line — the frame is attributed to
/// whichever Buff function its Rust line falls inside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionAnchor {
    /// Buff function name (e.g. `"helper"`, `"main"`).
    pub name: String,
    /// Byte span of the Buff function declaration.
    pub buff_span: Span,
    /// 1-based line of the Buff function declaration in the `.buff` file.
    pub buff_line: usize,
    /// 1-based column of the Buff function declaration.
    pub buff_col: usize,
    /// 1-based start line of the generated `fn <name>(...)` in the
    /// formatted Rust source.
    pub rust_start_line: usize,
    /// 1-based end line (closing `}`) of the generated fn in the Rust
    /// source.
    pub rust_end_line: usize,
    /// Cached Buff location used by [`SourceMap::lookup_buff`]'s
    /// fallback path. `None` when the anchor carries no resolvable
    /// Buff location (e.g. built manually without line/col context).
    pub buff_location: Option<BuffLocation>,
}

impl SourceMap {
    /// Create an empty source map with no file paths recorded.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the originating `.buff` file path + its [`SourceId`].
    pub fn with_buff_file(mut self, path: PathBuf, source_id: SourceId) -> Self {
        self.buff_file = Some(path);
        self.source_id = source_id;
        self
    }

    /// Record the generated `.rs` file path.
    pub fn with_rust_file(mut self, path: PathBuf) -> Self {
        self.rust_file = Some(path);
        self
    }

    /// Record that a 1-based `rust_line` corresponds to a Buff
    /// [`BuffLocation`]. Later calls for the same `rust_line` win
    /// (the previous entry is overwritten).
    pub fn add_line_mapping(&mut self, rust_line: usize, location: BuffLocation) {
        self.rust_to_buff.insert(rust_line, location);
    }

    /// Record a function-level [`FunctionAnchor`] — name + Buff span +
    /// Rust line range. The anchor is consulted by
    /// [`panic_hook::remap_panic_backtrace`] when no exact per-line
    /// mapping exists for a Rust frame's line (the frame falls inside
    /// the function's Rust line range).
    pub fn add_function(&mut self, anchor: FunctionAnchor) {
        self.functions.push(anchor);
    }

    /// Look up the [`BuffLocation`] for a 1-based `rust_line`.
    ///
    /// Returns the exact match when `rust_line` was recorded via
    /// [`add_line_mapping`](Self::add_line_mapping). Otherwise, returns
    /// the closest recorded line **at or below** `rust_line` — this
    /// mirrors how `rustc`/panic locations point at the *start* of the
    /// statement that failed, so the nearest mapped statement above is
    /// the best candidate. As a final fallback, walks the
    /// [`functions`](Self::functions) list for any anchor whose Rust
    /// line range contains `rust_line` and returns its Buff location.
    pub fn lookup_buff(&self, rust_line: usize) -> Option<&BuffLocation> {
        if let Some(loc) = self.rust_to_buff.get(&rust_line) {
            return Some(loc);
        }
        let nearest_below = self
            .rust_to_buff
            .iter()
            .filter(|(rl, _)| **rl <= rust_line)
            .max_by_key(|(rl, _)| **rl)
            .map(|(_, loc)| loc);
        if nearest_below.is_some() {
            return nearest_below;
        }
        // Final fallback: function-level containment. The first function
        // whose [rust_start_line, rust_end_line] range contains rust_line
        // wins. functions is a Vec (declaration order, deterministic) so
        // the order matches AST order.
        self.functions
            .iter()
            .find(|f| rust_line >= f.rust_start_line && rust_line <= f.rust_end_line)
            .and_then(|f| f.buff_location.as_ref())
    }

    /// Look up the originating Buff identifier name for a 1-based
    /// `rust_line`, if any. Convenience wrapper around
    /// [`lookup_buff`](Self::lookup_buff) for the panic hook (which
    /// shows function names in stack traces).
    pub fn lookup_name(&self, rust_line: usize) -> Option<&str> {
        self.lookup_buff(rust_line)
            .and_then(|loc| loc.name.as_deref())
    }

    /// Returns `true` when no Rust-line ↔ Buff mappings have been
    /// recorded (line OR function level). Consumers can use this to
    /// decide whether to fall back to the raw Rust trace.
    pub fn is_empty(&self) -> bool {
        self.rust_to_buff.is_empty() && self.functions.is_empty()
    }
}

impl Default for SourceMap {
    fn default() -> Self {
        Self {
            buff_file: None,
            rust_file: None,
            rust_to_buff: BTreeMap::new(),
            functions: Vec::new(),
            source_id: SourceId(0),
        }
    }
}
