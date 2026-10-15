//! Public-API streaming measurements for the response example.
//!
//! These measurements deliberately drive a sampled, piecewise-linear sine;
//! they are not an alternate implementation of the continuous transfer
//! function.  The fit uses the timestamps actually supplied to `update`.

use std::error::Error;

use ssfilt::{InputModel, IntegrationDiagnostics, LowPass, PhaseEqualizedLowPass, Response};

use super::analytic::minimum_settling_time_seconds;

pub(super) const DEFAULT_SAMPLES_PER_CYCLE: u32 = 256;
pub(super) const DEFAULT_WARMUP_CYCLES: u32 = 60;
pub(super) const DEFAULT_FIT_CYCLES: u32 = 8;

const JITTER_MULTIPLIERS: [f64; 6] = [0.625, 0.875, 1.0, 0.75, 0.9375, 0.6875];
const SETTLING_RELATIVE_GAIN_TOLERANCE: f64 = 1.0e-4;
const SETTLING_PHASE_TOLERANCE_RADIANS: f64 = 1.0e-4;
// Below -80 dB, the default f64 integration error floor and fit residual can
// dominate phase. Gain convergence then uses an absolute 1e-8 scale and phase
// is recorded in CSV but not claimed as a resolved plot marker.
const SETTLING_GAIN_SCALE_FLOOR: f64 = 1.0e-4;
const REQUIRED_SETTLED_COMPARISONS: u32 = 2;
const MAX_SETTLING_WINDOWS: u32 = 8;
const PHASE_EQUALIZER_SETTLING_E_FOLDS: f64 = 24.0;
// The documented validated design bounds every all-pass section by sigma >= 0.2.
// Use that conservative lower bound instead of copying the selected coefficients
// from the independent transfer-function oracle into this measurement driver.
const MIN_VALIDATED_PHASE_EQUALIZER_SIGMA: f64 = 0.2;

/// Deterministic schedules for piecewise-linear sampled-sine measurements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SampleSchedule {
    Uniform,
    #[allow(dead_code)] // Kept available for the development jitter matrix.
    BoundedJitter,
}

impl SampleSchedule {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::BoundedJitter => "bounded-jitter",
        }
    }

    pub(super) const fn pattern_label(self) -> &'static str {
        match self {
            Self::Uniform => "1.0",
            Self::BoundedJitter => "0.625|0.875|1.0|0.75|0.9375|0.6875",
        }
    }

    pub(super) const fn mean_interval_multiplier(self) -> f64 {
        match self {
            Self::Uniform => 1.0,
            Self::BoundedJitter => 0.8125,
        }
    }

    fn interval(self, nominal_interval: f64, index: usize) -> f64 {
        match self {
            Self::Uniform => nominal_interval,
            Self::BoundedJitter => {
                nominal_interval * JITTER_MULTIPLIERS[index % JITTER_MULTIPLIERS.len()]
            }
        }
    }
}

/// Aggregate integration work over all updates in one tone measurement.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct AggregateDiagnostics {
    pub(super) accepted_steps: usize,
    pub(super) rejected_steps: usize,
    pub(super) derivative_evaluations: usize,
    pub(super) smallest_accepted_step_seconds: Option<f64>,
    pub(super) largest_accepted_step_seconds: Option<f64>,
}

impl AggregateDiagnostics {
    fn record(&mut self, diagnostics: IntegrationDiagnostics<f64>) {
        self.accepted_steps += diagnostics.accepted_steps();
        self.rejected_steps += diagnostics.rejected_steps();
        self.derivative_evaluations += diagnostics.derivative_evaluations();
        self.smallest_accepted_step_seconds = minimum_option(
            self.smallest_accepted_step_seconds,
            diagnostics.smallest_accepted_step_seconds(),
        );
        self.largest_accepted_step_seconds = maximum_option(
            self.largest_accepted_step_seconds,
            diagnostics.largest_accepted_step_seconds(),
        );
    }
}

/// Fundamental fit and execution details for one public streaming measurement.
#[derive(Clone, Copy, Debug)]
pub(super) struct SineMeasurement {
    pub(super) response: Response,
    pub(super) order: usize,
    pub(super) frequency_ratio: f64,
    pub(super) schedule: SampleSchedule,
    pub(super) samples_per_cycle: u32,
    pub(super) maximum_interval_seconds: f64,
    pub(super) warmup_cycles: f64,
    pub(super) warmup_seconds: f64,
    pub(super) fit_cycles: u32,
    pub(super) warmup_update_count: usize,
    pub(super) fit_update_count: usize,
    pub(super) fit_sample_count: usize,
    pub(super) settling_window_count: u32,
    pub(super) settling_relative_gain_delta: f64,
    pub(super) settling_phase_delta_radians: f64,
    pub(super) fit_offset: f64,
    pub(super) gain: f64,
    pub(super) wrapped_phase_radians: f64,
    pub(super) fit_rms: f64,
    pub(super) diagnostics: AggregateDiagnostics,
}

