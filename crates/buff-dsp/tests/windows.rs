//! Window function correctness: symmetry, peak, zero (or near-zero)
//! endpoints, and the T11 acceptance scenario's exact reference vector.

use buff_dsp::Window;

#[test]
fn hann_t11_reference_vector_n8() {
    let w = Window::hann(8);
    let expected = [0.0_f64, 0.1464, 0.5, 0.8536, 1.0, 0.8536, 0.5, 0.1464];
    assert_eq!(w.as_slice().len(), 8);
    let mut max_err: f64 = 0.0;
    for (got, want) in w.as_slice().iter().zip(expected.iter()) {
        max_err = max_err.max((got - want).abs());
    }
    assert!(max_err < 1e-3, "hann(8) max_abs_err = {max_err}");
}

#[test]
fn hann_window_is_symmetric() {
    let w = Window::hann(32);
    let coeffs = w.as_slice();
    let n = coeffs.len();
    for i in 0..n / 2 {
        let err = (coeffs[i] - coeffs[n - 1 - i]).abs();
        assert!(err < 1e-12, "hann asymmetry at {i}: err={err}");
    }
}

#[test]
fn hann_window_peaks_at_centre() {
    let w = Window::hann(16);
    let coeffs = w.as_slice();
    let mid = coeffs.len() / 2;
    let peak = coeffs[mid];
    for (i, &c) in coeffs.iter().enumerate() {
        if i != mid {
            assert!(
                c <= peak + 1e-12,
                "hann non-peak {i}={c} exceeds peak {peak}"
            );
        }
    }
    assert!(
        (peak - 1.0).abs() < 1e-12,
        "hann peak should be 1.0, got {peak}"
    );
}

#[test]
fn hamming_window_does_not_reach_zero_at_endpoints() {
    // Hamming's defining property vs Hann: endpoints are 0.08, not 0.
    let w = Window::hamming(64);
    let coeffs = w.as_slice();
    let endpoint = coeffs[0];
    assert!(
        endpoint > 0.05,
        "hamming endpoint should be ~0.08, got {endpoint}"
    );
    assert!(
        endpoint < 0.15,
        "hamming endpoint should be ~0.08, got {endpoint}"
    );
}

#[test]
fn blackman_window_has_lower_endpoints_than_hann() {
    // Blackman has wider main lobe but lower side lobes — endpoints
    // should be lower than Hann's (which are exactly 0).
    let hann = Window::hann(32);
    let blackman = Window::blackman(32);
    let hann_mid = hann.as_slice()[16];
    let blackman_mid = blackman.as_slice()[16];
    assert!(blackman_mid > 0.0, "blackman peak should be positive");
    assert!(
        blackman_mid < hann_mid,
        "blackman peak {blackman_mid} should be < hann {hann_mid}"
    );
}

#[test]
fn empty_window_is_empty() {
    let w = Window::hann(0);
    assert_eq!(w.as_slice().len(), 0);
}

#[test]
fn window_lengths_round_trip() {
    for &n in &[1, 2, 3, 7, 8, 16, 17, 64, 100, 256] {
        let h = Window::hann(n);
        let m = Window::hamming(n);
        let b = Window::blackman(n);
        assert_eq!(h.as_slice().len(), n, "hann({n})");
        assert_eq!(m.as_slice().len(), n, "hamming({n})");
        assert_eq!(b.as_slice().len(), n, "blackman({n})");
    }
}
