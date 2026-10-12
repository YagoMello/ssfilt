//! Time-weighted oracle convergence checks and derived-record helpers.

use core::fmt;

use rug::Float;

use crate::{
    OracleOutput, RecordError, RecordScalar, RepresentedRecord, RepresentedValueField,
    ScalarRefinement, evaluate,
};

/// Minimum precision accepted by the independent oracle assessment helpers.
pub const MINIMUM_ASSESSMENT_PRECISION: u32 = 192;

/// Precision used for the lower-resolution convergence run.
pub const LOW_ORACLE_PRECISION: u32 = 192;

/// Precision used for the canonical oracle reference.
pub const HIGH_ORACLE_PRECISION: u32 = 256;

const CONVERGENCE_RMS_LIMIT: (u32, u32) = (2, 13);
const CONVERGENCE_PEAK_LIMIT: (u32, u32) = (1, 12);
const IMAGINARY_RATIO_LIMIT: (u32, u32) = (1, 30);

/// Identifies which measured series contained an invalid value or length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeriesSide {
    Actual,
    Reference,
}

/// Identifies one of the two fresh oracle evaluations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OracleSide {
    LowPrecision,
    HighPrecision,
}

/// The metric that failed the frozen oracle-integrity gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CertificationGate {
    ConvergenceRms,
    ConvergencePeak,
    LowPrecisionImaginaryRatio,
    HighPrecisionImaginaryRatio,
}

/// A duration or intermediate numerical failure while calculating metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArithmeticOperation {
    Difference,
    AbsoluteError,
    ErrorSquare,
    PairWeight,
    WeightedSum,
    DurationSum,
    MeanSquaredError,
    SquareRoot,
    Normalization,
    ImaginaryRatio,
}

/// A derived output buffer that could not reserve its checked size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssessmentBuffer {
    ReversedIntervals,
    ReversedSamples,
    OriginalKnots,
}

/// Why a total represented duration could not be used as an RMS weight.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurationIssue {
    Zero,
    NonFinite,
}

/// Contextual failure while assessing an oracle or constructing a derived view.
#[derive(Clone, Debug, PartialEq)]
pub struct AssessmentError {
    coordinate: Option<crate::CaseCoordinate>,
    kind: AssessmentErrorKind,
}

impl AssessmentError {
    fn new(kind: AssessmentErrorKind) -> Self {
        Self {
            coordinate: None,
            kind,
        }
    }

    fn at(coordinate: crate::CaseCoordinate, kind: AssessmentErrorKind) -> Self {
        Self {
            coordinate: Some(coordinate),
            kind,
        }
    }

    /// The represented case coordinate when the failure came from a derived case.
    #[must_use]
    pub const fn coordinate(&self) -> Option<crate::CaseCoordinate> {
        self.coordinate
    }

    /// The precise failure category and its indexed details.
    #[must_use]
    pub const fn kind(&self) -> &AssessmentErrorKind {
        &self.kind
    }
}

/// Assessment failures keep record, series, oracle, and certification problems distinct.
#[derive(Clone, Debug, PartialEq)]
pub enum AssessmentErrorKind {
    UnsupportedOrder {
        order: usize,
    },
    LowPrecision {
        requested: u32,
        minimum: u32,
    },
    RecordShape {
        interval_count: usize,
        sample_count: usize,
    },
    SeriesLength {
        side: SeriesSide,
        expected: usize,
        actual: usize,
    },
    NonFiniteValue {
        side: SeriesSide,
        index: usize,
    },
    InvalidInterval {
        interval_index: usize,
        value: f64,
    },
    InvalidTotalDuration {
        issue: DurationIssue,
    },
    NonFiniteArithmetic {
        operation: ArithmeticOperation,
        index: Option<usize>,
    },
    NonExactF32Storage {
        field: RepresentedValueField,
        index: Option<usize>,
        value: f64,
    },
    RecordSource(RecordError),
    CapacityReservationFailed {
        buffer: AssessmentBuffer,
        requested_capacity: usize,
    },
    RefinedLength {
        expected: usize,
        actual: usize,
    },
    RefinedMapping {
        original_index: usize,
        mapped_index: Option<usize>,
        refined_length: usize,
    },
    OracleShape {
        side: OracleSide,
        expected: usize,
        actual: usize,
    },
    OraclePrecision {
        side: OracleSide,
        expected: u32,
        actual: u32,
    },
    OracleNonFiniteValue {
        side: OracleSide,
        index: usize,
    },
    OracleNonFiniteImaginary {
        side: OracleSide,
    },
    CertificationGateFailed {
        gate: CertificationGate,
        value: Float,
        limit: Float,
    },
}