/// Source and phase-equalized records made from one shared timestamp stream.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // Consumed by the planned measured equalization overlay.
pub(super) struct PairedSineMeasurement {
    pub(super) source: SineMeasurement,
    pub(super) equalized: SineMeasurement,
}

impl SineMeasurement {
    pub(super) fn phase_is_resolved(self) -> bool {
        self.gain >= SETTLING_GAIN_SCALE_FLOOR
    }
}

/// One measured point paired with the independent continuous-time reference.
#[derive(Clone, Copy, Debug)]
pub(super) struct MeasuredFrequencyPoint {
    pub(super) streamed: SineMeasurement,
    pub(super) analytic_gain_db: f64,
    pub(super) analytic_phase_radians: f64,
    pub(super) unwrapped_phase_radians: f64,
}

impl MeasuredFrequencyPoint {
    pub(super) fn new(
        streamed: SineMeasurement,
        analytic_gain_db: f64,
        analytic_phase_radians: f64,
    ) -> Self {
        Self {
            unwrapped_phase_radians: unwrap_phase(
                streamed.wrapped_phase_radians,
                analytic_phase_radians,
            ),
            streamed,
            analytic_gain_db,
            analytic_phase_radians,
        }
    }

    pub(super) fn measured_gain_db(self) -> f64 {
        20.0 * self.streamed.gain.log10()
    }
}

#[derive(Clone, Copy, Debug)]
struct SineFit {
    offset: f64,
    sine: f64,
    cosine: f64,
    rms: f64,
}

#[derive(Debug)]
struct FitSamples {
    source: Vec<(f64, f64)>,
    equalized: Option<Vec<(f64, f64)>>,
}

impl FitSamples {
    fn new<const N: usize>(driver: &MeasurementDriver<N>, time_seconds: f64) -> Self {
        Self {
            source: vec![(time_seconds, driver.source_output())],
            equalized: driver
                .equalized_output()
                .map(|output| vec![(time_seconds, output)]),
        }
    }
}

struct MeasurementDriver<const N: usize> {
    source: LowPass<N, f64>,
    equalized: Option<PhaseEqualizedLowPass<N, f64>>,
    source_diagnostics: AggregateDiagnostics,
    equalized_diagnostics: Option<AggregateDiagnostics>,
}

impl<const N: usize> MeasurementDriver<N> {
    fn new(
        response: Response,
        include_equalized: bool,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let source = LowPass::<N>::builder(1.0)
            .response(response)
            .input_model(InputModel::Linear)
            .initial_input(0.0)
            .build()?;
        let equalized = if include_equalized {
            Some(
                LowPass::<N>::builder(1.0)
                    .response(response)
                    .input_model(InputModel::Linear)
                    .initial_input(0.0)
                    .build_phase_equalized()?,
            )
        } else {
            None
        };
        let equalized_diagnostics = equalized.as_ref().map(|_| AggregateDiagnostics::default());

        Ok(Self {
            source,
            equalized,
            source_diagnostics: AggregateDiagnostics::default(),
            equalized_diagnostics,
        })
    }

    fn source_output(&self) -> f64 {
        self.source.output()
    }

    fn equalized_output(&self) -> Option<f64> {
        self.equalized.as_ref().map(PhaseEqualizedLowPass::output)
    }

    fn update(
        &mut self,
        input: f64,
        dt_seconds: f64,
    ) -> Result<(f64, Option<f64>), Box<dyn Error + Send + Sync>> {
        let source_output = self.source.update(input, dt_seconds)?;
        self.source_diagnostics
            .record(self.source.last_diagnostics());

        let equalized_output = if let Some(filter) = self.equalized.as_mut() {
            let output = filter.update(input, dt_seconds)?;
            self.equalized_diagnostics
                .as_mut()
                .expect("equalized diagnostics accompany the equalized filter")
                .record(filter.last_diagnostics());
            Some(output)
        } else {
            None
        };
        Ok((source_output, equalized_output))
    }
}

#[derive(Clone, Copy, Debug)]
struct SettlingState {
    previous_fit: Option<SineFit>,
    final_fit: Option<SineFit>,
    settled_comparisons: u32,
    relative_gain_delta: f64,
    phase_delta_radians: f64,
}

impl SettlingState {
    fn new() -> Self {
        Self {
            previous_fit: None,
            final_fit: None,
            settled_comparisons: 0,
            relative_gain_delta: f64::INFINITY,
            phase_delta_radians: f64::INFINITY,
        }
    }

    fn observe(&mut self, fit: SineFit) {
        if let Some(previous) = self.previous_fit {
            self.relative_gain_delta = relative_gain_delta(previous, fit);
            self.phase_delta_radians =
                circular_phase_delta(previous.wrapped_phase(), fit.wrapped_phase());
            let phase_is_resolved = previous.gain().max(fit.gain()) >= SETTLING_GAIN_SCALE_FLOOR;
            let settled = self.relative_gain_delta <= SETTLING_RELATIVE_GAIN_TOLERANCE
                && (!phase_is_resolved
                    || self.phase_delta_radians <= SETTLING_PHASE_TOLERANCE_RADIANS);
            self.settled_comparisons = if settled {
                self.settled_comparisons + 1
            } else {
                0
            };
        }
        self.previous_fit = Some(fit);
        self.final_fit = Some(fit);
    }

