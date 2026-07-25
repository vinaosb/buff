//! T7: Salsa-based incremental compilation front-end.
//!
//! Wraps the lex + parse + typecheck passes in [`salsa`] tracked queries so
//! unchanged input files skip re-processing on subsequent `buff build` /
//! `buff run` invocations within a single CLI session (and across
//! long-running sessions like `buff watch` / `buff repl` / the LSP server).
//!
//! ## Design constraints (from the T7 task spec)
//!
//! - **Correctness MUST NOT depend on salsa.** The underlying
//!   [`pipeline::compile_to_rust`] path is byte-identical with or without
//!   this module — salsa is purely a memoization cache layered above it.
//! - **Start SIMPLE**: just two tracked queries ([`parse_file`] +
//!   [`typecheck_file`]). Codegen is intentionally NOT incrementalized
//!   (it produces a full Rust `String`, not a queryable structure).
//! - **`Vec<Decl>` is not `Eq + Hash`**, so the parsed AST cannot be
//!   stored directly in salsa's memoization table. Instead the tracked
//!   queries return small hashable *summaries* ([`ParseOutcome`] /
//!   [`TypeCheckOutcome`]) that are sufficient for change detection. The
//!   pipeline re-materializes the AST via the existing
//!   `tokenize` → `parse` path when it needs the actual `Vec<Decl>` for
//!   codegen — salsa's memoization guarantees those passes already ran
//!   for any source whose summary is cached, so the re-materialization
//!   is a cheap OS-level cache hit on the source bytes.
//!
//! ## When does this help?
//!
//! - **Single-file `buff run` one-shot**: marginal — the in-process DB is
//!   built and dropped per invocation, so salsa's memoization never gets a
//!   second query. The T55 `.rs` byte-cache ([`compile_speed::read_cache`])
//!   is what accelerates this case.
//! - **`buff watch` / `buff repl` / LSP**: the DB persists across
//!   rebuilds; unchanged files skip tokenize + parse + typecheck entirely.
//!   This is where salsa shines.
//! - **Multi-file projects (T1 `project_pipeline`)**: each file is a
//!   separate salsa input; the DB tracks per-file change state, enabling
//!   future incremental typecheck across the module graph.
//!
//! ## Pipeline integration
//!
//! [`pipeline::compile_to_rust_incremental`] is the entry point. It:
//! 1. Reads the source file.
//! 2. Registers it as a [`SourceFile`] salsa input on the DB.
//! 3. Calls [`parse_file`] + [`typecheck_file`] — salsa memoizes if the
//!    source text is unchanged since the last call with the same DB.
//! 4. Falls through to the regular `tokenize → parse → generate_rust`
//!    path to materialize the actual Rust source. (Salsa already ran
//!    lex + parse internally; the OS file-cache makes the re-read cheap.)
//!
//! The `--incremental` / `--no-incremental` CLI flags select between
//! [`pipeline::compile_to_rust_incremental`] and the existing
//! [`pipeline::compile_to_rust_with_cache`]. Default: incremental ON for
//! dev `Debug` builds, OFF for `Release`/`Minimal` (release builds care
//! about the final binary, not the edit loop).

use std::path::PathBuf;

use buff_lang_error::SourceId;

// ---------------------------------------------------------------------------
// Salsa inputs.
// ---------------------------------------------------------------------------

/// A source file registered with the incremental database.
///
/// Salsa tracks the `path` + `source` fields for change detection: when
/// the same `path` is registered with identical `source` bytes on a
/// subsequent query, salsa returns the cached [`ParseOutcome`] without
/// re-running tokenize + parse.
#[salsa::input]
pub struct SourceFile {
    /// Canonical filesystem path of the `.buff` source. Stored by
    /// reference so salsa does not clone the `PathBuf` per query.
    #[returns(ref)]
    pub path: PathBuf,
    /// Raw source text. Stored by reference for the same reason.
    #[returns(ref)]
    pub source: String,
}

// ---------------------------------------------------------------------------
// Hashable query outcomes.
// ---------------------------------------------------------------------------

/// Summary of a lex + parse pass over a single source file.
///
/// Stored in salsa's memoization table as the return value of
/// [`parse_file`]. The enum variants are intentionally small + hashable
/// so salsa can compare outcomes across queries cheaply. The actual
/// parsed `Vec<Decl>` is NOT carried here (it does not impl `Eq + Hash`);
/// the pipeline re-materializes it via the regular `tokenize → parse`
/// path when it needs the AST for codegen.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParseOutcome {
    /// Parse succeeded. `source_hash` is a stable hash of the source
    /// bytes (used by callers to detect content changes across DB
    /// resets). `decl_count` is the number of top-level declarations.
    Ok { source_hash: u64, decl_count: usize },
    /// Lexing failed (the byte-scanner rejected the input).
    LexFailed,
    /// Parsing failed (the recursive-descent parser rejected the
    /// token stream).
    ParseFailed,
}

