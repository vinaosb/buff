//! T105a - extern-crate emit-on-demand detection (program_uses_* walkers, part 2) + error_struct_items (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move - no logic changes. Child module of rust_codegen so it
//! inherits the parent imports via use super::* (zero per-module import lists).
//! Functions are pub(super) so the parent reaches them through the glob below.

use super::*;

// ---------------------------------------------------------------------------
// T124i - serde_yml + csv emit-on-demand detection (Yaml / Csv namespace
// modules).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any `Yaml.<method>(...)` call
/// (T124i). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to record `"serde_yml"` in the extern-crate
/// set so the pipeline knows the generated Cargo project depends on the
/// `serde_yml` crate (the maintained fork of the deprecated
/// `serde_yaml`).
///
/// Detection recognises the `Yaml` namespace as the receiver of a method
/// call (`Yaml.parse(s)`, `Yaml.stringify(v)`). The method name is NOT
/// matched here - `Yaml` is a reserved prelude namespace, so any
/// `Yaml.<anything>()` triggers `serde_yml` registration. Codegen will
/// surface a clear error if `<anything>` is not one of parse/stringify.
///
/// Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/hex/percent-
/// encoding/uuid/url detection patterns
/// (T124b/T124c/T124d/T124e/T124f/T124g/T124h); reuses the generic
/// `program_uses_namespace` helper (introduced in T124h for the five
/// web modules) so Yaml's walker is a one-liner. The walker is NARROW
/// (per the T124f gotcha that chrono was originally over-broad): it
/// flags ONLY the bare-Ident receiver name `Yaml`, NOT every prelude-
/// type Ident, NOT every method-name match.
pub(super) fn program_uses_serde_yml(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "Yaml")
}

/// T23: detect `Json.<method>(...)` calls. Returns `true` if at least
/// one is found, signalling [`RustCodegen::generate`] to record
/// `"serde_json"` in the extern-crate set so the pipeline knows the
/// generated Cargo project depends on the `serde_json` crate.
///
/// Mirrors the `program_uses_serde_yml` walker (T124i twin); reuses the
/// generic `program_uses_namespace` helper so Json's walker is also a
/// one-liner. The walker is NARROW: flags ONLY the bare-Ident receiver
/// name `Json`, NOT every prelude-type Ident, NOT every method-name match.
pub(super) fn program_uses_serde_json(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "Json")
}

/// Walk the declaration list looking for any `Csv.<method>(...)` call
/// (T124i). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to record `"csv"` in the extern-crate set
/// so the pipeline knows the generated Cargo project depends on the
/// `csv` crate (burntsushi/rust-csv).
///
/// Detection recognises the `Csv` namespace as the receiver of a method
/// call (`Csv.parse(s)`, `Csv.stringify(rows)`). The method name is NOT
/// matched here - `Csv` is a reserved prelude namespace, so any
/// `Csv.<anything>()` triggers `csv` registration. Codegen will surface
/// a clear error if `<anything>` is not one of parse/stringify.
///
/// Mirrors the `program_uses_serde_yml` walker (T124i twin); reuses the
/// generic `program_uses_namespace` helper so Csv's walker is also a
/// one-liner. The walker is NARROW (per the T124f gotcha): flags ONLY
/// the bare-Ident receiver name `Csv`, NOT every prelude-type Ident,
/// NOT every method-name match.
pub(super) fn program_uses_csv(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "Csv")
}

// ---------------------------------------------------------------------------
// T124j - filesystem module emit-on-demand detection (walkdir + tempfile
// extern crates). Two narrow walkers flag the specific receiver names
// (`Dir.walk` triggers walkdir; `Tempfile.create` / `Tempfile.dir`
// trigger tempfile). They reuse the generic `program_uses_namespace`
// helper introduced in T124h. The chrono over-broad-walker gotcha
// (T124f) is the cautionary tale: each walker stays minimal so it
// doesn't over-trigger on unrelated code.
//
// NOTE: `Dir.list` / `Dir.create` / `Dir.remove` use std::fs::*
// (std-only - NO extern crate needed, mirroring the Math/Strings/
// Args/Env stance from T124f/T124g). `Path` (value type) and its
// instance methods (parent/extension/basename/exists) use
// std::path::* (also std-only). `Tempfile.dir` uses std::env::temp_dir
// (std-only), but the narrow walker records `tempfile` for symmetry
// (any Tempfile.* call flags the crate).
// ---------------------------------------------------------------------------

