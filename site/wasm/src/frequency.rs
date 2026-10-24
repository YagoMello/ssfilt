//! Bounded time-frequency analysis for the browser playground.
//!
//! This is intentionally a private product spike. It reconstructs the signal
//! according to the same input policy selected for filtering, then evaluates
//! a compact-support complex Morlet wavelet bank on a fixed display grid.

use num_traits::ToPrimitive;
use serde::Serialize;

use super::InputPolicy;

const TIME_POINTS: usize = 640;
const FREQUENCY_BINS: usize = 30;
const MORLET_OMEGA: f64 = 6.0;
const SUPPORT_SCALES: f64 = 4.0;
const DISPLAY_FLOOR_DB: f64 = -48.0;

#[derive(Debug, Serialize)]
pub(crate) struct FrequencyGuideData {
    pub(crate) time_seconds: Vec<f64>,
    pub(crate) frequency_hz: Vec<f64>,
    /// Row-major relative magnitude in dB, with one row per frequency.
    pub(crate) magnitude_db: Vec<f64>,
    pub(crate) gap_start_seconds: Vec<f64>,
    pub(crate) gap_end_seconds: Vec<f64>,
    pub(crate) median_interval_seconds: f64,
    pub(crate) maximum_interval_seconds: f64,
    pub(crate) display_floor_db: f64,
    pub(crate) summary: String,
}

pub(crate) fn analyze(
    times: &[f64],
    values: &[f64],
    input_policy: InputPolicy,
) -> Result<FrequencyGuideData, String> {
    validate_samples(times, values)?;

    let duration = times[times.len() - 1] - times[0];
    let mut intervals: Vec<f64> = times.windows(2).map(|pair| pair[1] - pair[0]).collect();
    intervals.sort_unstable_by(f64::total_cmp);
    let median_interval = median(&intervals);
    let maximum_interval = intervals.last().copied().unwrap_or(median_interval);
    let grid_dt = duration / f64::from(u32::try_from(TIME_POINTS - 1).expect("small grid"));

    // Two observed cycles give the lowest displayed frequency at least some
    // temporal context. The upper bound is conservative relative to both the
    // observed median spacing and the regular analysis grid.
    let minimum_frequency = 2.0 / duration;
    let maximum_frequency = (0.4 / median_interval).min(0.45 / grid_dt);
    if !minimum_frequency.is_finite()
        || !maximum_frequency.is_finite()
        || maximum_frequency <= minimum_frequency * 1.05
    {
        return Err(
            "this capture does not contain enough temporal detail for the frequency guide"
                .to_owned(),
        );
    }

    let time_seconds = uniform_times(times[0], times[times.len() - 1]);
    let mut signal = reconstruct(times, values, &time_seconds, input_policy);
    let mean = signal.iter().sum::<f64>()
        / f64::from(u32::try_from(signal.len()).expect("bounded analysis grid"));
    for sample in &mut signal {
        *sample -= mean;
    }

    let frequency_hz = logarithmic_frequencies(minimum_frequency, maximum_frequency);
    let mut magnitude = Vec::with_capacity(TIME_POINTS * FREQUENCY_BINS);
    let mut peak = 0.0_f64;
    for &frequency in &frequency_hz {
        let scale = MORLET_OMEGA / (core::f64::consts::TAU * frequency);
        let half_width = (SUPPORT_SCALES * scale / grid_dt)
            .ceil()
            .to_usize()
            .unwrap_or(TIME_POINTS - 1)
            .min(TIME_POINTS - 1);
        let (kernel_real, kernel_imaginary) = morlet_kernel(half_width, grid_dt, scale);

        for center in 0..TIME_POINTS {
            let first = center.saturating_sub(half_width);
            let last = center.saturating_add(half_width).min(TIME_POINTS - 1);
            let mut real = 0.0;
            let mut imaginary = 0.0;
            let mut norm_squared = 0.0;
            for (sample_index, &sample) in signal.iter().enumerate().take(last + 1).skip(first) {
                let offset = sample_index + half_width - center;
                let kernel_re = kernel_real[offset];
                let kernel_im = kernel_imaginary[offset];
                real = sample.mul_add(kernel_re, real);
                imaginary = sample.mul_add(kernel_im, imaginary);
                norm_squared = kernel_re.mul_add(kernel_re, norm_squared);
                norm_squared = kernel_im.mul_add(kernel_im, norm_squared);
            }
            let amplitude = if norm_squared > 0.0 {
                real.hypot(imaginary) / norm_squared.sqrt()
            } else {
                0.0
            };
            peak = peak.max(amplitude);
            magnitude.push(amplitude);
        }
    }

    let magnitude_db = if peak > 0.0 {
        magnitude
            .into_iter()
            .map(|value| {
                if value > 0.0 {
                    (20.0 * (value / peak).log10()).max(DISPLAY_FLOOR_DB)
                } else {
                    DISPLAY_FLOOR_DB
                }
            })
            .collect()
    } else {
        vec![DISPLAY_FLOOR_DB; TIME_POINTS * FREQUENCY_BINS]
    };

    let gap_threshold = 4.0 * median_interval;
    let mut gap_start_seconds = Vec::new();
    let mut gap_end_seconds = Vec::new();
    for pair in times.windows(2) {
        if pair[1] - pair[0] > gap_threshold {
            gap_start_seconds.push(pair[0]);
            gap_end_seconds.push(pair[1]);
        }
    }

    Ok(FrequencyGuideData {
        time_seconds,
        frequency_hz,
        magnitude_db,
        gap_start_seconds,
        gap_end_seconds,
        median_interval_seconds: median_interval,
        maximum_interval_seconds: maximum_interval,
        display_floor_db: DISPLAY_FLOOR_DB,
        summary: format!(
            "Relative Morlet-wavelet magnitude on a {TIME_POINTS} × {FREQUENCY_BINS} display grid; {minimum_frequency:.4}–{maximum_frequency:.4} Hz."
        ),
    })
}