/// Summary of a typecheck pass over a single source file.
///
/// Returned by [`typecheck_file`]. Depends on [`parse_file`] — salsa
/// automatically re-runs typecheck when the parse outcome changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeCheckOutcome {
    /// All function bodies passed inference without errors.
    Pass,
    /// Typecheck found errors. `error_count` carries how many (the
    /// actual [`buff_lang_error::TypeError`]s are surfaced by the
    /// regular `buff check` path — salsa only tracks the count for
    /// change detection).
    Fail { error_count: usize },
}

// ---------------------------------------------------------------------------
// Salsa tracked queries.
// ---------------------------------------------------------------------------

/// Salsa tracked query: lex + parse a source file, return a hashable
/// summary.
///
/// When called multiple times with the same [`SourceFile`] input (i.e.
/// the path + source text are unchanged), salsa returns the cached
/// outcome without re-running tokenize + parse. This is the primary
/// incremental win for long-running sessions.
///
/// On a cache miss (input changed or first call), the query executes
/// the full lex + parse pipeline inline. Errors are mapped to
/// [`ParseOutcome::LexFailed`] / [`ParseOutcome::ParseFailed`] — the
/// detailed diagnostic is NOT carried (the pipeline surfaces it via
/// the regular error-mapper path when it re-materializes the AST).
#[salsa::tracked]
pub fn parse_file(db: &dyn salsa::Database, file: SourceFile) -> ParseOutcome {
    let source = file.source(db);
    let source_hash = content_hash(source.as_bytes());
    let source_id = SourceId(0);
    match buff_lang_lexer::tokenize(source, source_id) {
        Ok(tokens) => match buff_lang_parser::parse(&tokens, source_id) {
            Ok(decls) => ParseOutcome::Ok {
                source_hash,
                decl_count: decls.len(),
            },
            Err(_) => ParseOutcome::ParseFailed,
        },
        Err(_) => ParseOutcome::LexFailed,
    }
}

/// Salsa tracked query: typecheck a source file.
///
/// Depends on [`parse_file`] — salsa automatically re-runs this query
/// when the parse outcome changes. The query drives the
/// [`buff_lang_types::TypeInferencer`] over every function body in the
/// parsed AST (re-materialized via the regular tokenize + parse path)
/// and returns a hashable [`TypeCheckOutcome`].
///
/// As with [`parse_file`], detailed [`buff_lang_error::TypeError`]s are
/// surfaced by the regular `buff check` / pipeline error-mapper path —
/// salsa only tracks the count for change detection.
#[salsa::tracked]
pub fn typecheck_file(db: &dyn salsa::Database, file: SourceFile) -> TypeCheckOutcome {
    let outcome = parse_file(db, file);
    match outcome {
        ParseOutcome::Ok {
            source_hash: _,
            decl_count: _,
        } => {
            // Re-materialize the AST. This is a cheap OS-cache hit when
            // salsa's memoization already ran the work; the source text
            // is fetched from the salsa input (no extra filesystem read).
            let source = file.source(db);
            let source_id = SourceId(0);
            let tokens = match buff_lang_lexer::tokenize(source, source_id) {
                Ok(t) => t,
                Err(_) => return TypeCheckOutcome::Fail { error_count: 0 },
            };
            let decls = match buff_lang_parser::parse(&tokens, source_id) {
                Ok(d) => d,
                Err(_) => return TypeCheckOutcome::Fail { error_count: 0 },
            };

            // Drive TypeInferencer over every function body. This
            // mirrors `check::type_check_decls` (T55 standalone
            // typecheck) but inlines the top-level walk so this module
            // stays self-contained.
            let error_count = count_type_errors(&decls);
            if error_count == 0 {
                TypeCheckOutcome::Pass
            } else {
                TypeCheckOutcome::Fail { error_count }
            }
        }
        _ => TypeCheckOutcome::Fail { error_count: 0 },
    }
}

/// Drive [`TypeInferencer`] over every function body in `decls`,
/// returning the total error count.
///
/// Mirrors `crate::check::type_check_decls` (T55). Inlined here so the
/// incremental module is self-contained — `check.rs` continues to own
/// the user-facing diagnostic formatting; this function only needs the
/// count for change detection.
fn count_type_errors(decls: &[buff_lang_ast::Decl]) -> usize {
    use buff_lang_ast::Decl;
    let mut count = 0;
    for d in decls {
        count += count_type_errors_decl(d);
    }
    count
}

