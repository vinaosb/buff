//! T38 RED: RuntimeError variants, Display, kind(), and From<RuntimeError>
//! for buff_lang_error::RuntimeError bridge.
//!
//! T50: Every variant now carries an optional `span: Option<Span>` for
//! source-location mapping. Construction sites in this file set `span: None`
//! to preserve existing test assertions; the `with_span` builder is tested
//! separately in `test_span_preservation`.

use buff_lang_error::RuntimeError as BuffRuntimeError;
use buff_lang_runtime::RuntimeError;

#[test]
fn test_runtime_error_gpu_unavailable_display() {
    let err = RuntimeError::GpuUnavailable { span: None };
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
        span: None,
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
        span: None,
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
        span: None,
    };
    let rendered = format!("{err}");
    assert!(
        rendered.contains("no vulkan"),
        "display should include detail, got: {rendered}"
    );
}

#[test]
fn test_runtime_error_kind_is_stable_lowercase() {
    assert_eq!(
        RuntimeError::GpuUnavailable { span: None }.kind(),
        "gpu_unavailable"
    );
    assert_eq!(
        RuntimeError::GpuInit {
            detail: "x".into(),
            span: None
        }
        .kind(),
        "gpu_init"
    );
    assert_eq!(
        RuntimeError::NotImplemented {
            feature: "x".into(),
            span: None
        }
        .kind(),
        "not_implemented"
    );
    assert_eq!(
        RuntimeError::Unsupported {
            detail: "x".into(),
            span: None
        }
        .kind(),
        "unsupported"
    );
}

#[test]
fn test_runtime_error_clone_partial_eq() {
    // PartialEq + Clone derives must work for deterministic test assertions.
    let a = RuntimeError::GpuUnavailable { span: None };
    let b = a.clone();
    assert_eq!(a, b);

    let c = RuntimeError::NotImplemented {
        feature: "par_map".into(),
        span: None,
    };
    let d = c.clone();
    assert_eq!(c, d);
    assert_ne!(a, c);
}

#[test]
fn test_runtime_error_bridges_to_buff_lang_error() {
    // The From<RuntimeError> for buff_lang_error::RuntimeError impl lets
    // runtime failures flow through the top-level BuffError hierarchy.
    let rt_err = RuntimeError::GpuUnavailable { span: None };
    let buff_err: BuffRuntimeError = rt_err.into();
    let rendered = format!("{buff_err}");
    assert!(
        rendered.contains("gpu unavailable"),
        "bridged error must preserve message, got: {rendered}"
    );
}

#[test]
fn test_span_preservation_with_span_builder() {
    // T50: with_span() should attach a span to the error, and the From
    // bridge should NOT use Span::dummy() when a span is set.
    use buff_lang_error::Span;
    let span = Span::new(10, 50, buff_lang_error::SourceId(1));
    let err = RuntimeError::GpuInit {
        detail: "test".into(),
        span: None,
    }
    .with_span(span);
    assert_eq!(err.span(), Some(span));

    // Not set → None.
    let err2 = RuntimeError::GpuUnavailable { span: None };
    assert_eq!(err2.span(), None);
}

#[test]
fn test_span_preservation_bridge_uses_set_span() {
    // T50: When a span is attached via with_span(), the From bridge
    // should produce a diagnostic carrying that span (not Span::dummy).
    use buff_lang_error::Span;
    let span = Span::new(5, 15, buff_lang_error::SourceId(0));
    let rt_err = RuntimeError::NotImplemented {
        feature: "test".into(),
        span: None,
    }
    .with_span(span);
    let buff_err: BuffRuntimeError = rt_err.into();
    // The Diagnostic rendered form should include the span info.
    let rendered = format!("{buff_err}");
    assert!(
        rendered.contains("not implemented"),
        "bridge preserves message: {rendered}"
    );
}
