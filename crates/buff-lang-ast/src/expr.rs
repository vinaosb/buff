//! Expression nodes for the Buff AST.
//!
//! Every variant of [`Expr`] carries a [`Span`] so diagnostics can point at the
//! exact source range. Expressions own all their data (no lifetimes).

use std::fmt;

use crate::common::{Block, Ident, Param};
use crate::op::{BinaryOp, UnaryOp};
use crate::ty::TypeRef;
use buff_lang_error::Span;

/// A literal value embedded directly in the source.
///
/// NOTE: derives [`PartialEq`] but **not** [`Eq`] because `f32`/`f64` don't
/// implement `Eq`. Same applies to any type that contains a [`Literal`].
///
/// # Migration notes (additive AST changes)
///
/// ## T20 — `Literal::Decimal`
///
/// `Literal::Decimal` was **added** in T20 (v0.5) to carry the raw text of a
/// 128-bit fixed-point decimal literal (e.g. `99.90m`). It stores the source
/// text verbatim as a [`String`] so the value never rounds through `f32`/`f64`
/// — exactness is preserved all the way to the `rust_decimal_macros::dec!()`
/// codegen call. [`Type`](buff_lang_types::Type)::Decimal already existed in
/// `buff-lang-types`; the literal variant closed the gap.
///
/// ## T21 — `Literal::Char`
///
/// `Literal::Char` was **added** in T21 (v0.5) to represent a single Unicode
/// scalar value literal written with single quotes: `'A'`, `'é'`, `'🚀'`. It
/// stores a Rust [`char`] (which is always exactly one scalar value, never a
/// grapheme cluster). This is **additive**: no existing variant was renamed,
/// reordered, or had its payload altered, so all prior `match` arms remain
/// exhaustive. [`Type`](buff_lang_types::Type)::Char was added in lockstep.
/// `char` is `Copy + Eq`, so the `PartialEq`-but-not-`Eq` derivation rule is
/// unaffected.
///
/// ## T79 — `Literal::Regex`
///
/// `Literal::Regex` was **added** in T79 (v0.5) to carry the raw pattern
/// text of a regex literal written with slashes: `/\d+/`, `/\d{3}-\d{4}/`.
/// It stores the source text BETWEEN the slashes (excluding both
/// delimiters) so backslash classes survive verbatim (`/\d+/` →
/// `Regex("\\d+")`). This is **additive**: no existing variant was renamed,
/// reordered, or had its payload altered, so all prior `match` arms remain
/// exhaustive.
///
/// **Codegen is deferred in v0.5.** The generated Cargo project has no
/// `regex` crate dependency (T32-style dep wiring is a separate v1.0 task),
/// so emitting `Regex::new(...)` would fail to compile. As a documented
/// stub, codegen lowers `Literal::Regex(p)` to the pattern string `"<p>"`
/// (valid standalone Rust) so the pipeline stays green. Real
/// `regex::Regex::new` lowering + Cargo-project dep injection arrives in v1.0.
/// Inference treats the value as `Type::String` to match the stub.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// An integer literal, e.g. `42`. Stored as `i64`.
    Int(i64),
    /// A 32-bit float literal, e.g. `3.14f`.
    Float(f32),
    /// A 64-bit float literal, e.g. `99.9d`.
    Double(f64),
    /// A boolean literal: `true` / `false`.
    Bool(bool),
    /// A string literal, e.g. `"hello"`.
    String(String),
    /// A single byte literal, e.g. `0xFF`.
    Byte(u8),
    /// A single Unicode scalar value literal, e.g. `'A'`, `'é'`, `'🚀'` (T21).
    ///
    /// Always exactly one `char`; never a grapheme cluster. The lexer rejects
    /// `''` (empty) and `'ab'` (two scalar values).
    Char(char),
    /// A 128-bit fixed-point decimal literal, e.g. `99.90m` (T20).
    ///
    /// Stores the **raw source text** (without the trailing `m`/`M` suffix)
    /// so the value is never rounded through `f32`/`f64`. Codegen emits this
    /// verbatim as `rust_decimal_macros::dec!(<text>)`, preserving exactness.
    Decimal(String),
    /// A regex literal, e.g. `/\d+/`, `/\d{3}-\d{4}/` (T79).
    ///
    /// Stores the raw source text BETWEEN the slashes (excluding both
    /// delimiters). Backslash classes survive verbatim (`/\d+/` →
    /// `Regex("\\d+")`); an escaped `\/` inside the pattern keeps its
    /// backslash in the stored text.
    ///
    /// **Codegen is deferred in v0.5** — the generated Cargo project has no
    /// `regex` crate dep, so codegen stubs this as a plain `String` literal
    /// (valid standalone Rust). Real `regex::Regex::new(...)` lowering +
    /// Cargo-project dep injection arrives in v1.0. Inference treats the
    /// value as `Type::String` to match the stub.
    Regex(String),
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::Int(v) => write!(f, "Int({v})"),
            Literal::Float(v) => write!(f, "Float({v})"),
            Literal::Double(v) => write!(f, "Double({v})"),
            Literal::Bool(v) => write!(f, "Bool({v})"),
            Literal::String(v) => write!(f, "String({v:?})"),
            Literal::Byte(v) => write!(f, "Byte(0x{v:02X})"),
            // T21: render the char in single quotes for visual distinction
            // from String (which uses double quotes) and Byte (0x..).
            Literal::Char(c) => write!(f, "Char({c:?})"),
            // Show the decimal text double-quoted (consistent with String)
            // so it's visually distinct from a bare number.
            Literal::Decimal(v) => write!(f, "Decimal({v:?})"),
            // T79: show the regex pattern double-quoted with the `Regex`
            // prefix so it's visually distinct from a plain String.
            Literal::Regex(v) => write!(f, "Regex({v:?})"),
        }
    }
}

