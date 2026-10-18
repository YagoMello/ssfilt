//! Deterministic sine measurements made with public streaming updates.

use anyhow::{Result, bail};
use ssfilt::{BandPass, HighPass, InputModel, LowPass, Response, StreamingFilter};

use crate::Topology;
use crate::analytic::{self, FrequencyPoint};

pub(super) const DEFAULT_SAMPLES_PER_CYCLE: u32 = 256;
const DEFAULT_WARMUP_CYCLES: f64 = 24.0;
const DEFAULT_FIT_CYCLES: u32 = 4;
const SETTLING_RELATIVE_GAIN_TOLERANCE: f64 = 2.0e-3;
const SETTLING_PHASE_TOLERANCE_RADIANS: f64 = 2.0e-3;
const SETTLING_GAIN_SCALE_FLOOR: f64 = 3.0e-3;
const PHASE_RESOLUTION_GAIN_FLOOR: f64 = 1.0e-4;
const REQUIRED_SETTLED_COMPARISONS: u32 = 2;
const MAX_SETTLING_WINDOWS: u32 = 8;
const MAX_UPDATES_PER_MEASUREMENT: usize = 2_000_000;

#[derive(Clone, Copy, Debug)]
pub(super) struct MeasuredFrequencyPoint {
    pub(super) gain: f64,
    pub(super) gain_db: f64,
    pub(super) fit_offset: f64,
    pub(super) wrapped_phase_radians: f64,
    pub(super) unwrapped_phase_radians: f64,
    pub(super) fit_rms: f64,
    pub(super) fit_sample_count: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct InspectionPoint {
    pub(super) ratio: f64,
    pub(super) analytic: FrequencyPoint,
    pub(super) measured: Option<MeasuredFrequencyPoint>,
    pub(super) public_update_count: usize,
    pub(super) status: InspectionStatus,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum InspectionStatus {
    Measured,
    WorkLimitEstimate,
    WorkLimitReached,
}

impl InspectionStatus {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::WorkLimitEstimate => "work_limit_estimate",
            Self::WorkLimitReached => "work_limit_reached",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum MeasurementResult {
    Measured {
        point: MeasuredFrequencyPoint,
        public_update_count: usize,
    },
    WorkLimitExceeded {
        public_update_count: usize,
        limit: usize,
        minimum_work_exceeds_limit: bool,
    },
}

impl MeasuredFrequencyPoint {
    pub(super) const fn phase_is_resolved(self) -> bool {
        self.gain >= PHASE_RESOLUTION_GAIN_FLOOR
    }
}

#[derive(Clone, Copy, Debug)]
struct SineFit {
    offset: f64,
    sine: f64,
    cosine: f64,
    rms: f64,
}

impl SineFit {
    fn gain(self) -> f64 {
        self.sine.hypot(self.cosine)
    }

    fn wrapped_phase(self) -> f64 {
        self.cosine.atan2(self.sine)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SineFitAccumulator {
    sum: f64,
    sum_squares: f64,
    sum_sine: f64,
    sum_cosine: f64,
    sample_count: u32,
}

impl SineFitAccumulator {
    fn observe(&mut self, time: f64, value: f64, angular_frequency: f64) -> Result<()> {
        if !time.is_finite() || !value.is_finite() {
            bail!("sine fitting requires finite timestamps and values");
        }
        let phase = angular_frequency * time;
        let sine = phase.sin();
        let cosine = phase.cos();
        self.sum += value;
        self.sum_squares = value.mul_add(value, self.sum_squares);
        self.sum_sine = value.mul_add(sine, self.sum_sine);
        self.sum_cosine = value.mul_add(cosine, self.sum_cosine);
        self.sample_count += 1;
        Ok(())
    }

    fn finish(self) -> Result<SineFit> {
        if self.sample_count < 3 {
            bail!("sine fitting requires at least three samples");
        }
        let count = f64::from(self.sample_count);
        let offset = self.sum / count;
        // Each fit window spans an integer number of uniformly sampled cycles,
        // so the sine, cosine, and DC components are orthogonal.
        let sine = 2.0 * self.sum_sine / count;
        let cosine = 2.0 * self.sum_cosine / count;
        let rms_squared = (self.sum_squares / count
            - offset * offset
            - f64::midpoint(sine * sine, cosine * cosine))
        .max(0.0);
        let rms = rms_squared.sqrt();
        if !offset.is_finite() || !sine.is_finite() || !cosine.is_finite() || !rms.is_finite() {
            bail!("sine fitting produced a non-finite result");
        }
        Ok(SineFit {
            offset,
            sine,
            cosine,
            rms,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct SettlingState {
    final_fit: Option<SineFit>,
    settled_comparisons: u32,
    relative_gain_delta: f64,
    phase_delta_radians: f64,
}

impl SettlingState {
    const fn new() -> Self {
        Self {
            final_fit: None,
            settled_comparisons: 0,
            relative_gain_delta: f64::INFINITY,
            phase_delta_radians: f64::INFINITY,
        }
    }

    fn observe(&mut self, current: SineFit) {
        if let Some(previous) = self.final_fit {
            self.relative_gain_delta = relative_gain_delta(previous, current);
            self.phase_delta_radians =
                circular_phase_delta(previous.wrapped_phase(), current.wrapped_phase());
            let phase_is_resolved =
                previous.gain().min(current.gain()) >= PHASE_RESOLUTION_GAIN_FLOOR;
            let settled = self.relative_gain_delta <= SETTLING_RELATIVE_GAIN_TOLERANCE
                && (!phase_is_resolved
                    || self.phase_delta_radians <= SETTLING_PHASE_TOLERANCE_RADIANS);
            self.settled_comparisons = if settled {
                self.settled_comparisons + 1
            } else {
                0
            };
        }
        self.final_fit = Some(current);
    }

    const fn is_settled(self) -> bool {
        self.settled_comparisons >= REQUIRED_SETTLED_COMPARISONS
    }
}

pub(super) fn measure<const N: usize>(
    topology: Topology,
    response: Response,
    frequency_ratio: f64,
    analytic_point: FrequencyPoint,
) -> Result<MeasurementResult> {
    if !frequency_ratio.is_finite() || frequency_ratio <= 0.0 {
        bail!("measurement frequency ratio must be finite and positive");
    }

    let max_interval = 1.0 / (frequency_ratio * f64::from(DEFAULT_SAMPLES_PER_CYCLE));
    if !max_interval.is_finite() || max_interval <= 0.0 {
        bail!("measurement interval must be finite and positive");
    }
    let warmup_seconds = (DEFAULT_WARMUP_CYCLES / frequency_ratio).max(
        analytic::minimum_settling_time_seconds(topology, response, N)?,
    );
    let fit_sample_count = usize::try_from(DEFAULT_FIT_CYCLES * DEFAULT_SAMPLES_PER_CYCLE)?;
    let minimum_fit_updates =
        DEFAULT_FIT_CYCLES * DEFAULT_SAMPLES_PER_CYCLE * (REQUIRED_SETTLED_COMPARISONS + 1);
    let minimum_work = (warmup_seconds / max_interval).ceil() + f64::from(minimum_fit_updates);
    let maximum_work = f64::from(u32::try_from(MAX_UPDATES_PER_MEASUREMENT)?);
    if !minimum_work.is_finite() || minimum_work > maximum_work {
        return Ok(MeasurementResult::WorkLimitExceeded {
            public_update_count: 0,
            limit: MAX_UPDATES_PER_MEASUREMENT,
            minimum_work_exceeds_limit: true,
        });
    }
    let angular_frequency = core::f64::consts::TAU * frequency_ratio;
    let mut filter = build_filter::<N>(topology, response, InputModel::Linear, 0.0)?;
    let mut time_seconds = 0.0;
    let mut update_count = 0;
    if !advance_to(
        &mut *filter,
        &mut time_seconds,
        warmup_seconds,
        max_interval,
        angular_frequency,
        &mut update_count,
    )? {
        return Ok(MeasurementResult::WorkLimitExceeded {
            public_update_count: update_count,
            limit: MAX_UPDATES_PER_MEASUREMENT,
            minimum_work_exceeds_limit: false,
        });
    }

    let mut settling = SettlingState::new();
    for _ in 1..=MAX_SETTLING_WINDOWS {
        let mut accumulator = SineFitAccumulator::default();
        for _ in 0..fit_sample_count {
            if update_count >= MAX_UPDATES_PER_MEASUREMENT {
                return Ok(MeasurementResult::WorkLimitExceeded {
                    public_update_count: update_count,
                    limit: MAX_UPDATES_PER_MEASUREMENT,
                    minimum_work_exceeds_limit: false,
                });
            }
            let next_time = time_seconds + max_interval;
            if !next_time.is_finite() || next_time <= time_seconds {
                bail!("measurement timestamp did not advance by one sample interval");
            }
            time_seconds = next_time;
            let input = (angular_frequency * time_seconds).sin();
            let output = filter.update(input, max_interval)?;
            update_count += 1;
            accumulator.observe(time_seconds, output, angular_frequency)?;
        }
        let fit = accumulator.finish()?;
        settling.observe(fit);
        if settling.is_settled() {
            break;
        }
    }

    if !settling.is_settled() {
        let last_fit = settling
            .final_fit
            .expect("a settling window always has a fit");
        bail!(
            "measurement did not settle for {response:?} order {N} at {frequency_ratio} × cutoff: measured gain {:e}, fit RMS {:e}, analytic gain {:.1} dB, relative gain delta {:e}, phase delta {:e} rad",
            last_fit.gain(),
            last_fit.rms,
            analytic_point.gain_db,
            settling.relative_gain_delta,
            settling.phase_delta_radians
        );
    }

    let fit = settling
        .final_fit
        .expect("a settling window always has a fit");
    let gain = fit.gain();
    Ok(MeasurementResult::Measured {
        point: MeasuredFrequencyPoint {
            gain,
            gain_db: 20.0 * gain.log10(),
            fit_offset: fit.offset,
            wrapped_phase_radians: fit.wrapped_phase(),
            unwrapped_phase_radians: analytic::phase_near(
                fit.wrapped_phase(),
                analytic_point.phase_radians,
            ),
            fit_rms: fit.rms,
            fit_sample_count,
        },
        public_update_count: update_count,
    })
}

pub(super) fn build_filter<const N: usize>(
    topology: Topology,
    response: Response,
    input_model: InputModel,
    initial_input: f64,
) -> Result<Box<dyn StreamingFilter<Scalar = f64>>> {
    let filter: Box<dyn StreamingFilter<Scalar = f64>> = match topology {
        Topology::LowPass => Box::new(
            LowPass::<N>::builder(1.0)
                .response(response)
                .input_model(input_model)
                .initial_input(initial_input)
                .build()?,
        ),
        Topology::HighPass => Box::new(
            HighPass::<N>::builder(1.0)
                .response(response)
                .input_model(input_model)
                .initial_input(initial_input)
                .build()?,
        ),
        Topology::BandPass => Box::new(
            BandPass::<N>::builder(0.5, 2.0)
                .response(response)
                .input_model(input_model)
                .initial_input(initial_input)
                .build()?,
        ),
    };
    Ok(filter)
}

fn advance_to(
    filter: &mut dyn StreamingFilter<Scalar = f64>,
    time_seconds: &mut f64,
    end_seconds: f64,
    maximum_interval_seconds: f64,
    angular_frequency: f64,
    update_count: &mut usize,
) -> Result<bool> {
    while *time_seconds < end_seconds {
        if *update_count >= MAX_UPDATES_PER_MEASUREMENT {
            return Ok(false);
        }
        let remaining = end_seconds - *time_seconds;
        let reaches_end = maximum_interval_seconds >= remaining;
        let interval = maximum_interval_seconds.min(remaining);
        if !interval.is_finite() || interval <= 0.0 {
            bail!("measurement timestamp schedule did not advance");
        }
        let next_time = if reaches_end {
            end_seconds
        } else {
            *time_seconds + interval
        };
        if !next_time.is_finite() || next_time <= *time_seconds {
            bail!("measurement timestamp schedule did not advance");
        }
        *time_seconds = next_time;
        let input = (angular_frequency * *time_seconds).sin();
        filter.update(input, interval)?;
        *update_count += 1;
    }
    Ok(true)
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

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;
    use ssfilt::{InputModel, Response};

    use super::{
        DEFAULT_SAMPLES_PER_CYCLE, MeasurementResult, SineFitAccumulator, build_filter, measure,
    };
    use crate::Topology;
    use crate::analytic;

    #[test]
    fn sine_fit_recovers_offset_gain_and_phase() {
        let offset = 0.25;
        let gain = 1.7;
        let phase: f64 = -0.42;
        let omega = core::f64::consts::TAU * 1.25;
        let mut accumulator = SineFitAccumulator::default();
        for index in 1..=256 {
            let time = f64::from(index) / (64.0 * 1.25);
            accumulator
                .observe(time, offset + gain * (omega * time + phase).sin(), omega)
                .unwrap();
        }
        let fit = accumulator.finish().unwrap();
        assert_relative_eq!(fit.offset, offset, epsilon = 1.0e-12);
        assert_relative_eq!(fit.gain(), gain, epsilon = 1.0e-12);
        assert_relative_eq!(fit.wrapped_phase(), phase, epsilon = 1.0e-12);
        assert!(fit.rms < 1.0e-12);
    }

    #[test]
    fn streaming_measurement_tracks_the_analytic_low_pass_cutoff() {
        let analytic_point =
            analytic::frequency_point(Topology::LowPass, Response::Butterworth, 2, 1.0).unwrap();
        let (measured, update_count) = match measure::<2>(
            Topology::LowPass,
            Response::Butterworth,
            1.0,
            analytic_point,
        )
        .unwrap()
        {
            MeasurementResult::Measured {
                point,
                public_update_count,
            } => (point, public_update_count),
            MeasurementResult::WorkLimitExceeded { .. } => {
                panic!("the low-order cutoff point should be within the work limit")
            }
        };
        assert!(update_count > measured.fit_sample_count);
        assert_relative_eq!(measured.gain_db, analytic_point.gain_db, epsilon = 0.025);
        assert!(measured.phase_is_resolved());
        assert!(measured.fit_rms < 0.01);
        assert_eq!(DEFAULT_SAMPLES_PER_CYCLE, 256);
    }

    #[test]
    fn streaming_measurements_track_high_and_band_pass_edges() {
        for (topology, ratio) in [
            (Topology::HighPass, 1.0),
            (Topology::BandPass, 0.5),
            (Topology::BandPass, 2.0),
        ] {
            let analytic_point =
                analytic::frequency_point(topology, Response::Butterworth, 2, ratio).unwrap();
            let measured =
                match measure::<2>(topology, Response::Butterworth, ratio, analytic_point).unwrap()
                {
                    MeasurementResult::Measured { point, .. } => point,
                    MeasurementResult::WorkLimitExceeded { .. } => {
                        panic!("a low-order −3 dB edge should be within the work limit")
                    }
                };
            assert_relative_eq!(measured.gain_db, analytic_point.gain_db, epsilon = 0.025);
            assert!(measured.phase_is_resolved());
            assert!(
                (measured.unwrapped_phase_radians - analytic_point.phase_radians).abs() < 1.0e-3
            );
        }
    }

    #[test]
    fn high_pass_streaming_uses_the_new_endpoint_sample() {
        let mut filter = build_filter::<1>(
            Topology::HighPass,
            Response::RepeatedPole,
            InputModel::CurrentHold,
            0.0,
        )
        .unwrap();
        let output = filter.update(1.0, 1.0e-6).unwrap();
        assert!(output > 0.99);
    }
}
