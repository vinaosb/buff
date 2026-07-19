//! T38 RED: RuntimeError variants, Display, kind(), and From<RuntimeError>
//! for buff_lang_error::RuntimeError bridge.

use buff_lang_error::RuntimeError as BuffRuntimeError;
use buff_lang_runtime::RuntimeError;

#[test]
fn test_runtime_error_gpu_unavailable_display() {
    let err = RuntimeError::GpuUnavailable;
    let rendered = format!("{err}");
    assert!(
        rendered.contains("gpu unavailable"),
        "display should mention gpu unavailable, got: {rendered}"
    );
}

#[test]
fn test_runtime_error_gpu_init_display_includes_detail() {
    let err = RuntimeError::GpuInit {
        detail: "dx12 adapter crashed".into(),
    };
    let rendered = format!("{err}");
    assert!(
        rendered.contains("dx12 adapter crashed"),
        "display should include detail verbatim, got: {rendered}"
    );
    assert!(
        rendered.contains("gpu init"),
        "display should mention gpu init, got: {rendered}"
    );
}

#[test]
fn test_runtime_error_not_implemented_includes_feature() {
    let err = RuntimeError::NotImplemented {
        feature: "par_map".into(),
    };
    let rendered = format!("{err}");
    assert!(
        rendered.contains("par_map"),
        "display should include feature name, got: {rendered}"
    );
}

#[test]
fn test_runtime_error_unsupported_includes_detail() {
    let err = RuntimeError::Unsupported {
        detail: "no vulkan".into(),
    };
    let rendered = format!("{err}");
    assert!(
        rendered.contains("no vulkan"),
        "display should include detail, got: {rendered}"
    );
}

#[test]
fn test_runtime_error_kind_is_stable_lowercase() {
    assert_eq!(RuntimeError::GpuUnavailable.kind(), "gpu_unavailable");
    assert_eq!(
        RuntimeError::GpuInit { detail: "x".into() }.kind(),
        "gpu_init"
    );
    assert_eq!(
        RuntimeError::NotImplemented {
            feature: "x".into()
        }
        .kind(),
        "not_implemented"
    );
    assert_eq!(
        RuntimeError::Unsupported { detail: "x".into() }.kind(),
        "unsupported"
    );
}

#[test]
fn test_runtime_error_clone_partial_eq() {
    // PartialEq + Clone derives must work for deterministic test assertions.
    let a = RuntimeError::GpuUnavailable;
    let b = a.clone();
    assert_eq!(a, b);

    let c = RuntimeError::NotImplemented {
        feature: "par_map".into(),
    };
    let d = c.clone();
    assert_eq!(c, d);
    assert_ne!(a, c);
}

#[test]
fn test_runtime_error_bridges_to_buff_lang_error() {
    // The From<RuntimeError> for buff_lang_error::RuntimeError impl lets
    // runtime failures flow through the top-level BuffError hierarchy.
    let rt_err = RuntimeError::GpuUnavailable;
    let buff_err: BuffRuntimeError = rt_err.into();
    let rendered = format!("{buff_err}");
    assert!(
        rendered.contains("gpu unavailable"),
        "bridged error must preserve message, got: {rendered}"
    );
}
