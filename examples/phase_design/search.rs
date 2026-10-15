use std::error::Error;

use clap::ValueEnum;

use num_traits::ToPrimitive;
use ssfilt::Response;

use crate::transfer;

pub(super) const FIT_POINT_COUNT: usize = 4_096;
pub(super) const HOLDOUT_INTERVAL_COUNT: usize = 8_193;
pub(super) const CROSSCHECK_INTERVAL_COUNT: usize = 16_386;
pub(super) const COARSE_POINTS_PER_AXIS: usize = 65;
pub(super) const LOCAL_GRID_POINTS_PER_AXIS: usize = 33;
pub(super) const REFINEMENT_SEED_COUNT: usize = 8;
pub(super) const REFINEMENT_LEVELS: usize = 18;
pub(super) const MAX_REFINEMENT_MOVES_PER_LEVEL: usize = 16;

const MIN_SIGMA: f64 = 0.2;
const MIN_Q: f64 = 0.5;
const MAX_Q: f64 = 2.0;
const MAX_NATURAL_FREQUENCY: f64 = 4.0;

const MAX_PEAK_RESIDUAL: f64 = 0.10;
const MAX_PEAK_RESIDUAL_FRACTION: f64 = 0.25;
const MAX_PHASE_RESIDUAL_RADIANS: f64 = 0.05;
const MAX_MEAN_ADDED_DELAY: f64 = 5.0;
const MAX_F64_MAGNITUDE_CHANGE: f64 = 1.0e-12;
const MAX_F32_MAGNITUDE_CHANGE: f64 = 1.0e-5;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(super) enum Band {
    Core,
    Cutoff,
}

