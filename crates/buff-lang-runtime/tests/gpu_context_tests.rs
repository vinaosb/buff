//! T38 RED: GpuContext::new() returns Result and never panics. On hosts
//! without a GPU adapter it returns Err(GpuContextError::NoAdapter).
//!
//! T43 EXT: GpuContext::device_queue() lazily acquires and CACHES a
//! (Device, Queue) pair via OnceLock. Cached-ness is verified by
//! `device_init_count()` staying at 1 across many calls and by
//! `std::ptr::eq` on the returned `&Device`/`&Queue` references. The
//! unavailable() placeholder context returns cached
//! `GpuContextError::NoAdapter` and never panics.

use buff_lang_runtime::{DispatchKind, Dispatcher, GpuContext, GpuContextError, RuntimeError};

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

// =============================================================================
// T43 — Device + Queue lazy init + caching tests.
// =============================================================================

#[test]
fn test_gpu_context_device_init_count_starts_at_zero_new() {
    // Fresh Ok(GpuContext) from new() must report 0 device init attempts.
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(GpuContextError::NoAdapter) => return, // No GPU on this host; skip.
        Err(other) => panic!("unexpected error from GpuContext::new(): {other:?}"),
    };
    assert_eq!(
        ctx.device_init_count(),
        0,
        "freshly constructed GpuContext should not have attempted device init yet"
    );
}

#[test]
fn test_gpu_context_device_init_count_starts_at_zero_unavailable() {
    // The unavailable() placeholder must also report 0 device init attempts
    // before any device_queue() call.
    let ctx = GpuContext::unavailable();
    assert_eq!(ctx.device_init_count(), 0);
}

#[test]
fn test_gpu_context_device_queue_never_panics_on_real_adapter() {
    // On a host with a GPU, device_queue() must not panic — success OR
    // graceful Err both acceptable. On a host without a GPU, new() returns
    // NoAdapter and we skip.
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(GpuContextError::NoAdapter) => return,
        Err(other) => panic!("unexpected error from GpuContext::new(): {other:?}"),
    };

    // catch_unwind on a closure that drives device_queue. AssertUnwindSafe
    // wrapper is needed because wgpu::Adapter (held inside GpuContext)
    // transitively contains RwLock/Mutex (interior mutability), which is
    // not UnwindSafe by default. We are only observing that device_queue()
    // does not panic — we drop the result — so AssertUnwindSafe is sound.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Drop the result — we only care that it did not panic.
        let _ = ctx.device_queue();
    }));
    assert!(
        result.is_ok(),
        "device_queue() must not panic on any path, got: {:?}",
        result.err()
    );
}

#[test]
fn test_gpu_context_device_queue_cached_on_success() {
    // CRITICAL T43 INVARIANT: if device_queue() succeeds once, subsequent
    // calls must NOT re-request the device (verified by device_init_count
    // staying at exactly 1 across many calls).
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(GpuContextError::NoAdapter) => return, // No GPU; cached-ness covered by other tests.
        Err(other) => panic!("unexpected error from GpuContext::new(): {other:?}"),
    };

    // Drive device_queue many times.
    let first = ctx.device_queue();
    // device_init_count is now either 1 (we attempted init) — there is no
    // path where it stays 0 after a device_queue() call.
    assert!(
        ctx.device_init_count() >= 1,
        "device_queue() must increment device_init_count"
    );

    match &first {
        Ok(_) => {
            // SUCCESS path — subsequent calls MUST stay at 1.
            for _ in 0..5 {
                let _ = ctx.device_queue();
            }
            assert_eq!(
                ctx.device_init_count(),
                1,
                "device_queue() must be cached — device_init_count must stay at 1 after success"
            );
        }
        Err(e) => {
            // FAILURE path (some hosts can't acquire a device even with an
            // adapter — e.g. driver issues). The failure must STILL be cached:
            // subsequent calls return cached Err without re-requesting.
            let first_err_detail = format!("{e}");
            for _ in 0..5 {
                let _ = ctx.device_queue();
            }
            assert_eq!(
                ctx.device_init_count(),
                1,
                "device_queue() must cache failures too — count must stay at 1"
            );
            // And the same error must come back from a follow-up call.
            match ctx.device_queue() {
                Err(e2) => assert_eq!(
                    format!("{e2}"),
                    first_err_detail,
                    "cached failure must be the same on subsequent calls"
                ),
                Ok(_) => {
                    panic!("device_queue returned Ok on second call after first returned Err")
                }
            }
        }
    }
}

