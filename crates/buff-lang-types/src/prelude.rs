//! The Buff **standard library prelude** (T96).
//!
//! The prelude is the set of function names that are **implicitly in scope**
//! in every Buff program — no `import` is required. They are recognised as
//! built-in call targets during type inference and Rust codegen.
//!
//! ## Categories (REFACTOR step)
//!
//! The prelude is grouped into four categories. Each category is a separate
//! `const` slice so future tasks (e.g. T99 will add `args`/`env`/`exit`) can
//! append without rewriting existing arms.
//!
//! | Category     | Members                                                 |
//! |--------------|---------------------------------------------------------|
//! | [`Math`]     | `abs`, `min`, `max`, `sqrt`, `floor`, `ceil`, `round`, `pow` |
//! | [`Convert`]  | `Int`, `Float`, `String`, `Bool`                        |
//! | [`Io`]       | `print`, `println`, `read_line`                         |
//! | [`Collection`] | reserved — populated by T23/T67 collection tasks      |
//!
//! ## Return-type rules
//!
//! Most prelude functions have a *fixed* return type independent of their
//! arguments (e.g. [`PreludeFn::String`] always returns [`Type::String`]).
//! The interesting cases are the polymorphic ones:
//!
//! - `abs(x)` returns the same type as `x` (works for any numeric).
//! - `min(a, b)` / `max(a, b)` return the promoted type of `(a, b)` (so
//!   `min(Int, Float)` returns `Float`).
//! - `pow(base, exp)` returns the type of `base`.
//! - `floor`/`ceil`/`round`/`sqrt` are float-returning (the arg may be int
//!   or float; result is `Float` for `sqrt`, else the float-promotion of
//!   the arg).
//!
//! [`Math`]: PreludeCategory::Math
//! [`Convert`]: PreludeCategory::Convert
//! [`Io`]: PreludeCategory::Io
//! [`Collection`]: PreludeCategory::Collection
//! [`PreludeFn::String`]: PreludeFn::String

use crate::promote::promote_binary;
use crate::ty::Type;

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

/// The four prelude categories (REFACTOR step of T96).
///
/// Used by [`category_of`] to label a name for documentation/diagnostics;
/// the type-inference and codegen passes do not need this — they switch on
/// [`PreludeFn`] directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreludeCategory {
    /// `abs`, `min`, `max`, `sqrt`, `floor`, `ceil`, `round`, `pow`.
    Math,
    /// `Int`, `Float`, `String`, `Bool` (type-conversion constructors).
    Convert,
    /// `print`, `println`, `read_line`.
    Io,
    /// `args`, `env`, `exit` (T99 — process environment access).
    System,
    /// Reserved for the collection-prelude (T23/T67). Empty today.
    Collection,
    /// `assert_eq` and (future) `assert` — testing assertions (T35).
    Test,
}

