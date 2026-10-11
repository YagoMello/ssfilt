//! Private FFT helpers for deterministic waveform-reference tests.

#![allow(clippy::cast_precision_loss)]

use rustfft::FftPlannerScalar;
use rustfft::num_complex::Complex;

#[derive(Clone, Debug)]
pub(super) struct Spectrum {
    cutoff_cycles: f64,
    bins: Vec<Complex<f64>>,
}

impl Spectrum {
    /// Forward-transform real samples whose frame spans `cutoff_cycles`.
    pub(super) fn forward_real(samples: &[f64], cutoff_cycles: f64) -> Self {
        assert!(!samples.is_empty(), "an FFT frame must contain samples");
        assert!(
            cutoff_cycles.is_finite() && cutoff_cycles > 0.0,
            "cutoff cycles must be finite and positive"
        );

        let mut bins: Vec<_> = samples
            .iter()
            .copied()
            .map(|sample| Complex::new(sample, 0.0))
            .collect();
        FftPlannerScalar::<f64>::new()
            .plan_fft_forward(bins.len())
            .process(&mut bins);

        Self {
            cutoff_cycles,
            bins,
        }
    }

    pub(super) fn bins(&self) -> &[Complex<f64>] {
        &self.bins
    }

    /// Map a full-spectrum index to frequency in cycles per cutoff.
    pub(super) fn signed_normalized_frequency(&self, index: usize) -> f64 {
        assert!(index < self.bins.len(), "FFT bin index is out of range");

        // For even frame lengths, the Nyquist bin takes the negative sign.
        let positive_bins = self.bins.len().div_ceil(2);
        let signed_index = if index < positive_bins {
            index as f64
        } else {
            -((self.bins.len() - index) as f64)
        };
        signed_index / self.cutoff_cycles
    }

    /// Return the fraction of two-sided spectral energy at `|W| <= band_edge`.
    pub(super) fn in_band_energy_fraction(&self, band_edge: f64) -> f64 {
        assert!(band_edge.is_finite() && band_edge >= 0.0);

        let total_energy: f64 = self.bins.iter().map(Complex::norm_sqr).sum();
        assert!(total_energy > 0.0, "spectral energy must be nonzero");

        let in_band_energy: f64 = self
            .bins
            .iter()
            .enumerate()
            .filter(|(index, _)| self.signed_normalized_frequency(*index).abs() <= band_edge)
            .map(|(_, bin)| bin.norm_sqr())
            .sum();

        in_band_energy / total_energy
    }

    /// Remove every bin outside the two-sided band `|W| <= band_edge`.
    pub(super) fn project_in_band(&mut self, band_edge: f64) {
        assert!(band_edge.is_finite() && band_edge >= 0.0);

        for index in 0..self.bins.len() {
            if self.signed_normalized_frequency(index).abs() > band_edge {
                self.bins[index] = Complex::new(0.0, 0.0);
            }
        }
    }

    /// Evaluate the real periodic signal represented by this full spectrum.
    ///
    /// `normalized_time` is measured in cutoff cycles. Exact zero bins are
    /// skipped, which makes this inexpensive for spectra projected to a band.
    pub(super) fn evaluate_real_periodic(&self, normalized_time: f64) -> f64 {
        assert!(normalized_time.is_finite());

        let mut sum = 0.0;
        for (index, bin) in self.bins.iter().enumerate() {
            if bin.re == 0.0 && bin.im == 0.0 {
                continue;
            }

            let phase =
                core::f64::consts::TAU * self.signed_normalized_frequency(index) * normalized_time;
            sum += bin.re.mul_add(phase.cos(), -bin.im * phase.sin());
        }

        sum / self.bins.len() as f64
    }

    /// Apply the independent Butterworth-magnitude, constant-delay oracle.
    pub(super) fn butterworth_constant_delay(&self, order: usize, delay: f64) -> Self {
        assert!(order > 0, "Butterworth order must be positive");
        self.magnitude_plus_constant_delay(delay, |frequency| {
            1.0 / (1.0 + frequency.powf(2.0 * order as f64)).sqrt()
        })
    }

