//! T38 RED: GpuContext::new() returns Result and never panics. On hosts
//! without a GPU adapter it returns Err(GpuContextError::NoAdapter).

use buff_lang_runtime::{DispatchKind, Dispatcher, GpuContext, GpuContextError};

#[test]
fn test_gpu_context_new_returns_result_never_panics() {
    // Whether or not this host has a GPU, GpuContext::new() must return a
    // Result and never panic. We accept both Ok and Err(GpuUnavailable).
    let result = std::panic::catch_unwind(GpuContext::new);
    assert!(
        result.is_ok(),
        "GpuContext::new() must not panic, got: {:?}",
        result.err()
    );
    // Inner Result: either Ok(ctx) or Err(NoAdapter). Either is acceptable.
    let _ = result
        .expect("checked ok")
        .map_err(|e| format!("GpuContext::new() returned an error (acceptable if no GPU): {e}"));
}

#[test]
fn test_gpu_context_new_err_is_no_adapter_when_no_gpu() {
    // When the host has no GPU, the error must specifically be NoAdapter,
    // not a panic and not a generic failure.
    match GpuContext::new() {
        Ok(ctx) => {
            // This host has a GPU — verify the context is usable.
            assert!(ctx.has_adapter(), "Ok(ctx) requires has_adapter()==true");
            let d: &dyn Dispatcher = &ctx;
            assert_eq!(d.kind(), DispatchKind::GpuCompute);
            assert!(d.supports_gpu());
        }
        Err(GpuContextError::NoAdapter) => {
            // Expected on CI / hosts without GPU drivers.
        }
        Err(other) => panic!(
            "GpuContext::new() returned unexpected error variant: {other:?} \
             (only NoAdapter is acceptable here in T38)"
        ),
    }
}

#[test]
fn test_gpu_context_adapter_name_is_string_when_ok() {
    // When Ok, adapter_name() must return a non-empty string (or empty if
    // the adapter driver returned no name — but the type must be &str).
    match GpuContext::new() {
        Ok(ctx) => {
            let _name: &str = ctx.adapter_name();
        }
        Err(_) => {
            // No GPU on this host; skip.
        }
    }
}

#[test]
fn test_gpu_context_kind_is_gpu_compute_even_without_adapter() {
    // A placeholder context (T43 will use this for cfg-gated fallback)
    // still reports DispatchKind::GpuCompute as its target kind.
    let ctx = GpuContext::unavailable();
    let d: &dyn Dispatcher = &ctx;
    assert_eq!(d.kind(), DispatchKind::GpuCompute);
    assert!(!d.supports_gpu());
    assert!(!ctx.has_adapter());
}

#[test]
fn test_gpu_context_error_no_adapter_display() {
    let err = GpuContextError::NoAdapter;
    let rendered = format!("{err}");
    assert!(
        rendered.to_lowercase().contains("adapter"),
        "NoAdapter display should mention adapter, got: {rendered}"
    );
}

#[test]
fn test_gpu_context_supports_gpu_only_when_adapter_present() {
    // supports_gpu() must reflect has_adapter() consistently.
    let placeholder = GpuContext::unavailable();
    assert_eq!(placeholder.has_adapter(), placeholder.supports_gpu());

    if let Ok(ctx) = GpuContext::new() {
        assert_eq!(ctx.has_adapter(), ctx.supports_gpu());
    }
}

#[test]
fn test_gpu_context_can_be_held_as_dyn_dispatcher() {
    // Trait object works for both Ok and unavailable paths.
    let mut holders: Vec<Box<dyn Dispatcher>> = Vec::new();
    holders.push(Box::new(GpuContext::unavailable()));
    if let Ok(ctx) = GpuContext::new() {
        holders.push(Box::new(ctx));
    }
    // All entries report DispatchKind::GpuCompute (target backend).
    for d in &holders {
        assert_eq!(d.kind(), DispatchKind::GpuCompute);
    }
}