/// A recognised prelude function name.
///
/// The `&str` returned by [`PreludeFn::name`] is the literal identifier the
/// user writes (e.g. `"abs"`, `"String"`). The discriminants are kept in a
/// flat enum rather than a string set so the inference pass can exhaustively
/// match without risking a typo'd string comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreludeFn {
    // --- Math -----------------------------------------------------------
    /// `abs(x)` — absolute value, polymorphic over numeric arg type.
    Abs,
    /// `min(a, b)` — minimum; returns promoted type of the pair.
    Min,
    /// `max(a, b)` — maximum; returns promoted type of the pair.
    Max,
    /// `sqrt(x)` — square root; always returns `Float`.
    Sqrt,
    /// `floor(x)` — round towards −∞.
    Floor,
    /// `ceil(x)` — round towards +∞.
    Ceil,
    /// `round(x)` — round to nearest, ties away from zero.
    Round,
    /// `pow(base, exp)` — exponentiation; returns type of `base`.
    Pow,
    // --- Type conversions ----------------------------------------------
    /// `Int(x)` — convert anything reasonable to `Int<64>`.
    Int,
    /// `Float(x)` — convert anything reasonable to `Float<32>`.
    Float,
    /// `String(x)` — convert anything reasonable to `String`.
    String,
    /// `Bool(x)` — convert anything reasonable to `Bool`.
    Bool,
    // --- I/O ------------------------------------------------------------
    /// `print(x)` — print without trailing newline (maps to Rust `println!`).
    Print,
    /// `println(x)` — print with trailing newline (maps to Rust `println!`).
    Println,
    /// `read_line()` — read a line of stdin, returns `String`.
    ReadLine,
    // --- System / environment (T99) -------------------------------------
    /// `args()` — command-line arguments, returns `Vector<String>`.
    Args,
    /// `env("NAME")` — environment variable lookup, returns `Option<String>`.
    Env,
    /// `exit(code)` — terminate the process with the given exit code.
    Exit,
    // --- System I/O / async (T124g) -------------------------------------
    /// `input()` / `input(prompt)` — read one line from stdin (optionally
    /// printing a prompt first). Returns `String` (trimmed of trailing
    /// newline). Wraps `std::io::stdin().read_line(...)`.
    Input,
    /// `sleep(duration)` — async-transparent sleep. Lowers to
    /// `tokio::time::sleep(<duration>).await` (records `tokio` in
    /// codegen `extern_crates`). Buff has no `await` keyword — the
    /// `.await` is inserted by codegen and propagates async-ness up
    /// the call graph. Returns `Void`.
    Sleep,
    // --- Testing (T35) -------------------------------------------------
    /// `assert_eq(a, b)` — assert two values are equal (panics otherwise).
    /// Maps to Rust's `assert_eq!` macro. Only meaningful inside `@test`
    /// functions, but recognised everywhere (a bare `assert_eq` call in a
    /// non-test fn still lowers to `assert_eq!` — Rust accepts it).
    AssertEq,
    // --- Testing (T38) -------------------------------------------------
    /// `assertThat(value)` — fluent test assertion entry point. Returns an
    /// `AssertThat<T>` wrapper whose methods (isEqualTo, isGreaterThan, ...)
    /// panic with descriptive messages on failure. Lowers to
    /// `buff_assertions::assertThat(value)`.
    AssertThat,
}

impl PreludeFn {
    /// All prelude functions, in declared order (Math, Convert, Io).
    pub const ALL: &'static [PreludeFn] = &[
        // Math
        PreludeFn::Abs,
        PreludeFn::Min,
        PreludeFn::Max,
        PreludeFn::Sqrt,
        PreludeFn::Floor,
        PreludeFn::Ceil,
        PreludeFn::Round,
        PreludeFn::Pow,
        // Convert
        PreludeFn::Int,
        PreludeFn::Float,
        PreludeFn::String,
        PreludeFn::Bool,
        // I/O
        PreludeFn::Print,
        PreludeFn::Println,
        PreludeFn::ReadLine,
        // System
        PreludeFn::Args,
        PreludeFn::Env,
        PreludeFn::Exit,
        // T124g: System I/O / async.
        PreludeFn::Input,
        PreludeFn::Sleep,
        // Testing
        PreludeFn::AssertEq,
        // T38: Fluent test assertions.
        PreludeFn::AssertThat,
    ];

    /// The source-name of this prelude function (the identifier the user
    /// writes in Buff source).
    pub const fn name(self) -> &'static str {
        match self {
            PreludeFn::Abs => "abs",
            PreludeFn::Min => "min",
            PreludeFn::Max => "max",
            PreludeFn::Sqrt => "sqrt",
            PreludeFn::Floor => "floor",
            PreludeFn::Ceil => "ceil",
            PreludeFn::Round => "round",
            PreludeFn::Pow => "pow",
            PreludeFn::Int => "Int",
            PreludeFn::Float => "Float",
            PreludeFn::String => "String",
            PreludeFn::Bool => "Bool",
            PreludeFn::Print => "print",
            PreludeFn::Println => "println",
            PreludeFn::ReadLine => "read_line",
            PreludeFn::Args => "args",
            PreludeFn::Env => "env",
            PreludeFn::Exit => "exit",
            // T124g: System I/O / async free fns.
            PreludeFn::Input => "input",
            PreludeFn::Sleep => "sleep",
            PreludeFn::AssertEq => "assert_eq",
            PreludeFn::AssertThat => "assertThat",
        }
    }

    /// The category label for diagnostics/docs.
    pub const fn category(self) -> PreludeCategory {
        match self {
            PreludeFn::Abs
            | PreludeFn::Min
            | PreludeFn::Max
            | PreludeFn::Sqrt
            | PreludeFn::Floor
            | PreludeFn::Ceil
            | PreludeFn::Round
            | PreludeFn::Pow => PreludeCategory::Math,
            PreludeFn::Int | PreludeFn::Float | PreludeFn::String | PreludeFn::Bool => {
                PreludeCategory::Convert
            }
            PreludeFn::Print | PreludeFn::Println | PreludeFn::ReadLine => PreludeCategory::Io,
            PreludeFn::Args | PreludeFn::Env | PreludeFn::Exit => PreludeCategory::System,
            // T124g: input() is Io (reads stdin); sleep() is System
            // (async-transparent runtime facility, mirrors exit()'s
            // process-level System category).
            PreludeFn::Input => PreludeCategory::Io,
            PreludeFn::Sleep => PreludeCategory::System,
            PreludeFn::AssertEq => PreludeCategory::Test,
            PreludeFn::AssertThat => PreludeCategory::Test,
        }
    }
}