    fn is_settled(self) -> bool {
        self.settled_comparisons >= REQUIRED_SETTLED_COMPARISONS
    }
}

struct MeasurementPaths {
    source: SineMeasurement,
    #[allow(dead_code)] // Read by the paired measurement entry point.
    equalized: Option<SineMeasurement>,
}

#[derive(Clone, Copy, Debug)]
struct SettledFits {
    source: SettlingState,
    equalized: Option<SettlingState>,
    fit_update_count: usize,
    fit_sample_count: usize,
    settling_window_count: u32,
}

impl SineFit {
    fn gain(self) -> f64 {
        self.sine.hypot(self.cosine)
    }

    fn wrapped_phase(self) -> f64 {
        self.cosine.atan2(self.sine)
    }
}

/// Measures a newly constructed low-pass filter with the default settling and
/// fitting windows.
///
/// The public cutoff is one hertz, so `frequency_ratio` is also the physical
/// input frequency in hertz. This keeps the stored maximum interval directly
/// useful to CSV consumers while preserving normalized response behavior.
pub(super) fn measure_low_pass<const N: usize>(
    response: Response,
    frequency_ratio: f64,
    schedule: SampleSchedule,
    samples_per_cycle: u32,
) -> Result<SineMeasurement, Box<dyn Error + Send + Sync>> {
    Ok(measure_low_pass_paths::<N>(
        response,
        frequency_ratio,
        schedule,
        samples_per_cycle,
        false,
    )?
    .source)
}

/// Measures Butterworth orders 4 and 8 through the ordinary and delayed-phase
/// public APIs using one shared input and timestamp stream.
#[allow(dead_code)] // The response plot will consume this with the equalized overlay.
pub(super) fn measure_butterworth_low_pass_pair<const N: usize>(
    frequency_ratio: f64,
    schedule: SampleSchedule,
    samples_per_cycle: u32,
) -> Result<PairedSineMeasurement, Box<dyn Error + Send + Sync>> {
    let paths = measure_low_pass_paths::<N>(
        Response::Butterworth,
        frequency_ratio,
        schedule,
        samples_per_cycle,
        true,
    )?;
    Ok(PairedSineMeasurement {
        source: paths.source,
        equalized: paths
            .equalized
            .expect("paired measurements construct an equalized filter"),
    })
}

fn measure_low_pass_paths<const N: usize>(
    response: Response,
    frequency_ratio: f64,
    schedule: SampleSchedule,
    samples_per_cycle: u32,
    include_equalized: bool,
) -> Result<MeasurementPaths, Box<dyn Error + Send + Sync>> {
    if !frequency_ratio.is_finite() || frequency_ratio <= 0.0 {
        return Err("frequency ratio must be finite and positive".into());
    }
    if samples_per_cycle < 3 {
        return Err("samples per cycle must be at least three".into());
    }

    let maximum_interval_seconds = 1.0 / (frequency_ratio * f64::from(samples_per_cycle));
    let mut driver = MeasurementDriver::<N>::new(response, include_equalized)?;
    let mut time_seconds = 0.0;
    let mut schedule_index = 0;

    let warmup_end_seconds =
        minimum_warmup_seconds::<N>(response, frequency_ratio, include_equalized)?;
    let warmup_update_count = advance_to(
        &mut driver,
        &mut time_seconds,
        warmup_end_seconds,
        frequency_ratio,
        schedule,
        maximum_interval_seconds,
        &mut schedule_index,
        None,
    )?;

    let fits = fit_until_settled::<N>(
        &mut driver,
        response,
        frequency_ratio,
        schedule,
        maximum_interval_seconds,
        &mut time_seconds,
        &mut schedule_index,
    )?;

    let source = make_measurement(
        response,
        frequency_ratio,
        schedule,
        samples_per_cycle,
        maximum_interval_seconds,
        warmup_end_seconds,
        warmup_update_count,
        fits.fit_update_count,
        fits.fit_sample_count,
        fits.settling_window_count,
        fits.source,
        N,
        driver.source_diagnostics,
    );
    let equalized = fits.equalized.map(|settling| {
        make_measurement(
            Response::Butterworth,
            frequency_ratio,
            schedule,
            samples_per_cycle,
            maximum_interval_seconds,
            warmup_end_seconds,
            warmup_update_count,
            fits.fit_update_count,
            fits.fit_sample_count,
            fits.settling_window_count,
            settling,
            N,
            driver
                .equalized_diagnostics
                .expect("equalized diagnostics accompany the equalized path"),
        )
    });
    Ok(MeasurementPaths { source, equalized })
}