impl fmt::Display for AssessmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(coordinate) = self.coordinate {
            write!(formatter, "assessment failed for {coordinate:?}: ")?;
        } else {
            formatter.write_str("assessment failed: ")?;
        }
        match &self.kind {
            AssessmentErrorKind::UnsupportedOrder { order } => {
                write!(formatter, "oracle order {order} is unsupported")
            }
            AssessmentErrorKind::LowPrecision { requested, minimum } => write!(
                formatter,
                "precision {requested} is below the minimum of {minimum} bits"
            ),
            AssessmentErrorKind::RecordShape {
                interval_count,
                sample_count,
            } => write!(
                formatter,
                "record shape has {interval_count} intervals and {sample_count} samples"
            ),
            AssessmentErrorKind::SeriesLength {
                side,
                expected,
                actual,
            } => write!(
                formatter,
                "{side:?} series has {actual} values; expected exactly {expected}"
            ),
            AssessmentErrorKind::NonFiniteValue { side, index } => {
                write!(formatter, "{side:?} value {index} is not finite")
            }
            AssessmentErrorKind::InvalidInterval {
                interval_index,
                value,
            } => write!(
                formatter,
                "represented interval {interval_index} must be finite and positive, got {value}"
            ),
            AssessmentErrorKind::InvalidTotalDuration { issue } => {
                write!(formatter, "total represented duration is {issue:?}")
            }
            AssessmentErrorKind::NonFiniteArithmetic { operation, index } => match index {
                Some(index) => write!(
                    formatter,
                    "{operation:?} arithmetic at index {index} is not finite"
                ),
                None => write!(formatter, "{operation:?} arithmetic is not finite"),
            },
            AssessmentErrorKind::NonExactF32Storage {
                field,
                index,
                value,
            } => match index {
                Some(index) => write!(
                    formatter,
                    "represented {field:?} {index} ({value}) does not round-trip exactly through f32"
                ),
                None => write!(
                    formatter,
                    "represented {field:?} ({value}) does not round-trip exactly through f32"
                ),
            },
            AssessmentErrorKind::RecordSource(source) => {
                write!(formatter, "represented-record validation failed: {source}")
            }
            AssessmentErrorKind::CapacityReservationFailed {
                buffer,
                requested_capacity,
            } => write!(
                formatter,
                "could not reserve {requested_capacity} entries for the {buffer:?} buffer"
            ),
            AssessmentErrorKind::RefinedLength { expected, actual } => write!(
                formatter,
                "refined values have length {actual}; expected exactly {expected}"
            ),
            AssessmentErrorKind::RefinedMapping {
                original_index,
                mapped_index,
                refined_length,
            } => write!(
                formatter,
                "original knot {original_index} maps to {mapped_index:?}, outside {refined_length} refined values"
            ),
            AssessmentErrorKind::OracleShape {
                side,
                expected,
                actual,
            } => write!(
                formatter,
                "{side:?} oracle output has {actual} values; expected {expected}"
            ),
            AssessmentErrorKind::OraclePrecision {
                side,
                expected,
                actual,
            } => write!(
                formatter,
                "{side:?} oracle output has {actual} bits of precision; expected {expected}"
            ),
            AssessmentErrorKind::OracleNonFiniteValue { side, index } => {
                write!(
                    formatter,
                    "{side:?} oracle output value {index} is not finite"
                )
            }
            AssessmentErrorKind::OracleNonFiniteImaginary { side } => {
                write!(
                    formatter,
                    "{side:?} oracle imaginary residual is not finite"
                )
            }
            AssessmentErrorKind::CertificationGateFailed { gate, value, limit } => write!(
                formatter,
                "oracle certification gate {gate:?} measured {value}; limit is {limit}"
            ),
        }
    }
}

impl std::error::Error for AssessmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            AssessmentErrorKind::RecordSource(source) => Some(source),
            _ => None,
        }
    }
}

/// Absolute and record-scale-normalized time-weighted error metrics.
#[derive(Clone, Debug)]
pub struct ErrorMetrics {
    absolute_rms: Float,
    normalized_rms: Float,
    absolute_peak: Float,
    normalized_peak: Float,
}

impl ErrorMetrics {
    /// Time-weighted RMS in the units of the represented samples.
    #[must_use]
    pub const fn absolute_rms(&self) -> &Float {
        &self.absolute_rms
    }

    /// Time-weighted RMS divided by the represented record scale.
    #[must_use]
    pub const fn normalized_rms(&self) -> &Float {
        &self.normalized_rms
    }

    /// Endpoint-inclusive maximum absolute error in sample units.
    #[must_use]
    pub const fn absolute_peak(&self) -> &Float {
        &self.absolute_peak
    }

    /// Endpoint-inclusive maximum error divided by the represented record scale.
    #[must_use]
    pub const fn normalized_peak(&self) -> &Float {
        &self.normalized_peak
    }
}

