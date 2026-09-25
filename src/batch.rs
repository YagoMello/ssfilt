//! Offline processing of a complete sample sequence.

use core::fmt;

use num_traits::{Float, Zero};

use crate::{StreamingFilter, UpdateError};

/// Error returned by a forward-backward batch operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BatchError {
    /// Input and output slices had different lengths.
    OutputLengthMismatch,
    /// The interval slice did not contain one entry per sample gap.
    IntervalLengthMismatch,
    /// An input sample was NaN or infinite.
    NonFiniteSample,
    /// An interval was non-finite, zero, or negative.
    InvalidInterval,
    /// A filter update failed during one of the two passes.
    Update(UpdateError),
}

impl fmt::Display for BatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputLengthMismatch => formatter.write_str("input and output lengths differ"),
            Self::IntervalLengthMismatch => {
                formatter.write_str("interval count must be one less than the sample count")
            }
            Self::NonFiniteSample => formatter.write_str("all input samples must be finite"),
            Self::InvalidInterval => {
                formatter.write_str("sample intervals must be finite and positive")
            }
            Self::Update(error) => write!(formatter, "filter update failed: {error}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for BatchError {}

/// Filters a complete signal forward and backward using its sample intervals.
///
/// `intervals_seconds[i]` is the elapsed time between `input[i]` and
/// `input[i + 1]`. The input and filter are preserved. The output must have the
/// same length as the input; an empty input is valid and requires no intervals.
/// Each pass starts at the steady state of its first sample. This simple
/// boundary condition can cause transients near both ends of a finite signal.
///
/// Away from the ends, forward-backward filtering cancels phase delay. For a
/// uniformly sampled linear signal, its magnitude response is the squared
/// magnitude of the one-pass response. It is noncausal and needs the complete
/// input signal before producing an output.
///
/// # Errors
///
/// Invalid lengths, samples, and intervals are rejected before `output` is
/// changed. If numerical integration fails during a pass, `output` may contain
/// a partially filtered signal; the supplied `filter` and `input` remain
/// unchanged.
pub fn forward_backward_into<F>(
    filter: &F,
    input: &[F::Scalar],
    intervals_seconds: &[F::Scalar],
    output: &mut [F::Scalar],
) -> Result<(), BatchError>
where
    F: StreamingFilter + Clone,
{
    if input.len() != output.len() {
        return Err(BatchError::OutputLengthMismatch);
    }
    if intervals_seconds.len() != input.len().saturating_sub(1) {
        return Err(BatchError::IntervalLengthMismatch);
    }
    if !input.iter().all(|sample| sample.is_finite()) {
        return Err(BatchError::NonFiniteSample);
    }
    if !intervals_seconds
        .iter()
        .all(|interval| interval.is_finite() && *interval > F::Scalar::zero())
    {
        return Err(BatchError::InvalidInterval);
    }

    output.copy_from_slice(input);
    forward_backward_validated(filter, output, |index| intervals_seconds[index])
}

/// Filters a complete, uniformly sampled signal forward and backward.
///
/// This convenience form uses `dt_seconds` for every sample gap. A single
/// sample still requires a finite, positive interval for a consistent API.
/// See [`forward_backward_into`] for boundary and error behavior.
///
/// # Errors
///
/// Returns [`BatchError`] for invalid lengths, samples, intervals, or failed
/// integration.
pub fn forward_backward_uniform_into<F>(
    filter: &F,
    input: &[F::Scalar],
    dt_seconds: F::Scalar,
    output: &mut [F::Scalar],
) -> Result<(), BatchError>
where
    F: StreamingFilter + Clone,
{
    if input.len() != output.len() {
        return Err(BatchError::OutputLengthMismatch);
    }
    if !dt_seconds.is_finite() || dt_seconds <= F::Scalar::zero() {
        return Err(BatchError::InvalidInterval);
    }
    if !input.iter().all(|sample| sample.is_finite()) {
        return Err(BatchError::NonFiniteSample);
    }

    output.copy_from_slice(input);
    forward_backward_validated(filter, output, |_| dt_seconds)
}

fn forward_backward_validated<F>(
    filter: &F,
    output: &mut [F::Scalar],
    interval_at: impl Fn(usize) -> F::Scalar,
) -> Result<(), BatchError>
where
    F: StreamingFilter + Clone,
{
    let Some(&first) = output.first() else {
        return Ok(());
    };

    let mut working = filter.clone();
    working
        .reset_to_steady(first)
        .map_err(|_| BatchError::NonFiniteSample)?;
    output[0] = working.output();
    for (index, sample) in output.iter_mut().enumerate().skip(1) {
        *sample = working
            .update(*sample, interval_at(index - 1))
            .map_err(BatchError::Update)?;
    }

    working
        .reset_to_steady(output[output.len() - 1])
        .map_err(|_| BatchError::NonFiniteSample)?;
    output[output.len() - 1] = working.output();
    for index in (0..output.len().saturating_sub(1)).rev() {
        output[index] = working
            .update(output[index], interval_at(index))
            .map_err(BatchError::Update)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use approx::assert_relative_eq;

    use super::*;
    use crate::{HighPass, InputModel, LowPass, Response};

    #[test]
    fn first_order_held_impulse_has_symmetric_squared_response_away_from_edges() {
        let filter = LowPass::<1>::builder(2.0)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        let mut input = [0.0; 801];
        input[400] = 1.0;
        let mut output = [0.0; 801];
        forward_backward_uniform_into(&filter, &input, 0.005, &mut output).unwrap();

        for offset in 1..=20 {
            assert_relative_eq!(
                output[400 - offset],
                output[400 + offset],
                epsilon = 1.0e-10
            );
        }
        assert!(output[400] > output[399]);
        assert_eq!(input[400], 1.0);
        assert_eq!(filter.output(), 0.0);
    }

    #[test]
    fn irregular_intervals_use_the_correct_gap_in_each_direction() {
        let filter = LowPass::<1>::builder(1.0)
            .input_model(InputModel::Linear)
            .build()
            .unwrap();
        let input = [0.0, 1.0, 0.0, -1.0, 0.0];
        let intervals = [0.01, 0.02, 0.05, 0.03];
        let mut output = [0.0; 5];
        forward_backward_into(&filter, &input, &intervals, &mut output).unwrap();

        let mut manual = filter;
        manual.reset_to_steady(input[0]).unwrap();
        let mut forward = [0.0; 5];
        forward[0] = manual.output();
        for index in 1..input.len() {
            forward[index] = manual.update(input[index], intervals[index - 1]).unwrap();
        }
        manual.reset_to_steady(forward[4]).unwrap();
        let mut expected = [0.0; 5];
        expected[4] = manual.output();
        for index in (0..4).rev() {
            expected[index] = manual.update(forward[index], intervals[index]).unwrap();
        }
        assert_eq!(output, expected);
    }

    #[test]
    fn constant_signals_preserve_steady_outputs() {
        let low = LowPass::<4>::builder(10.0)
            .response(Response::Bessel)
            .build()
            .unwrap();
        let high = HighPass::<3>::builder(10.0)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let samples = [2.5; 20];
        let mut output = [f64::NAN; 20];

        forward_backward_uniform_into(&low, &samples, 0.01, &mut output).unwrap();
        assert!(output.iter().all(|&sample| sample == 2.5));
        forward_backward_uniform_into(&high, &samples, 0.01, &mut output).unwrap();
        assert!(output.iter().all(|&sample| sample == 0.0));
    }

    #[test]
    fn validation_does_not_modify_output() {
        let filter = LowPass::<2>::builder(1.0).build().unwrap();
        let mut output = [9.0; 3];
        assert_eq!(
            forward_backward_into(&filter, &[1.0, f64::NAN, 3.0], &[0.1, 0.1], &mut output),
            Err(BatchError::NonFiniteSample)
        );
        assert_eq!(output, [9.0; 3]);
        assert_eq!(
            forward_backward_into(&filter, &[1.0, 2.0, 3.0], &[0.1, 0.0], &mut output),
            Err(BatchError::InvalidInterval)
        );
        assert_eq!(output, [9.0; 3]);
    }
}
