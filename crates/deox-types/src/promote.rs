//! Numeric promotion rules for the Deox type system.
//!
//! [`promote_binary`] computes the result type of a binary operation between
//! two numeric (or compatible) types, applying the language's implicit
//! widening / coercion rules.

use crate::ty::{FloatWidth, IntWidth, Type};

/// Computes the result type of applying a binary operator to two types.
///
/// Returns `Some(result)` if the operands are compatible, or `None` if they
/// are not (e.g. `Int + String`).
///
/// # Rules (highest precedence first)
///
/// - `Decimal` dominates all numeric types (`Decimal + Int` → `Decimal`).
/// - `Double` dominates all non-decimal numerics (`Double + Float` → `Double`).
/// - `Float` dominates integers (`Float + Int` → `Float`, max width wins).
/// - Two integers promote to the wider one (`Int<32> + Int<64>` → `Int<64>`);
///   when signedness differs, the signed (`Int`) width wins.
/// - `Bool`/`String` combine only with themselves.
/// - `Unknown` combines with anything, yielding `Unknown` (suppresses error
///   cascades after a prior type error).
pub fn promote_binary(lhs: &Type, rhs: &Type) -> Option<Type> {
    use Type::*;
    match (lhs, rhs) {
        // Unknown suppresses cascading errors.
        (Unknown, _) | (_, Unknown) => Some(Unknown),

        // Same simple types.
        (Bool, Bool) => Some(Bool),
        (String, String) => Some(String),
        (Void, Void) => Some(Void),

        // Decimal dominates all numerics.
        (Decimal, other) | (other, Decimal) if other.is_numeric() => Some(Decimal),

        // Double dominates non-decimal numerics.
        (Double, other) | (other, Double) if other.is_numeric() => Some(Double),

        // Float vs Float — max width.
        (Float { width: w1 }, Float { width: w2 }) => Some(Float {
            width: max_float(*w1, *w2),
        }),
        // Float vs integer — Float wins (widen).
        (Float { .. }, Int { .. } | Bits { .. }) => Some(lhs.clone()),
        (Int { .. } | Bits { .. }, Float { .. }) => Some(rhs.clone()),

        // Int vs Int — signed, max width.
        (Int { width: w1 }, Int { width: w2 }) => Some(Int {
            width: max_int(*w1, *w2),
        }),
        // Bits vs Bits — unsigned, max width.
        (Bits { width: w1 }, Bits { width: w2 }) => Some(Bits {
            width: max_int(*w1, *w2),
        }),
        // Int vs Bits — signed wins, using the Int width.
        (Int { width }, Bits { .. }) | (Bits { .. }, Int { width }) => Some(Int { width: *width }),

        _ => None,
    }
}

/// Returns `true` if a value of type `value` can be assigned to a binding
/// annotated with `annotated`.
///
/// Equality always passes. Numeric widening passes when `value` promotes up
/// to `annotated` (e.g. `Int` → `Float`, `Int` → `Double`, `Float` → `Double`).
/// Narrowing (e.g. `Float` → `Int`) is rejected.
pub fn assignable_to(annotated: &Type, value: &Type) -> bool {
    if annotated == value {
        return true;
    }
    if let Some(promoted) = promote_binary(annotated, value) {
        return promoted == *annotated;
    }
    false
}

fn max_int(a: IntWidth, b: IntWidth) -> IntWidth {
    if a.bits() >= b.bits() {
        a
    } else {
        b
    }
}

fn max_float(a: FloatWidth, b: FloatWidth) -> FloatWidth {
    if a.bits() >= b.bits() {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_types_promote() {
        assert_eq!(
            promote_binary(&Type::bool(), &Type::bool()),
            Some(Type::bool())
        );
        assert_eq!(
            promote_binary(&Type::string(), &Type::string()),
            Some(Type::string())
        );
    }

    #[test]
    fn decimal_dominates() {
        assert_eq!(
            promote_binary(&Type::Decimal, &Type::int_default()),
            Some(Type::Decimal)
        );
        assert_eq!(
            promote_binary(&Type::double(), &Type::Decimal),
            Some(Type::Decimal)
        );
    }

    #[test]
    fn double_dominates_float() {
        assert_eq!(
            promote_binary(&Type::double(), &Type::float_default()),
            Some(Type::double())
        );
        assert_eq!(
            promote_binary(&Type::int_default(), &Type::double()),
            Some(Type::double())
        );
    }

    #[test]
    fn int_widens_to_float() {
        assert_eq!(
            promote_binary(&Type::int_default(), &Type::float_default()),
            Some(Type::float_default())
        );
    }

    #[test]
    fn incompatible_types_return_none() {
        assert_eq!(promote_binary(&Type::bool(), &Type::int_default()), None);
        assert_eq!(promote_binary(&Type::string(), &Type::int_default()), None);
    }
}