fn minimum_warmup_seconds<const N: usize>(
    response: Response,
    frequency_ratio: f64,
    include_equalized: bool,
) -> Result<f64, Box<dyn Error + Send + Sync>> {
    let source_minimum = (f64::from(DEFAULT_WARMUP_CYCLES) / frequency_ratio)
        .max(minimum_settling_time_seconds::<N>(response)?);
    if !include_equalized {
        return Ok(source_minimum);
    }
    let equalizer_minimum = PHASE_EQUALIZER_SETTLING_E_FOLDS
        / (core::f64::consts::TAU * MIN_VALIDATED_PHASE_EQUALIZER_SIGMA);
    Ok(source_minimum.max(equalizer_minimum))
}

fn fit_until_settled<const N: usize>(
    driver: &mut MeasurementDriver<N>,
    response: Response,
    frequency_ratio: f64,
    schedule: SampleSchedule,
    maximum_interval_seconds: f64,
    time_seconds: &mut f64,
    schedule_index: &mut usize,
) -> Result<SettledFits, Box<dyn Error + Send + Sync>> {
    let angular_frequency = core::f64::consts::TAU * frequency_ratio;
    let mut source_settling = SettlingState::new();
    let mut equalized_settling = driver.equalized.as_ref().map(|_| SettlingState::new());
    let mut fit_update_count = 0;
    let mut fit_sample_count = 0;
    let mut settling_window_count = 0;
    for window in 1..=MAX_SETTLING_WINDOWS {
        let fit_end_seconds = *time_seconds + f64::from(DEFAULT_FIT_CYCLES) / frequency_ratio;
        let mut fit_samples = FitSamples::new(driver, *time_seconds);
        fit_update_count += advance_to(
            driver,
            time_seconds,
            fit_end_seconds,
            frequency_ratio,
            schedule,
            maximum_interval_seconds,
            schedule_index,
            Some(&mut fit_samples),
        )?;
        source_settling.observe(fit_sine(&fit_samples.source, angular_frequency)?);
        if let (Some(samples), Some(settling)) =
            (fit_samples.equalized.as_ref(), equalized_settling.as_mut())
        {
            settling.observe(fit_sine(samples, angular_frequency)?);
        }
        fit_sample_count = fit_samples.source.len();
        settling_window_count = window;
        let all_paths_settled = source_settling.is_settled()
            && equalized_settling.is_none_or(SettlingState::is_settled);
        if all_paths_settled {
            break;
        }
    }

    if !source_settling.is_settled() || !equalized_settling.is_none_or(SettlingState::is_settled) {
        if let Some(equalized) = equalized_settling {
            return Err(format!(
                "paired measurement did not settle for Butterworth order {N} at {frequency_ratio} × cutoff with {} timestamps: source relative gain delta {:e}, phase delta {:e} rad; equalized relative gain delta {:e}, phase delta {:e} rad",
                schedule.label(),
                source_settling.relative_gain_delta,
                source_settling.phase_delta_radians,
                equalized.relative_gain_delta,
                equalized.phase_delta_radians,
            )
            .into());
        }
        return Err(format!(
            "measurement did not settle for {response:?} order {N} at {frequency_ratio} × cutoff with {} timestamps: relative gain delta {:e}, phase delta {:e} rad",
            schedule.label(),
            source_settling.relative_gain_delta,
            source_settling.phase_delta_radians,
        )
        .into());
    }

    Ok(SettledFits {
        source: source_settling,
        equalized: equalized_settling,
        fit_update_count,
        fit_sample_count,
        settling_window_count,
    })
}

#[allow(clippy::too_many_arguments)]
fn make_measurement(
    response: Response,
    frequency_ratio: f64,
    schedule: SampleSchedule,
    samples_per_cycle: u32,
    maximum_interval_seconds: f64,
    warmup_seconds: f64,
    warmup_update_count: usize,
    fit_update_count: usize,
    fit_sample_count: usize,
    settling_window_count: u32,
    settling: SettlingState,
    order: usize,
    diagnostics: AggregateDiagnostics,
) -> SineMeasurement {
    let fit = settling
        .final_fit
        .expect("at least one settling window is configured");
    SineMeasurement {
        response,
        order,
        frequency_ratio,
        schedule,
        samples_per_cycle,
        maximum_interval_seconds,
        warmup_cycles: warmup_seconds * frequency_ratio,
        warmup_seconds,
        fit_cycles: DEFAULT_FIT_CYCLES,
        warmup_update_count,
        fit_update_count,
        fit_sample_count,
        settling_window_count,
        settling_relative_gain_delta: settling.relative_gain_delta,
        settling_phase_delta_radians: settling.phase_delta_radians,
        fit_offset: fit.offset,
        gain: fit.gain(),
        wrapped_phase_radians: fit.wrapped_phase(),
        fit_rms: fit.rms,
        diagnostics,
    }
}

fn relative_gain_delta(previous: SineFit, current: SineFit) -> f64 {
    (previous.gain() - current.gain()).abs()
        / previous
            .gain()
            .max(current.gain())
            .max(SETTLING_GAIN_SCALE_FLOOR)
}