fn validate_samples(times: &[f64], values: &[f64]) -> Result<(), String> {
    if times.len() != values.len() {
        return Err("time and value columns must have the same number of rows".to_owned());
    }
    if times.len() < 4 {
        return Err("the frequency guide needs at least four samples".to_owned());
    }
    if times.len() > super::MAX_UPLOAD_ROWS {
        return Err(format!(
            "the frequency guide accepts at most {} samples",
            super::MAX_UPLOAD_ROWS
        ));
    }
    if times.iter().chain(values).any(|value| !value.is_finite()) {
        return Err("timestamps and sample values must all be finite".to_owned());
    }
    if times.windows(2).any(|pair| pair[1] <= pair[0]) {
        return Err("timestamps must be strictly increasing".to_owned());
    }
    Ok(())
}

fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        sorted[middle - 1].midpoint(sorted[middle])
    } else {
        sorted[middle]
    }
}

fn uniform_times(first: f64, last: f64) -> Vec<f64> {
    let duration = last - first;
    (0..TIME_POINTS)
        .map(|index| {
            let fraction = f64::from(u32::try_from(index).expect("small grid index"))
                / f64::from(u32::try_from(TIME_POINTS - 1).expect("small grid"));
            fraction.mul_add(duration, first)
        })
        .collect()
}

fn reconstruct(times: &[f64], values: &[f64], grid: &[f64], input_policy: InputPolicy) -> Vec<f64> {
    let mut interval = 0;
    grid.iter()
        .map(|&time| {
            while interval + 1 < times.len() && time >= times[interval + 1] {
                interval += 1;
            }
            if interval + 1 >= times.len() || time <= times[interval] {
                return values[interval];
            }

            match input_policy {
                InputPolicy::PreviousHold => values[interval],
                InputPolicy::CurrentHold => values[interval + 1],
                InputPolicy::Linear => {
                    let fraction =
                        (time - times[interval]) / (times[interval + 1] - times[interval]);
                    fraction.mul_add(values[interval + 1] - values[interval], values[interval])
                }
            }
        })
        .collect()
}

fn logarithmic_frequencies(minimum: f64, maximum: f64) -> Vec<f64> {
    let log_minimum = minimum.ln();
    let log_span = maximum.ln() - log_minimum;
    (0..FREQUENCY_BINS)
        .map(|index| {
            let fraction = f64::from(u32::try_from(index).expect("small bin index"))
                / f64::from(u32::try_from(FREQUENCY_BINS - 1).expect("small bin count"));
            fraction.mul_add(log_span, log_minimum).exp()
        })
        .collect()
}