/// Compute trapezoidal time-weighted RMS and endpoint-inclusive peak error.
///
/// Interval weights are the exact promoted binary values stored in the
/// represented record. The operation uses constant scratch space regardless
/// of record length. A singleton has no duration weights, so both absolute
/// RMS and peak equal its one-sample absolute error.
pub fn weighted_error(
    record: &RepresentedRecord,
    actual: &[Float],
    reference: &[Float],
    precision: u32,
) -> Result<ErrorMetrics, AssessmentError> {
    if precision < MINIMUM_ASSESSMENT_PRECISION {
        return Err(AssessmentError::new(AssessmentErrorKind::LowPrecision {
            requested: precision,
            minimum: MINIMUM_ASSESSMENT_PRECISION,
        }));
    }

    let sample_count = record.samples().len();
    if sample_count == 0 || record.intervals_seconds().len().checked_add(1) != Some(sample_count) {
        return Err(AssessmentError::new(AssessmentErrorKind::RecordShape {
            interval_count: record.intervals_seconds().len(),
            sample_count,
        }));
    }
    if actual.len() != sample_count {
        return Err(AssessmentError::new(AssessmentErrorKind::SeriesLength {
            side: SeriesSide::Actual,
            expected: sample_count,
            actual: actual.len(),
        }));
    }
    if reference.len() != sample_count {
        return Err(AssessmentError::new(AssessmentErrorKind::SeriesLength {
            side: SeriesSide::Reference,
            expected: sample_count,
            actual: reference.len(),
        }));
    }

    for (index, value) in actual.iter().enumerate() {
        if !value.is_finite() {
            return Err(AssessmentError::new(AssessmentErrorKind::NonFiniteValue {
                side: SeriesSide::Actual,
                index,
            }));
        }
    }
    for (index, value) in reference.iter().enumerate() {
        if !value.is_finite() {
            return Err(AssessmentError::new(AssessmentErrorKind::NonFiniteValue {
                side: SeriesSide::Reference,
                index,
            }));
        }
    }

    let scale = record.scale(precision);
    if !scale.is_finite() || scale <= 0 {
        return Err(AssessmentError::new(
            AssessmentErrorKind::NonFiniteArithmetic {
                operation: ArithmeticOperation::Normalization,
                index: None,
            },
        ));
    }

    let mut maximum_error = Float::new(precision);
    let mut previous_error: Option<Float> = None;
    let mut weighted_sum = Float::new(precision);
    let mut total_duration = Float::new(precision);

    for index in 0..sample_count {
        let promoted_actual = Float::with_val(precision, &actual[index]);
        let promoted_reference = Float::with_val(precision, &reference[index]);
        let difference = Float::with_val(precision, promoted_actual - promoted_reference);
        if !difference.is_finite() {
            return Err(AssessmentError::new(
                AssessmentErrorKind::NonFiniteArithmetic {
                    operation: ArithmeticOperation::Difference,
                    index: Some(index),
                },
            ));
        }
        let absolute_error = difference.abs();
        if !absolute_error.is_finite() {
            return Err(AssessmentError::new(
                AssessmentErrorKind::NonFiniteArithmetic {
                    operation: ArithmeticOperation::AbsoluteError,
                    index: Some(index),
                },
            ));
        }
        if absolute_error > maximum_error {
            maximum_error = absolute_error.clone();
        }

        if let Some(previous) = previous_error.as_ref() {
            let interval_index = index - 1;
            let duration_value = record.intervals_seconds()[interval_index];
            let duration = interval_weight(duration_value, interval_index, precision)?;

            let previous_square = Float::with_val(precision, previous * previous);
            let current_square = Float::with_val(precision, &absolute_error * &absolute_error);
            if !previous_square.is_finite() || !current_square.is_finite() {
                return Err(AssessmentError::new(
                    AssessmentErrorKind::NonFiniteArithmetic {
                        operation: ArithmeticOperation::ErrorSquare,
                        index: Some(interval_index),
                    },
                ));
            }

            let mut endpoint_mean = Float::with_val(precision, previous_square + current_square);
            endpoint_mean /= 2;
            let interval_error = Float::with_val(precision, endpoint_mean * &duration);
            if !interval_error.is_finite() {
                return Err(AssessmentError::new(
                    AssessmentErrorKind::NonFiniteArithmetic {
                        operation: ArithmeticOperation::PairWeight,
                        index: Some(interval_index),
                    },
                ));
            }
            weighted_sum += interval_error;
            if !weighted_sum.is_finite() {
                return Err(AssessmentError::new(
                    AssessmentErrorKind::NonFiniteArithmetic {
                        operation: ArithmeticOperation::WeightedSum,
                        index: Some(interval_index),
                    },
                ));
            }
            total_duration += duration;
            if !total_duration.is_finite() {
                return Err(AssessmentError::new(
                    AssessmentErrorKind::NonFiniteArithmetic {
                        operation: ArithmeticOperation::DurationSum,
                        index: Some(interval_index),
                    },
                ));
            }
        }
        previous_error = Some(absolute_error);
    }

    let absolute_rms = if sample_count == 1 {
        maximum_error.clone()
    } else {
        if !total_duration.is_finite() {
            return Err(AssessmentError::new(
                AssessmentErrorKind::InvalidTotalDuration {
                    issue: DurationIssue::NonFinite,
                },
            ));
        }
        if total_duration == 0 {
            return Err(AssessmentError::new(
                AssessmentErrorKind::InvalidTotalDuration {
                    issue: DurationIssue::Zero,
                },
            ));
        }
        let mut mean_squared = Float::with_val(precision, &weighted_sum / &total_duration);
        if !mean_squared.is_finite() || mean_squared < 0 {
            return Err(AssessmentError::new(
                AssessmentErrorKind::NonFiniteArithmetic {
                    operation: ArithmeticOperation::MeanSquaredError,
                    index: None,
                },
            ));
        }
        mean_squared.sqrt_mut();
        if !mean_squared.is_finite() {
            return Err(AssessmentError::new(
                AssessmentErrorKind::NonFiniteArithmetic {
                    operation: ArithmeticOperation::SquareRoot,
                    index: None,
                },
            ));
        }
        mean_squared
    };

    let normalized_rms = Float::with_val(precision, &absolute_rms / &scale);
    let normalized_peak = Float::with_val(precision, &maximum_error / &scale);
    if !normalized_rms.is_finite() || !normalized_peak.is_finite() {
        return Err(AssessmentError::new(
            AssessmentErrorKind::NonFiniteArithmetic {
                operation: ArithmeticOperation::Normalization,
                index: None,
            },
        ));
    }

    Ok(ErrorMetrics {
        absolute_rms,
        normalized_rms,
        absolute_peak: maximum_error,
        normalized_peak,
    })
}

/// Fresh 192-bit and 256-bit oracle outputs and their convergence assessment.
#[derive(Clone, Debug)]
pub struct OracleAssessment {
    order: usize,
    low_output: OracleOutput,
    high_output: OracleOutput,
    convergence: ErrorMetrics,
    low_imaginary_ratio: Float,
    high_imaginary_ratio: Float,
}

impl OracleAssessment {
    /// Per-pass Butterworth order used by both fresh oracle evaluations.
    #[must_use]
    pub const fn order(&self) -> usize {
        self.order
    }

    /// Fresh 192-bit oracle output.
    #[must_use]
    pub const fn low_output(&self) -> &OracleOutput {
        &self.low_output
    }

    /// Fresh 256-bit oracle output, which becomes canonical after certification.
    #[must_use]
    pub const fn high_output(&self) -> &OracleOutput {
        &self.high_output
    }

    /// Error metrics comparing the low output promoted to 256 bits against high.
    #[must_use]
    pub const fn convergence(&self) -> &ErrorMetrics {
        &self.convergence
    }

    /// Low-output maximum imaginary residual divided by the represented scale.
    #[must_use]
    pub const fn low_imaginary_ratio(&self) -> &Float {
        &self.low_imaginary_ratio
    }

    /// High-output maximum imaginary residual divided by the represented scale.
    #[must_use]
    pub const fn high_imaginary_ratio(&self) -> &Float {
        &self.high_imaginary_ratio
    }

    /// Check the frozen convergence and imaginary-residual integrity gates.
    pub fn certify(self) -> Result<CertifiedOracle, AssessmentError> {
        check_certification(&self)?;
        Ok(CertifiedOracle { assessment: self })
    }
}

/// An oracle assessment that passed every frozen integrity gate.
#[derive(Clone, Debug)]
pub struct CertifiedOracle {
    assessment: OracleAssessment,
}

impl CertifiedOracle {
    /// Return the canonical 256-bit reference output.
    #[must_use]
    pub const fn reference(&self) -> &OracleOutput {
        self.assessment.high_output()
    }

    /// Return the complete assessment and its low-precision diagnostic output.
    #[must_use]
    pub const fn assessment(&self) -> &OracleAssessment {
        &self.assessment
    }
}