// ---------------------------------------------------------------------------
// Lookup
// ---------------------------------------------------------------------------

/// Returns `true` iff `name` is a recognised prelude function name.
///
/// This is the predicate the type inferencer uses to decide whether a
/// `FuncCall` with a bare-ident callee is a prelude call (and therefore
/// resolvable WITHOUT an `import`).
pub fn is_prelude(name: &str) -> bool {
    lookup(name).is_some()
}

/// Look up a prelude function by its source name. Returns `None` for
/// unrecognised names (including user-defined functions and the future
/// collection-prelude names, which aren't defined yet).
pub fn lookup(name: &str) -> Option<PreludeFn> {
    // Linear scan over 15 entries — the prelude is small and the lookup is
    // only hit once per `FuncCall`. A `phf`/`HashMap` would be over-engineering.
    PreludeFn::ALL.iter().find(|&&f| f.name() == name).copied()
}

/// Returns the category label of a prelude name, or `None` if the name is
/// not in the prelude. Convenience wrapper over [`lookup`] + [`category`].
pub fn category_of(name: &str) -> Option<PreludeCategory> {
    lookup(name).map(PreludeFn::category)
}

// ---------------------------------------------------------------------------
// Return-type inference
// ---------------------------------------------------------------------------

/// Infer the return type of a prelude call given the *resolved* argument
/// types.
///
/// The caller (the type inferencer) is responsible for first inferring the
/// type of each argument expression; this function only consumes the
/// resulting [`Type`] slice. `Unknown` arguments are tolerated — they
/// propagate as `Unknown` rather than producing a type error, mirroring the
/// inferencer's general "report once, suppress cascades" philosophy.
///
/// # Rules
///
/// - **Math** (`abs`/`min`/`max`/`pow`): polymorphic — see the table in the
///   [module docs](self).
/// - **Float-returning math** (`sqrt`/`floor`/`ceil`/`round`): the result is
///   `Float<32>` if any arg is integer-like (Rust's `.sqrt()` etc. only
///   exist on floats); otherwise it is the float-promotion of the arg.
/// - **Convert**: each constructor returns its target type regardless of
///   the arg type (`Int(..) -> Int<64>`, `String(..) -> String`, etc.).
/// - **I/O**: `print`/`println` return `Void`; `read_line` returns `String`.
pub fn return_type(fn_: PreludeFn, arg_tys: &[Type]) -> Type {
    match fn_ {
        // --- Math (polymorphic) ----------------------------------------
        PreludeFn::Abs => {
            // abs(x) returns the type of x.
            arg_tys.first().cloned().unwrap_or(Type::Unknown)
        }
        PreludeFn::Min | PreludeFn::Max => {
            // min(a, b) / max(a, b) return the promoted type of the pair.
            match arg_tys {
                [a, b] => promote_binary(a, b).unwrap_or(Type::Unknown),
                [single] => single.clone(),
                _ => Type::Unknown,
            }
        }
        PreludeFn::Pow => {
            // pow(base, exp) returns the type of base.
            arg_tys.first().cloned().unwrap_or(Type::Unknown)
        }

        // --- Float-returning math --------------------------------------
        // sqrt/floor/ceil/round are float operations in Rust. We coerce
        // integer args up to Float<32> (the default float); a float arg
        // keeps its width.
        PreludeFn::Sqrt => Type::float_default(),
        PreludeFn::Floor | PreludeFn::Ceil | PreludeFn::Round => {
            match arg_tys.first() {
                Some(t) if t.is_float_like() => t.clone(),
                Some(t) if t.is_numeric() => Type::float_default(),
                // Unknown / non-numeric arg — propagate Unknown so the
                // caller can flag a type error if desired.
                Some(_) => Type::Unknown,
                None => Type::Unknown,
            }
        }

        // --- Conversions (fixed target type) --------------------------
        PreludeFn::Int => Type::int_default(),
        PreludeFn::Float => Type::float_default(),
        PreludeFn::String => Type::string(),
        PreludeFn::Bool => Type::bool(),

        // --- I/O -------------------------------------------------------
        PreludeFn::Print | PreludeFn::Println => Type::Void,
        PreludeFn::ReadLine => Type::string(),

        // --- System / environment (T99) --------------------------------
        // args() -> Vector<String>
        PreludeFn::Args => Type::vector(Type::string()),
        // env("NAME") -> Option<String>
        PreludeFn::Env => Type::option(Type::string()),
        // exit(code) -> Void (never returns)
        PreludeFn::Exit => Type::Void,

        // --- System I/O / async (T124g) --------------------------------
        // input() / input(prompt) -> String (the trimmed line). The
        // optional prompt arg is a String literal printed before reading;
        // it has no effect on the return type.
        PreludeFn::Input => Type::string(),
        // sleep(duration) -> Void. Async-transparent (lowers to
        // `tokio::time::sleep(...).await`); the surrounding fn MUST be
        // async (declared or propagated). Returns no value (the Buff
        // surface treats it as a side-effect-only call).
        PreludeFn::Sleep => Type::Void,

        // --- Testing (T35) ---------------------------------------------
        // assert_eq(a, b) -> Void (panics on mismatch, returns () on success)
        PreludeFn::AssertEq => Type::Void,
        // --- Testing (T38) ---------------------------------------------
        // assertThat(value) -> AssertThat<T> (fluent assertion wrapper).
        // Return type tracks the input value's type so the wrapper's
        // chained methods (isEqualTo, isGreaterThan, etc.) typecheck
        // against the original value. Lowers to buff_assertions::assertThat(v).
        PreludeFn::AssertThat => arg_tys.first().cloned().unwrap_or(Type::Unknown),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::{FloatWidth, IntWidth};

    #[test]
    fn prelude_lookup_known_names() {
        for &f in PreludeFn::ALL {
            assert_eq!(lookup(f.name()), Some(f), "lookup({:?})", f);
            assert!(is_prelude(f.name()));
        }
    }

    #[test]
    fn prelude_lookup_rejects_unknown() {
        assert!(!is_prelude("user_func"));
        assert!(!is_prelude(""));
        assert!(!is_prelude("nonexistent"));
        assert_eq!(lookup("nonexistent"), None);
    }

    #[test]
    fn prelude_categories_are_partitioned() {
        for &f in PreludeFn::ALL {
            let cat = f.category();
            // Each name is in exactly one category and category_of agrees.
            assert_eq!(category_of(f.name()), Some(cat));
        }
        // Spot-check the four categories.
        assert_eq!(PreludeFn::Abs.category(), PreludeCategory::Math);
        assert_eq!(PreludeFn::Int.category(), PreludeCategory::Convert);
        assert_eq!(PreludeFn::Print.category(), PreludeCategory::Io);
    }

    #[test]
    fn prelude_return_type_abs_is_polymorphic() {
        assert_eq!(
            return_type(PreludeFn::Abs, &[Type::int_default()]),
            Type::int_default()
        );
        assert_eq!(
            return_type(PreludeFn::Abs, &[Type::float_default()]),
            Type::float_default()
        );
        assert_eq!(return_type(PreludeFn::Abs, &[Type::Double]), Type::Double);
        assert_eq!(return_type(PreludeFn::Abs, &[Type::byte()]), Type::byte());
    }

    #[test]
    fn prelude_return_type_min_max_promotes() {
        // min/max of two same-type args returns that type.
        assert_eq!(
            return_type(PreludeFn::Min, &[Type::int_default(), Type::int_default()]),
            Type::int_default()
        );
        // min(Int, Float) -> Float (promote_binary widens).
        assert_eq!(
            return_type(
                PreludeFn::Max,
                &[Type::int_default(), Type::float_default()]
            ),
            Type::float_default()
        );
        // max(Int, Double) -> Double.
        assert_eq!(
            return_type(PreludeFn::Max, &[Type::int_default(), Type::Double]),
            Type::Double
        );
    }

    #[test]
    fn prelude_return_type_conversions_fixed() {
        assert_eq!(
            return_type(PreludeFn::Int, &[Type::string()]),
            Type::int_default()
        );
        assert_eq!(
            return_type(PreludeFn::Int, &[Type::float_default()]),
            Type::int_default()
        );
        assert_eq!(
            return_type(PreludeFn::Float, &[Type::int_default()]),
            Type::float_default()
        );
        assert_eq!(
            return_type(PreludeFn::String, &[Type::int_default()]),
            Type::string()
        );
        assert_eq!(
            return_type(PreludeFn::Bool, &[Type::int_default()]),
            Type::bool()
        );
    }

    #[test]
    fn prelude_return_type_sqrt_is_float() {
        // sqrt(Int) -> Float (sqrt is a float op).
        assert_eq!(
            return_type(PreludeFn::Sqrt, &[Type::int_default()]),
            Type::float_default()
        );
        // sqrt(Float) -> Float.
        assert_eq!(
            return_type(PreludeFn::Sqrt, &[Type::float_default()]),
            Type::float_default()
        );
    }

    #[test]
    fn prelude_return_type_io() {
        // print/println -> Void.
        assert_eq!(return_type(PreludeFn::Print, &[Type::string()]), Type::Void);
        assert_eq!(
            return_type(PreludeFn::Println, &[Type::int_default()]),
            Type::Void
        );
        // read_line() -> String.
        assert_eq!(return_type(PreludeFn::ReadLine, &[]), Type::string());
    }

    #[test]
    fn prelude_return_type_floor_ceil_round_float() {
        // floor on a Float returns Float (width preserved).
        assert_eq!(
            return_type(PreludeFn::Floor, &[Type::float_default()]),
            Type::float_default()
        );
        // floor on an Int returns Float (coerced up).
        assert_eq!(
            return_type(PreludeFn::Ceil, &[Type::int_default()]),
            Type::float_default()
        );
        // round on Double stays Double.
        assert_eq!(return_type(PreludeFn::Round, &[Type::Double]), Type::Double);
    }

    #[test]
    fn prelude_return_type_pow_is_base_type() {
        // pow(Int, Int) -> Int.
        assert_eq!(
            return_type(PreludeFn::Pow, &[Type::int_default(), Type::int_default()]),
            Type::int_default()
        );
        // pow(Float, Int) -> Float.
        assert_eq!(
            return_type(
                PreludeFn::Pow,
                &[Type::float_default(), Type::int_default()]
            ),
            Type::float_default()
        );
    }

    #[test]
    fn prelude_all_count_and_no_duplicates() {
        // 15 prelude functions today (8 math + 4 convert + 3 io) + 3 env
        // (args, env, exit) = 18 + 2 system-io/async (input, sleep) = 20.
        let all_names: Vec<&str> = PreludeFn::ALL.iter().map(|f| f.name()).collect();
        let unique: std::collections::HashSet<&str> = all_names.iter().copied().collect();
        assert_eq!(all_names.len(), unique.len(), "duplicate prelude names");
        // Sanity: at least the eight math + four convert + three io + three env
        // + two system-io/async.
        assert!(PreludeFn::ALL.len() >= 20);
        // Width helpers exist on IntWidth/FloatWidth.
        assert_eq!(IntWidth::W8.bits(), 8);
        assert_eq!(FloatWidth::W32.bits(), 32);
    }
}
