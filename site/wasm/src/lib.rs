//! Browser adapter for the ssfilt project site.
//!
//! This crate is deliberately separate from the published, `no_std` runtime.
//! It translates browser-friendly data into calls to the existing public API;
//! it does not add a second filter implementation.

#![forbid(unsafe_code)]

use js_sys::Float64Array;
use serde::{Deserialize, Serialize};
use ssfilt::{
    BandPass, HighPass, InputModel, IntegrationDiagnostics, LowPass, MAX_BESSEL_BAND_PASS_ORDER,
    MAX_BESSEL_ORDER, Response,
};
use wasm_bindgen::prelude::*;

mod frequency;

#[allow(dead_code)]
#[path = "../../../examples/support/transfer.rs"]
mod transfer;

const FREQUENCY_POINTS: u32 = 1_201;
const BAND_DETAIL_POINTS: u32 = 801;
const BAND_MAPPED_LIMIT: f64 = 20.0;
const STEP_POINTS: u32 = 1_201;
const MAX_UPLOAD_ROWS: usize = 50_000;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(clippy::enum_variant_names)]
enum Topology {
    LowPass,
    HighPass,
    BandPass,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ResponseFamily {
    RepeatedPole,
    Butterworth,
    Bessel,
    Chebyshev1,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InputPolicy {
    Linear,
    PreviousHold,
    CurrentHold,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InitialMode {
    Sample0,
    Custom,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct FilterConfig {
    topology: Topology,
    response: ResponseFamily,
    order: usize,
    cutoff_hz: Option<f64>,
    lower_cutoff_hz: Option<f64>,
    upper_cutoff_hz: Option<f64>,
    #[serde(default = "default_ripple_db")]
    ripple_db: f64,
    #[serde(default = "default_input_policy")]
    input_model: InputPolicy,
    #[serde(default = "default_initial_mode")]
    initial_mode: InitialMode,
    #[serde(default)]
    initial_value: f64,
    first_interval_seconds: Option<f64>,
}

#[derive(Debug, Serialize)]
struct ResponseData {
    frequency_hz: Vec<f64>,
    gain_db: Vec<f64>,
    phase_degrees: Vec<f64>,
    group_delay_seconds: Vec<f64>,
    step_time_seconds: Vec<f64>,
    step_output: Vec<f64>,
    summary: String,
}

#[derive(Debug, Serialize)]
struct FilteredData {
    time: Vec<f64>,
    input: Vec<f64>,
    output: Vec<f64>,
    accepted_steps: usize,
    rejected_steps: usize,
    derivative_evaluations: usize,
    equilibrium_shortcuts: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct WorkTotals {
    accepted_steps: usize,
    rejected_steps: usize,
    derivative_evaluations: usize,
    equilibrium_shortcuts: usize,
}

impl WorkTotals {
    fn observe(&mut self, diagnostics: IntegrationDiagnostics<f64>) {
        self.accepted_steps = self
            .accepted_steps
            .saturating_add(diagnostics.accepted_steps());
        self.rejected_steps = self
            .rejected_steps
            .saturating_add(diagnostics.rejected_steps());
        self.derivative_evaluations = self
            .derivative_evaluations
            .saturating_add(diagnostics.derivative_evaluations());
        if diagnostics.used_equilibrium_shortcut() {
            self.equilibrium_shortcuts = self.equilibrium_shortcuts.saturating_add(1);
        }
    }
}

trait BrowserFilter {
    fn update(&mut self, input: f64, dt_seconds: f64) -> Result<f64, String>;
    fn output(&self) -> f64;
    fn diagnostics(&self) -> IntegrationDiagnostics<f64>;
}

impl<const N: usize> BrowserFilter for LowPass<N> {
    fn update(&mut self, input: f64, dt_seconds: f64) -> Result<f64, String> {
        LowPass::update(self, input, dt_seconds).map_err(|error| error.to_string())
    }

    fn output(&self) -> f64 {
        LowPass::output(self)
    }

    fn diagnostics(&self) -> IntegrationDiagnostics<f64> {
        self.last_diagnostics()
    }
}

impl<const N: usize> BrowserFilter for HighPass<N> {
    fn update(&mut self, input: f64, dt_seconds: f64) -> Result<f64, String> {
        HighPass::update(self, input, dt_seconds).map_err(|error| error.to_string())
    }

    fn output(&self) -> f64 {
        HighPass::output(self)
    }

    fn diagnostics(&self) -> IntegrationDiagnostics<f64> {
        self.last_diagnostics()
    }
}

impl<const N: usize> BrowserFilter for BandPass<N> {
    fn update(&mut self, input: f64, dt_seconds: f64) -> Result<f64, String> {
        BandPass::update(self, input, dt_seconds).map_err(|error| error.to_string())
    }

    fn output(&self) -> f64 {
        BandPass::output(self)
    }

    fn diagnostics(&self) -> IntegrationDiagnostics<f64> {
        self.last_diagnostics()
    }
}

#[wasm_bindgen]
/// Calculates ideal response curves and a streamed unit-step trace.
///
/// # Errors
///
/// Returns a JavaScript error value when the configuration is invalid or the
/// public filter cannot be constructed or advanced.
pub fn response_data(config: JsValue) -> Result<JsValue, JsValue> {
    let config = parse_config(config)?;
    let data = calculate_response(config).map_err(js_error)?;
    serde_wasm_bindgen::to_value(&data).map_err(|error| js_error(error.to_string()))
}

#[wasm_bindgen]
/// Filters timestamped samples using the public streaming API.
///
/// # Errors
///
/// Returns a JavaScript error value for invalid configuration, malformed
/// timestamp/value arrays, or a failed transactional filter update.
pub fn filter_samples(
    config: JsValue,
    times: &Float64Array,
    values: &Float64Array,
) -> Result<JsValue, JsValue> {
    let config = parse_config(config)?;
    let data = filter_uploaded_data(config, &times.to_vec(), &values.to_vec()).map_err(js_error)?;
    serde_wasm_bindgen::to_value(&data).map_err(|error| js_error(error.to_string()))
}

#[wasm_bindgen]
/// Builds a bounded time-frequency guide for uploaded samples.
///
/// The guide reconstructs the signal with the selected public input model
/// before applying the private display-only wavelet analysis.
///
/// # Errors
///
/// Returns a JavaScript error value when the samples or configuration cannot
/// support a meaningful bounded analysis.
pub fn frequency_guide(
    config: JsValue,
    times: &Float64Array,
    values: &Float64Array,
) -> Result<JsValue, JsValue> {
    let config = parse_config(config)?;
    validate_config(config).map_err(js_error)?;
    let data = frequency::analyze(&times.to_vec(), &values.to_vec(), config.input_model)
        .map_err(js_error)?;
    serde_wasm_bindgen::to_value(&data).map_err(|error| js_error(error.to_string()))
}

fn parse_config(config: JsValue) -> Result<FilterConfig, JsValue> {
    serde_wasm_bindgen::from_value(config).map_err(|error| js_error(error.to_string()))
}

fn calculate_response(config: FilterConfig) -> Result<ResponseData, String> {
    validate_config(config)?;
    let response = response(config)?;
    let (reference_hz, lower_ratio, upper_ratio) = frequency_mapping(config)?;
    let prototype_order = prototype_order(config);
    let bandwidth = fractional_bandwidth(config)?;
    let ratios = response_frequency_ratios(config, lower_ratio, upper_ratio, bandwidth)?;
    let mut frequency_hz = Vec::with_capacity(ratios.len());
    let mut gain_db = Vec::with_capacity(ratios.len());
    let mut phase_degrees = Vec::with_capacity(ratios.len());
    let mut group_delay_seconds = Vec::with_capacity(ratios.len());

    for ratio in ratios {
        let point = transfer_point(config.topology, response, prototype_order, ratio, bandwidth)?;
        frequency_hz.push(reference_hz * ratio);
        gain_db.push(point.gain_db);
        phase_degrees.push(point.phase_radians.to_degrees());
        group_delay_seconds
            .push(point.normalized_group_delay / (core::f64::consts::TAU * reference_hz));
    }

    let (step_time_seconds, step_output) = calculate_step(config, response, reference_hz)?;
    Ok(ResponseData {
        frequency_hz,
        gain_db,
        phase_degrees,
        group_delay_seconds,
        step_time_seconds,
        step_output,
        summary: summary(config),
    })
}

#[derive(Clone, Copy, Debug)]
struct TransferPoint {
    gain_db: f64,
    phase_radians: f64,
    normalized_group_delay: f64,
}

fn transfer_point(
    topology: Topology,
    response: Response<f64>,
    prototype_order: usize,
    ratio: f64,
    fractional_bandwidth: f64,
) -> Result<TransferPoint, String> {
    match topology {
        Topology::LowPass => {
            let point = transfer::low_pass(response, prototype_order, ratio)
                .map_err(|error| error.to_string())?;
            Ok(TransferPoint {
                gain_db: point.gain_db,
                phase_radians: point.phase_radians,
                normalized_group_delay: point.normalized_group_delay,
            })
        }
        Topology::HighPass => {
            let point = transfer::low_pass(response, prototype_order, 1.0 / ratio)
                .map_err(|error| error.to_string())?;
            Ok(TransferPoint {
                gain_db: point.gain_db,
                phase_radians: -point.phase_radians,
                normalized_group_delay: point.normalized_group_delay / ratio.powi(2),
            })
        }
        Topology::BandPass => {
            let mapped = (ratio - 1.0 / ratio) / fractional_bandwidth;
            let point = transfer::low_pass(response, prototype_order, mapped.abs())
                .map_err(|error| error.to_string())?;
            Ok(TransferPoint {
                gain_db: point.gain_db,
                phase_radians: point.phase_radians * mapped.signum(),
                normalized_group_delay: point.normalized_group_delay * (1.0 + 1.0 / ratio.powi(2))
                    / fractional_bandwidth,
            })
        }
    }
}

fn calculate_step(
    config: FilterConfig,
    response: Response<f64>,
    reference_hz: f64,
) -> Result<(Vec<f64>, Vec<f64>), String> {
    let order = f64::from(u32::try_from(config.order).expect("validated order fits in u32"));
    let duration = order.max(4.0) / reference_hz;
    let dt = duration / f64::from(STEP_POINTS - 1);
    // A canonical continuous-time step applies the new value throughout the
    // first interval. Uploaded-data filtering still honors the user's chosen
    // reconstruction policy.
    let step_config = FilterConfig {
        input_model: InputPolicy::CurrentHold,
        ..config
    };
    let mut filter = build_filter(step_config, response, 0.0)?;
    let point_count = usize::try_from(STEP_POINTS).expect("small step point count");
    let mut times = Vec::with_capacity(point_count);
    let mut values = Vec::with_capacity(point_count);
    times.push(0.0);
    values.push(filter.output());
    for index in 1..STEP_POINTS {
        let output = filter.update(1.0, dt)?;
        times.push(f64::from(index) * dt);
        values.push(output);
    }
    Ok((times, values))
}

fn filter_uploaded_data(
    config: FilterConfig,
    times: &[f64],
    values: &[f64],
) -> Result<FilteredData, String> {
    validate_config(config)?;
    if times.len() != values.len() {
        return Err("time and value columns must have the same number of rows".to_owned());
    }
    if times.is_empty() {
        return Err("the CSV must contain at least one sample".to_owned());
    }
    if times.len() > MAX_UPLOAD_ROWS {
        return Err(format!(
            "the CSV has {} rows; the browser demo accepts at most {MAX_UPLOAD_ROWS}",
            times.len()
        ));
    }
    if times.iter().chain(values).any(|value| !value.is_finite()) {
        return Err("timestamps and sample values must all be finite".to_owned());
    }
    if times.windows(2).any(|pair| pair[1] <= pair[0]) {
        return Err("timestamps must be strictly increasing".to_owned());
    }

    let response = response(config)?;
    let initial_input = match config.initial_mode {
        InitialMode::Sample0 => values[0],
        InitialMode::Custom => {
            let first_interval = config.first_interval_seconds.ok_or_else(|| {
                "a positive first interval is required for a custom initial value".to_owned()
            })?;
            if !first_interval.is_finite() || first_interval <= 0.0 {
                return Err("the first interval must be finite and positive".to_owned());
            }
            config.initial_value
        }
    };
    if !initial_input.is_finite() {
        return Err("the initial value must be finite".to_owned());
    }

    let mut filter = build_filter(config, response, initial_input)?;
    let mut output = Vec::with_capacity(values.len());
    let mut work = WorkTotals::default();
    let first_update = match config.initial_mode {
        InitialMode::Sample0 => {
            output.push(filter.output());
            1
        }
        InitialMode::Custom => 0,
    };

    for index in first_update..values.len() {
        let dt = if index == 0 {
            config
                .first_interval_seconds
                .expect("custom initialization validated the first interval")
        } else {
            times[index] - times[index - 1]
        };
        let filtered = filter
            .update(values[index], dt)
            .map_err(|error| format!("filter update failed at CSV row {}: {error}", index + 1))?;
        output.push(filtered);
        work.observe(filter.diagnostics());
    }

    Ok(FilteredData {
        time: times.to_vec(),
        input: values.to_vec(),
        output,
        accepted_steps: work.accepted_steps,
        rejected_steps: work.rejected_steps,
        derivative_evaluations: work.derivative_evaluations,
        equilibrium_shortcuts: work.equilibrium_shortcuts,
    })
}

fn validate_config(config: FilterConfig) -> Result<(), String> {
    if !(1..=50).contains(&config.order) {
        return Err(format!(
            "order must be between 1 and 50, got {}",
            config.order
        ));
    }
    if matches!(config.topology, Topology::BandPass) && !config.order.is_multiple_of(2) {
        return Err("band-pass final order must be positive and even".to_owned());
    }
    if matches!(config.response, ResponseFamily::Bessel) {
        let limit = match config.topology {
            Topology::LowPass | Topology::HighPass => MAX_BESSEL_ORDER,
            Topology::BandPass => MAX_BESSEL_BAND_PASS_ORDER,
        };
        if config.order > limit {
            return Err(format!(
                "Bessel supports final order up to {limit} for this topology"
            ));
        }
    }
    match config.topology {
        Topology::LowPass | Topology::HighPass => {
            positive(config.cutoff_hz, "cutoff")?;
        }
        Topology::BandPass => {
            let lower = positive(config.lower_cutoff_hz, "lower cutoff")?;
            let upper = positive(config.upper_cutoff_hz, "upper cutoff")?;
            if upper <= lower {
                return Err("upper cutoff must be greater than lower cutoff".to_owned());
            }
        }
    }
    response(config)?;
    Ok(())
}

fn positive(value: Option<f64>, name: &str) -> Result<f64, String> {
    let value = value.ok_or_else(|| format!("{name} is required"))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{name} must be finite and positive"));
    }
    Ok(value)
}

fn response(config: FilterConfig) -> Result<Response<f64>, String> {
    match config.response {
        ResponseFamily::RepeatedPole => Ok(Response::RepeatedPole),
        ResponseFamily::Butterworth => Ok(Response::Butterworth),
        ResponseFamily::Bessel => Ok(Response::Bessel),
        ResponseFamily::Chebyshev1 => {
            if !config.ripple_db.is_finite()
                || config.ripple_db <= 0.0
                || config.ripple_db >= 10.0 * core::f64::consts::LOG10_2
            {
                return Err("Chebyshev ripple must be between 0 and 3.0103 dB".to_owned());
            }
            Ok(Response::Chebyshev1 {
                ripple_db: config.ripple_db,
            })
        }
    }
}

fn input_model(policy: InputPolicy) -> InputModel {
    match policy {
        InputPolicy::Linear => InputModel::Linear,
        InputPolicy::PreviousHold => InputModel::PreviousHold,
        InputPolicy::CurrentHold => InputModel::CurrentHold,
    }
}

fn prototype_order(config: FilterConfig) -> usize {
    match config.topology {
        Topology::LowPass | Topology::HighPass => config.order,
        Topology::BandPass => config.order / 2,
    }
}

fn frequency_mapping(config: FilterConfig) -> Result<(f64, f64, f64), String> {
    match config.topology {
        Topology::LowPass | Topology::HighPass => {
            Ok((positive(config.cutoff_hz, "cutoff")?, 0.01, 100.0))
        }
        Topology::BandPass => {
            let lower = positive(config.lower_cutoff_hz, "lower cutoff")?;
            let upper = positive(config.upper_cutoff_hz, "upper cutoff")?;
            Ok((lower.sqrt() * upper.sqrt(), 0.05, 20.0))
        }
    }
}

fn response_frequency_ratios(
    config: FilterConfig,
    lower_ratio: f64,
    upper_ratio: f64,
    fractional_bandwidth: f64,
) -> Result<Vec<f64>, String> {
    let mut ratios = Vec::with_capacity(
        usize::try_from(FREQUENCY_POINTS + BAND_DETAIL_POINTS).expect("small response point count"),
    );
    let log_lower = lower_ratio.ln();
    let log_span = upper_ratio.ln() - log_lower;
    for index in 0..FREQUENCY_POINTS {
        let fraction = f64::from(index) / f64::from(FREQUENCY_POINTS - 1);
        ratios.push((fraction.mul_add(log_span, log_lower)).exp());
    }

    if matches!(config.topology, Topology::BandPass) {
        // Uniform sampling in the low-pass transformed frequency keeps even a
        // very narrow passband visible without making the global log grid
        // impractically dense.
        for index in 0..BAND_DETAIL_POINTS {
            let fraction = f64::from(index) / f64::from(BAND_DETAIL_POINTS - 1);
            let mapped = fraction.mul_add(2.0 * BAND_MAPPED_LIMIT, -BAND_MAPPED_LIMIT);
            let scaled = fractional_bandwidth * mapped;
            let root = scaled.hypot(2.0);
            let ratio = if scaled >= 0.0 {
                scaled.midpoint(root)
            } else {
                2.0 / (root - scaled)
            };
            ratios.push(ratio);
        }

        let lower = positive(config.lower_cutoff_hz, "lower cutoff")?;
        let upper = positive(config.upper_cutoff_hz, "upper cutoff")?;
        let center = lower.sqrt() * upper.sqrt();
        ratios.extend([lower / center, 1.0, upper / center]);
    }

    ratios.sort_unstable_by(f64::total_cmp);
    ratios.dedup_by(|left, right| {
        (*left - *right).abs() <= 8.0 * f64::EPSILON * left.abs().max(right.abs()).max(1.0)
    });
    Ok(ratios)
}

fn fractional_bandwidth(config: FilterConfig) -> Result<f64, String> {
    match config.topology {
        Topology::LowPass | Topology::HighPass => Ok(1.0),
        Topology::BandPass => {
            let lower = positive(config.lower_cutoff_hz, "lower cutoff")?;
            let upper = positive(config.upper_cutoff_hz, "upper cutoff")?;
            let center = lower.sqrt() * upper.sqrt();
            Ok((upper - lower) / center)
        }
    }
}

fn build_filter(
    config: FilterConfig,
    response: Response<f64>,
    initial_input: f64,
) -> Result<Box<dyn BrowserFilter>, String> {
    macro_rules! dispatch_order {
        ($builder:ident) => {
            match config.order {
                1 => $builder::<1>(config, response, initial_input),
                2 => $builder::<2>(config, response, initial_input),
                3 => $builder::<3>(config, response, initial_input),
                4 => $builder::<4>(config, response, initial_input),
                5 => $builder::<5>(config, response, initial_input),
                6 => $builder::<6>(config, response, initial_input),
                7 => $builder::<7>(config, response, initial_input),
                8 => $builder::<8>(config, response, initial_input),
                9 => $builder::<9>(config, response, initial_input),
                10 => $builder::<10>(config, response, initial_input),
                11 => $builder::<11>(config, response, initial_input),
                12 => $builder::<12>(config, response, initial_input),
                13 => $builder::<13>(config, response, initial_input),
                14 => $builder::<14>(config, response, initial_input),
                15 => $builder::<15>(config, response, initial_input),
                16 => $builder::<16>(config, response, initial_input),
                17 => $builder::<17>(config, response, initial_input),
                18 => $builder::<18>(config, response, initial_input),
                19 => $builder::<19>(config, response, initial_input),
                20 => $builder::<20>(config, response, initial_input),
                21 => $builder::<21>(config, response, initial_input),
                22 => $builder::<22>(config, response, initial_input),
                23 => $builder::<23>(config, response, initial_input),
                24 => $builder::<24>(config, response, initial_input),
                25 => $builder::<25>(config, response, initial_input),
                26 => $builder::<26>(config, response, initial_input),
                27 => $builder::<27>(config, response, initial_input),
                28 => $builder::<28>(config, response, initial_input),
                29 => $builder::<29>(config, response, initial_input),
                30 => $builder::<30>(config, response, initial_input),
                31 => $builder::<31>(config, response, initial_input),
                32 => $builder::<32>(config, response, initial_input),
                33 => $builder::<33>(config, response, initial_input),
                34 => $builder::<34>(config, response, initial_input),
                35 => $builder::<35>(config, response, initial_input),
                36 => $builder::<36>(config, response, initial_input),
                37 => $builder::<37>(config, response, initial_input),
                38 => $builder::<38>(config, response, initial_input),
                39 => $builder::<39>(config, response, initial_input),
                40 => $builder::<40>(config, response, initial_input),
                41 => $builder::<41>(config, response, initial_input),
                42 => $builder::<42>(config, response, initial_input),
                43 => $builder::<43>(config, response, initial_input),
                44 => $builder::<44>(config, response, initial_input),
                45 => $builder::<45>(config, response, initial_input),
                46 => $builder::<46>(config, response, initial_input),
                47 => $builder::<47>(config, response, initial_input),
                48 => $builder::<48>(config, response, initial_input),
                49 => $builder::<49>(config, response, initial_input),
                50 => $builder::<50>(config, response, initial_input),
                _ => Err("order must be between 1 and 50".to_owned()),
            }
        };
    }

    match config.topology {
        Topology::LowPass => dispatch_order!(build_low_pass),
        Topology::HighPass => dispatch_order!(build_high_pass),
        Topology::BandPass => dispatch_order!(build_band_pass),
    }
}

fn build_low_pass<const N: usize>(
    config: FilterConfig,
    response: Response<f64>,
    initial_input: f64,
) -> Result<Box<dyn BrowserFilter>, String> {
    let cutoff = positive(config.cutoff_hz, "cutoff")?;
    LowPass::<N>::builder(cutoff)
        .response(response)
        .input_model(input_model(config.input_model))
        .initial_input(initial_input)
        .build()
        .map(|filter| Box::new(filter) as Box<dyn BrowserFilter>)
        .map_err(|error| error.to_string())
}

fn build_high_pass<const N: usize>(
    config: FilterConfig,
    response: Response<f64>,
    initial_input: f64,
) -> Result<Box<dyn BrowserFilter>, String> {
    let cutoff = positive(config.cutoff_hz, "cutoff")?;
    HighPass::<N>::builder(cutoff)
        .response(response)
        .input_model(input_model(config.input_model))
        .initial_input(initial_input)
        .build()
        .map(|filter| Box::new(filter) as Box<dyn BrowserFilter>)
        .map_err(|error| error.to_string())
}

fn build_band_pass<const N: usize>(
    config: FilterConfig,
    response: Response<f64>,
    initial_input: f64,
) -> Result<Box<dyn BrowserFilter>, String> {
    let lower = positive(config.lower_cutoff_hz, "lower cutoff")?;
    let upper = positive(config.upper_cutoff_hz, "upper cutoff")?;
    BandPass::<N>::builder(lower, upper)
        .response(response)
        .input_model(input_model(config.input_model))
        .initial_input(initial_input)
        .build()
        .map(|filter| Box::new(filter) as Box<dyn BrowserFilter>)
        .map_err(|error| error.to_string())
}

fn summary(config: FilterConfig) -> String {
    let response = match config.response {
        ResponseFamily::RepeatedPole => "repeated-pole",
        ResponseFamily::Butterworth => "Butterworth",
        ResponseFamily::Bessel => "Bessel",
        ResponseFamily::Chebyshev1 => "Chebyshev Type I",
    };
    match config.topology {
        Topology::LowPass => format!(
            "Order-{} {response} low-pass; {:.6} Hz whole-filter −3 dB cutoff.",
            config.order,
            config.cutoff_hz.unwrap_or_default()
        ),
        Topology::HighPass => format!(
            "Order-{} {response} high-pass; {:.6} Hz whole-filter −3 dB cutoff.",
            config.order,
            config.cutoff_hz.unwrap_or_default()
        ),
        Topology::BandPass => format!(
            "Order-{} {response} band-pass; {:.6} Hz and {:.6} Hz whole-filter −3 dB edges.",
            config.order,
            config.lower_cutoff_hz.unwrap_or_default(),
            config.upper_cutoff_hz.unwrap_or_default()
        ),
    }
}

const fn default_ripple_db() -> f64 {
    0.5
}

const fn default_input_policy() -> InputPolicy {
    InputPolicy::Linear
}

const fn default_initial_mode() -> InitialMode {
    InitialMode::Sample0
}

fn js_error(message: impl AsRef<str>) -> JsValue {
    JsValue::from_str(message.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn low_pass_config() -> FilterConfig {
        FilterConfig {
            topology: Topology::LowPass,
            response: ResponseFamily::Butterworth,
            order: 4,
            cutoff_hz: Some(20.0),
            lower_cutoff_hz: None,
            upper_cutoff_hz: None,
            ripple_db: 0.5,
            input_model: InputPolicy::Linear,
            initial_mode: InitialMode::Sample0,
            initial_value: 0.0,
            first_interval_seconds: None,
        }
    }

    #[test]
    fn response_uses_the_requested_physical_cutoff() {
        let data = calculate_response(low_pass_config()).unwrap();
        let cutoff_index = data
            .frequency_hz
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| (*left - 20.0).abs().total_cmp(&(*right - 20.0).abs()))
            .unwrap()
            .0;
        assert!((data.gain_db[cutoff_index] + 3.010_299_956_639_812).abs() < 0.03);
        assert_eq!(
            data.step_time_seconds.len(),
            usize::try_from(STEP_POINTS).unwrap()
        );
    }

    #[test]
    fn high_pass_response_uses_the_requested_physical_cutoff() {
        let mut config = low_pass_config();
        config.topology = Topology::HighPass;
        let data = calculate_response(config).unwrap();
        let cutoff_index = data
            .frequency_hz
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| (*left - 20.0).abs().total_cmp(&(*right - 20.0).abs()))
            .unwrap()
            .0;
        assert!((data.gain_db[cutoff_index] + 3.010_299_956_639_812).abs() < 0.03);
    }

    #[test]
    fn response_data_is_not_clamped_to_the_chart_window() {
        let mut config = low_pass_config();
        config.order = 12;
        let data = calculate_response(config).unwrap();
        assert!(data.gain_db.iter().any(|gain| *gain < -140.0));
    }

    #[test]
    fn single_cutoff_response_grid_is_centered_on_the_cutoff() {
        let data = calculate_response(low_pass_config()).unwrap();
        assert!((data.frequency_hz[0] - 0.2).abs() < 1.0e-12);
        assert!((data.frequency_hz.last().copied().unwrap() - 2_000.0).abs() < 1.0e-9);
        assert!(
            (data.frequency_hz[0] * data.frequency_hz.last().copied().unwrap() - 20.0_f64.powi(2))
                .abs()
                < 1.0e-9
        );
    }

    #[test]
    fn narrow_band_pass_grid_contains_both_cutoff_edges() {
        let mut config = low_pass_config();
        config.topology = Topology::BandPass;
        config.order = 4;
        config.cutoff_hz = None;
        config.lower_cutoff_hz = Some(10.0);
        config.upper_cutoff_hz = Some(10.001);
        let data = calculate_response(config).unwrap();

        for edge in [10.0, 10.001] {
            let edge_index = data
                .frequency_hz
                .iter()
                .enumerate()
                .min_by(|(_, left), (_, right)| {
                    (*left - edge).abs().total_cmp(&(*right - edge).abs())
                })
                .unwrap()
                .0;
            assert!((data.frequency_hz[edge_index] - edge).abs() < edge * 1.0e-12);
            assert!((data.gain_db[edge_index] + 3.010_299_956_639_812).abs() < 0.03);
        }
    }

    #[test]
    fn bessel_limit_is_enforced_at_the_public_boundary() {
        let mut config = low_pass_config();
        config.response = ResponseFamily::Bessel;
        config.order = MAX_BESSEL_ORDER;
        assert!(validate_config(config).is_ok());
        config.order += 1;
        assert!(validate_config(config).unwrap_err().contains("Bessel"));
    }

    #[test]
    fn canonical_step_trace_does_not_depend_on_csv_reconstruction() {
        let linear = calculate_response(low_pass_config()).unwrap();
        let mut previous_hold_config = low_pass_config();
        previous_hold_config.input_model = InputPolicy::PreviousHold;
        let previous_hold = calculate_response(previous_hold_config).unwrap();
        assert_eq!(linear.step_time_seconds, previous_hold.step_time_seconds);
        assert_eq!(linear.step_output, previous_hold.step_output);
    }

    #[test]
    fn sample_zero_initialization_preserves_the_first_row() {
        let data = filter_uploaded_data(low_pass_config(), &[0.0, 0.01], &[3.0, 3.0]).unwrap();
        assert_eq!(data.output[0], 3.0);
        assert_eq!(data.output[1], 3.0);
        assert_eq!(data.equilibrium_shortcuts, 1);
    }

    #[test]
    fn custom_initialization_integrates_the_first_endpoint() {
        let mut config = low_pass_config();
        config.initial_mode = InitialMode::Custom;
        config.initial_value = 0.0;
        config.first_interval_seconds = Some(0.01);
        let data = filter_uploaded_data(config, &[1.0, 1.01], &[1.0, 1.0]).unwrap();
        assert_eq!(data.output.len(), 2);
        assert!(data.output[0] > 0.0 && data.output[0] < 1.0);
        assert!(data.output[1] > data.output[0]);
    }

    #[test]
    fn timestamps_must_be_strictly_increasing() {
        let error = filter_uploaded_data(low_pass_config(), &[0.0, 0.0], &[1.0, 2.0]).unwrap_err();
        assert!(error.contains("strictly increasing"));
    }

    #[test]
    fn band_pass_rejects_odd_final_order() {
        let mut config = low_pass_config();
        config.topology = Topology::BandPass;
        config.order = 3;
        config.cutoff_hz = None;
        config.lower_cutoff_hz = Some(10.0);
        config.upper_cutoff_hz = Some(40.0);
        assert!(validate_config(config).unwrap_err().contains("even"));
    }
}