/// T124j: detect `Dir.walk(...)` calls. The `walkdir` crate is
/// needed ONLY for `Dir.walk` (Dir.list/create/remove use std::fs - no
/// extern crate). A NARROW method-aware walker is required here: a
/// generic `program_uses_namespace("Dir")` would over-register walkdir
/// for programs using only Dir.list/create/remove (those compile
/// without walkdir in [dependencies]).
///
/// Detection recognises a `MethodCall` whose receiver is the bare Ident
/// `Dir` AND whose method name is exactly `walk`. The receiver-name
/// gate mirrors the chrono-over-broad cautionary tale (T124f gotcha):
/// flags ONLY the specific (Dir, walk) combination, NOT every
/// `Dir.<anything>()` call.
pub(super) fn program_uses_walkdir(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_dir_walk(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_walkdir`]: scan a block for
/// `Dir.walk(...)` calls.
pub(super) fn block_uses_dir_walk(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_dir_walk)
}

/// Check a single statement (and its nested expressions) for
/// `Dir.walk(...)` usage. Mirrors the `stmt_uses_namespace` shape
/// exactly with the additional `walk` method-name gate.
pub(super) fn stmt_uses_dir_walk(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_dir_walk(value),
        Stmt::Assignment { target, value, .. } => {
            expr_uses_dir_walk(target) || expr_uses_dir_walk(value)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_dir_walk(iter) || block_uses_dir_walk(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_dir_walk(cond) || block_uses_dir_walk(body),
        Stmt::While { cond, body, .. } => expr_uses_dir_walk(cond) || block_uses_dir_walk(body),
        Stmt::ForLet { value, body, .. } => expr_uses_dir_walk(value) || block_uses_dir_walk(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_dir_walk(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_dir_walk(e),
            }) || block_uses_dir_walk(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_dir_walk(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_dir_walk(body),
    }
}

/// Recursively scan an expression tree for a `Dir.walk(...)` call.
/// Same conservative bare-Ident-receiver + method-name strategy.
pub(super) fn expr_uses_dir_walk(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            // Match `Dir.walk(...)` exactly: bare Ident `Dir` receiver
            // AND method name `walk`. Other Dir methods (list/create/
            // remove) do NOT trigger walkdir registration (they use
            // std::fs::* - no extern crate needed).
            if method.name == "walk" {
                if let Expr::Ident(id, _) = receiver.as_ref() {
                    if id.name == "Dir" {
                        return true;
                    }
                }
            }
            expr_uses_dir_walk(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_dir_walk(lhs) || expr_uses_dir_walk(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_dir_walk(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_dir_walk(callee) || args.iter().any(expr_uses_dir_walk)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_dir_walk(cond)
                || block_uses_dir_walk(then_block)
                || else_block.as_ref().is_some_and(block_uses_dir_walk)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_dir_walk(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_dir_walk),
        Expr::Index { base, indices, .. } => {
            expr_uses_dir_walk(base) || indices.iter().any(expr_uses_dir_walk)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_dir_walk(k) || expr_uses_dir_walk(v)),
        Expr::Lambda { body, .. } => block_uses_dir_walk(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_dir_walk(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_dir_walk(scrutinee) || arms.iter().any(|arm| block_uses_dir_walk(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_dir_walk(inner),
        Expr::Try { expr, .. } => expr_uses_dir_walk(expr),
        Expr::Spawn { task, .. } => expr_uses_dir_walk(task),
        Expr::Range { start, end, .. } => expr_uses_dir_walk(start) || expr_uses_dir_walk(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_dir_walk(value)
                || block_uses_dir_walk(then_block)
                || else_block.as_ref().is_some_and(block_uses_dir_walk)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_dir_walk),
        Expr::NamedArg { value, .. } => expr_uses_dir_walk(value),
    }
}

/// T124j: detect `Tempfile.create()` / `Tempfile.dir()` calls. The
/// `tempfile` crate is needed for `Tempfile.create` (the
/// `NamedTempFile::new()` API). `Tempfile.dir` uses std::env::temp_dir
/// (std-only) but the narrow walker records `tempfile` for symmetry -
/// a program using `Tempfile.dir` likely uses `Tempfile.create` too,
/// and over-registration is benign (rustc never errors on unused
/// dependencies when cargo registers them).
///
/// Detection recognises the `Tempfile` namespace as the receiver of a
/// method call (`Tempfile.create()`, `Tempfile.dir()`). The method
/// name is NOT matched here - `Tempfile` is a reserved prelude
/// namespace, so any `Tempfile.<anything>()` triggers `tempfile`
/// registration. Codegen will surface a clear error if `<anything>`
/// is not one of create/dir.
///
/// Mirrors the serde_yml / csv walker pattern (T124i); reuses the
/// generic `program_uses_namespace` helper so Tempfile's walker is a
/// one-liner. The walker is NARROW (per the T124f gotcha): flags ONLY
/// the bare-Ident receiver name `Tempfile`, NOT every prelude-type
/// Ident, NOT every method-name match.
pub(super) fn program_uses_tempfile(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "Tempfile")
}

// ---------------------------------------------------------------------------
// T124k - crypto module emit-on-demand detection (sha2 + md5 + hmac
// extern crates). Three NARROW walkers flag the specific (receiver,
// method) combinations so a program using only `Hash.md5` doesn't
// pull in `sha2` (and vice versa). They mirror the `program_uses_dir_walk`
// shape (T124j) - method-aware narrow walkers - rather than the
// `program_uses_namespace` one-liner (T124h/T124i) which would
// over-register.
//
// `hex` recording is handled in the `generate()` caller (recorded
// when ANY of sha2/md5/hmac fires, since every Hash.* / HMAC.* call
// emits a `hex::encode(...)` for the digest / MAC bytes).
//
// NOTE: `HMAC.sha256` lowers to `hmac::Hmac<sha2::Sha256>` so the
// `hmac` walker ALSO records `sha2` (idempotent if the program also
// uses Hash.sha256/sha512 - extern_crates is a BTreeSet). This is
// handled in the `generate()` caller, NOT in the walker itself (the
// walker stays minimal - one crate per walker).
// ---------------------------------------------------------------------------

/// T124k: detect `Hash.sha256(...)` / `Hash.sha512(...)` /
/// `HMAC.sha256(...)` calls. The `sha2` crate is needed for any of
/// these three (SHA-2 family digest for sha256/sha512; `Sha256` as
/// the inner hasher for HMAC-SHA256). A NARROW method-aware walker
/// is required: a generic `program_uses_namespace("Hash")` would
/// over-register sha2 for programs using only `Hash.md5` (which
/// needs `md5`, NOT `sha2`); symmetrically, the HMAC.sha256 call
/// lives on a DIFFERENT receiver (`HMAC`, not `Hash`) so a pure
/// Hash-only walker would miss it.
///
/// Detection recognises a `MethodCall` whose receiver is the bare
/// Ident `Hash` AND whose method name is `sha256` OR `sha512`, OR
/// whose receiver is the bare Ident `HMAC` AND whose method name is
/// `sha256`. The receiver-name + method-name gate mirrors the
/// chrono-over-broad cautionary tale (T124f gotcha): flags ONLY the
/// specific (receiver, method) combinations that lower to sha2.
pub(super) fn program_uses_sha2(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_sha2(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_sha2`]: scan a block for
/// `Hash.sha256` / `Hash.sha512` / `HMAC.sha256` calls.
pub(super) fn block_uses_sha2(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_sha2)
}