/// One piece of a string-interpolation expression (T21).
///
/// An interpolation like `"Hello {name}, you are {age}!"` parses to:
///
/// ```text
/// StringInterp {
///     parts: vec![
///         InterpPart::Literal("Hello ".to_string()),
///         InterpPart::Expr(Box::new(Expr::Ident("name"))),
///         InterpPart::Literal(", you are ".to_string()),
///         InterpPart::Expr(Box::new(Expr::Ident("age"))),
///         InterpPart::Literal("!".to_string()),
///     ],
///     span: ...,
/// }
/// ```
///
/// Literal runs of zero or more `StringPart` tokens are collapsed into one
/// `InterpPart::Literal` so adjacent strings stay readable. Adjacent
/// expressions are kept separate (one `InterpPart::Expr` each).
#[derive(Debug, Clone, PartialEq)]
pub enum InterpPart {
    /// A literal text run from the source string (no escapes processed yet).
    Literal(String),
    /// An embedded expression `{expr}`.
    Expr(Box<Expr>),
}

impl fmt::Display for InterpPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InterpPart::Literal(s) => write!(f, "Lit({s:?})"),
            InterpPart::Expr(e) => write!(f, "Expr({e})"),
        }
    }
}

/// A top-level expression. Every variant is annotated with its source [`Span`].
///
/// # Migration notes (additive AST changes)
///
/// ## T23 — `Expr::ArrayLit` and `Expr::Index`
///
/// Two variants were **added** in T23 (v0.5) to bring collection support online:
///
/// - [`Expr::ArrayLit`] — a collection literal `[e1, e2, e3]` (or empty `[]`).
///   Lowers to Rust's `vec![...]` macro and infers type `Vector<T>` where `T`
///   is the element type (integer literals get auto-width via T22 range
///   analysis, so `[1, 2, 3]` -> `Vector<Int<8>>`). This is **additive**: no
///   existing variant was renamed, reordered, or had its payload altered.
///
///     - [`Expr::Index`] — an indexing expression `base[index]`. Lowers to Rust
///       `base[index as usize]` (the index is coerced to `usize`). String-literal
///       receivers are still rejected at parse time with the helpful T21 message
///       ("for strings use .chars() or .first()"); all other receivers produce an
///       `Expr::Index` node. This unblocks T99's deferred `args()[0]` and T21's
///       typed string-index rejection.
///
/// Both variants derive the standard `Debug, Clone, PartialEq` (no `Eq` because
/// the containing `Expr` already isn't `Eq` due to floats). All internal
/// `match`es on `Expr` were extended with arms for the new variants: `span()`,
/// `Display`, parser, type inference, and Rust codegen.
///
/// ## T24 — `Expr::Index` generalized to multi-index
///
/// The `Expr::Index` variant was **generalized** in T24 (v0.5) to carry a
/// `Vec<Expr>` of indices instead of a single index, enabling 2-D matrix
/// indexing `m[row, col]`. The shape changed from `{ base, index, span }` to
/// `{ base, indices: Vec<Expr>, span }`. This is a **migration** (not purely
/// additive — the field was renamed/retyped), so every `match`/construction
/// site was updated: `span()`, `Display`, parser, type inference, Rust codegen,
/// IR `collect_uses`, and the T23 `vector_codegen` test helper. Single-index
/// `base[i]` still works (a one-element `indices` vec); it lowers identically
/// to the pre-T24 form (`base[i as usize]`). Two-index `m[r, c]` lowers to the
/// flat-storage Matrix access `m.data[(r * m.cols + c) as usize]`.
///
/// This generalization is forward-compatible with N-dimensional indexing
/// (tensors, v1.0+) — additional indices simply lengthen the vec.
///
/// ## T25 — `Expr::MapLit`
///
/// A new variant was **added** in T25 (v0.5) to carry a map/dictionary literal
/// `{"k": v, ...}` (note: braces + colon-separated entries). The `entries` are
/// a `Vec<(Expr, Expr)>` of `(key, value)` pairs (so each pair preserves the
/// span-bearing Expr nodes). This is **additive**: no existing variant was
/// renamed, reordered, or had its payload altered, so all prior `match` arms
/// remain exhaustive. The variant carries a `span: Span` for diagnostics.
///
/// Brace disambiguation (parser-side, see `parse_brace_primary`): a `{` at
/// primary position can be a closure `{ params => expr }` (T23) or a map
/// literal `{ key: value, ... }`. The parser uses speculative save/restore
/// on the `TokenStream` position: it tries the closure shape first, and on
/// failure rolls back and tries the map shape. Empty `{:}` is an empty map;
/// bare `{}` is rejected (ambiguous with code blocks per layout).
///
/// ## T30 — `Expr::Try` (the `?` postfix operator)
///
/// A new variant was **added** in T30 (v0.5) to carry the error-propagation
/// postfix operator `expr?`. The operand is boxed. This is **additive**: no
/// existing variant was renamed, reordered, or had its payload altered, so
/// all prior `match` arms remain exhaustive. The variant carries a
/// `span: Span` for diagnostics.
///
/// The parser fills this node from the `?` postfix position in `parse_postfix`
/// (the lexer already produced a `TokenKind::Question` token — `?` was never
/// a reserved keyword, so no lexer change was needed). Codegen lowers it to
/// Rust's **native `?` operator** (`<expr>?`) — the cleanest mapping because
/// the enclosing Buff function already lowers to a Rust function returning
/// `Result<T, E>`, which is exactly what Rust's `?` requires. The explicit
/// `match expr { Ok(v) => v, Err(e) => return Err(e) }` desugaring (the task's
/// option (b)) is NOT used; native `?` is simpler and equally correct.
///
/// ## T31 — `Expr::Spawn` (the `spawn <task>` async-spawn form)
///
/// A new variant was **added** in T31 (v0.5) to carry Buff's async-spawn
/// prefix operator: `spawn <expr>`. The lexer tokenises `spawn` as a
/// reserved keyword (`TokenKind::KwSpawn`); it can never be an ordinary
/// identifier, so the parser MUST treat it specially — it builds an
/// `Expr::Spawn` node in `parse_primary`. The operand (the task to spawn)
/// is boxed.
///
/// This is **additive**: no existing variant was renamed, reordered, or had
/// its payload altered, so all prior `match` arms remain exhaustive. The
/// variant carries a `span: Span` for diagnostics.
///
/// Codegen lowers `spawn expr` to Rust's `tokio::spawn(async move { expr })`,
/// yielding a `tokio::task::JoinHandle<T>`. Buff's `Task<T>` is a thin alias
/// for `JoinHandle<T>`; the auto-inserted `.await` (the only `.await` ever
/// emitted) lands when the user calls `t.result()` (see T31 codegen rules).
/// The `spawn` keyword does NOT propagate async-ness up the call graph on
/// its own — but every well-formed Buff program that uses `spawn` does so
/// inside an already-async function (the codegen warning for `block()`
/// inside async is the only direct effect of mis-use).
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal: `42`, `"hi"`, `true`, …
    Literal(Literal, Span),
    /// A bare identifier reference: `x`, `foo_bar`.
    Ident(Ident, Span),
    /// A binary operator application: `lhs op rhs`.
    BinaryOp {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// A unary operator application: `op operand`.
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    /// An `if` expression: `if cond { then } else { else }`.
    IfExpr {
        cond: Box<Expr>,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },
    /// A free function call: `callee(args...)`.
    FuncCall {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// A method call: `receiver.method(args...)`.
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
        span: Span,
    },
    /// A lambda / anonymous function.
    Lambda {
        params: Vec<Param>,
        body: Block,
        return_type: Option<TypeRef>,
        span: Span,
    },
    /// A struct literal: `TypeName { field: value, ... }`.
    StructInit {
        type_name: Ident,
        fields: Vec<(Ident, Expr)>,
        span: Span,
    },
    /// A `match` expression: ` scrutinee match { arms... } `.
    MatchExpr {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    /// A suspension point in an async context (placeholder for future async work).
    SuspendExpr { inner: Box<Expr>, span: Span },
    /// A collection literal: `[e1, e2, e3]` or `[]` (T23).
    ///
    /// Comma-separated elements; trailing comma allowed; empty literal allowed.
    /// Lowers to Rust's `vec![...]` macro. The element type is inferred from
    /// the elements — integer literals get auto-width via T22 range analysis
    /// (`[1, 2, 3]` -> `Vector<Int<8>>`), so a `let v = [1, 2, 3]` binding
    /// picks up a `Vec<i8>` Rust annotation automatically.
    ///
    /// This is **additive**: no existing variant was renamed or reordered.
    ArrayLit { elements: Vec<Expr>, span: Span },
    /// An indexing expression: `base[index]` or `base[row, col]` (T23, T24).
    ///
    /// Carries **one or more** indices in a `Vec` so the same node shape serves
    /// 1-D Vector indexing (`v[i]`) and 2-D Matrix indexing (`m[row, col]`).
    /// The parser fills `indices` from the comma-separated list inside `[...]`.
    ///
    /// Codegen dispatches on the arity:
    /// - 1 index → Rust `base[index as usize]` (Vector path, T23).
    /// - 2 indices → Rust `base.data[(row * base.cols + col) as usize]`
    ///   (flat-storage Matrix path, T24). The `data`/`cols` fields come from
    ///   the builtin `Matrix<T>` struct the codegen emits when a program uses
    ///   `Matrix.new(...)`.
    ///
    /// String-literal receivers are rejected at parse time (the T21 helpful
    /// error); all other receivers build this node.
    ///
    /// **T24 migration note**: this variant previously held a single
    /// `index: Box<Expr>` (T23). It was generalized to `indices: Vec<Expr>` so
    /// the same node can carry a comma-separated index list. Every match site
    /// was updated; single-index call sites now pass a one-element vec. This
    /// change is forward-compatible with N-D indexing (tensors, v1.0+).
    Index {
        base: Box<Expr>,
        indices: Vec<Expr>,
        span: Span,
    },
    /// A string interpolation: `"text {expr} more {expr2}"` (T21).
    ///
    /// Built from the lexer's `StringStart / StringPart / InterpStart / ...
    /// InterpEnd / StringEnd` token stream. Each part is either literal text
    /// or an arbitrary expression. Codegen lowers this to a Rust `format!(...)`
    /// macro call with one `{}` per expression and comma-separated arguments.
    ///
    /// This is an **additive** change: no existing variant was renamed or
    /// reordered. A simple `"abc"` (no `{...}`) still parses as
    /// `Expr::Literal(Literal::String(_), _)`; only strings that actually
    /// contain interpolation produce this variant.
    StringInterp { parts: Vec<InterpPart>, span: Span },
    /// A map literal: `{"key": value, ...}` or `{:}` (empty) (T25).
    ///
    /// Comma-separated `(key, value)` entries; trailing comma allowed; empty
    /// map spelled `{:}` (bare `{}` is rejected to avoid ambiguity with code
    /// blocks). Lowers to Rust's `std::collections::HashMap::from([...])`
    /// (fully-qualified, so no `use` import is needed in generated code).
    /// Both key and value types are inferred from the first entry — literals
    /// with mixed kinds fall back to the first entry's types.
    ///
    /// This is **additive**: no existing variant was renamed or reordered.
    MapLit {
        entries: Vec<(Expr, Expr)>,
        span: Span,
    },
    /// The error-propagation postfix operator `expr?` (T30).
    ///
    /// Wraps its operand. Codegen lowers this to Rust's **native `?`**
    /// (`<expr>?`), which requires (and assumes) the enclosing function
    /// returns a `Result<T, E>` — Buff functions that use `?` lower to
    /// Rust functions returning `Result`, so this lines up directly. The
    /// `?` operator propagates the `Err(e)` early (via `return Err(e)`)
    /// and yields the unwrapped `Ok(v)` value.
    ///
    /// This is **additive** (T30): see the migration note on [`Expr`].
    /// Parsing happens in `parse_postfix` (the `?` token is
    /// `TokenKind::Question`, already produced by the lexer — it is NOT a
    /// reserved keyword).
    Try { expr: Box<Expr>, span: Span },
    /// The async-spawn prefix operator `spawn <expr>` (T31).
    ///
    /// Buff tokenises `spawn` as a reserved keyword (`TokenKind::KwSpawn`)
    /// so it can never be parsed as an ordinary identifier. The parser
    /// builds this node in `parse_primary` when it sees `KwSpawn` at a
    /// primary position; the operand is the next expression (the task to
    /// spawn).
    ///
    /// Codegen lowers `spawn expr` to Rust's
    /// `tokio::spawn(async move { expr })`, yielding a
    /// `tokio::task::JoinHandle<T>`. Buff's `Task<T>` type is a thin alias
    /// for `JoinHandle<T>`; the only `.await` ever emitted is at the
    /// `t.result()` site (see T31 codegen rules).
    ///
    /// This is **additive** (T31): see the migration note on [`Expr`].
    Spawn { task: Box<Expr>, span: Span },
    /// A range expression: `start..end` (exclusive) or `start..=end` (inclusive) (T68).
    ///
    /// The `inclusive` flag distinguishes `..` (exclusive, `0..10` → `0..10`)
    /// from `..=` (inclusive, `0..=10` → `0..=10`). Both bounds are full
    /// expressions so `a + 1..b * 2` works. Range has lower precedence than
    /// additive operators, so `a+1..b*2` parses as `(a+1)..(b*2)`.
    ///
    /// This is **additive** (T68): no existing variant was renamed, reordered,
    /// or had its payload altered, so all prior `match` arms remain exhaustive.
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
        span: Span,
    },
    /// A conditional binding: `if let PATTERN = EXPR { then } else { else }` (T72).
    ///
    /// The `pattern` is matched against `value`; on success the `then_block`
    /// runs with the pattern's bindings in scope; on failure the optional
    /// `else_block` runs (or the whole expression evaluates to `()` when no
    /// `else` is present). Codegen lowers this to Rust's native `if let`
    /// expression, so the borrow-checker enforces the binding lifetime for
    /// free.
    ///
    /// This variant carries a single `let`-binding condition only (NOT a
    /// let-chain — `if let a = x, let b = y` is T74, a separate task). The
    /// `pattern` reuses the shared [`Pattern`] enum (Variant/Ident/Tuple/
    /// Struct/...) that `match` arms and T71 destructuring already use, so
    /// `if let Some(x) = opt` parses the `Some(x)` via the same
    /// `parse_pattern` path as `match opt { Some(x) => ... }`.
    ///
    /// This is **additive** (T72): no existing variant was renamed, reordered,
    /// or had its payload altered, so all prior `match` arms remain
    /// exhaustive. See the migration-note block on [`Expr`] for the
    /// established pattern.
    IfLet {
        pattern: Pattern,
        value: Box<Expr>,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },
    /// A tuple value literal: `(e1, e2, ...)` with 2+ members, e.g.
    /// `("A", 42)` (T103).
    ///
    /// Each element is a full [`Expr`] so `(a + 1, foo(), [1, 2])` works.
    /// The 2+-element rule lives at parse time: a single `(e)` is grouping
    /// (returns the bare `e`), NOT an `Expr::TupleLit`. So this variant
    /// always carries 2+ elements. A trailing comma `(a, b,)` is allowed
    /// and lowered to the same shape as `(a, b)`. Element order is
    /// preserved as written (Vec, never a HashMap — determinism).
    ///
    /// Codegen lowers `(e1, e2)` to Rust's native tuple `(e1, e2)`, so the
    /// resulting type is a real Rust tuple `(T1, T2)`.
    ///
    /// This is **additive** (T103): no existing variant was renamed,
    /// reordered, or had its payload altered, so all prior `match` arms
    /// remain exhaustive. See the migration-note block on [`Expr`] for the
    /// established pattern (T68 `Expr::Range` is the closest template).
    TupleLit(Vec<Expr>, Span),
}