#[test]
fn test_gpu_context_device_returns_same_pointer_each_call() {
    // Cached-ness: device() returns the SAME &Device reference on every
    // call (proved by std::ptr::eq on the addresses).
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(GpuContextError::NoAdapter) => return,
        Err(other) => panic!("unexpected error from GpuContext::new(): {other:?}"),
    };

    let d1 = match ctx.device() {
        Ok(d) => d,
        Err(_) => return, // device init failed (acceptable on some hosts); cached-ness covered elsewhere.
    };
    let d2 = ctx
        .device()
        .expect("second device() call must succeed if first did");
    let d3 = ctx
        .device()
        .expect("third device() call must succeed if first did");

    assert!(
        std::ptr::eq(d1, d2),
        "device() must return the same pointer on every call (cached)"
    );
    assert!(
        std::ptr::eq(d2, d3),
        "device() must return the same pointer on every call (cached)"
    );
}

#[test]
fn test_gpu_context_queue_returns_same_pointer_each_call() {
    // Cached-ness for queue(): symmetric to the device() test.
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(GpuContextError::NoAdapter) => return,
        Err(other) => panic!("unexpected error from GpuContext::new(): {other:?}"),
    };

    let q1 = match ctx.queue() {
        Ok(q) => q,
        Err(_) => return,
    };
    let q2 = ctx
        .queue()
        .expect("second queue() call must succeed if first did");
    let q3 = ctx
        .queue()
        .expect("third queue() call must succeed if first did");

    assert!(
        std::ptr::eq(q1, q2),
        "queue() must return the same pointer on every call (cached)"
    );
    assert!(
        std::ptr::eq(q2, q3),
        "queue() must return the same pointer on every call (cached)"
    );
}

#[test]
fn test_gpu_context_device_and_queue_consistent_with_device_queue() {
    // When device_queue() returns Ok, device() and queue() must ALSO return
    // Ok (and they must come from the same cached tuple).
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(GpuContextError::NoAdapter) => return,
        Err(other) => panic!("unexpected error from GpuContext::new(): {other:?}"),
    };

    match ctx.device_queue() {
        Ok((d_from_pair, q_from_pair)) => {
            let d_from_device = ctx
                .device()
                .expect("device() must be Ok if device_queue() is Ok");
            let q_from_queue = ctx
                .queue()
                .expect("queue() must be Ok if device_queue() is Ok");
            assert!(
                std::ptr::eq(d_from_pair, d_from_device),
                "device() must return the same device as device_queue().0"
            );
            assert!(
                std::ptr::eq(q_from_pair, q_from_queue),
                "queue() must return the same queue as device_queue().1"
            );
        }
        Err(_) => {
            // device_queue failed — device() and queue() must also fail.
            assert!(
                ctx.device().is_err(),
                "device() must be Err if device_queue() is Err"
            );
            assert!(
                ctx.queue().is_err(),
                "queue() must be Err if device_queue() is Err"
            );
        }
    }
}

#[test]
fn test_gpu_context_unavailable_device_queue_returns_no_adapter() {
    // The unavailable() placeholder context must return Err(NoAdapter)
    // from device_queue() — never Ok, never panic, never DeviceRequest.
    let ctx = GpuContext::unavailable();
    let result = ctx.device_queue();
    match result {
        Err(GpuContextError::NoAdapter) => {
            // Expected: no adapter means we cannot request a device.
        }
        Err(other) => panic!("unavailable() device_queue() must return NoAdapter, got: {other:?}"),
        Ok(_) => panic!("unavailable() device_queue() must NEVER return Ok"),
    }
}

#[test]
fn test_gpu_context_unavailable_device_queue_never_panics() {
    // catch_unwind: device_queue() on unavailable() must not panic under
    // any circumstance, even on the first call (which drives the OnceLock
    // initialization).
    let ctx = GpuContext::unavailable();
    // AssertUnwindSafe wrapper is required because GpuContext transitively
    // contains wgpu::Adapter (which embeds RwLock/Mutex deep inside via
    // wgpu-core's Hub). For the unavailable() case the adapter is None, but
    // the *type* still must satisfy UnwindSafe — AssertUnwindSafe asserts
    // that we are only observing absence of panic, which is sound.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = ctx.device_queue();
    }));
    assert!(
        result.is_ok(),
        "device_queue() on unavailable() must not panic, got: {:?}",
        result.err()
    );
}

#[test]
fn test_gpu_context_unavailable_device_queue_cached_no_adapter() {
    // Multiple calls to device_queue() on unavailable() must ALL return
    // NoAdapter AND device_init_count must stay at exactly 1 (the failure
    // is cached, not re-attempted).
    let ctx = GpuContext::unavailable();
    for i in 0..5 {
        match ctx.device_queue() {
            Err(GpuContextError::NoAdapter) => { /* expected */ }
            Err(other) => panic!(
                "call #{i}: unavailable() device_queue() returned unexpected error: {other:?}"
            ),
            Ok(_) => panic!("call #{i}: unavailable() device_queue() must NEVER return Ok"),
        }
    }
    assert_eq!(
        ctx.device_init_count(),
        1,
        "device_init_count must be exactly 1 after first device_queue() on unavailable()"
    );
}