/// Evaluate a fresh supported-order 192/256-bit pair and compute its convergence metrics.
pub fn assess_oracle(
    order: usize,
    record: &RepresentedRecord,
) -> Result<OracleAssessment, AssessmentError> {
    if !matches!(order, 1 | 2 | 4 | 8) {
        return Err(AssessmentError::new(
            AssessmentErrorKind::UnsupportedOrder { order },
        ));
    }
    validate_record_shape(record)?;

    let low_output = evaluate(order, record, LOW_ORACLE_PRECISION);
    let high_output = evaluate(order, record, HIGH_ORACLE_PRECISION);
    validate_oracle_output(
        &low_output,
        OracleSide::LowPrecision,
        LOW_ORACLE_PRECISION,
        record.samples().len(),
    )?;
    validate_oracle_output(
        &high_output,
        OracleSide::HighPrecision,
        HIGH_ORACLE_PRECISION,
        record.samples().len(),
    )?;

    let convergence = weighted_error(
        record,
        low_output.values(),
        high_output.values(),
        HIGH_ORACLE_PRECISION,
    )?;
    let scale = record.scale(HIGH_ORACLE_PRECISION);
    let low_imaginary_ratio =
        normalized_imaginary_ratio(&low_output, &scale, OracleSide::LowPrecision)?;
    let high_imaginary_ratio =
        normalized_imaginary_ratio(&high_output, &scale, OracleSide::HighPrecision)?;

    Ok(OracleAssessment {
        order,
        low_output,
        high_output,
        convergence,
        low_imaginary_ratio,
        high_imaginary_ratio,
    })
}

/// Evaluate and certify an oracle pair, returning its canonical 256-bit output.
pub fn certify_oracle(
    order: usize,
    record: &RepresentedRecord,
) -> Result<CertifiedOracle, AssessmentError> {
    assess_oracle(order, record)?.certify()
}

fn validate_record_shape(record: &RepresentedRecord) -> Result<(), AssessmentError> {
    if record.samples().is_empty()
        || record.intervals_seconds().len().checked_add(1) != Some(record.samples().len())
    {
        return Err(AssessmentError::new(AssessmentErrorKind::RecordShape {
            interval_count: record.intervals_seconds().len(),
            sample_count: record.samples().len(),
        }));
    }
    Ok(())
}

fn validate_oracle_output(
    output: &OracleOutput,
    side: OracleSide,
    expected_precision: u32,
    expected_length: usize,
) -> Result<(), AssessmentError> {
    if output.precision() != expected_precision {
        return Err(AssessmentError::new(AssessmentErrorKind::OraclePrecision {
            side,
            expected: expected_precision,
            actual: output.precision(),
        }));
    }
    if output.values().len() != expected_length {
        return Err(AssessmentError::new(AssessmentErrorKind::OracleShape {
            side,
            expected: expected_length,
            actual: output.values().len(),
        }));
    }
    for (index, value) in output.values().iter().enumerate() {
        if !value.is_finite() {
            return Err(AssessmentError::new(
                AssessmentErrorKind::OracleNonFiniteValue { side, index },
            ));
        }
    }
    if !output.maximum_imaginary_residual().is_finite() {
        return Err(AssessmentError::new(
            AssessmentErrorKind::OracleNonFiniteImaginary { side },
        ));
    }
    Ok(())
}

fn normalized_imaginary_ratio(
    output: &OracleOutput,
    scale: &Float,
    side: OracleSide,
) -> Result<Float, AssessmentError> {
    let precision = HIGH_ORACLE_PRECISION;
    let residual = Float::with_val(precision, output.maximum_imaginary_residual());
    let ratio = Float::with_val(precision, residual / scale);
    if !ratio.is_finite() {
        return Err(AssessmentError::new(
            AssessmentErrorKind::NonFiniteArithmetic {
                operation: ArithmeticOperation::ImaginaryRatio,
                index: None,
            },
        ));
    }
    if ratio < 0 {
        return Err(AssessmentError::new(
            AssessmentErrorKind::OracleNonFiniteImaginary { side },
        ));
    }
    Ok(ratio)
}

fn interval_weight(
    value: f64,
    interval_index: usize,
    precision: u32,
) -> Result<Float, AssessmentError> {
    let duration = Float::with_val(precision, value);
    if !duration.is_finite() || duration <= 0 {
        return Err(AssessmentError::new(AssessmentErrorKind::InvalidInterval {
            interval_index,
            value,
        }));
    }
    Ok(duration)
}

fn decimal_limit(precision: u32, numerator: u32, decimal_power: u32) -> Float {
    let mut denominator = Float::with_val(precision, 1);
    for _ in 0..decimal_power {
        denominator *= 10;
    }
    let mut limit = Float::with_val(precision, numerator);
    limit /= denominator;
    limit
}

fn check_certification(assessment: &OracleAssessment) -> Result<(), AssessmentError> {
    let precision = HIGH_ORACLE_PRECISION;
    let gates = [
        (
            CertificationGate::ConvergenceRms,
            assessment.convergence.normalized_rms(),
            CONVERGENCE_RMS_LIMIT,
        ),
        (
            CertificationGate::ConvergencePeak,
            assessment.convergence.normalized_peak(),
            CONVERGENCE_PEAK_LIMIT,
        ),
        (
            CertificationGate::LowPrecisionImaginaryRatio,
            &assessment.low_imaginary_ratio,
            IMAGINARY_RATIO_LIMIT,
        ),
        (
            CertificationGate::HighPrecisionImaginaryRatio,
            &assessment.high_imaginary_ratio,
            IMAGINARY_RATIO_LIMIT,
        ),
    ];

    for (gate, value, (numerator, decimal_power)) in gates {
        let limit = decimal_limit(precision, numerator, decimal_power);
        if value > &limit {
            return Err(AssessmentError::new(
                AssessmentErrorKind::CertificationGateFailed {
                    gate,
                    value: value.clone(),
                    limit,
                },
            ));
        }
    }
    Ok(())
}

/// A record with both intervals and samples reversed from its source.
#[derive(Clone, Debug)]
pub struct ReversedRecord {
    record: RepresentedRecord,
    sample_count: usize,
}