fn morlet_kernel(half_width: usize, dt: f64, scale: f64) -> (Vec<f64>, Vec<f64>) {
    let length = 2 * half_width + 1;
    let mut real = Vec::with_capacity(length);
    let mut imaginary = Vec::with_capacity(length);
    for index in 0..length {
        let signed_offset = i32::try_from(index).expect("bounded kernel")
            - i32::try_from(half_width).expect("bounded kernel");
        let normalized_time = f64::from(signed_offset) * dt / scale;
        let envelope = (-0.5 * normalized_time * normalized_time).exp();
        let phase = MORLET_OMEGA * normalized_time;
        real.push(envelope * phase.cos());
        imaginary.push(-envelope * phase.sin());
    }

    // Remove the small discrete/truncation DC component. This also keeps a
    // constant reconstructed signal visually quiet at the longest scales.
    let count = f64::from(u32::try_from(length).expect("bounded kernel"));
    let real_mean = real.iter().sum::<f64>() / count;
    let imaginary_mean = imaginary.iter().sum::<f64>() / count;
    for value in &mut real {
        *value -= real_mean;
    }
    for value in &mut imaginary {
        *value -= imaginary_mean;
    }
    (real, imaginary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstruction_matches_the_selected_interval_policy() {
        let times = [0.0, 1.0, 2.0];
        let values = [0.0, 10.0, 20.0];
        let grid = [0.0, 0.5, 1.0, 1.5, 2.0];
        assert_eq!(
            reconstruct(&times, &values, &grid, InputPolicy::Linear),
            [0.0, 5.0, 10.0, 15.0, 20.0]
        );
        assert_eq!(
            reconstruct(&times, &values, &grid, InputPolicy::PreviousHold),
            [0.0, 0.0, 10.0, 10.0, 20.0]
        );
        assert_eq!(
            reconstruct(&times, &values, &grid, InputPolicy::CurrentHold),
            [0.0, 10.0, 10.0, 20.0, 20.0]
        );
    }

    #[test]
    fn guide_finds_a_tone_on_irregular_timestamps() {
        let mut times = Vec::new();
        let mut values = Vec::new();
        let mut time = 0.0;
        for index in 0..420 {
            times.push(time);
            values.push((core::f64::consts::TAU * 8.0 * time).sin());
            time += if index % 3 == 0 { 0.004 } else { 0.006 };
        }

        let guide = analyze(&times, &values, InputPolicy::Linear).unwrap();
        let strongest = guide
            .frequency_hz
            .iter()
            .enumerate()
            .max_by(|(left, _), (right, _)| {
                let left_peak = guide.magnitude_db[left * TIME_POINTS..(left + 1) * TIME_POINTS]
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max);
                let right_peak = guide.magnitude_db[right * TIME_POINTS..(right + 1) * TIME_POINTS]
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max);
                left_peak.total_cmp(&right_peak)
            })
            .map(|(_, frequency)| *frequency)
            .unwrap();
        assert!(
            (strongest / 8.0).ln().abs() < 0.3,
            "strongest frequency: {strongest}"
        );
        assert_eq!(guide.magnitude_db.len(), TIME_POINTS * FREQUENCY_BINS);
        assert!(guide.magnitude_db.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn guide_marks_intervals_much_larger_than_the_median() {
        let guide = analyze(
            &[0.0, 0.01, 0.02, 0.5, 0.51, 0.52],
            &[0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            InputPolicy::Linear,
        )
        .unwrap();
        assert_eq!(guide.gap_start_seconds, [0.02]);
        assert_eq!(guide.gap_end_seconds, [0.5]);
    }

    fn direct_irregular_ridge(
        times: &[f64],
        values: &[f64],
        frequencies: &[f64],
        center: f64,
    ) -> f64 {
        let quadrature_weights: Vec<f64> = (0..times.len())
            .map(|index| match index {
                0 => (times[1] - times[0]) / 2.0,
                last if last + 1 == times.len() => (times[last] - times[last - 1]) / 2.0,
                _ => (times[index + 1] - times[index - 1]) / 2.0,
            })
            .collect();

        frequencies
            .iter()
            .copied()
            .max_by(|left, right| {
                let amplitude = |frequency: f64| {
                    let scale = MORLET_OMEGA / (core::f64::consts::TAU * frequency);
                    let mut weighted_sum = 0.0;
                    let mut weight_sum = 0.0;
                    for ((&time, &value), &quadrature) in
                        times.iter().zip(values).zip(&quadrature_weights)
                    {
                        let normalized_time = (time - center) / scale;
                        if normalized_time.abs() <= SUPPORT_SCALES {
                            let weight =
                                (-0.5 * normalized_time * normalized_time).exp() * quadrature;
                            weighted_sum = value.mul_add(weight, weighted_sum);
                            weight_sum += weight;
                        }
                    }
                    let local_mean = weighted_sum / weight_sum.max(f64::MIN_POSITIVE);
                    let mut real = 0.0;
                    let mut imaginary = 0.0;
                    let mut norm = 0.0;
                    for ((&time, &value), &quadrature) in
                        times.iter().zip(values).zip(&quadrature_weights)
                    {
                        let normalized_time = (time - center) / scale;
                        if normalized_time.abs() <= SUPPORT_SCALES {
                            let envelope = (-0.5 * normalized_time * normalized_time).exp();
                            let phase = MORLET_OMEGA * normalized_time;
                            let weighted_value = (value - local_mean) * envelope * quadrature;
                            real = phase.cos().mul_add(weighted_value, real);
                            imaginary = (-phase.sin()).mul_add(weighted_value, imaginary);
                            norm = envelope.mul_add(quadrature, norm);
                        }
                    }
                    real.hypot(imaginary) / norm.max(f64::MIN_POSITIVE)
                };
                amplitude(*left).total_cmp(&amplitude(*right))
            })
            .unwrap()
    }

    fn guide_ridge(guide: &FrequencyGuideData, center: f64) -> f64 {
        let time_index = guide
            .time_seconds
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                (*left - center).abs().total_cmp(&(*right - center).abs())
            })
            .unwrap()
            .0;
        guide
            .frequency_hz
            .iter()
            .enumerate()
            .max_by(|(left, _), (right, _)| {
                guide.magnitude_db[left * TIME_POINTS + time_index]
                    .total_cmp(&guide.magnitude_db[right * TIME_POINTS + time_index])
            })
            .map(|(_, frequency)| *frequency)
            .unwrap()
    }

    fn frequency_bin_error(frequencies: &[f64], observed: f64, expected: f64) -> usize {
        let nearest = |target: f64| {
            frequencies
                .iter()
                .enumerate()
                .min_by(|(_, left), (_, right)| {
                    (*left - target).abs().total_cmp(&(*right - target).abs())
                })
                .unwrap()
                .0
        };
        nearest(observed).abs_diff(nearest(expected))
    }

    fn irregular_timestamps(duration: f64, severe: bool) -> Vec<f64> {
        let mut state = 0x5f37_59df_u32;
        let mut times = Vec::new();
        let mut time = 0.0;
        while time <= duration {
            times.push(time);
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let unit = f64::from(state) / f64::from(u32::MAX);
            time += if severe {
                0.001 + 0.017 * unit
            } else {
                0.006 + 0.004 * unit
            };
        }
        times
    }

    fn magnitude_at(guide: &FrequencyGuideData, time: f64, frequency: f64) -> f64 {
        let time_index = guide
            .time_seconds
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| (*left - time).abs().total_cmp(&(*right - time).abs()))
            .unwrap()
            .0;
        let frequency_index = guide
            .frequency_hz
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                (*left - frequency)
                    .abs()
                    .total_cmp(&(*right - frequency).abs())
            })
            .unwrap()
            .0;
        10.0_f64.powf(guide.magnitude_db[frequency_index * TIME_POINTS + time_index] / 20.0)
    }

    #[test]
    fn reconstructed_and_direct_irregular_guides_agree_on_tone_ridges() {
        for clustered in [false, true] {
            let mut times = Vec::new();
            let mut values = Vec::new();
            let mut time = 0.0;
            let mut index = 0;
            while time < 3.0 {
                times.push(time);
                values.push((core::f64::consts::TAU * 8.0 * time).sin());
                time += if clustered {
                    [0.001, 0.0015, 0.0175][index % 3]
                } else {
                    [0.004, 0.006, 0.005][index % 3]
                };
                index += 1;
            }

            let guide = analyze(&times, &values, InputPolicy::Linear).unwrap();
            for center in [0.8, 1.5, 2.2] {
                let reconstructed = guide_ridge(&guide, center);
                let direct = direct_irregular_ridge(&times, &values, &guide.frequency_hz, center);
                assert!((reconstructed / 8.0).ln().abs() < 0.28);
                assert!((direct / 8.0).ln().abs() < 0.28);
                assert!((reconstructed / direct).ln().abs() < 0.28);
            }
        }
    }

    #[test]
    fn severe_jitter_preserves_a_stationary_tone_ridge() {
        let times = irregular_timestamps(4.0, true);
        let values: Vec<f64> = times
            .iter()
            .map(|time| (core::f64::consts::TAU * 8.0 * time).sin())
            .collect();
        let guide = analyze(&times, &values, InputPolicy::Linear).unwrap();

        for center in [0.9, 1.8, 2.8] {
            let reconstructed = guide_ridge(&guide, center);
            let direct = direct_irregular_ridge(&times, &values, &guide.frequency_hz, center);
            assert!(frequency_bin_error(&guide.frequency_hz, reconstructed, 8.0) <= 1);
            assert!(frequency_bin_error(&guide.frequency_hz, direct, 8.0) <= 1);
            assert!(frequency_bin_error(&guide.frequency_hz, reconstructed, direct) <= 1);
        }
    }

    #[test]
    fn severe_jitter_preserves_an_interior_chirp_ridge() {
        let duration = 4.0_f64;
        let start_hz = 2.0_f64;
        let rate_hz_per_second = 4.0_f64;
        let times = irregular_timestamps(duration, true);
        let values: Vec<f64> = times
            .iter()
            .map(|time| {
                let cycles = start_hz.mul_add(*time, 0.5 * rate_hz_per_second * time * time);
                (core::f64::consts::TAU * cycles).sin()
            })
            .collect();
        let guide = analyze(&times, &values, InputPolicy::Linear).unwrap();

        for center in [0.8, 1.8, 2.8] {
            let expected = rate_hz_per_second.mul_add(center, start_hz);
            let reconstructed = guide_ridge(&guide, center);
            let direct = direct_irregular_ridge(&times, &values, &guide.frequency_hz, center);
            assert!(frequency_bin_error(&guide.frequency_hz, reconstructed, expected) <= 2);
            assert!(frequency_bin_error(&guide.frequency_hz, direct, expected) <= 2);
            assert!(frequency_bin_error(&guide.frequency_hz, reconstructed, direct) <= 2);
        }
    }

    #[test]
    fn step_broadband_energy_is_localized_near_the_transition() {
        let times = irregular_timestamps(4.0, false);
        let values: Vec<f64> = times
            .iter()
            .map(|time| if *time < 2.0 { 0.0 } else { 1.0 })
            .collect();
        let guide = analyze(&times, &values, InputPolicy::PreviousHold).unwrap();

        for frequency in [8.0, 12.0, 18.0] {
            let near = magnitude_at(&guide, 2.0, frequency);
            let away =
                magnitude_at(&guide, 1.0, frequency).max(magnitude_at(&guide, 3.0, frequency));
            assert!(near > 2.0 * away, "frequency {frequency}: {near} vs {away}");
        }
    }

    #[test]
    fn analysis_work_stays_bounded_at_the_upload_limit() {
        let times: Vec<f64> = (0..super::super::MAX_UPLOAD_ROWS)
            .map(|index| f64::from(u32::try_from(index).expect("upload limit fits u32")) * 0.001)
            .collect();
        let values: Vec<f64> = times
            .iter()
            .map(|time| (core::f64::consts::TAU * 8.0 * time).sin())
            .collect();
        let guide = analyze(&times, &values, InputPolicy::Linear).unwrap();
        assert_eq!(guide.time_seconds.len(), TIME_POINTS);
        assert_eq!(guide.frequency_hz.len(), FREQUENCY_BINS);
        assert_eq!(guide.magnitude_db.len(), TIME_POINTS * FREQUENCY_BINS);
    }
}