#[test]
fn test_gpu_context_has_device_starts_false() {
    // Before any device_queue() call, has_device() must be false on both
    // Ok(new()) and unavailable() contexts.
    let placeholder = GpuContext::unavailable();
    assert!(
        !placeholder.has_device(),
        "has_device() must be false before any device_queue() call"
    );
    if let Ok(ctx) = GpuContext::new() {
        assert!(
            !ctx.has_device(),
            "has_device() must be false before any device_queue() call (lazy init)"
        );
    }
}

#[test]
fn test_gpu_context_has_device_true_after_successful_init() {
    // After a successful device_queue() call, has_device() must report true.
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(GpuContextError::NoAdapter) => return,
        Err(other) => panic!("unexpected error from GpuContext::new(): {other:?}"),
    };

    match ctx.device_queue() {
        Ok(_) => assert!(
            ctx.has_device(),
            "has_device() must be true after a successful device_queue() call"
        ),
        Err(_) => assert!(
            !ctx.has_device(),
            "has_device() must stay false after a failed device_queue() call"
        ),
    }
}

#[test]
fn test_gpu_context_has_device_stays_false_on_unavailable() {
    // unavailable() context must NEVER report has_device() == true, even
    // after device_queue() has been called.
    let ctx = GpuContext::unavailable();
    assert!(!ctx.has_device(), "has_device() must be false initially");
    let _ = ctx.device_queue();
    assert!(
        !ctx.has_device(),
        "has_device() must stay false on unavailable() even after device_queue() call"
    );
    let _ = ctx.device_queue();
    assert!(
        !ctx.has_device(),
        "has_device() must stay false on unavailable() across multiple calls"
    );
}

#[test]
fn test_gpu_context_default_is_unavailable_equivalent() {
    // Default::default() must produce a context equivalent to unavailable():
    // no adapter, NoAdapter from device_queue(), supports_gpu == false.
    let ctx = GpuContext::default();
    assert!(
        !ctx.has_adapter(),
        "Default::default() must have no adapter"
    );
    assert!(
        !ctx.supports_gpu(),
        "Default::default() must not support GPU"
    );
    assert!(
        !ctx.has_device(),
        "Default::default() must not have a device"
    );
    match ctx.device_queue() {
        Err(GpuContextError::NoAdapter) => { /* expected */ }
        other => panic!("Default::default().device_queue() must be NoAdapter, got: {other:?}"),
    }
}

#[test]
fn test_gpu_context_error_device_request_display() {
    // The DeviceRequest variant (now wired up by T43) must render with a
    // mention of "device request" so diagnostics are useful.
    let detail = "RequestDeviceError { inner: SomeLimitExceeded }";
    let err = GpuContextError::DeviceRequest(detail.to_string());
    let rendered = format!("{err}");
    assert!(
        rendered.to_lowercase().contains("device request"),
        "DeviceRequest display should mention 'device request', got: {rendered}"
    );
    assert!(
        rendered.contains(detail),
        "DeviceRequest display should include the detail string verbatim, got: {rendered}"
    );
}

#[test]
fn test_gpu_context_device_request_bridges_to_runtime_error_gpu_init() {
    // GpuContextError::DeviceRequest(s) must bridge into RuntimeError::GpuInit { detail: s }
    // via the From impl — so that device-init failures flow through the
    // same diagnostic pipeline as every other runtime error.
    let detail = "RequestDeviceError { .. }".to_string();
    let gpu_err: RuntimeError = GpuContextError::DeviceRequest(detail.clone()).into();
    match gpu_err {
        RuntimeError::GpuInit { detail: got, .. } => assert_eq!(
            got, detail,
            "GpuInit detail must be the same string passed to DeviceRequest"
        ),
        other => panic!("DeviceRequest must bridge to GpuInit, got: {other:?}"),
    }
}

#[test]
fn test_gpu_context_no_adapter_bridges_to_runtime_error_gpu_unavailable() {
    // GpuContextError::NoAdapter must bridge to RuntimeError::GpuUnavailable.
    let gpu_err: RuntimeError = GpuContextError::NoAdapter.into();
    assert!(
        matches!(gpu_err, RuntimeError::GpuUnavailable { .. }),
        "NoAdapter must bridge to GpuUnavailable, got: {gpu_err:?}"
    );
}