impl ReversedRecord {
    /// The validated represented record with interval and sample order reversed.
    #[must_use]
    pub const fn record(&self) -> &RepresentedRecord {
        &self.record
    }

    /// Number of knots in the reversed record.
    #[must_use]
    pub const fn sample_count(&self) -> usize {
        self.sample_count
    }

    /// Map a source knot index to the corresponding reversed knot index.
    #[must_use]
    pub fn reversed_index_for_original(&self, original_index: usize) -> Option<usize> {
        reversed_index(self.sample_count, original_index)
    }

    /// Map a reversed knot index back to its source knot index.
    #[must_use]
    pub fn original_index_for_reversed(&self, index: usize) -> Option<usize> {
        reversed_index(self.sample_count, index)
    }
}

/// Construct a reversed record while preserving every represented scalar bit.
pub fn reverse_record(record: &RepresentedRecord) -> Result<ReversedRecord, AssessmentError> {
    validate_record_shape(record)?;
    let sample_count = record.samples().len();
    let reversed = match record.scalar() {
        RecordScalar::F32 => reverse_f32_record(record)?,
        RecordScalar::F64 => reverse_f64_record(record)?,
    };
    Ok(ReversedRecord {
        record: reversed,
        sample_count,
    })
}

fn reverse_f32_record(record: &RepresentedRecord) -> Result<RepresentedRecord, AssessmentError> {
    let cutoff = exact_f32(record.cutoff_hz(), RepresentedValueField::Cutoff, None)?;
    for (index, value) in record.intervals_seconds().iter().copied().enumerate() {
        exact_f32(value, RepresentedValueField::Interval, Some(index))?;
    }
    for (index, value) in record.samples().iter().copied().enumerate() {
        exact_f32(value, RepresentedValueField::Sample, Some(index))?;
    }

    let mut intervals = Vec::new();
    intervals
        .try_reserve_exact(record.intervals_seconds().len())
        .map_err(|_| {
            AssessmentError::new(AssessmentErrorKind::CapacityReservationFailed {
                buffer: AssessmentBuffer::ReversedIntervals,
                requested_capacity: record.intervals_seconds().len(),
            })
        })?;
    for (reversed_index, value) in record.intervals_seconds().iter().rev().copied().enumerate() {
        let source_index = record.intervals_seconds().len() - 1 - reversed_index;
        intervals.push(exact_f32(
            value,
            RepresentedValueField::Interval,
            Some(source_index),
        )?);
    }
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(record.samples().len())
        .map_err(|_| {
            AssessmentError::new(AssessmentErrorKind::CapacityReservationFailed {
                buffer: AssessmentBuffer::ReversedSamples,
                requested_capacity: record.samples().len(),
            })
        })?;
    for (reversed_index, value) in record.samples().iter().rev().copied().enumerate() {
        let source_index = record.samples().len() - 1 - reversed_index;
        samples.push(exact_f32(
            value,
            RepresentedValueField::Sample,
            Some(source_index),
        )?);
    }
    RepresentedRecord::from_f32(cutoff, intervals, samples)
        .map_err(|source| AssessmentError::new(AssessmentErrorKind::RecordSource(source)))
}

fn reverse_f64_record(record: &RepresentedRecord) -> Result<RepresentedRecord, AssessmentError> {
    let mut intervals = Vec::new();
    intervals
        .try_reserve_exact(record.intervals_seconds().len())
        .map_err(|_| {
            AssessmentError::new(AssessmentErrorKind::CapacityReservationFailed {
                buffer: AssessmentBuffer::ReversedIntervals,
                requested_capacity: record.intervals_seconds().len(),
            })
        })?;
    intervals.extend(record.intervals_seconds().iter().rev().copied());

    let mut samples = Vec::new();
    samples
        .try_reserve_exact(record.samples().len())
        .map_err(|_| {
            AssessmentError::new(AssessmentErrorKind::CapacityReservationFailed {
                buffer: AssessmentBuffer::ReversedSamples,
                requested_capacity: record.samples().len(),
            })
        })?;
    samples.extend(record.samples().iter().rev().copied());

    RepresentedRecord::from_f64(record.cutoff_hz(), intervals, samples)
        .map_err(|source| AssessmentError::new(AssessmentErrorKind::RecordSource(source)))
}

fn exact_f32(
    value: f64,
    field: RepresentedValueField,
    index: Option<usize>,
) -> Result<f32, AssessmentError> {
    let narrowed = value as f32;
    if !narrowed.is_finite() || f64::from(narrowed).to_bits() != value.to_bits() {
        return Err(AssessmentError::new(
            AssessmentErrorKind::NonExactF32Storage {
                field,
                index,
                value,
            },
        ));
    }
    Ok(narrowed)
}

fn reversed_index(sample_count: usize, index: usize) -> Option<usize> {
    if index >= sample_count {
        return None;
    }
    sample_count.checked_sub(1)?.checked_sub(index)
}

/// Copy only the original knots from a scalar-refined value series.
pub fn extract_original_knots<T: Clone>(
    refinement: &ScalarRefinement,
    refined_values: &[T],
) -> Result<Vec<T>, AssessmentError> {
    let expected_length = refinement.record().samples().len();
    if refined_values.len() != expected_length {
        return Err(AssessmentError::at(
            refinement.coordinate(),
            AssessmentErrorKind::RefinedLength {
                expected: expected_length,
                actual: refined_values.len(),
            },
        ));
    }

    let original_count = refinement.original_sample_count();
    let mut originals = Vec::new();
    originals.try_reserve_exact(original_count).map_err(|_| {
        AssessmentError::at(
            refinement.coordinate(),
            AssessmentErrorKind::CapacityReservationFailed {
                buffer: AssessmentBuffer::OriginalKnots,
                requested_capacity: original_count,
            },
        )
    })?;

    for original_index in 0..original_count {
        let refined_index = refinement.refined_index_for_original(original_index);
        let Some(refined_index) = refined_index.filter(|index| *index < refined_values.len())
        else {
            return Err(AssessmentError::at(
                refinement.coordinate(),
                AssessmentErrorKind::RefinedMapping {
                    original_index,
                    mapped_index: refined_index,
                    refined_length: refined_values.len(),
                },
            ));
        };
        originals.push(refined_values[refined_index].clone());
    }

    Ok(originals)
}