fn circular_phase_delta(previous: f64, current: f64) -> f64 {
    ((current - previous + core::f64::consts::PI).rem_euclid(core::f64::consts::TAU)
        - core::f64::consts::PI)
        .abs()
}

#[allow(clippy::too_many_arguments)]
fn advance_to<const N: usize>(
    driver: &mut MeasurementDriver<N>,
    time_seconds: &mut f64,
    end_seconds: f64,
    frequency_hz: f64,
    schedule: SampleSchedule,
    maximum_interval_seconds: f64,
    schedule_index: &mut usize,
    mut samples: Option<&mut FitSamples>,
) -> Result<usize, Box<dyn Error + Send + Sync>> {
    let mut updates = 0;
    while *time_seconds < end_seconds {
        let remaining = end_seconds - *time_seconds;
        let requested = schedule.interval(maximum_interval_seconds, *schedule_index);
        let reaches_end = requested >= remaining;
        let dt_seconds = requested.min(remaining);
        if !dt_seconds.is_finite() || dt_seconds <= 0.0 {
            return Err("timestamp schedule did not advance".into());
        }
        *schedule_index += 1;
        if reaches_end {
            *time_seconds = end_seconds;
        } else {
            *time_seconds += dt_seconds;
        }
        let input = (core::f64::consts::TAU * frequency_hz * *time_seconds).sin();
        let (source_output, equalized_output) = driver.update(input, dt_seconds)?;
        if let Some(samples) = samples.as_deref_mut() {
            samples.source.push((*time_seconds, source_output));
            if let Some(equalized_samples) = samples.equalized.as_mut() {
                equalized_samples.push((
                    *time_seconds,
                    equalized_output.expect("fit buffers match the active filter paths"),
                ));
            }
        }
        updates += 1;
    }
    Ok(updates)
}

/// Fits `y = c + a sin(omega t) + b cos(omega t)` with trapezoidal timestamp
/// weights, so irregular spacing cannot silently become a sample-count fit.
fn fit_sine(
    samples: &[(f64, f64)],
    angular_frequency: f64,
) -> Result<SineFit, Box<dyn Error + Send + Sync>> {
    if samples.len() < 3 || !angular_frequency.is_finite() || angular_frequency <= 0.0 {
        return Err("sine fitting requires three samples and a positive finite frequency".into());
    }

    let mut normal = [[0.0; 3]; 3];
    let mut rhs = [0.0; 3];
    for (index, &(time, value)) in samples.iter().enumerate() {
        if !time.is_finite() || !value.is_finite() {
            return Err("sine fitting requires finite timestamps and values".into());
        }
        let weight = trapezoidal_weight(samples, index)?;
        let basis = [
            1.0,
            (angular_frequency * time).sin(),
            (angular_frequency * time).cos(),
        ];
        for row in 0..3 {
            rhs[row] = basis[row].mul_add(weight * value, rhs[row]);
            for column in 0..3 {
                normal[row][column] =
                    basis[row].mul_add(weight * basis[column], normal[row][column]);
            }
        }
    }
    let [offset, sine, cosine] = solve_3x3(normal, rhs)?;

    let (weighted_square_error, total_weight) = samples.iter().enumerate().try_fold(
        (0.0, 0.0),
        |(square_error, weight_sum), (index, &(time, value))| {
            let weight = trapezoidal_weight(samples, index)?;
            let fitted = (sine * (angular_frequency * time).sin())
                .mul_add(1.0, cosine * (angular_frequency * time).cos())
                + offset;
            let error = value - fitted;
            Ok::<_, Box<dyn Error + Send + Sync>>((
                error.mul_add(error * weight, square_error),
                weight_sum + weight,
            ))
        },
    )?;
    Ok(SineFit {
        offset,
        sine,
        cosine,
        rms: (weighted_square_error / total_weight).sqrt(),
    })
}

fn trapezoidal_weight(
    samples: &[(f64, f64)],
    index: usize,
) -> Result<f64, Box<dyn Error + Send + Sync>> {
    let before = index
        .checked_sub(1)
        .map(|previous| samples[index].0 - samples[previous].0);
    let after = (index + 1 < samples.len()).then(|| samples[index + 1].0 - samples[index].0);
    let weight = match (before, after) {
        (None, Some(after)) | (Some(after), None) => 0.5 * after,
        (Some(before), Some(after)) => before * 0.5 + after * 0.5,
        (None, None) => return Err("sine fitting requires multiple timestamps".into()),
    };
    if !weight.is_finite() || weight <= 0.0 {
        return Err("sine fitting timestamps must be strictly increasing".into());
    }
    Ok(weight)
}