impl Expr {
    /// Returns the [`Span`] associated with this expression.
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(_, s)
            | Expr::Ident(_, s)
            | Expr::BinaryOp { span: s, .. }
            | Expr::UnaryOp { span: s, .. }
            | Expr::IfExpr { span: s, .. }
            | Expr::FuncCall { span: s, .. }
            | Expr::MethodCall { span: s, .. }
            | Expr::Lambda { span: s, .. }
            | Expr::StructInit { span: s, .. }
            | Expr::MatchExpr { span: s, .. }
            | Expr::SuspendExpr { span: s, .. }
            | Expr::ArrayLit { span: s, .. }
            | Expr::Index { span: s, .. }
            | Expr::StringInterp { span: s, .. }
            | Expr::MapLit { span: s, .. }
            | Expr::Try { span: s, .. }
            | Expr::Spawn { span: s, .. }
            | Expr::Range { span: s, .. }
            | Expr::IfLet { span: s, .. }
            | Expr::TupleLit(_, s) => *s,
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Literal(lit, _) => write!(f, "Lit({lit})"),
            Expr::Ident(ident, _) => write!(f, "Ident({ident})"),
            Expr::BinaryOp { op, lhs, rhs, .. } => {
                write!(f, "BinaryOp({op}, {lhs}, {rhs})")
            }
            Expr::UnaryOp { op, operand, .. } => write!(f, "UnaryOp({op}, {operand})"),
            Expr::IfExpr {
                cond,
                then_block,
                else_block,
                ..
            } => match else_block {
                Some(els) => write!(f, "If({cond}, {then_block}, {els})"),
                None => write!(f, "If({cond}, {then_block})"),
            },
            Expr::FuncCall { callee, args, .. } => {
                write!(f, "Call({callee}, [")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str("])")
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                write!(f, "MethodCall({receiver}.{method}, [")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str("])")
            }
            Expr::Lambda {
                params,
                body,
                return_type,
                ..
            } => {
                f.write_str("Lambda(fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                f.write_str(")")?;
                if let Some(rt) = return_type {
                    write!(f, " -> {rt}")?;
                }
                write!(f, " {body})")
            }
            Expr::StructInit {
                type_name, fields, ..
            } => {
                write!(f, "StructInit({type_name} {{ ")?;
                for (i, (n, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{n}: {v}")?;
                }
                f.write_str(" })")
            }
            Expr::MatchExpr {
                scrutinee, arms, ..
            } => {
                write!(f, "Match({scrutinee}, [")?;
                for (i, arm) in arms.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{arm}")?;
                }
                f.write_str("])")
            }
            Expr::SuspendExpr { inner, .. } => write!(f, "Suspend({inner})"),
            Expr::ArrayLit { elements, .. } => {
                f.write_str("Array[")?;
                for (i, e) in elements.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{e}")?;
                }
                f.write_str("]")
            }
            Expr::Index { base, indices, .. } => {
                write!(f, "Index({base}, [")?;
                for (i, idx) in indices.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{idx}")?;
                }
                f.write_str("])")
            }
            Expr::StringInterp { parts, .. } => {
                f.write_str("Interp[")?;
                for (i, p) in parts.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                f.write_str("]")
            }
            // T25: `{k: v, ...}` -> `Map[k: v, ...]`.
            Expr::MapLit { entries, .. } => {
                f.write_str("Map[")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                f.write_str("]")
            }
            // T30: `expr?` -> `Try(expr)`.
            Expr::Try { expr, .. } => write!(f, "Try({expr})"),
            // T31: `spawn expr` -> `Spawn(expr)`.
            Expr::Spawn { task, .. } => write!(f, "Spawn({task})"),
            // T68: `start..end` -> `Range(start, end, excl/incl)`.
            Expr::Range {
                start,
                end,
                inclusive,
                ..
            } => {
                let kind = if *inclusive { "incl" } else { "excl" };
                write!(f, "Range({start}, {end}, {kind})")
            }
            // T72: `if let PAT = EXPR { then } else { else }` -> IfLet(...).
            Expr::IfLet {
                pattern,
                value,
                then_block,
                else_block,
                ..
            } => match else_block {
                Some(els) => write!(f, "IfLet({pattern} = {value}, {then_block}, {els})"),
                None => write!(f, "IfLet({pattern} = {value}, {then_block})"),
            },
            // T103: `(e1, e2, ...)` -> Tuple[e1, e2, ...].
            Expr::TupleLit(members, _) => {
                f.write_str("Tuple[")?;
                for (i, e) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{e}")?;
                }
                f.write_str("]")
            }
        }
    }
}

