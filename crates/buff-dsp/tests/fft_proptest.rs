//! Proptest: FFT→IFFT roundtrip is approximately the identity for
//! arbitrary real signals. This is the canonical FFT correctness
//! property — a real signal of length N, FFT'd then IFFT'd, must
//! recover the original samples within float noise.

use buff_dsp::Signal;
use proptest::collection::vec;
use proptest::prelude::*;

proptest! {
    /// Roundtrip property: `s.fft().ifft() ≈ s` for any real signal
    /// of even length 8..=512. Odd lengths are excluded: realfft's
    /// C2R output length is `2*(bins-1)`, which drops the final
    /// sample of an odd-length signal (power-of-2 NOT required).
    #[test]
    fn fft_ifft_roundtrip_arbitrary_length(
        samples in vec(-1e3..1e3, 8..=512)
            .prop_filter("even length (realfft c2r)", |s| s.len() % 2 == 0)
    ) {
        let original = Signal::from_vec(samples.clone(), 8_000);
        let recovered = Signal::ifft(original.clone().fft());
        prop_assert_eq!(recovered.len(), original.len());
        let max_err = original.as_slice().iter()
            .zip(recovered.as_slice().iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f64, f64::max);
        let scale = original.as_slice().iter().map(|x| x.abs()).fold(1e-9_f64, f64::max);
        prop_assert!(
            max_err / scale < 1e-6,
            "roundtrip max relative error too large: {max_err} / {scale}"
        );
    }

    /// DC property: a constant signal's FFT has all energy in bin 0.
    /// The `mags[0] > n-1` threshold implicitly requires |value| > 1
    /// (bin 0 = value*n), so near-zero amplitudes are filtered out.
    #[test]
    fn fft_dc_signal_only_dc_bin(
        value in (-1e3f64..1e3).prop_filter("amplitude must exceed 1", |v| v.abs() > 1.0),
        n in 16usize..=64,
    ) {
        let s = Signal::from_vec(vec![value; n], n as u32);
        let spec = s.fft();
        let mags = spec.magnitudes();
        prop_assert!(mags[0] > (n as f64 - 1.0).abs());
        for (i, &m) in mags.iter().enumerate().skip(1) {
            prop_assert!(m < 1e-6, "non-DC bin {i} has magnitude {m}");
        }
    }

    /// Linearity: FFT(c·a + d·b) = c·FFT(a) + d·FFT(b) per-bin.
    #[test]
    fn fft_linearity(
        a in vec(-1e3f64..1e3f64, 32),
        b in vec(-1e3f64..1e3f64, 32),
        c in -2.0f64..2.0f64,
        d in -2.0f64..2.0f64,
    ) {
        let sa = Signal::from_vec(a.clone(), 32);
        let sb = Signal::from_vec(b.clone(), 32);
        let combined: Vec<f64> = a.iter().zip(b.iter()).map(|(x, y)| c * x + d * y).collect();
        let scomb = Signal::from_vec(combined, 32);

        let spec_a = sa.fft();
        let spec_b = sb.fft();
        let spec_comb = scomb.fft();

        for i in 0..spec_a.len() {
            let lhs_re = spec_comb.iter().nth(i).unwrap().re;
            let lhs_im = spec_comb.iter().nth(i).unwrap().im;
            let rhs_re = c * spec_a.iter().nth(i).unwrap().re + d * spec_b.iter().nth(i).unwrap().re;
            let rhs_im = c * spec_a.iter().nth(i).unwrap().im + d * spec_b.iter().nth(i).unwrap().im;
            prop_assert!((lhs_re - rhs_re).abs() < 1e-3, "real mismatch at bin {i}");
            prop_assert!((lhs_im - rhs_im).abs() < 1e-3, "imag mismatch at bin {i}");
        }
    }

    /// Parseval: the full-spectrum energy equals N * the time-domain
    /// energy (forward FFT is unnormalized). The half (hermitian)
    /// spectrum double-counts every interior bin and holds DC +
    /// Nyquist once, so
    /// `full = 2*half - |X[0]|^2 - |X[N/2]|^2`.
    /// Zero-energy (all-zero) signals are filtered out — the identity
    /// degenerates to 0/0 there.
    #[test]
    fn fft_parseval_energy_conservation(
        samples in vec(-1.0f64..1.0f64, 16usize..=128)
            .prop_filter("signal must have nonzero energy", |s| {
                s.iter().any(|&x| x != 0.0)
            })
            .prop_filter("even length (hermitian pairing)", |s| s.len() % 2 == 0),
    ) {
        let n = samples.len();
        let s = Signal::from_vec(samples.clone(), n as u32);
        let spec = s.fft();
        let mags = spec.magnitudes();
        let time_energy: f64 = samples.iter().map(|x| x * x).sum();
        let half_energy: f64 = mags.iter().map(|m| m * m).sum();
        let full_energy =
            2.0 * half_energy - mags[0] * mags[0] - mags[mags.len() - 1] * mags[mags.len() - 1];
        let ratio = full_energy / time_energy.max(1e-12);
        let tolerance = 1e-6 * (n as f64);
        prop_assert!((ratio - n as f64).abs() < tolerance);
    }
}