#[cfg(test)]
mod tests {
    use rug::Float;

    use super::*;
    use crate::{
        PhysicalCutoff, QMax, ScheduleKind, ToneFrequency, ToneOrder, TonePhase,
        build_nominal_tone_record, build_represented_tone_case, refine_represented_case,
    };

    const PRECISION: u32 = 256;

    fn float_values(values: &[f64]) -> Vec<Float> {
        values
            .iter()
            .map(|value| Float::with_val(PRECISION, *value))
            .collect()
    }

    fn sample_record() -> RepresentedRecord {
        RepresentedRecord::from_f64(1.0, vec![1.0, 3.0], vec![2.0, 0.0, 1.0]).unwrap()
    }

    #[test]
    fn unequal_intervals_use_the_hand_calculated_trapezoid() {
        let record = sample_record();
        let actual = float_values(&[2.0, 2.0, 1.0]);
        let reference = float_values(&[2.0, 0.0, 1.0]);

        let metrics = weighted_error(&record, &actual, &reference, PRECISION).unwrap();
        let mut expected_rms = Float::with_val(PRECISION, 2);
        expected_rms.sqrt_mut();

        assert!(
            Float::with_val(PRECISION, metrics.absolute_rms() - &expected_rms).abs()
                < Float::with_val(PRECISION, 1.0e-70)
        );
        assert_eq!(metrics.absolute_peak(), &Float::with_val(PRECISION, 2));
        let mut expected_normalized = expected_rms.clone();
        expected_normalized /= 2;
        assert!(
            Float::with_val(PRECISION, metrics.normalized_rms() - expected_normalized).abs()
                < Float::with_val(PRECISION, 1.0e-70)
        );
        assert_eq!(metrics.normalized_peak(), &Float::with_val(PRECISION, 1));
    }

    #[test]
    fn zero_error_sign_swap_and_common_duration_scaling_are_invariant() {
        let record = sample_record();
        let actual = float_values(&[3.0, 2.0, 4.0]);
        let reference = float_values(&[4.0, 4.0, 4.0]);
        let zero = weighted_error(&record, &reference, &reference, PRECISION).unwrap();
        assert_eq!(zero.absolute_rms(), &Float::with_val(PRECISION, 0));
        assert_eq!(zero.absolute_peak(), &Float::with_val(PRECISION, 0));

        let forward = weighted_error(&record, &actual, &reference, PRECISION).unwrap();
        let reverse = weighted_error(&record, &reference, &actual, PRECISION).unwrap();
        let scaled_record =
            RepresentedRecord::from_f64(1.0, vec![10.0, 30.0], vec![2.0, 0.0, 1.0]).unwrap();
        let scaled = weighted_error(&scaled_record, &actual, &reference, PRECISION).unwrap();

        assert_eq!(forward.absolute_rms(), reverse.absolute_rms());
        assert_eq!(forward.absolute_peak(), reverse.absolute_peak());
        assert_eq!(forward.absolute_rms(), scaled.absolute_rms());
        assert_eq!(forward.normalized_rms(), scaled.normalized_rms());
        assert_eq!(forward.absolute_peak(), scaled.absolute_peak());
    }

    #[test]
    fn singleton_rms_is_its_endpoint_peak() {
        let record = RepresentedRecord::from_f64(1.0, vec![], vec![0.5]).unwrap();
        let actual = float_values(&[0.375]);
        let reference = float_values(&[0.125]);
        let metrics = weighted_error(&record, &actual, &reference, PRECISION).unwrap();

        assert_eq!(metrics.absolute_rms(), &Float::with_val(PRECISION, 0.25));
        assert_eq!(metrics.absolute_rms(), metrics.absolute_peak());
        assert_eq!(metrics.normalized_rms(), metrics.normalized_peak());
    }

    #[test]
    fn f32_weights_are_promoted_exactly_and_scale_uses_its_frozen_floor() {
        let dt0 = 0.1_f32;
        let dt1 = 0.7_f32;
        let record =
            RepresentedRecord::from_f32(37.0, vec![dt0, dt1], vec![0.001, -0.002, 0.003]).unwrap();
        let actual_native = [0.004_f32, 0.006, -0.001];
        let reference_native = [0.001_f32, -0.002, 0.003];
        let actual = actual_native
            .iter()
            .map(|value| Float::with_val(PRECISION, *value))
            .collect::<Vec<_>>();
        let reference = reference_native
            .iter()
            .map(|value| Float::with_val(PRECISION, *value))
            .collect::<Vec<_>>();

        let metrics = weighted_error(&record, &actual, &reference, PRECISION).unwrap();
        let e0 = Float::with_val(PRECISION, &actual[0] - &reference[0]);
        let e1 = Float::with_val(PRECISION, &actual[1] - &reference[1]);
        let e2 = Float::with_val(PRECISION, &actual[2] - &reference[2]);
        let w0 = Float::with_val(PRECISION, f64::from(dt0));
        let w1 = Float::with_val(PRECISION, f64::from(dt1));
        let mut numerator = Float::with_val(PRECISION, &e0 * &e0 + &e1 * &e1);
        numerator /= 2;
        numerator *= &w0;
        let mut second = Float::with_val(PRECISION, &e1 * &e1 + &e2 * &e2);
        second /= 2;
        second *= &w1;
        numerator += second;
        let duration = w0 + w1;
        let mut expected_rms = numerator / &duration;
        expected_rms.sqrt_mut();

        assert_eq!(metrics.absolute_rms(), &expected_rms);
        assert_eq!(record.scale(PRECISION), Float::with_val(PRECISION, 1.0e-2));
        let mut expected_normalized = expected_rms.clone();
        expected_normalized /= Float::with_val(PRECISION, 1.0e-2);
        assert_eq!(metrics.normalized_rms(), &expected_normalized);

        let f64_record =
            RepresentedRecord::from_f64(1.0, vec![0.25], vec![1.0e-4, -2.0e-4]).unwrap();
        assert_eq!(
            f64_record.scale(PRECISION),
            Float::with_val(PRECISION, 1.0e-3)
        );
    }