fn solve_3x3(
    mut matrix: [[f64; 3]; 3],
    mut rhs: [f64; 3],
) -> Result<[f64; 3], Box<dyn Error + Send + Sync>> {
    for column in 0..3 {
        let pivot = (column..3)
            .max_by(|&left, &right| {
                matrix[left][column]
                    .abs()
                    .total_cmp(&matrix[right][column].abs())
            })
            .expect("nonempty fixed pivot range");
        if matrix[pivot][column].abs() <= f64::EPSILON || !matrix[pivot][column].is_finite() {
            return Err("sine fit normal matrix is singular".into());
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let scale = matrix[column][column];
        for entry in &mut matrix[column][column..] {
            *entry /= scale;
        }
        rhs[column] /= scale;
        let pivot_row = matrix[column];
        for row in 0..3 {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            for (entry, pivot_entry) in matrix[row][column..].iter_mut().zip(&pivot_row[column..]) {
                *entry -= factor * pivot_entry;
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    Ok(rhs)
}

/// Returns the representative of a wrapped phase nearest the analytic phase.
pub(super) fn unwrap_phase(measured_wrapped: f64, analytic_phase: f64) -> f64 {
    let difference = (measured_wrapped - analytic_phase + core::f64::consts::PI)
        .rem_euclid(core::f64::consts::TAU)
        - core::f64::consts::PI;
    analytic_phase + difference
}

fn minimum_option(current: Option<f64>, candidate: Option<f64>) -> Option<f64> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.min(candidate)),
        (Some(current), None) => Some(current),
        (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

fn maximum_option(current: Option<f64>, candidate: Option<f64>) -> Option<f64> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.max(candidate)),
        (Some(current), None) => Some(current),
        (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;
    use ssfilt::Response;

    use super::{
        DEFAULT_FIT_CYCLES, DEFAULT_SAMPLES_PER_CYCLE, DEFAULT_WARMUP_CYCLES, MAX_SETTLING_WINDOWS,
        MIN_VALIDATED_PHASE_EQUALIZER_SIGMA, MeasuredFrequencyPoint,
        PHASE_EQUALIZER_SETTLING_E_FOLDS, SampleSchedule, SineMeasurement, fit_sine,
        measure_butterworth_low_pass_pair, measure_low_pass, minimum_warmup_seconds, unwrap_phase,
    };
    use crate::analytic::{exact_frequency_point, minimum_settling_time_seconds};

    fn assert_shared_pair_structure(source: SineMeasurement, equalized: SineMeasurement) {
        assert_eq!(source.frequency_ratio, equalized.frequency_ratio);
        assert_eq!(source.schedule, equalized.schedule);
        assert_eq!(source.samples_per_cycle, equalized.samples_per_cycle);
        assert_eq!(
            source.maximum_interval_seconds,
            equalized.maximum_interval_seconds
        );
        assert_eq!(source.warmup_cycles, equalized.warmup_cycles);
        assert_eq!(source.warmup_seconds, equalized.warmup_seconds);
        assert_eq!(source.fit_cycles, equalized.fit_cycles);
        assert_eq!(source.warmup_update_count, equalized.warmup_update_count);
        assert_eq!(source.fit_update_count, equalized.fit_update_count);
        assert_eq!(source.fit_sample_count, equalized.fit_sample_count);
        assert_eq!(
            source.settling_window_count,
            equalized.settling_window_count
        );
        assert_eq!(source.response, Response::Butterworth);
        assert_eq!(equalized.response, Response::Butterworth);
        assert_eq!(source.order, equalized.order);
        assert!(source.settling_window_count >= 3);
        assert!(source.settling_window_count <= MAX_SETTLING_WINDOWS);
        for measurement in [source, equalized] {
            assert!(measurement.settling_relative_gain_delta <= 1.0e-4);
            assert!(measurement.settling_phase_delta_radians <= 1.0e-4);
            assert!(measurement.fit_rms.is_finite());
            assert!(measurement.diagnostics.derivative_evaluations > 0);
        }
    }

    fn expected_equalized_phase<const N: usize>(frequency_ratio: f64, source_phase: f64) -> f64 {
        // These tool-local coefficients/formula independently mirror the
        // transfer-function oracle for a test expectation only.
        let (sigma, eta) = match N {
            4 => (0.774_282_178_267_629_5, 0.420_158_711_359_650_1),
            8 => (0.631_643_162_371_801_7, 0.381_855_079_433_413_2),
            _ => panic!("the phase equalizer is validated for orders 4 and 8"),
        };
        let all_pass_phase = -2.0
            * (2.0 * sigma * frequency_ratio)
                .atan2(sigma.mul_add(sigma, eta * eta) - frequency_ratio * frequency_ratio);
        source_phase + all_pass_phase
    }

    fn assert_pair_matches_analytic<const N: usize>(
        frequency_ratio: f64,
        schedule: SampleSchedule,
    ) {
        let pair = measure_butterworth_low_pass_pair::<N>(
            frequency_ratio,
            schedule,
            DEFAULT_SAMPLES_PER_CYCLE,
        )
        .unwrap();
        assert_shared_pair_structure(pair.source, pair.equalized);
        assert_eq!(pair.source.frequency_ratio, frequency_ratio);
        assert_eq!(pair.source.schedule, schedule);
        assert_eq!(pair.source.fit_cycles, DEFAULT_FIT_CYCLES);
        assert!(pair.source.warmup_cycles >= f64::from(DEFAULT_WARMUP_CYCLES));

        let analytic = exact_frequency_point::<N>(Response::Butterworth, frequency_ratio).unwrap();
        let expected_gain = 10.0_f64.powf(analytic.gain_db / 20.0);
        for streamed in [pair.source, pair.equalized] {
            let relative_gain_error = (streamed.gain - expected_gain).abs() / expected_gain;
            assert!(
                relative_gain_error < 1.0e-4,
                "order {N} at {frequency_ratio} with {schedule:?}: gain error {relative_gain_error}"
            );
        }
        let source =
            MeasuredFrequencyPoint::new(pair.source, analytic.gain_db, analytic.phase_radians);
        let expected_equalized_phase =
            expected_equalized_phase::<N>(frequency_ratio, analytic.phase_radians);
        let equalized_phase = unwrap_phase(
            pair.equalized.wrapped_phase_radians,
            expected_equalized_phase,
        );
        let source_phase_error = (source.unwrapped_phase_radians - analytic.phase_radians).abs();
        let equalized_phase_error = (equalized_phase - expected_equalized_phase).abs();
        assert!(
            source_phase_error < 1.0e-6,
            "order {N} at {frequency_ratio} with {schedule:?}: source phase error {source_phase_error}"
        );
        assert!(
            equalized_phase_error < 1.0e-6,
            "order {N} at {frequency_ratio} with {schedule:?}: equalized phase error {equalized_phase_error}"
        );
    }

    #[test]
    fn schedules_are_deterministic_and_bounded_by_the_nominal_interval() {
        let nominal = 0.125;
        for schedule in [SampleSchedule::Uniform, SampleSchedule::BoundedJitter] {
            let first = (0..24)
                .map(|index| schedule.interval(nominal, index))
                .collect::<Vec<_>>();
            let second = (0..24)
                .map(|index| schedule.interval(nominal, index))
                .collect::<Vec<_>>();
            assert_eq!(first, second);
            assert!(
                first
                    .iter()
                    .all(|&dt| dt.is_finite() && dt > 0.0 && dt <= nominal)
            );
        }
        assert!(
            (0..24)
                .any(|index| { SampleSchedule::BoundedJitter.interval(nominal, index) != nominal })
        );
        assert_eq!(SampleSchedule::Uniform.pattern_label(), "1.0");
        assert_eq!(
            SampleSchedule::BoundedJitter.pattern_label(),
            "0.625|0.875|1.0|0.75|0.9375|0.6875"
        );
        assert_eq!(SampleSchedule::Uniform.mean_interval_multiplier(), 1.0);
        assert_eq!(
            SampleSchedule::BoundedJitter.mean_interval_multiplier(),
            0.8125
        );
    }

    #[test]
    fn weighted_fit_recovers_an_offset_sine_on_irregular_timestamps() {
        let angular_frequency: f64 = 1.7;
        let offset: f64 = 1.25;
        let sine: f64 = 0.8;
        let cosine: f64 = -0.35;
        let timestamps = [
            0.0, 0.07, 0.19, 0.42, 0.5, 0.81, 1.04, 1.37, 1.49, 1.86, 2.3, 2.77, 3.01, 3.44, 3.9,
            4.21, 4.68, 5.0, 5.39, 5.81, 6.22, 6.7, 7.08,
        ];
        let samples = timestamps
            .into_iter()
            .map(|time| {
                let angle = angular_frequency * time;
                (time, offset + sine * angle.sin() + cosine * angle.cos())
            })
            .collect::<Vec<_>>();

        let fit = fit_sine(&samples, angular_frequency).unwrap();
        assert_relative_eq!(fit.offset, offset, epsilon = 2.0e-13);
        assert_relative_eq!(fit.sine, sine, epsilon = 2.0e-13);
        assert_relative_eq!(fit.cosine, cosine, epsilon = 2.0e-13);
        assert_relative_eq!(fit.rms, 0.0, epsilon = 2.0e-13);
    }

    #[test]
    fn phase_unwrap_selects_the_turn_nearest_the_analytic_phase() {
        let analytic: f64 = -3.4;
        let measured_wrapped = analytic.rem_euclid(core::f64::consts::TAU);
        assert_relative_eq!(
            unwrap_phase(measured_wrapped, analytic),
            analytic,
            epsilon = 1.0e-14
        );
        assert_relative_eq!(
            unwrap_phase(-0.1, core::f64::consts::TAU - 0.1),
            core::f64::consts::TAU - 0.1,
            epsilon = 1.0e-14
        );
    }

    #[test]
    fn public_api_measurements_converge_near_cutoff() {
        let coarse =
            measure_low_pass::<4>(Response::Butterworth, 1.0, SampleSchedule::Uniform, 256)
                .unwrap();
        let fine = measure_low_pass::<4>(Response::Butterworth, 1.0, SampleSchedule::Uniform, 512)
            .unwrap();
        assert!((coarse.gain - fine.gain).abs() / fine.gain < 1.0e-4);
        let fine_phase = unwrap_phase(fine.wrapped_phase_radians, coarse.wrapped_phase_radians);
        assert!((coarse.wrapped_phase_radians - fine_phase).abs() < 1.0e-4);
        assert!(coarse.fit_rms.is_finite());
        assert!(coarse.diagnostics.derivative_evaluations > 0);
    }

    #[test]
    fn source_only_measurement_retains_its_original_warmup_and_settling_contract() {
        let measurement =
            measure_low_pass::<4>(Response::Butterworth, 0.7, SampleSchedule::Uniform, 256)
                .unwrap();
        let expected_warmup = (f64::from(DEFAULT_WARMUP_CYCLES) / 0.7)
            .max(minimum_settling_time_seconds::<4>(Response::Butterworth).unwrap());
        let analytic = exact_frequency_point::<4>(Response::Butterworth, 0.7).unwrap();
        let measured =
            MeasuredFrequencyPoint::new(measurement, analytic.gain_db, analytic.phase_radians);

        assert_eq!(measurement.warmup_seconds, expected_warmup);
        assert_relative_eq!(measurement.warmup_cycles, 60.0, epsilon = 1.0e-12);
        assert_eq!(measurement.fit_cycles, DEFAULT_FIT_CYCLES);
        assert!(measurement.settling_window_count >= 3);
        assert!(measurement.settling_window_count <= MAX_SETTLING_WINDOWS);
        assert!(measurement.settling_relative_gain_delta <= 1.0e-4);
        assert!(measurement.settling_phase_delta_radians <= 1.0e-4);
        assert!((measurement.gain - 10.0_f64.powf(analytic.gain_db / 20.0)).abs() < 1.0e-4);
        assert!((measured.unwrapped_phase_radians - analytic.phase_radians).abs() < 1.0e-6);
    }

    #[test]
    fn paired_measurements_share_boundaries_and_match_analytic_responses() {
        for frequency_ratio in [0.7, 0.9, 1.0] {
            assert_pair_matches_analytic::<4>(frequency_ratio, SampleSchedule::Uniform);
            assert_pair_matches_analytic::<8>(frequency_ratio, SampleSchedule::Uniform);
        }
    }

    #[test]
    fn paired_measurement_shares_the_bounded_jitter_schedule() {
        assert_pair_matches_analytic::<8>(0.7, SampleSchedule::BoundedJitter);
    }

    #[test]
    fn paired_warmup_applies_the_validated_equalizer_decay_floor() {
        let source = minimum_warmup_seconds::<4>(Response::Butterworth, 100.0, false).unwrap();
        let paired = minimum_warmup_seconds::<4>(Response::Butterworth, 100.0, true).unwrap();
        let equalizer_floor = PHASE_EQUALIZER_SETTLING_E_FOLDS
            / (core::f64::consts::TAU * MIN_VALIDATED_PHASE_EQUALIZER_SIGMA);

        assert!(source < equalizer_floor);
        assert_eq!(paired, equalizer_floor);
    }

    #[test]
    fn public_api_measurements_match_analytic_passband_points() {
        for response in [
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            for frequency_ratio in [0.7, 0.9, 1.0] {
                let analytic = exact_frequency_point::<4>(response, frequency_ratio).unwrap();
                for schedule in [SampleSchedule::Uniform, SampleSchedule::BoundedJitter] {
                    let streamed =
                        measure_low_pass::<4>(response, frequency_ratio, schedule, 256).unwrap();
                    let relative_gain_error =
                        (streamed.gain - 10.0_f64.powf(analytic.gain_db / 20.0)).abs()
                            / streamed.gain;
                    let point = MeasuredFrequencyPoint::new(
                        streamed,
                        analytic.gain_db,
                        analytic.phase_radians,
                    );
                    assert!(
                        relative_gain_error < 1.0e-4,
                        "{response:?} at {frequency_ratio} with {schedule:?}: gain error {relative_gain_error}"
                    );
                    let phase_error =
                        (point.unwrapped_phase_radians - analytic.phase_radians).abs();
                    assert!(
                        phase_error < 1.0e-6,
                        "{response:?} at {frequency_ratio} with {schedule:?}: phase error {phase_error}"
                    );
                }
            }
        }
    }

    #[test]
    fn slow_high_order_chebyshev_transient_is_settled_before_fitting() {
        let response = Response::Chebyshev1 { ripple_db: 0.5 };
        let analytic = exact_frequency_point::<25>(response, 1.0).unwrap();
        let streamed = measure_low_pass::<25>(response, 1.0, SampleSchedule::Uniform, 256).unwrap();
        let point = MeasuredFrequencyPoint::new(streamed, analytic.gain_db, analytic.phase_radians);
        let expected_gain = 10.0_f64.powf(analytic.gain_db / 20.0);

        assert!(streamed.warmup_cycles > 60.0);
        assert!((streamed.gain - expected_gain).abs() / expected_gain < 1.0e-4);
        assert!((point.unwrapped_phase_radians - analytic.phase_radians).abs() < 1.0e-6);
    }
}