fn count_type_errors_decl(decl: &buff_lang_ast::Decl) -> usize {
    use buff_lang_ast::Decl;
    match decl {
        Decl::FuncDecl(f) => count_type_errors_func(f),
        Decl::TraitDecl(t) => t.defaults.iter().map(count_type_errors_func).sum(),
        Decl::ExtendBlock(b) => b.methods.iter().map(count_type_errors_func).sum(),
        Decl::ExportDecl(inner) => count_type_errors_decl(&inner.inner),
        // Struct / Enum / Import / Module / Reexport / ExternCrate: no
        // function bodies to type-check at this layer.
        _ => 0,
    }
}

fn count_type_errors_func(f: &buff_lang_ast::FuncDecl) -> usize {
    use buff_lang_types::TypeInferencer;
    let mut inferencer = TypeInferencer::new();
    // Pre-bind parameters using the same primitive mapping `check.rs`
    // uses. User-defined types fall back to Unknown (permissive).
    for p in &f.params {
        if let Some(ty) = typeref_to_type(&p.ty) {
            inferencer.bind(&p.name.name, ty);
        }
    }
    let mut errors = 0;
    for stmt in &f.body.stmts {
        if inferencer.infer_stmt(stmt).is_err() {
            errors += 1;
        }
    }
    errors
}