/// A single arm of a `match` expression.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Block,
    pub span: Span,
}

impl fmt::Display for MatchArm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} => {}", self.pattern, self.body)
    }
}

/// A pattern usable inside a [`MatchArm`].
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// The wildcard `_`.
    Wildcard(Span),
    /// A literal pattern: `1`, `"foo"`, `true`.
    Literal(Literal, Span),
    /// A binding pattern: `x`.
    Ident(Ident, Span),
    /// An enum variant pattern: `Option::Some(x)` or `Color::Rgb(r, g, b)`.
    Variant {
        enum_name: Ident,
        variant: Ident,
        subpatterns: Vec<Pattern>,
        span: Span,
    },
    /// A tuple destructuring pattern: `(x, y)`, `(a, _, c)` (T71).
    Tuple(Vec<Pattern>, Span),
    /// A struct destructuring pattern: `Point { x, y }` (T71).
    ///
    /// Each entry is `(field_name, subpattern)`. Shorthand `Point { x, y }`
    /// is parsed as `{ x: x, y: y }` — i.e. a field whose name equals its
    /// binding. Field order is preserved as written (determinism: never use a
    /// HashMap here).
    Struct {
        name: Ident,
        fields: Vec<(Ident, Pattern)>,
        span: Span,
    },
}