    #[test]
    fn indexed_input_errors_and_precision_failures_are_distinct() {
        let record = sample_record();
        let values = float_values(&[0.0, 0.0, 0.0]);
        let low_precision = weighted_error(&record, &values, &values, 128).unwrap_err();
        assert!(matches!(
            low_precision.kind(),
            AssessmentErrorKind::LowPrecision {
                requested: 128,
                minimum: 192
            }
        ));

        let actual_length = weighted_error(&record, &values[..2], &values, PRECISION).unwrap_err();
        assert!(matches!(
            actual_length.kind(),
            AssessmentErrorKind::SeriesLength {
                side: SeriesSide::Actual,
                expected: 3,
                actual: 2
            }
        ));
        let reference_length =
            weighted_error(&record, &values, &values[..2], PRECISION).unwrap_err();
        assert!(matches!(
            reference_length.kind(),
            AssessmentErrorKind::SeriesLength {
                side: SeriesSide::Reference,
                expected: 3,
                actual: 2
            }
        ));

        let mut bad_actual = values.clone();
        bad_actual[1] = Float::with_val(PRECISION, f64::NAN);
        assert!(matches!(
            weighted_error(&record, &bad_actual, &values, PRECISION)
                .unwrap_err()
                .kind(),
            AssessmentErrorKind::NonFiniteValue {
                side: SeriesSide::Actual,
                index: 1
            }
        ));
        let mut bad_reference = values.clone();
        bad_reference[2] = Float::with_val(PRECISION, f64::INFINITY);
        assert!(matches!(
            weighted_error(&record, &values, &bad_reference, PRECISION)
                .unwrap_err()
                .kind(),
            AssessmentErrorKind::NonFiniteValue {
                side: SeriesSide::Reference,
                index: 2
            }
        ));
    }

    #[test]
    fn interval_indices_and_exact_f32_narrowing_fail_with_context() {
        for invalid in [0.0, f64::NAN, f64::INFINITY] {
            let error = interval_weight(invalid, 7, PRECISION).unwrap_err();
            assert!(matches!(
                error.kind(),
                AssessmentErrorKind::InvalidInterval {
                    interval_index: 7,
                    ..
                }
            ));
        }

        let error = exact_f32(0.1, RepresentedValueField::Sample, Some(4)).unwrap_err();
        assert!(matches!(
            error.kind(),
            AssessmentErrorKind::NonExactF32Storage {
                field: RepresentedValueField::Sample,
                index: Some(4),
                ..
            }
        ));
    }

    #[test]
    fn representative_assessments_are_fresh_and_certify_all_supported_orders() {
        let records = [
            RepresentedRecord::from_f32(
                37.0,
                vec![0.001_f32, 0.000_25, 0.002, 0.000_75],
                vec![-0.4_f32, 0.7, 1.2, -0.3, 0.1],
            )
            .unwrap(),
            RepresentedRecord::from_f64(
                1.0,
                vec![0.031_25, 0.007_812_5, 0.023_437_5, 0.015_625],
                vec![-0.4, 0.7, 1.2, -0.3, 0.1],
            )
            .unwrap(),
        ];

        for record in &records {
            for order in [1, 2, 4, 8] {
                let assessment = assess_oracle(order, record).unwrap();
                assert_eq!(assessment.order(), order);
                assert_eq!(assessment.low_output().precision(), LOW_ORACLE_PRECISION);
                assert_eq!(assessment.high_output().precision(), HIGH_ORACLE_PRECISION);
                assert_eq!(
                    assessment.low_output().values().len(),
                    record.samples().len()
                );
                assert!(
                    assessment.convergence().normalized_rms()
                        <= &decimal_limit(
                            PRECISION,
                            CONVERGENCE_RMS_LIMIT.0,
                            CONVERGENCE_RMS_LIMIT.1,
                        )
                );
                assert!(
                    assessment.convergence().normalized_peak()
                        <= &decimal_limit(
                            PRECISION,
                            CONVERGENCE_PEAK_LIMIT.0,
                            CONVERGENCE_PEAK_LIMIT.1,
                        )
                );
                assert!(
                    assessment.low_imaginary_ratio()
                        <= &decimal_limit(
                            PRECISION,
                            IMAGINARY_RATIO_LIMIT.0,
                            IMAGINARY_RATIO_LIMIT.1,
                        )
                );
                assert!(
                    assessment.high_imaginary_ratio()
                        <= &decimal_limit(
                            PRECISION,
                            IMAGINARY_RATIO_LIMIT.0,
                            IMAGINARY_RATIO_LIMIT.1,
                        )
                );

                let certified = assessment.certify().unwrap();
                assert_eq!(certified.reference().precision(), HIGH_ORACLE_PRECISION);
                assert_eq!(certified.reference().values().len(), record.samples().len());
            }
        }

        let certified = certify_oracle(2, &records[0]).unwrap();
        assert_eq!(certified.reference().precision(), HIGH_ORACLE_PRECISION);
    }

    #[test]
    fn certification_reports_a_perturbed_frozen_gate_failure() {
        let record = sample_record();
        let mut assessment = assess_oracle(1, &record).unwrap();
        let mut perturbed =
            decimal_limit(PRECISION, CONVERGENCE_RMS_LIMIT.0, CONVERGENCE_RMS_LIMIT.1);
        perturbed *= 2;
        assessment.convergence.normalized_rms = perturbed;

        let error = assessment.certify().unwrap_err();
        assert!(matches!(
            error.kind(),
            AssessmentErrorKind::CertificationGateFailed {
                gate: CertificationGate::ConvergenceRms,
                ..
            }
        ));
    }