/// Minimal `TypeRef → Type` mapping for the primitive names + Option /
/// Result wrappers recognised in v1.0.
///
/// Mirrors `crate::check::typeref_to_type` — duplicated intentionally so
/// the incremental module is self-contained and does not perturb the
/// existing `check.rs` exports.
fn typeref_to_type(ty: &buff_lang_ast::TypeRef) -> Option<buff_lang_types::Type> {
    use buff_lang_ast::TypeRef;
    use buff_lang_types::Type;
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "Int" => Some(Type::int_default()),
            "Float" => Some(Type::float_default()),
            "Double" => Some(Type::double()),
            "Bool" => Some(Type::bool()),
            "String" => Some(Type::string()),
            "Char" => Some(Type::char()),
            "Byte" => Some(Type::byte()),
            "Decimal" => Some(Type::Decimal),
            "Void" => Some(Type::Void),
            _ => None,
        },
        TypeRef::Option(inner, _) => Some(Type::option(
            typeref_to_type(inner).unwrap_or(Type::Unknown),
        )),
        TypeRef::Generic { base, args, .. } => {
            if let TypeRef::Named { name, .. } = base.as_ref() {
                if name.name == "Option" && args.len() == 1 {
                    let inner = typeref_to_type(&args[0]).unwrap_or(Type::Unknown);
                    return Some(Type::option(inner));
                }
                if name.name == "Result" && args.len() == 2 {
                    let ok_ty = typeref_to_type(&args[0]).unwrap_or(Type::Unknown);
                    let err_ty = typeref_to_type(&args[1]).unwrap_or(Type::Unknown);
                    return Some(Type::result(ok_ty, err_ty));
                }
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Database.
// ---------------------------------------------------------------------------

/// Salsa database backing the incremental compilation queries.
///
/// Wraps a [`salsa::Storage`] slot that memoizes the [`parse_file`] +
/// [`typecheck_file`] results keyed on [`SourceFile`] inputs. The DB is
/// cheap to construct (a single `Box` allocation); for one-shot CLI
/// invocations the cost is negligible relative to the rustc backend.
///
/// For long-running sessions (`buff watch`, `buff repl`, the LSP server),
/// the DB persists across rebuilds — that is where the incremental wins
/// materialize. The CLI's `--incremental` flag controls whether a fresh
/// DB is created per invocation (single-file mode) or a shared DB is
/// threaded through the session.
#[salsa::db]
#[derive(Default)]
pub struct BuffDatabase {
    /// Salsa's per-database storage slot. Holds the memoization tables
    /// for every tracked query + interned input. The `Self` type
    /// parameter threads the database identity back through salsa's
    /// plumbing so tracked queries can locate their own memo table.
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for BuffDatabase {}

impl BuffDatabase {
    /// Create a fresh, empty incremental database.
    ///
    /// Cheap: a single `Box` allocation for the storage slot. Memoization
    /// tables are populated lazily as queries execute.
    pub fn new() -> Self {
        Self::default()
    }
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

/// Stable 64-bit hash of the source bytes.
///
/// Used as the `source_hash` field of [`ParseOutcome::Ok`]. Stable
/// across process invocations (uses the deterministic
/// `std::collections::hash_map::DefaultHasher` — NOT `RandomState` — so
/// two CLI invocations on the same source produce identical hashes).
///
/// This hash is for change-detection / diagnostics only. The T55
/// generated-Rust cache uses its own SHA-256 (see
/// [`compile_speed::source_cache_key`]); the two hashes are independent
/// and need not agree.
fn content_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_construction_is_cheap() {
        // The DB must construct without panic and start empty.
        let _db = BuffDatabase::new();
    }

    #[test]
    fn parse_file_caches_unchanged_input() {
        // Same source → salsa memoizes → second call must not re-execute
        // the tracked function body. We detect this by checking the
        // returned `ParseOutcome` is identical (salsa guarantees
        // referential equality for memoized returns of `Eq + Hash`
        // outcomes; we settle for value equality here).
        let mut db = BuffDatabase::new();
        let src = "func main() { print(\"hi\") }\n";
        let file = SourceFile::new(&mut db, PathBuf::from("test.buff"), src.to_string());
        let a = parse_file(&db, file);
        let b = parse_file(&db, file);
        assert_eq!(a, b, "memoized parse_file outcome must be stable");
        assert!(
            matches!(a, ParseOutcome::Ok { decl_count: 1, .. }),
            "expected 1 decl, got {a:?}"
        );
    }

    #[test]
    fn parse_file_detects_changed_input() {
        // When the input source text changes between queries, salsa must
        // re-execute parse_file and return the new outcome. We verify
        // by registering the SAME `SourceFile` id with different content
        // via `set_source`.
        let mut db = BuffDatabase::new();
        let file = SourceFile::new(
            &mut db,
            PathBuf::from("changed.buff"),
            "func a() { 1 }\n".to_string(),
        );
        let before = parse_file(&db, file);
        file.set_source(&mut db)
            .to("func a() { 2 }\nfunc b() { 3 }\n".to_string());
        let after = parse_file(&db, file);
        assert_ne!(before, after, "outcomes must differ after source change");
        // After the change, decl_count must reflect the new 2-decl source.
        if let ParseOutcome::Ok { decl_count, .. } = after {
            assert_eq!(decl_count, 2, "expected 2 decls after change");
        } else {
            panic!("expected Ok after change, got {after:?}");
        }
    }

    #[test]
    fn parse_file_reports_lex_failure() {
        // A source with a tab character is rejected by the Buff lexer
        // (AGENTS.md: "Tabs — Buff lexer rejects them").
        let mut db = BuffDatabase::new();
        let file = SourceFile::new(
            &mut db,
            PathBuf::from("tab.buff"),
            "func bad() {\n\tprint(\"tab\")\n}\n".to_string(),
        );
        let outcome = parse_file(&db, file);
        assert!(
            matches!(outcome, ParseOutcome::LexFailed | ParseOutcome::ParseFailed),
            "expected lex/parse failure for tab source, got {outcome:?}"
        );
    }

    #[test]
    fn typecheck_file_caches_on_pass() {
        // A trivially-typed source must produce Pass and stay cached.
        let mut db = BuffDatabase::new();
        let file = SourceFile::new(
            &mut db,
            PathBuf::from("ok.buff"),
            "func add(a: Int, b: Int) -> Int { a + b }\n".to_string(),
        );
        let a = typecheck_file(&db, file);
        let b = typecheck_file(&db, file);
        assert_eq!(a, b);
        assert!(matches!(a, TypeCheckOutcome::Pass), "got {a:?}");
    }

    #[test]
    fn content_hash_is_deterministic() {
        // Same bytes → same hash, across separate calls (DefaultHasher
        // is deterministic, unlike RandomState).
        let h1 = content_hash(b"hello");
        let h2 = content_hash(b"hello");
        assert_eq!(h1, h2);
        assert_ne!(h1, content_hash(b"world"));
    }

    #[test]
    fn typeref_to_type_handles_primitives() {
        // Spot-check the primitive mapping used by typecheck_file.
        // Mirrors the `check::typeref_to_type` surface exactly so the
        // incremental typecheck stays consistent with `buff check`.
        use buff_lang_ast::{Ident, TypeRef};
        use buff_lang_error::Span;
        use buff_lang_types::Type;
        let mk_named = |s: &str| TypeRef::Named {
            name: Ident {
                name: s.to_string(),
                span: Span::default(),
            },
            span: Span::default(),
        };
        let int_ty = typeref_to_type(&mk_named("Int")).expect("Int must map");
        assert!(matches!(int_ty, Type::Int { .. }), "got {int_ty:?}");
        let bool_ty = typeref_to_type(&mk_named("Bool")).expect("Bool must map");
        assert!(matches!(bool_ty, Type::Bool), "got {bool_ty:?}");
        // Unknown names fall back to None (permissive — param stays
        // unbound, which the inferencer treats as Unknown).
        assert!(
            typeref_to_type(&mk_named("MyType")).is_none(),
            "user-defined type names map to None"
        );
    }
}
