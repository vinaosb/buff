use super::*;

#[test]
fn type_display_variants() {
    assert_eq!(Type::int_default().to_string(), "Int<64>");
    assert_eq!(Type::byte().to_string(), "Bits<8>");
    assert_eq!(Type::float_default().to_string(), "Float<32>");
    assert_eq!(Type::double().to_string(), "Double");
    assert_eq!(Type::bool().to_string(), "Bool");
    assert_eq!(Type::string().to_string(), "String");
    assert_eq!(Type::char().to_string(), "Char");
    assert_eq!(Type::Decimal.to_string(), "Decimal");
    assert_eq!(Type::Unknown.to_string(), "Unknown");
    assert_eq!(Type::Void.to_string(), "Void");
}

#[test]
fn numeric_classification() {
    assert!(Type::int_default().is_numeric());
    assert!(Type::byte().is_numeric());
    assert!(Type::float_default().is_numeric());
    assert!(Type::double().is_numeric());
    assert!(Type::Decimal.is_numeric());
    assert!(!Type::bool().is_numeric());
    assert!(!Type::string().is_numeric());

    assert!(Type::float_default().is_float_like());
    assert!(Type::double().is_float_like());
    assert!(!Type::int_default().is_float_like());

    assert!(Type::int_default().is_integer_like());
    assert!(Type::byte().is_integer_like());
    assert!(!Type::float_default().is_integer_like());
}

// T20: GPU/CPU dispatch type-metadata predicates.
#[test]
fn gpu_cpu_dispatch_metadata() {
    // WGSL-native 32-bit scalars are GPU-eligible.
    assert!(Type::float_default().is_gpu_eligible()); // Float<32>
    assert!(Type::Bool.is_gpu_eligible());
    assert!(Type::Int {
        width: IntWidth::W32
    }
    .is_gpu_eligible());
    assert!(Type::Bits {
        width: IntWidth::W32
    }
    .is_gpu_eligible());

    // Decimal is NEVER GPU-eligible Ã¢â‚¬â€ it must run on CPU (Rayon).
    assert!(!Type::Decimal.is_gpu_eligible());
    assert!(Type::Decimal.must_run_on_cpu());

    // Double (f64) and wide integers are also CPU-only (no WGSL scalar).
    assert!(!Type::Double.is_gpu_eligible());
    assert!(Type::Double.must_run_on_cpu());
    assert!(!Type::int_default().is_gpu_eligible()); // Int<64>
    assert!(!Type::byte().is_gpu_eligible()); // Bits<8>

    // Predicate complementarity for Decimal.
    assert_ne!(
        Type::Decimal.is_gpu_eligible(),
        Type::Decimal.must_run_on_cpu()
    );
}