    /// Apply an analytic low-pass magnitude and constant-delay phase oracle.
    pub(super) fn magnitude_plus_constant_delay(
        &self,
        delay: f64,
        magnitude_at: impl Fn(f64) -> f64,
    ) -> Self {
        assert!(delay.is_finite(), "delay must be finite");

        let nyquist = self
            .bins
            .len()
            .is_multiple_of(2)
            .then_some(self.bins.len() / 2);
        let bins = self
            .bins
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let frequency = self.signed_normalized_frequency(index);
                let magnitude = magnitude_at(frequency.abs());
                assert!(magnitude.is_finite() && magnitude >= 0.0);
                let phase = -delay * frequency;

                // An even-length Nyquist bin is self-conjugate. Project the
                // +/- Nyquist phase pair onto its real component so the
                // reference remains a real sequence at this aliased endpoint.
                let delay_factor = if Some(index) == nyquist {
                    Complex::new(phase.cos(), 0.0)
                } else {
                    Complex::from_polar(1.0, phase)
                };

                *input * magnitude * delay_factor
            })
            .collect();

        Self {
            cutoff_cycles: self.cutoff_cycles,
            bins,
        }
    }

    /// Inverse-transform and apply rustfft's required `1/N` normalization.
    pub(super) fn inverse_real(&self) -> Vec<f64> {
        let mut bins = self.bins.clone();
        FftPlannerScalar::<f64>::new()
            .plan_fft_inverse(bins.len())
            .process(&mut bins);

        let scale = 1.0 / self.bins.len() as f64;
        bins.into_iter().map(|sample| sample.re * scale).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Spectrum;
    use rustfft::num_complex::Complex;

    const TAU: f64 = core::f64::consts::TAU;

    fn cosine_frame(length: usize, components: &[(usize, f64)]) -> Vec<f64> {
        (0..length)
            .map(|sample| {
                components
                    .iter()
                    .map(|&(bin, amplitude)| {
                        amplitude * (TAU * (bin * sample) as f64 / length as f64).cos()
                    })
                    .sum()
            })
            .collect()
    }

    fn assert_samples_close(actual: &[f64], expected: &[f64], tolerance: f64) {
        assert_eq!(actual.len(), expected.len());
        for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
            assert!(
                (actual - expected).abs() <= tolerance,
                "sample {index}: got {actual}, expected {expected}"
            );
        }
    }

    #[test]
    fn forward_and_inverse_fft_round_trip_real_samples() {
        let samples: Vec<_> = (0..37)
            .map(|index| {
                let time = f64::from(index);
                (TAU * time / 13.0).sin() + 0.3 * (TAU * time / 7.0).cos()
            })
            .collect();

        let restored = Spectrum::forward_real(&samples, 2.75).inverse_real();

        assert_samples_close(&restored, &samples, 2.0e-14);
    }

    #[test]
    fn fft_bins_map_to_signed_normalized_frequencies() {
        let even = Spectrum::forward_real(&[0.0; 8], 4.0);
        let expected_even = [0.0, 0.25, 0.5, 0.75, -1.0, -0.75, -0.5, -0.25];
        for (index, expected) in expected_even.into_iter().enumerate() {
            assert!((even.signed_normalized_frequency(index) - expected).abs() < 1.0e-15);
        }

        let odd = Spectrum::forward_real(&[0.0; 5], 2.5);
        let expected_odd = [0.0, 0.4, 0.8, -0.8, -0.4];
        for (index, expected) in expected_odd.into_iter().enumerate() {
            assert!((odd.signed_normalized_frequency(index) - expected).abs() < 1.0e-15);
        }
    }

    #[test]
    fn bin_aligned_tones_have_the_expected_two_sided_energy_fraction() {
        let length = 128;
        let cutoff_cycles = 8.0;
        let samples = cosine_frame(length, &[(8, 1.0), (16, 0.5)]);
        let spectrum = Spectrum::forward_real(&samples, cutoff_cycles);

        // The two tones have orthogonal bins and powers in the ratio 1:0.25.
        let in_band_fraction = spectrum.in_band_energy_fraction(1.0);
        assert!((in_band_fraction - 0.8).abs() < 1.0e-13);

        let single_tone = Spectrum::forward_real(&cosine_frame(length, &[(8, 1.0)]), cutoff_cycles);
        assert!((single_tone.in_band_energy_fraction(1.0) - 1.0).abs() < 1.0e-13);
        assert!(single_tone.in_band_energy_fraction(0.99) < 1.0e-13);
    }

    #[test]
    fn band_projection_removes_out_of_band_bins() {
        let length = 64;
        let samples = cosine_frame(length, &[(4, 1.0), (20, 0.4)]);
        let expected = cosine_frame(length, &[(4, 1.0)]);
        let mut spectrum = Spectrum::forward_real(&samples, 8.0);

        spectrum.project_in_band(0.7);

        assert_eq!(spectrum.bins()[20], Complex::new(0.0, 0.0));
        assert_eq!(spectrum.bins()[length - 20], Complex::new(0.0, 0.0));
        assert_samples_close(&spectrum.inverse_real(), &expected, 2.0e-14);
    }

    #[test]
    fn periodic_evaluation_matches_fft_samples_and_repeats() {
        const SAMPLE_COUNT: usize = 96;
        let cutoff_cycles = 7.5;
        let samples: Vec<_> = (0..SAMPLE_COUNT)
            .map(|index| {
                let time = index as f64 * cutoff_cycles / SAMPLE_COUNT as f64;
                (TAU * 0.31 * time + 0.2).cos() + 0.4 * (TAU * 0.8 * time - 0.7).sin()
            })
            .collect();
        let spectrum = Spectrum::forward_real(&samples, cutoff_cycles);
        let restored = spectrum.inverse_real();

        for (index, expected) in restored.iter().copied().enumerate() {
            let time = index as f64 * cutoff_cycles / samples.len() as f64;
            let actual = spectrum.evaluate_real_periodic(time);
            assert!(
                (actual - expected).abs() < 2.0e-13,
                "sample {index}: got {actual}, expected {expected}"
            );
        }

        for time in [-1.375, 0.0, 0.125, 3.75, 7.499, 19.25] {
            let actual = spectrum.evaluate_real_periodic(time);
            let repeated = spectrum.evaluate_real_periodic(time + cutoff_cycles);
            assert!((actual - repeated).abs() < 2.0e-12);
        }
    }

    #[test]
    fn constant_delay_butterworth_oracle_preserves_real_spectrum_symmetry() {
        let length = 128;
        let cutoff_cycles = 8.0;
        let delay = 6.615_894_124_512_561;
        let samples = cosine_frame(length, &[(4, 1.0), (11, 0.3), (64, 0.2)]);
        let input = Spectrum::forward_real(&samples, cutoff_cycles);
        let oracle = input.butterworth_constant_delay(4, delay);

        for index in 1..length / 2 {
            let mirror = length - index;
            assert!((oracle.bins()[index] - oracle.bins()[mirror].conj()).norm() < 1.0e-12);
        }
        assert!(oracle.bins()[0].im.abs() < 1.0e-12);
        assert!(oracle.bins()[length / 2].im.abs() < 1.0e-12);

        let nyquist_frequency = -(length as f64) / 2.0 / cutoff_cycles;
        let expected_nyquist_gain = 1.0 / (1.0 + nyquist_frequency.abs().powi(8)).sqrt();
        let expected_nyquist_response = expected_nyquist_gain * (-delay * nyquist_frequency).cos();
        let measured_nyquist_response = (oracle.bins()[length / 2] / input.bins()[length / 2]).re;
        assert!((measured_nyquist_response - expected_nyquist_response).abs() < 1.0e-14);

        let frequency = 4.0 / cutoff_cycles;
        let expected_gain = 1.0 / (1.0 + frequency.powi(8)).sqrt();
        let expected_response = Complex::from_polar(expected_gain, -delay * frequency);
        let measured_response = oracle.bins()[4] / input.bins()[4];
        assert!((measured_response - expected_response).norm() < 1.0e-14);

        let oracle_samples = oracle.inverse_real();
        assert!(oracle_samples.iter().all(|sample| sample.is_finite()));
        let reconstructed = Spectrum::forward_real(&oracle_samples, cutoff_cycles);
        for (actual, expected) in reconstructed.bins().iter().zip(oracle.bins()) {
            assert!((*actual - *expected).norm() < 1.0e-12);
        }
    }
}