    #[test]
    fn reversed_records_preserve_bits_maps_and_oracle_covariance() {
        let records = [
            RepresentedRecord::from_f32(
                37.0,
                vec![0.1_f32, 0.003, 0.2, 0.75],
                vec![0.0_f32, -0.0, 0.375, -1.25, 0.5],
            )
            .unwrap(),
            RepresentedRecord::from_f64(
                1.0,
                vec![0.1, 0.003, 0.2, 0.75],
                vec![0.0, -0.0, 0.375, -1.25, 0.5],
            )
            .unwrap(),
        ];

        for record in records {
            let reversed = reverse_record(&record).unwrap();
            assert_eq!(reversed.sample_count(), record.samples().len());
            assert_eq!(reversed.reversed_index_for_original(0), Some(4));
            assert_eq!(reversed.reversed_index_for_original(2), Some(2));
            assert_eq!(reversed.reversed_index_for_original(4), Some(0));
            assert_eq!(reversed.reversed_index_for_original(5), None);
            assert_eq!(reversed.reversed_index_for_original(usize::MAX), None);
            assert_eq!(reversed.original_index_for_reversed(0), Some(4));
            assert_eq!(reversed.original_index_for_reversed(5), None);

            match record.scalar() {
                RecordScalar::F32 => {
                    assert_eq!(
                        reversed
                            .record()
                            .intervals_seconds()
                            .iter()
                            .map(|value| (*value as f32).to_bits())
                            .collect::<Vec<_>>(),
                        record
                            .intervals_seconds()
                            .iter()
                            .rev()
                            .map(|value| (*value as f32).to_bits())
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(
                        reversed
                            .record()
                            .samples()
                            .iter()
                            .map(|value| (*value as f32).to_bits())
                            .collect::<Vec<_>>(),
                        record
                            .samples()
                            .iter()
                            .rev()
                            .map(|value| (*value as f32).to_bits())
                            .collect::<Vec<_>>()
                    );
                }
                RecordScalar::F64 => {
                    assert_eq!(
                        reversed
                            .record()
                            .intervals_seconds()
                            .iter()
                            .map(|value| value.to_bits())
                            .collect::<Vec<_>>(),
                        record
                            .intervals_seconds()
                            .iter()
                            .rev()
                            .map(|value| value.to_bits())
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(
                        reversed
                            .record()
                            .samples()
                            .iter()
                            .map(|value| value.to_bits())
                            .collect::<Vec<_>>(),
                        record
                            .samples()
                            .iter()
                            .rev()
                            .map(|value| value.to_bits())
                            .collect::<Vec<_>>()
                    );
                }
            }

            let twice = reverse_record(reversed.record()).unwrap();
            assert_eq!(
                twice
                    .record()
                    .samples()
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                record
                    .samples()
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>()
            );

            for order in [1, 2, 4, 8] {
                let forward_output = evaluate(order, &record, HIGH_ORACLE_PRECISION);
                let reverse_output = evaluate(order, reversed.record(), HIGH_ORACLE_PRECISION);
                let reversed_values = reverse_output
                    .values()
                    .iter()
                    .rev()
                    .cloned()
                    .collect::<Vec<_>>();
                let covariance = weighted_error(
                    &record,
                    &reversed_values,
                    forward_output.values(),
                    HIGH_ORACLE_PRECISION,
                )
                .unwrap();
                assert!(covariance.normalized_peak() < &Float::with_val(PRECISION, 1.0e-40));
            }
        }
    }

    #[test]
    fn extraction_keeps_original_knots_for_tone_terminal_remainder_refinements() {
        let coordinate = crate::ToneCoordinate {
            order: ToneOrder::One,
            frequency: ToneFrequency::OneSixteenth,
            phase: TonePhase::SeventeenHundredths,
            q_max: QMax::PiOver128,
            schedule: ScheduleKind::Uniform,
        };
        let nominal = build_nominal_tone_record(coordinate).unwrap();
        assert_eq!(
            nominal.normalized_intervals().len(),
            nominal.full_interval_count() + 1
        );

        for scalar in [RecordScalar::F32, RecordScalar::F64] {
            let case =
                build_represented_tone_case(&nominal, scalar, PhysicalCutoff::OneHz).unwrap();
            let refinement = refine_represented_case(&case).unwrap();
            let refined_count = refinement.record().samples().len();
            let indexed_values = (0..refined_count).collect::<Vec<_>>();
            let originals = extract_original_knots(&refinement, &indexed_values).unwrap();
            assert_eq!(originals.len(), refinement.original_sample_count());
            assert_eq!(originals[0], 0);
            assert_eq!(originals[1], 3);
            assert_eq!(originals[2], 6);
            for (original_index, value) in originals.iter().enumerate() {
                assert_eq!(*value, original_index * 3);
            }

            let oracle_values = (0..refined_count)
                .map(|index| Float::with_val(PRECISION, index))
                .collect::<Vec<_>>();
            let oracle_originals = extract_original_knots(&refinement, &oracle_values).unwrap();
            for (original_index, value) in oracle_originals.iter().enumerate() {
                assert_eq!(value, &Float::with_val(PRECISION, original_index * 3));
            }

            let short = extract_original_knots(&refinement, &indexed_values[..refined_count - 1])
                .unwrap_err();
            assert_eq!(short.coordinate(), Some(refinement.coordinate()));
            assert!(matches!(
                short.kind(),
                AssessmentErrorKind::RefinedLength {
                    expected,
                    actual
                } if *expected == refined_count && *actual == refined_count - 1
            ));

            match scalar {
                RecordScalar::F32 => {
                    let candidate_values = (0..refined_count)
                        .map(|index| index as f32)
                        .collect::<Vec<_>>();
                    let candidate_originals =
                        extract_original_knots(&refinement, &candidate_values).unwrap();
                    assert_eq!(&candidate_originals[..3], &[0.0, 3.0, 6.0]);
                }
                RecordScalar::F64 => {
                    let candidate_values = (0..refined_count)
                        .map(|index| index as f64)
                        .collect::<Vec<_>>();
                    let candidate_originals =
                        extract_original_knots(&refinement, &candidate_values).unwrap();
                    assert_eq!(&candidate_originals[..3], &[0.0, 3.0, 6.0]);
                }
            }
        }
    }

    #[test]
    fn unsupported_orders_fail_before_oracle_evaluation() {
        let record = sample_record();
        let error = assess_oracle(3, &record).unwrap_err();
        assert!(matches!(
            error.kind(),
            AssessmentErrorKind::UnsupportedOrder { order: 3 }
        ));
    }
}