impl Band {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Cutoff => "cutoff",
        }
    }

    pub(super) const fn upper_frequency_ratio(self) -> f64 {
        match self {
            Self::Core => 0.7,
            Self::Cutoff => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Case {
    pub(super) response: Response,
    pub(super) response_label: &'static str,
    pub(super) order: usize,
    pub(super) band: Band,
}

impl Case {
    pub(super) const fn has_public_streaming_support(self) -> bool {
        matches!(self.order, 4 | 8) && matches!(self.band, Band::Core)
    }

    pub(super) const fn streaming_status(self) -> &'static str {
        if self.has_public_streaming_support() {
            "streaming-validated"
        } else {
            "streaming-not-validated"
        }
    }

    pub(super) const fn public_support_status(self) -> &'static str {
        if self.has_public_streaming_support() {
            "public-support-yes"
        } else {
            "public-support-no"
        }
    }

    pub(super) const fn evidence_scope(self) -> &'static str {
        if self.has_public_streaming_support() {
            "analytic-candidate / streaming-validated / public-support-yes"
        } else {
            "analytic-candidate / streaming-not-validated / public-support-no"
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Candidate {
    pub(super) sigma: f64,
    pub(super) eta: f64,
}

impl Candidate {
    fn from_unit_coordinates(q_coordinate: f64, sigma_coordinate: f64) -> Self {
        let q = (MAX_Q - MIN_Q).mul_add(q_coordinate.clamp(0.0, 1.0), MIN_Q);
        let maximum_sigma = MAX_NATURAL_FREQUENCY / (2.0 * q);
        let sigma = MIN_SIGMA * (maximum_sigma / MIN_SIGMA).powf(sigma_coordinate.clamp(0.0, 1.0));
        let eta = sigma * (4.0 * q * q - 1.0).max(0.0).sqrt();
        Self { sigma, eta }
    }

    pub(super) fn natural_frequency(self) -> f64 {
        self.sigma.hypot(self.eta)
    }

    pub(super) fn q(self) -> f64 {
        self.natural_frequency() / (2.0 * self.sigma)
    }

    pub(super) fn delay(self, frequency_ratio: f64) -> f64 {
        let natural_frequency_squared = self.sigma.mul_add(self.sigma, self.eta * self.eta);
        let real = natural_frequency_squared - frequency_ratio * frequency_ratio;
        let imaginary = 2.0 * self.sigma * frequency_ratio;
        4.0 * self.sigma * (natural_frequency_squared + frequency_ratio * frequency_ratio)
            / real.mul_add(real, imaginary * imaginary)
    }

    pub(super) fn phase(self, frequency_ratio: f64) -> f64 {
        let natural_frequency_squared = self.sigma.mul_add(self.sigma, self.eta * self.eta);
        -2.0 * (2.0 * self.sigma * frequency_ratio)
            .atan2(natural_frequency_squared - frequency_ratio * frequency_ratio)
    }

    fn mean_delay(self, upper_frequency_ratio: f64) -> f64 {
        2.0 * (((upper_frequency_ratio - self.eta) / self.sigma).atan()
            + ((upper_frequency_ratio + self.eta) / self.sigma).atan())
            / upper_frequency_ratio
    }

    fn maximum_delay(self, upper_frequency_ratio: f64) -> f64 {
        let mut maximum = self.delay(0.0).max(self.delay(upper_frequency_ratio));
        let natural_frequency_squared = self.sigma.mul_add(self.sigma, self.eta * self.eta);
        let numerator =
            natural_frequency_squared * (3.0 * self.eta * self.eta - self.sigma * self.sigma);
        if numerator > 0.0 {
            let denominator =
                natural_frequency_squared + 2.0 * self.eta * natural_frequency_squared.sqrt();
            let stationary = (numerator / denominator).sqrt();
            if stationary < upper_frequency_ratio {
                maximum = maximum.max(self.delay(stationary));
            }
        }
        maximum
    }

    fn magnitude_change(self, frequency_ratio: f64) -> f64 {
        let natural_frequency_squared = self.sigma.mul_add(self.sigma, self.eta * self.eta);
        let real = natural_frequency_squared - frequency_ratio * frequency_ratio;
        let imaginary = 2.0 * self.sigma * frequency_ratio;
        (real.hypot(-imaginary) / real.hypot(imaginary) - 1.0).abs()
    }

    #[allow(clippy::cast_possible_truncation)] // This explicitly validates f32-rounded behavior.
    fn magnitude_change_f32(self, frequency_ratio: f64) -> f64 {
        let sigma = self.sigma as f32;
        let eta = self.eta as f32;
        let frequency = frequency_ratio as f32;
        let natural_frequency_squared = sigma.mul_add(sigma, eta * eta);
        let real = natural_frequency_squared - frequency * frequency;
        let imaginary = 2.0 * sigma * frequency;
        f64::from((real.hypot(-imaginary) / real.hypot(imaginary) - 1.0).abs())
    }

    #[allow(clippy::cast_possible_truncation)] // This explicitly validates f32-rounded behavior.
    fn rounded_to_f32(self) -> Self {
        Self {
            sigma: f64::from(self.sigma as f32),
            eta: f64::from(self.eta as f32),
        }
    }

    fn satisfies_bounds(self) -> bool {
        self.sigma.is_finite()
            && self.eta.is_finite()
            && self.sigma >= MIN_SIGMA
            && self.eta >= 0.0
            && self.q() <= MAX_Q
            && self.natural_frequency() <= MAX_NATURAL_FREQUENCY
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DelayMetrics {
    pub(super) target_delay: f64,
    pub(super) peak_residual: f64,
    pub(super) rms_residual: f64,
    pub(super) maximum_phase_residual_radians: f64,
}

#[derive(Clone, Copy, Debug)]
#[allow(clippy::struct_excessive_bools)] // Each named acceptance gate is reported independently.
pub(super) struct GateResults {
    pub(super) absolute_peak: bool,
    pub(super) relative_peak: bool,
    pub(super) phase: bool,
    pub(super) mean_added_delay: bool,
    pub(super) f64_magnitude: bool,
    pub(super) f32_magnitude: bool,
    pub(super) f64_bounds: bool,
    pub(super) f32_bounds: bool,
}

impl GateResults {
    pub(super) const fn passes(self) -> bool {
        self.absolute_peak
            && self.relative_peak
            && self.phase
            && self.mean_added_delay
            && self.f64_magnitude
            && self.f32_magnitude
            && self.f64_bounds
            && self.f32_bounds
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SweepPoint {
    pub(super) frequency_ratio: f64,
    pub(super) source_delay: f64,
    pub(super) equalizer_delay: f64,
    pub(super) total_delay: f64,
    pub(super) source_phase_radians: f64,
    pub(super) equalizer_phase_radians: f64,
    pub(super) total_phase_radians: f64,
    pub(super) phase_residual_radians: f64,
}

#[derive(Debug)]
pub(super) struct CaseResult {
    pub(super) case: Case,
    pub(super) candidate: Candidate,
    pub(super) source_metrics: DelayMetrics,
    pub(super) corrected_metrics: DelayMetrics,
    pub(super) f32_corrected_metrics: DelayMetrics,
    pub(super) mean_added_delay: f64,
    pub(super) maximum_added_delay: f64,
    pub(super) f32_mean_added_delay: f64,
    pub(super) f32_maximum_added_delay: f64,
    pub(super) maximum_f64_magnitude_change: f64,
    pub(super) maximum_f32_magnitude_change: f64,
    pub(super) gates: GateResults,
    pub(super) fit_evaluations: usize,
    pub(super) holdout_points: Vec<SweepPoint>,
    pub(super) crosscheck_evaluated_point_count: usize,
    pub(super) crosscheck_metrics: DelayMetrics,
}

#[derive(Clone, Copy)]
struct SourcePoint {
    frequency_ratio: f64,
    delay: f64,
    phase_radians: f64,
}

#[derive(Clone, Copy, Debug)]
struct CandidateScore {
    q_coordinate: f64,
    sigma_coordinate: f64,
    candidate: Candidate,
    peak_residual: f64,
    mean_added_delay: f64,
    maximum_phase_residual: f64,
}

impl CandidateScore {
    fn satisfies_fit_constraints(self) -> bool {
        self.mean_added_delay <= MAX_MEAN_ADDED_DELAY
            && self.maximum_phase_residual <= MAX_PHASE_RESIDUAL_RADIANS
    }
}

pub(super) fn design(case: Case) -> Result<CaseResult, Box<dyn Error + Send + Sync>> {
    let upper = case.band.upper_frequency_ratio();
    let fit_frequencies = midpoint_frequencies(upper, FIT_POINT_COUNT);
    let fit_source = source_points(case, &fit_frequencies)?;
    let (candidate, fit_evaluations) = search_candidate(&fit_source, upper);

    let holdout_grid = holdout_frequencies(candidate, upper, HOLDOUT_INTERVAL_COUNT);
    let holdout_source = source_points(case, &holdout_grid)?;
    let (source_metrics, _) = evaluate(&holdout_source, None, upper);
    let (corrected_metrics, mut holdout_points) = evaluate(&holdout_source, Some(candidate), upper);
    for point in &mut holdout_points {
        point.phase_residual_radians =
            point.total_phase_radians + corrected_metrics.target_delay * point.frequency_ratio;
    }

    let rounded = candidate.rounded_to_f32();
    let (f32_corrected_metrics, _) = evaluate(&holdout_source, Some(rounded), upper);
    let maximum_f64_magnitude_change = holdout_grid
        .iter()
        .map(|&frequency| candidate.magnitude_change(frequency))
        .fold(0.0, f64::max);
    let maximum_f32_magnitude_change = holdout_grid
        .iter()
        .map(|&frequency| rounded.magnitude_change_f32(frequency))
        .fold(0.0, f64::max);
    let mean_added_delay = candidate.mean_delay(upper);
    let maximum_added_delay = candidate.maximum_delay(upper);
    let f32_mean_added_delay = rounded.mean_delay(upper);
    let f32_maximum_added_delay = rounded.maximum_delay(upper);
    let gates = GateResults {
        absolute_peak: corrected_metrics.peak_residual <= MAX_PEAK_RESIDUAL
            && f32_corrected_metrics.peak_residual <= MAX_PEAK_RESIDUAL,
        relative_peak: corrected_metrics.peak_residual
            <= MAX_PEAK_RESIDUAL_FRACTION * source_metrics.peak_residual
            && f32_corrected_metrics.peak_residual
                <= MAX_PEAK_RESIDUAL_FRACTION * source_metrics.peak_residual,
        phase: corrected_metrics.maximum_phase_residual_radians <= MAX_PHASE_RESIDUAL_RADIANS
            && f32_corrected_metrics.maximum_phase_residual_radians <= MAX_PHASE_RESIDUAL_RADIANS,
        mean_added_delay: mean_added_delay <= MAX_MEAN_ADDED_DELAY
            && f32_mean_added_delay <= MAX_MEAN_ADDED_DELAY,
        f64_magnitude: maximum_f64_magnitude_change <= MAX_F64_MAGNITUDE_CHANGE,
        f32_magnitude: maximum_f32_magnitude_change <= MAX_F32_MAGNITUDE_CHANGE,
        f64_bounds: candidate.satisfies_bounds(),
        f32_bounds: rounded.satisfies_bounds(),
    };

    let crosscheck_frequencies = holdout_frequencies(candidate, upper, CROSSCHECK_INTERVAL_COUNT);
    let crosscheck_source = source_points(case, &crosscheck_frequencies)?;
    let (crosscheck_metrics, _) = evaluate(&crosscheck_source, Some(candidate), upper);

    Ok(CaseResult {
        case,
        candidate,
        source_metrics,
        corrected_metrics,
        f32_corrected_metrics,
        mean_added_delay,
        maximum_added_delay,
        f32_mean_added_delay,
        f32_maximum_added_delay,
        maximum_f64_magnitude_change,
        maximum_f32_magnitude_change,
        gates,
        fit_evaluations,
        holdout_points,
        crosscheck_evaluated_point_count: crosscheck_frequencies.len(),
        crosscheck_metrics,
    })
}

fn search_candidate(source: &[SourcePoint], upper: f64) -> (Candidate, usize) {
    let mut evaluations = 0;
    let denominator = (COARSE_POINTS_PER_AXIS - 1)
        .to_f64()
        .expect("small grid size fits in f64");
    let mut scores = Vec::with_capacity(COARSE_POINTS_PER_AXIS * COARSE_POINTS_PER_AXIS);
    for q_index in 0..COARSE_POINTS_PER_AXIS {
        for sigma_index in 0..COARSE_POINTS_PER_AXIS {
            let q_coordinate =
                q_index.to_f64().expect("small grid index fits in f64") / denominator;
            let sigma_coordinate =
                sigma_index.to_f64().expect("small grid index fits in f64") / denominator;
            scores.push(score_candidate(
                source,
                upper,
                q_coordinate,
                sigma_coordinate,
            ));
            evaluations += 1;
        }
    }
    scores.sort_by(compare_scores);

    let coarse_best = scores[0];
    let coarse_step = 1.0 / denominator;
    let local_denominator = (LOCAL_GRID_POINTS_PER_AXIS - 1)
        .to_f64()
        .expect("small grid size fits in f64");
    for q_index in 0..LOCAL_GRID_POINTS_PER_AXIS {
        for sigma_index in 0..LOCAL_GRID_POINTS_PER_AXIS {
            let q_offset = (2.0 * q_index.to_f64().expect("small grid index fits in f64")
                / local_denominator
                - 1.0)
                * coarse_step;
            let sigma_offset = (2.0 * sigma_index.to_f64().expect("small grid index fits in f64")
                / local_denominator
                - 1.0)
                * coarse_step;
            scores.push(score_candidate(
                source,
                upper,
                (coarse_best.q_coordinate + q_offset).clamp(0.0, 1.0),
                (coarse_best.sigma_coordinate + sigma_offset).clamp(0.0, 1.0),
            ));
            evaluations += 1;
        }
    }
    scores.sort_by(compare_scores);

    let mut finalists = scores
        .iter()
        .take(REFINEMENT_SEED_COUNT)
        .copied()
        .collect::<Vec<_>>();
    let initial_step = 0.5 / denominator;
    for finalist in &mut finalists {
        let mut step = initial_step;
        for _ in 0..REFINEMENT_LEVELS {
            for _ in 0..MAX_REFINEMENT_MOVES_PER_LEVEL {
                let center = *finalist;
                let mut moved = false;
                for q_direction in [-1.0, 0.0, 1.0] {
                    for sigma_direction in [-1.0, 0.0, 1.0] {
                        if q_direction == 0.0 && sigma_direction == 0.0 {
                            continue;
                        }
                        let score = score_candidate(
                            source,
                            upper,
                            (center.q_coordinate + q_direction * step).clamp(0.0, 1.0),
                            (center.sigma_coordinate + sigma_direction * step).clamp(0.0, 1.0),
                        );
                        evaluations += 1;
                        if compare_scores(&score, finalist).is_lt() {
                            *finalist = score;
                            moved = true;
                        }
                    }
                }
                if !moved {
                    break;
                }
            }
            step *= 0.5;
        }
    }
    finalists.sort_by(compare_scores);
    (finalists[0].candidate, evaluations)
}

fn score_candidate(
    source: &[SourcePoint],
    upper: f64,
    q_coordinate: f64,
    sigma_coordinate: f64,
) -> CandidateScore {
    let candidate = Candidate::from_unit_coordinates(q_coordinate, sigma_coordinate);
    let (minimum, maximum) = source
        .iter()
        .map(|point| point.delay + candidate.delay(point.frequency_ratio))
        .fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(minimum, maximum), delay| (minimum.min(delay), maximum.max(delay)),
        );
    let target = minimum.midpoint(maximum);
    let maximum_phase_residual = source
        .iter()
        .map(|point| {
            (point.phase_radians
                + candidate.phase(point.frequency_ratio)
                + target * point.frequency_ratio)
                .abs()
        })
        .fold(0.0, f64::max);
    CandidateScore {
        q_coordinate,
        sigma_coordinate,
        candidate,
        peak_residual: 0.5 * (maximum - minimum),
        mean_added_delay: candidate.mean_delay(upper),
        maximum_phase_residual,
    }
}

fn compare_scores(left: &CandidateScore, right: &CandidateScore) -> std::cmp::Ordering {
    right
        .satisfies_fit_constraints()
        .cmp(&left.satisfies_fit_constraints())
        .then_with(|| left.peak_residual.total_cmp(&right.peak_residual))
        .then_with(|| left.mean_added_delay.total_cmp(&right.mean_added_delay))
        .then_with(|| left.candidate.sigma.total_cmp(&right.candidate.sigma))
        .then_with(|| left.candidate.eta.total_cmp(&right.candidate.eta))
}

fn source_points(
    case: Case,
    frequencies: &[f64],
) -> Result<Vec<SourcePoint>, Box<dyn Error + Send + Sync>> {
    frequencies
        .iter()
        .map(|&frequency_ratio| {
            let point = transfer::low_pass(case.response, case.order, frequency_ratio)?;
            Ok(SourcePoint {
                frequency_ratio,
                delay: point.normalized_group_delay,
                phase_radians: point.phase_radians,
            })
        })
        .collect()
}

fn evaluate(
    source: &[SourcePoint],
    candidate: Option<Candidate>,
    upper: f64,
) -> (DelayMetrics, Vec<SweepPoint>) {
    let mut points = source
        .iter()
        .map(|source| {
            let equalizer_delay =
                candidate.map_or(0.0, |candidate| candidate.delay(source.frequency_ratio));
            let equalizer_phase_radians =
                candidate.map_or(0.0, |candidate| candidate.phase(source.frequency_ratio));
            SweepPoint {
                frequency_ratio: source.frequency_ratio,
                source_delay: source.delay,
                equalizer_delay,
                total_delay: source.delay + equalizer_delay,
                source_phase_radians: source.phase_radians,
                equalizer_phase_radians,
                total_phase_radians: source.phase_radians + equalizer_phase_radians,
                phase_residual_radians: 0.0,
            }
        })
        .collect::<Vec<_>>();
    let (minimum, maximum) = points.iter().map(|point| point.total_delay).fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(minimum, maximum), delay| (minimum.min(delay), maximum.max(delay)),
    );
    let target_delay = minimum.midpoint(maximum);
    let mean_delay = trapezoidal_integral(&points, |point| point.total_delay) / upper;
    let rms_residual = (trapezoidal_integral(&points, |point| {
        let residual = point.total_delay - mean_delay;
        residual * residual
    }) / upper)
        .sqrt();
    let maximum_phase_residual_radians = points
        .iter_mut()
        .map(|point| {
            point.phase_residual_radians =
                point.total_phase_radians + target_delay * point.frequency_ratio;
            point.phase_residual_radians.abs()
        })
        .fold(0.0, f64::max);
    (
        DelayMetrics {
            target_delay,
            peak_residual: 0.5 * (maximum - minimum),
            rms_residual,
            maximum_phase_residual_radians,
        },
        points,
    )
}

fn trapezoidal_integral(points: &[SweepPoint], value: impl Fn(&SweepPoint) -> f64) -> f64 {
    points
        .windows(2)
        .map(|window| {
            let width = window[1].frequency_ratio - window[0].frequency_ratio;
            0.5 * width * (value(&window[0]) + value(&window[1]))
        })
        .sum()
}

fn midpoint_frequencies(upper: f64, count: usize) -> Vec<f64> {
    let count_float = count.to_f64().expect("small grid size fits in f64");
    (0..count)
        .map(|index| {
            upper * (index.to_f64().expect("small grid index fits in f64") + 0.5) / count_float
        })
        .collect()
}

fn holdout_frequencies(candidate: Candidate, upper: f64, intervals: usize) -> Vec<f64> {
    let interval_count = intervals.to_f64().expect("small grid size fits in f64");
    let mut frequencies = (0..=intervals)
        .map(|index| upper * index.to_f64().expect("small grid index fits in f64") / interval_count)
        .collect::<Vec<_>>();
    let natural_frequency = candidate.natural_frequency();
    for frequency in [
        candidate.eta,
        natural_frequency,
        candidate.eta - 2.0 * candidate.sigma,
        candidate.eta - candidate.sigma,
        candidate.eta + candidate.sigma,
        candidate.eta + 2.0 * candidate.sigma,
        1.0,
    ] {
        if frequency > 0.0 && frequency < upper {
            frequencies.push(frequency);
        }
    }
    frequencies.sort_by(f64::total_cmp);
    frequencies.dedup_by(|left, right| left.total_cmp(right).is_eq());
    frequencies
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::{
        Band, Candidate, Case, FIT_POINT_COUNT, HOLDOUT_INTERVAL_COUNT, design,
        holdout_frequencies, midpoint_frequencies,
    };
    use ssfilt::Response;

    #[test]
    fn all_pass_delay_matches_its_unwrapped_phase_derivative() {
        let candidate = Candidate {
            sigma: 0.63,
            eta: 0.38,
        };
        let step = 1.0e-6;
        for frequency in [0.0_f64, 0.2, 0.7, 1.0] {
            let before = candidate.phase((frequency - step).max(0.0));
            let after = candidate.phase(frequency + step);
            let width = if frequency == 0.0 { step } else { 2.0 * step };
            assert_relative_eq!(
                candidate.delay(frequency),
                -(after - before) / width,
                epsilon = 2.0e-9
            );
        }
    }

    #[test]
    fn fit_and_holdout_frequency_grids_are_disjoint() {
        let candidate = Candidate {
            sigma: 0.63,
            eta: 0.38,
        };
        let fit = midpoint_frequencies(0.7, FIT_POINT_COUNT);
        let holdout = holdout_frequencies(candidate, 0.7, HOLDOUT_INTERVAL_COUNT);
        assert!(fit.iter().all(|fit_frequency| {
            holdout
                .binary_search_by(|holdout_frequency| holdout_frequency.total_cmp(fit_frequency))
                .is_err()
        }));
    }

    #[test]
    fn closed_form_added_delay_statistics_match_dense_numerics() {
        let candidate = Candidate {
            sigma: 0.63,
            eta: 0.38,
        };
        let upper = 0.7;
        let intervals = 100_000;
        let step = upper / f64::from(intervals);
        let mut integral = 0.0;
        let mut numerical_maximum: f64 = 0.0;
        let mut previous = candidate.delay(0.0);
        numerical_maximum = numerical_maximum.max(previous);
        for index in 1..=intervals {
            let frequency = step * f64::from(index);
            let current = candidate.delay(frequency);
            integral += 0.5 * step * (previous + current);
            numerical_maximum = numerical_maximum.max(current);
            previous = current;
        }
        assert_relative_eq!(
            candidate.mean_delay(upper),
            integral / upper,
            epsilon = 5.0e-11
        );
        assert_relative_eq!(
            candidate.maximum_delay(upper),
            numerical_maximum,
            epsilon = 2.0e-9
        );
    }

    #[test]
    fn mandatory_butterworth_core_cases_pass_the_analytic_gates() {
        for order in [4, 8] {
            let result = design(Case {
                response: Response::Butterworth,
                response_label: "Butterworth",
                order,
                band: Band::Core,
            })
            .unwrap();
            assert!(result.gates.passes(), "order {order}: {result:#?}");
            let expected = match order {
                4 => (0.774_282_178_267_629_5, 0.420_158_711_359_650_1),
                8 => (0.631_643_162_371_801_7, 0.381_855_079_433_413_2),
                _ => unreachable!(),
            };
            assert_relative_eq!(result.candidate.sigma, expected.0, epsilon = 1.0e-15);
            assert_relative_eq!(result.candidate.eta, expected.1, epsilon = 1.0e-15);
            assert_eq!(result.case.streaming_status(), "streaming-validated");
            assert_eq!(result.case.public_support_status(), "public-support-yes");
            assert!(
                (result.corrected_metrics.peak_residual - result.crosscheck_metrics.peak_residual)
                    .abs()
                    < 1.0e-5
            );
            assert!(
                (result.corrected_metrics.maximum_phase_residual_radians
                    - result.crosscheck_metrics.maximum_phase_residual_radians)
                    .abs()
                    < 1.0e-5
            );
        }
    }

    #[test]
    fn cutoff_wide_butterworth_cases_are_not_overclaimed() {
        for order in [4, 8] {
            let result = design(Case {
                response: Response::Butterworth,
                response_label: "Butterworth",
                order,
                band: Band::Cutoff,
            })
            .unwrap();
            assert!(!result.gates.passes());
            assert!(!result.gates.absolute_peak);
        }
    }
}