/// Check a single statement (and its nested expressions) for
/// sha2-triggering usage. Mirrors the `stmt_uses_dir_walk` shape
/// exactly with the additional method-name + receiver-name gate.
pub(super) fn stmt_uses_sha2(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_sha2(value),
        Stmt::Assignment { target, value, .. } => expr_uses_sha2(target) || expr_uses_sha2(value),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_sha2(iter) || block_uses_sha2(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_sha2(cond) || block_uses_sha2(body),
        Stmt::While { cond, body, .. } => expr_uses_sha2(cond) || block_uses_sha2(body),
        Stmt::ForLet { value, body, .. } => expr_uses_sha2(value) || block_uses_sha2(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_sha2(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_sha2(e),
            }) || block_uses_sha2(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_sha2(expr),
        // T53: comptime block — recurse into body for sha2 detection.
        Stmt::ComptimeBlock { body, .. } => block_uses_sha2(body),
    }
}

/// Recursively scan an expression tree for a sha2-triggering call
/// (`Hash.sha256` / `Hash.sha512` / `HMAC.sha256`). Same conservative
/// bare-Ident-receiver + method-name strategy as `expr_uses_dir_walk`.
pub(super) fn expr_uses_sha2(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            // Match the three (receiver, method) pairs that lower to
            // sha2: (Hash, sha256) / (Hash, sha512) / (HMAC, sha256).
            if method.name == "sha256" || method.name == "sha512" {
                if let Expr::Ident(id, _) = receiver.as_ref() {
                    if (id.name == "Hash" && (method.name == "sha256" || method.name == "sha512"))
                        || (id.name == "HMAC" && method.name == "sha256")
                    {
                        return true;
                    }
                }
            }
            expr_uses_sha2(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_sha2(lhs) || expr_uses_sha2(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_sha2(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_sha2(callee) || args.iter().any(expr_uses_sha2)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_sha2(cond)
                || block_uses_sha2(then_block)
                || else_block.as_ref().is_some_and(block_uses_sha2)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_sha2(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_sha2),
        Expr::Index { base, indices, .. } => {
            expr_uses_sha2(base) || indices.iter().any(expr_uses_sha2)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_sha2(k) || expr_uses_sha2(v)),
        Expr::Lambda { body, .. } => block_uses_sha2(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_sha2(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_sha2(scrutinee) || arms.iter().any(|arm| block_uses_sha2(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_sha2(inner),
        Expr::Try { expr, .. } => expr_uses_sha2(expr),
        Expr::Spawn { task, .. } => expr_uses_sha2(task),
        Expr::Range { start, end, .. } => expr_uses_sha2(start) || expr_uses_sha2(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_sha2(value)
                || block_uses_sha2(then_block)
                || else_block.as_ref().is_some_and(block_uses_sha2)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_sha2),
        Expr::NamedArg { value, .. } => expr_uses_sha2(value),
    }
}

/// T124k: detect `Hash.md5(...)` calls. The `md5` crate is needed
/// ONLY for `Hash.md5` (the SHA-2 methods record `sha2` instead). A
/// NARROW method-aware walker is required here: a generic
/// `program_uses_namespace("Hash")` would over-register md5 for
/// programs using only `Hash.sha256`/`sha512`.
///
/// Detection recognises a `MethodCall` whose receiver is the bare
/// Ident `Hash` AND whose method name is exactly `md5`. The
/// receiver-name + method-name gate mirrors the chrono-over-broad
/// cautionary tale (T124f gotcha).
pub(super) fn program_uses_md5(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_md5(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_md5`]: scan a block for
/// `Hash.md5(...)` calls.
pub(super) fn block_uses_md5(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_md5)
}

/// Check a single statement (and its nested expressions) for
/// `Hash.md5(...)` usage. Mirrors the `stmt_uses_dir_walk` /
/// `stmt_uses_sha2` shape exactly.
pub(super) fn stmt_uses_md5(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_md5(value),
        Stmt::Assignment { target, value, .. } => expr_uses_md5(target) || expr_uses_md5(value),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_md5(iter) || block_uses_md5(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_md5(cond) || block_uses_md5(body),
        Stmt::While { cond, body, .. } => expr_uses_md5(cond) || block_uses_md5(body),
        Stmt::ForLet { value, body, .. } => expr_uses_md5(value) || block_uses_md5(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_md5(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_md5(e),
            }) || block_uses_md5(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_md5(expr),
        // T53: comptime block — recurse into body for md5 detection.
        Stmt::ComptimeBlock { body, .. } => block_uses_md5(body),
    }
}

/// Recursively scan an expression tree for a `Hash.md5(...)` call.
/// Same conservative bare-Ident-receiver + method-name strategy.
pub(super) fn expr_uses_md5(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            // Match `Hash.md5(...)` exactly: bare Ident `Hash`
            // receiver AND method name `md5`.
            if method.name == "md5" {
                if let Expr::Ident(id, _) = receiver.as_ref() {
                    if id.name == "Hash" {
                        return true;
                    }
                }
            }
            expr_uses_md5(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_md5(lhs) || expr_uses_md5(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_md5(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_md5(callee) || args.iter().any(expr_uses_md5)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_md5(cond)
                || block_uses_md5(then_block)
                || else_block.as_ref().is_some_and(block_uses_md5)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_md5(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_md5),
        Expr::Index { base, indices, .. } => {
            expr_uses_md5(base) || indices.iter().any(expr_uses_md5)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_md5(k) || expr_uses_md5(v)),
        Expr::Lambda { body, .. } => block_uses_md5(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_md5(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_md5(scrutinee) || arms.iter().any(|arm| block_uses_md5(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_md5(inner),
        Expr::Try { expr, .. } => expr_uses_md5(expr),
        Expr::Spawn { task, .. } => expr_uses_md5(task),
        Expr::Range { start, end, .. } => expr_uses_md5(start) || expr_uses_md5(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_md5(value)
                || block_uses_md5(then_block)
                || else_block.as_ref().is_some_and(block_uses_md5)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_md5),
        Expr::NamedArg { value, .. } => expr_uses_md5(value),
    }
}

/// T124k: detect `HMAC.sha256(...)` calls. The `hmac` crate is needed
/// ONLY for `HMAC.sha256` (Hash.* records `sha2` / `md5` instead). A
/// NARROW method-aware walker is required: a generic
/// `program_uses_namespace("HMAC")` would over-register hmac for
/// programs using any future HMAC method that doesn't lower to
/// `hmac::Hmac` (none today, but the narrow stance is future-proof).
///
/// Detection recognises a `MethodCall` whose receiver is the bare
/// Ident `HMAC` AND whose method name is exactly `sha256`. The
/// receiver-name + method-name gate mirrors the sha2/md5 walkers
/// (T124k) + the chrono-over-broad cautionary tale (T124f gotcha).
///
/// NOTE: the `generate()` caller ALSO records `sha2` when this walker
/// fires (HMAC.sha256 lowers to `hmac::Hmac<sha2::Sha256>` so the
/// path needs both crates). That cross-crate coupling is handled in
/// the caller (not the walker) so the walker stays minimal - one
/// crate per walker.
pub(super) fn program_uses_hmac(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_hmac(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_hmac`]: scan a block for
/// `HMAC.sha256(...)` calls.
pub(super) fn block_uses_hmac(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_hmac)
}

/// Check a single statement (and its nested expressions) for
/// `HMAC.sha256(...)` usage. Mirrors the `stmt_uses_sha2` /
/// `stmt_uses_md5` shape exactly.
pub(super) fn stmt_uses_hmac(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_hmac(value),
        Stmt::Assignment { target, value, .. } => expr_uses_hmac(target) || expr_uses_hmac(value),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_hmac(iter) || block_uses_hmac(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_hmac(cond) || block_uses_hmac(body),
        Stmt::While { cond, body, .. } => expr_uses_hmac(cond) || block_uses_hmac(body),
        Stmt::ForLet { value, body, .. } => expr_uses_hmac(value) || block_uses_hmac(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_hmac(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_hmac(e),
            }) || block_uses_hmac(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_hmac(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_hmac(body),
    }
}

/// Recursively scan an expression tree for a `HMAC.sha256(...)` call.
/// Same conservative bare-Ident-receiver + method-name strategy.
pub(super) fn expr_uses_hmac(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            // Match `HMAC.sha256(...)` exactly: bare Ident `HMAC`
            // receiver AND method name `sha256`. NOTE: `HMAC` is the
            // Buff namespace name (all-uppercase); the underlying
            // Rust crate + type is `hmac::Hmac<...>` (lowercase).
            if method.name == "sha256" {
                if let Expr::Ident(id, _) = receiver.as_ref() {
                    if id.name == "HMAC" {
                        return true;
                    }
                }
            }
            expr_uses_hmac(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_hmac(lhs) || expr_uses_hmac(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_hmac(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_hmac(callee) || args.iter().any(expr_uses_hmac)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_hmac(cond)
                || block_uses_hmac(then_block)
                || else_block.as_ref().is_some_and(block_uses_hmac)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_hmac(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_hmac),
        Expr::Index { base, indices, .. } => {
            expr_uses_hmac(base) || indices.iter().any(expr_uses_hmac)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_hmac(k) || expr_uses_hmac(v)),
        Expr::Lambda { body, .. } => block_uses_hmac(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_hmac(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_hmac(scrutinee) || arms.iter().any(|arm| block_uses_hmac(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_hmac(inner),
        Expr::Try { expr, .. } => expr_uses_hmac(expr),
        Expr::Spawn { task, .. } => expr_uses_hmac(task),
        Expr::Range { start, end, .. } => expr_uses_hmac(start) || expr_uses_hmac(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_hmac(value)
                || block_uses_hmac(then_block)
                || else_block.as_ref().is_some_and(block_uses_hmac)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_hmac),
        Expr::NamedArg { value, .. } => expr_uses_hmac(value),
    }
}

// ---------------------------------------------------------------------------
// T124l - system module emit-on-demand detection (num_cpus extern crate).
// ONE narrow walker flags the specific (receiver, method) combination
// (`OS.cpus`) so a program using only `OS.name` / `OS.arch` / `OS.hostname`
// doesn't pull in `num_cpus` (those calls use std::env::consts + env-var
// hostname - std-only). It mirrors the `program_uses_dir_walk` shape
// (T124j) - method-aware narrow walker - rather than the
// `program_uses_namespace` one-liner (T124h/T124i) which would
// over-register.
//
// NOTE: `Process.*` uses `std::process::*` (std-only - NO extern crate
// needed, mirrors the Path / Dir.list / Tempfile.dir stance from T124j).
// No walker is needed for Process - it never records an extern crate.
// ---------------------------------------------------------------------------

/// T124l: detect `OS.cpus()` calls. The `num_cpus` crate is needed
/// ONLY for `OS.cpus` (`OS.name` / `OS.arch` use compile-time
/// `std::env::consts` and `OS.hostname` uses env-var lookup - all
/// std-only with NO extern crate needed). A NARROW method-aware
/// walker is required: a generic `program_uses_namespace("OS")`
/// would over-register num_cpus for programs using only
/// `OS.name` / `OS.arch` / `OS.hostname` (those compile without
/// num_cpus in [dependencies]).
///
/// Detection recognises a `MethodCall` whose receiver is the bare
/// Ident `OS` AND whose method name is exactly `cpus`. The
/// receiver-name + method-name gate mirrors the chrono-over-broad
/// cautionary tale (T124f gotcha): flags ONLY the specific (OS,
/// cpus) combination, NOT every `OS.<anything>()` call.
pub(super) fn program_uses_num_cpus(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_num_cpus(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_num_cpus`]: scan a block for
/// `OS.cpus(...)` calls.
pub(super) fn block_uses_num_cpus(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_num_cpus)
}

/// Check a single statement (and its nested expressions) for
/// `OS.cpus(...)` usage. Mirrors the `stmt_uses_dir_walk` shape
/// exactly with the `cpus` method-name gate.
pub(super) fn stmt_uses_num_cpus(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_num_cpus(value),
        Stmt::Assignment { target, value, .. } => {
            expr_uses_num_cpus(target) || expr_uses_num_cpus(value)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_num_cpus(iter) || block_uses_num_cpus(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_num_cpus(cond) || block_uses_num_cpus(body),
        Stmt::While { cond, body, .. } => expr_uses_num_cpus(cond) || block_uses_num_cpus(body),
        Stmt::ForLet { value, body, .. } => expr_uses_num_cpus(value) || block_uses_num_cpus(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_num_cpus(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_num_cpus(e),
            }) || block_uses_num_cpus(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_num_cpus(expr),
        Stmt::ComptimeBlock { body, .. } => block_uses_num_cpus(body),
    }
}

/// Recursively scan an expression tree for an `OS.cpus(...)` call.
/// Same conservative bare-Ident-receiver + method-name strategy as
/// the dir_walk / sha2 / md5 / hmac walkers.
pub(super) fn expr_uses_num_cpus(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            // Match `OS.cpus(...)` exactly: bare Ident `OS` receiver
            // AND method name `cpus`. Other OS methods (name / arch /
            // hostname) do NOT trigger num_cpus registration (they
            // use std::env::consts / env-var - no extern crate
            // needed).
            if method.name == "cpus" {
                if let Expr::Ident(id, _) = receiver.as_ref() {
                    if id.name == "OS" {
                        return true;
                    }
                }
            }
            expr_uses_num_cpus(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_num_cpus(lhs) || expr_uses_num_cpus(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_num_cpus(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_num_cpus(callee) || args.iter().any(expr_uses_num_cpus)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_num_cpus(cond)
                || block_uses_num_cpus(then_block)
                || else_block.as_ref().is_some_and(block_uses_num_cpus)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e, _) => expr_uses_num_cpus(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_num_cpus),
        Expr::Index { base, indices, .. } => {
            expr_uses_num_cpus(base) || indices.iter().any(expr_uses_num_cpus)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_num_cpus(k) || expr_uses_num_cpus(v)),
        Expr::Lambda { body, .. } => block_uses_num_cpus(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_num_cpus(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_num_cpus(scrutinee) || arms.iter().any(|arm| block_uses_num_cpus(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_num_cpus(inner),
        Expr::Try { expr, .. } => expr_uses_num_cpus(expr),
        Expr::Spawn { task, .. } => expr_uses_num_cpus(task),
        Expr::Range { start, end, .. } => expr_uses_num_cpus(start) || expr_uses_num_cpus(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_num_cpus(value)
                || block_uses_num_cpus(then_block)
                || else_block.as_ref().is_some_and(block_uses_num_cpus)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_num_cpus),
        Expr::NamedArg { value, .. } => expr_uses_num_cpus(value),
    }
}

// ---------------------------------------------------------------------------
// T124m - networking module emit-on-demand detection (WebSocket only).
// TCP.* and UDP.* reuse the existing `program_uses_tokio` walker (they
// also lower to `tokio::*` paths, so the existing sleep-callee-based
// walker would NOT fire on TCP/UDP calls alone - we extend the tokio
// walker to ALSO flag `TCP.<method>(...)` / `UDP.<method>(...)` calls
// below). The new `program_uses_tokio_tungstenite` walker is NARROW:
// gated ONLY on `WebSocket.<method>(...)` usage (mirrors the chrono-
// over-broad cautionary tale, T124f gotcha). See the `generate()`
// caller for the matching `extern_crates.insert("tokio-tungstenite")`
// + `extern_crates.insert("futures-util")` calls.
// ---------------------------------------------------------------------------

/// T124m: detect `WebSocket.<method>(...)` calls. The
/// `tokio-tungstenite` + `futures-util` crates are needed ONLY for
/// `WebSocket.*` (TCP.* / UDP.* record `tokio` via the existing
/// tokio walker, which is reused - see below). A NARROW
/// receiver-aware walker is required: a generic
/// `program_uses_namespace("WebSocket")` would over-register the
/// crates for programs that import but don't call (no such program
/// today, but the narrow stance is future-proof).
///
/// Detection recognises a `MethodCall` whose receiver is the bare
/// Ident `WebSocket` (e.g. `WebSocket.connect(url)`). The
/// receiver-name gate mirrors the chrono-over-broad cautionary tale
/// (T124f gotcha) and the existing namespace walkers (T124h
/// Base64 / Hex / URLEncode / UUID / URL).
pub(super) fn program_uses_tokio_tungstenite(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "WebSocket")
}

/// T124m: detect `TCP.<method>(...)` calls. Returns `true` if at
/// least one is found, signalling [`RustCodegen::generate`] to
/// record the `tokio` crate in the extern-crate set (idempotent
/// with the existing tokio walker from T124g - the existing walker
/// flags ONLY the bare-Ident `sleep(...)` free-fn call, NOT
/// TCP / UDP / WebSocket calls, so this walker is the canonical
/// TCP / UDP -> tokio signal).
pub(super) fn program_uses_tcp(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "TCP")
}

/// T124m: detect `UDP.<method>(...)` calls. Same shape as
/// [`program_uses_tcp`] - flags `UDP.connect` / `UDP.bind` usage to
/// record the `tokio` crate in extern_crates.
pub(super) fn program_uses_udp(decls: &[Decl]) -> bool {
    program_uses_namespace(decls, "UDP")
}

/// Build the builtin `Error` struct + its `new` impl + `Display` + Error trait
/// impls as a `Vec<Item>` (T30).
///
/// Emits (conceptually):
///
/// ```rust,ignore
/// #[derive(Clone, Debug)]
/// pub struct Error {
///     pub message: String,
/// }
///
/// impl Error {
///     pub fn new(message: impl Into<String>) -> Self {
///         Self { message: message.into() }
///     }
/// }
///
/// impl std::fmt::Display for Error {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         write!(f, "{}", self.message)
///     }
/// }
///
/// impl std::error::Error for Error {}
/// ```
///
/// This makes `Error` a proper Rust error type: it implements
/// `std::error::Error` (so `?` propagation's `From` bound is satisfiable
/// when the enclosing fn returns `Result<T, Error>`), `Display` (required by
/// `std::error::Error`), and `Debug` + `Clone` (consistent with every other
/// generated type via [`derive_and_repr_attrs`]).
///
/// Built via the same fixed-template-then-`syn::parse_str` approach as
/// [`matrix_struct_items`] (T24). See that function's docstring for the
/// "this is NOT raw-string codegen" rationale — the string is a
/// compile-time-fixed scaffold re-parsed into `syn::Item`s.
pub(super) fn error_struct_items() -> Vec<Item> {
    let src = r#"
        #[derive(Clone, Debug)]
        pub struct Error {
            pub message: String,
        }

        impl Error {
            pub fn new(message: impl Into<String>) -> Self {
                Self {
                    message: message.into(),
                }
            }
        }

        impl std::fmt::Display for Error {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.message)
            }
        }

        impl std::error::Error for Error {}
    "#;
    match syn::parse_str::<File>(src) {
        Ok(file) => file.items,
        Err(_) => Vec::new(),
    }
}