impl Pattern {
    /// Returns the [`Span`] associated with this pattern (T27).
    ///
    /// Every variant carries its own `Span`; this accessor lets the match
    /// parser (and downstream diagnostics) treat patterns uniformly.
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard(s)
            | Pattern::Literal(_, s)
            | Pattern::Ident(_, s)
            | Pattern::Variant { span: s, .. }
            | Pattern::Tuple(_, s)
            | Pattern::Struct { span: s, .. } => *s,
        }
    }

    /// Returns the canonical variant-name key this pattern would cover, if
    /// it could be a variant reference (T27).
    ///
    /// Used by the exhaustiveness checker to match arms against enum
    /// variants by NAME without needing to resolve the variant-vs-binding
    /// ambiguity upfront. Returns:
    /// - `Some(name)` for `Pattern::Ident(name)` and
    ///   `Pattern::Variant { variant: name, .. }` (the parser fills
    ///   `enum_name` with `""` for variant patterns it builds from source).
    /// - `None` for `Pattern::Wildcard` and `Pattern::Literal` (these never
    ///   cover a named variant).
    pub fn variant_name_key(&self) -> Option<&str> {
        match self {
            Pattern::Ident(name, _) => Some(&name.name),
            Pattern::Variant { variant, .. } => Some(&variant.name),
            Pattern::Wildcard(_)
            | Pattern::Literal(_, _)
            | Pattern::Tuple(_, _)
            | Pattern::Struct { .. } => None,
        }
    }

    /// Returns the identifiers bound by this pattern (T71).
    ///
    /// - `Ident(name)` → `[name]`
    /// - `Tuple(subs)` / `Variant { subpatterns, .. }` → bindings of each
    ///   sub-pattern (flattened, order preserved).
    /// - `Struct { fields, .. }` → bindings of each field's sub-pattern.
    /// - `Wildcard` / `Literal` → `[]` (they bind nothing).
    ///
    /// Used by inference, ownership analysis, and IR lowering to introduce
    /// the names a destructuring `let` brings into scope. Determinism is
    /// preserved by walking the source-order `Vec`s (never a HashMap).
    pub fn bindings(&self) -> Vec<Ident> {
        match self {
            Pattern::Ident(name, _) => vec![name.clone()],
            Pattern::Tuple(subs, _) => subs.iter().flat_map(Pattern::bindings).collect(),
            Pattern::Variant { subpatterns, .. } => {
                subpatterns.iter().flat_map(Pattern::bindings).collect()
            }
            Pattern::Struct { fields, .. } => {
                fields.iter().flat_map(|(_, p)| p.bindings()).collect()
            }
            Pattern::Wildcard(_) | Pattern::Literal(_, _) => Vec::new(),
        }
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pattern::Wildcard(_) => f.write_str("_"),
            Pattern::Literal(lit, _) => write!(f, "{lit}"),
            Pattern::Ident(name, _) => write!(f, "{name}"),
            Pattern::Variant {
                enum_name,
                variant,
                subpatterns,
                ..
            } => {
                write!(f, "{enum_name}::{variant}")?;
                if !subpatterns.is_empty() {
                    f.write_str("(")?;
                    for (i, p) in subpatterns.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{p}")?;
                    }
                    f.write_str(")")?;
                }
                Ok(())
            }
            Pattern::Tuple(subs, _) => {
                f.write_str("(")?;
                for (i, p) in subs.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                f.write_str(")")
            }
            Pattern::Struct { name, fields, .. } => {
                write!(f, "{name} {{ ")?;
                for (i, (fname, p)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{fname}: {p}")?;
                }
                f.write_str(" }}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_span() -> Span {
        Span::dummy()
    }

    #[test]
    fn literal_display() {
        assert_eq!(Literal::Int(42).to_string(), "Int(42)");
        assert_eq!(Literal::Bool(true).to_string(), "Bool(true)");
        assert_eq!(Literal::Byte(255).to_string(), "Byte(0xFF)");
        assert_eq!(
            Literal::String("hi".to_string()).to_string(),
            "String(\"hi\")"
        );
        // T20: Decimal displays its raw source text, double-quoted.
        assert_eq!(
            Literal::Decimal("99.90".to_string()).to_string(),
            "Decimal(\"99.90\")"
        );
        // T21: Char renders in single quotes via Debug ({:?}) form.
        assert_eq!(Literal::Char('A').to_string(), "Char('A')");
        assert_eq!(Literal::Char('é').to_string(), "Char('é')");
        assert_eq!(Literal::Char('🚀').to_string(), "Char('🚀')");
    }

    #[test]
    fn string_interp_display() {
        // T21: StringInterp renders as `Interp[Lit("..."), Expr(...), ...]`.
        let parts = vec![
            InterpPart::Literal("Hello ".into()),
            InterpPart::Expr(Box::new(Expr::Ident(
                Ident::new("name", dummy_span()),
                dummy_span(),
            ))),
            InterpPart::Literal("!".into()),
        ];
        let e = Expr::StringInterp {
            parts,
            span: dummy_span(),
        };
        assert_eq!(
            e.to_string(),
            "Interp[Lit(\"Hello \"), Expr(Ident(name)), Lit(\"!\")]"
        );
    }

    #[test]
    fn ident_expr_display() {
        let e = Expr::Ident(Ident::new("x", dummy_span()), dummy_span());
        assert_eq!(e.to_string(), "Ident(x)");
    }

    #[test]
    fn binary_op_expr_display() {
        let e = Expr::BinaryOp {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Literal(Literal::Int(1), dummy_span())),
            rhs: Box::new(Expr::Literal(Literal::Int(2), dummy_span())),
            span: dummy_span(),
        };
        assert_eq!(e.to_string(), "BinaryOp(+, Lit(Int(1)), Lit(Int(2)))");
    }

    #[test]
    fn variant_pattern_display() {
        let p = Pattern::Variant {
            enum_name: Ident::new("Option", dummy_span()),
            variant: Ident::new("Some", dummy_span()),
            subpatterns: vec![Pattern::Ident(Ident::new("x", dummy_span()), dummy_span())],
            span: dummy_span(),
        };
        assert_eq!(p.to_string(), "Option::Some(x)");
    }
}
