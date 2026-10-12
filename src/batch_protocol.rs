#![forbid(unsafe_code)]

//! Versioned, length-prefixed wire format shared by the forward-backward
//! acceptance harness and its candidate client process.
//!
//! A frame is encoded as a four-byte little-endian length followed by a
//! one-byte protocol version, a one-byte frame tag, and the frame payload. The
//! length includes the version and tag, but [`DecodeLimits::max_payload_bytes`]
//! limits only the payload after those two bytes. All integers and scalar bit
//! patterns are little-endian. Scalar values are transported as bits; this
//! crate does not interpret or filter them.
//!
//! Version 1 frame tags are handshake (`1`), case request (`2`), success
//! (`3`), candidate error (`4`), and done (`5`). The handshake carries a
//! 16-byte run nonce. Requests and responses identify a case by its `u64`
//! coordinate and 32-byte represented-record digest. Request order and scalar
//! tags are validated, and interval/sample shapes are checked with overflow
//! checks.
//!
//! The payload layouts are fixed in field order:
//!
//! - Handshake: 16-byte nonce.
//! - Case request: case ID (`u64`), record digest (32 bytes), order (`u8`),
//!   scalar (`u8`), cutoff bits, interval count (`u32`), sample count (`u32`),
//!   interval bits, then sample bits. Scalar tag `1` means `f32` (four bytes
//!   per value); tag `2` means `f64` (eight bytes per value). Order tags are
//!   the literal values `1`, `2`, `4`, and `8`.
//! - Success: case ID, digest, scalar tag, output count (`u32`), output bits,
//!   tail-evidence tag and evidence, then forward, tail, and reverse counters,
//!   each as accepted, rejected, and derivative-evaluation `u64` values. Tail
//!   tag `0` means omitted, `1` means the candidate used the analytic tail,
//!   and `2` means numerical continuation. Numerical evidence carries the
//!   final normalized residual, public-normalized extension, and the complete
//!   output obtained with the analytic tail substituted; all use the success
//!   scalar width and the existing output count.
//! - Candidate error: case ID, digest, category tag, pass tag, interval
//!   presence tag, optional interval (`u64`), and source-category tag. Pass
//!   tags are `0` for absent, `1` forward, `2` reverse, and `3` tail. The
//!   interval-presence tag is `0` or `1`. Source tag `0` means absent; tags
//!   `1` through `5` name the corresponding `SourceCategory` variants and
//!   `255` means `Other`. Candidate category tags are `1` through `5` for the
//!   named categories and `255` for `Other`.
//! - Done: empty payload.
//!
//! [`read_frame`] returns `Ok(None)` only when the reader reaches EOF before
//! reading any byte of the next length prefix. EOF in a prefix, header, or
//! payload is an error. Decoded payload sizes are checked against explicit
//! limits before allocation. [`validate_case_result`] additionally checks a
//! response against the request that caused it, including identity, scalar
//! width, and output length.

use std::error::Error as StdError;
use std::fmt;
use std::io::{self, Read, Write};

/// The protocol version carried by every frame.
pub const PROTOCOL_VERSION: u8 = 1;

const FRAME_HEADER_BYTES: usize = 2;
const IDENTITY_BYTES: usize = 8 + 32;
const PASS_COUNTER_BYTES: usize = 3 * 8;
const SUCCESS_COUNTER_BYTES: usize = 3 * PASS_COUNTER_BYTES;

/// Limits applied before allocating decoded frame or record data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeLimits {
    /// Maximum number of bytes in a frame payload, excluding version and tag.
    pub max_payload_bytes: usize,
    /// Maximum number of samples in a request or successful response.
    pub max_samples: usize,
}

impl DecodeLimits {
    /// Creates explicit decoding limits.
    #[must_use]
    pub const fn new(max_payload_bytes: usize, max_samples: usize) -> Self {
        Self {
            max_payload_bytes,
            max_samples,
        }
    }
}

/// A coordinate and digest that together identify one represented record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaseIdentity {
    /// Frozen case coordinate assigned by the harness.
    pub case_id: u64,
    /// Digest of the exact represented record bytes, supplied by the harness.
    pub record_digest: [u8; 32],
}

/// One supported per-pass order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterOrder {
    /// Order one.
    One,
    /// Order two.
    Two,
    /// Order four.
    Four,
    /// Order eight.
    Eight,
}

impl FilterOrder {
    const fn tag(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, CodecError> {
        match tag {
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            4 => Ok(Self::Four),
            8 => Ok(Self::Eight),
            other => Err(CodecError::InvalidOrder(other)),
        }
    }
}

/// Exact represented inputs for a case request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordBits {
    /// `f32` values represented by their exact IEEE-754 bit patterns.
    F32 {
        /// Cutoff frequency bits.
        cutoff_bits: u32,
        /// Positive interval-duration bits, one per adjacent sample pair.
        interval_bits: Vec<u32>,
        /// Input sample bits.
        sample_bits: Vec<u32>,
    },
    /// `f64` values represented by their exact IEEE-754 bit patterns.
    F64 {
        /// Cutoff frequency bits.
        cutoff_bits: u64,
        /// Positive interval-duration bits, one per adjacent sample pair.
        interval_bits: Vec<u64>,
        /// Input sample bits.
        sample_bits: Vec<u64>,
    },
}

impl RecordBits {
    fn scalar_tag(&self) -> u8 {
        match self {
            Self::F32 { .. } => 1,
            Self::F64 { .. } => 2,
        }
    }

    fn width(&self) -> usize {
        match self {
            Self::F32 { .. } => 4,
            Self::F64 { .. } => 8,
        }
    }

    fn lengths(&self) -> (usize, usize) {
        match self {
            Self::F32 {
                interval_bits,
                sample_bits,
                ..
            } => (interval_bits.len(), sample_bits.len()),
            Self::F64 {
                interval_bits,
                sample_bits,
                ..
            } => (interval_bits.len(), sample_bits.len()),
        }
    }
}

/// A request for one represented record and filter order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaseRequest {
    /// Record coordinate and represented-record digest.
    pub identity: CaseIdentity,
    /// Per-pass Butterworth order.
    pub order: FilterOrder,
    /// Exact cutoff, interval, and input bits.
    pub record: RecordBits,
}

/// Exact output values returned by a successful candidate case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputBits {
    /// `f32` output bit patterns.
    F32(Vec<u32>),
    /// `f64` output bit patterns.
    F64(Vec<u64>),
}

impl OutputBits {
    fn scalar_tag(&self) -> u8 {
        match self {
            Self::F32(_) => 1,
            Self::F64(_) => 2,
        }
    }

    fn width(&self) -> usize {
        match self {
            Self::F32(_) => 4,
            Self::F64(_) => 8,
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::F32(values) => values.len(),
            Self::F64(values) => values.len(),
        }
    }
}

/// Work counters for one direction of the candidate operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PassCounters {
    /// Accepted adaptive steps.
    pub accepted_steps: u64,
    /// Rejected adaptive steps.
    pub rejected_steps: u64,
    /// Model derivative evaluations.
    pub derivative_evaluations: u64,
}

/// Evidence describing how the forward constant-continuation tail was handled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TailEvidence {
    /// The negative-control candidate deliberately omitted the physical tail.
    Omitted,
    /// The candidate used the analytic constant-continuation tail directly.
    Analytic,
    /// An `f32` candidate numerically continued the tail.
    NumericalF32 {
        /// Final normalized tail residual, as exact `f32` bits.
        final_residual_bits: u32,
        /// Public-normalized continuation duration, as exact `f32` bits.
        public_normalized_extension_bits: u32,
        /// Complete output with the analytic tail substituted.
        analytic_output_bits: Vec<u32>,
    },
    /// An `f64` candidate numerically continued the tail.
    NumericalF64 {
        /// Final normalized tail residual, as exact `f64` bits.
        final_residual_bits: u64,
        /// Public-normalized continuation duration, as exact `f64` bits.
        public_normalized_extension_bits: u64,
        /// Complete output with the analytic tail substituted.
        analytic_output_bits: Vec<u64>,
    },
}

/// Successful result for one case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuccessResponse {
    /// Identity copied from the request.
    pub identity: CaseIdentity,
    /// Exact output bit patterns.
    pub output_bits: OutputBits,
    /// Tail treatment and evidence for its independent numerical gates.
    pub tail_evidence: TailEvidence,
    /// Forward-pass work counters.
    pub forward: PassCounters,
    /// Constant-continuation tail work counters.
    pub tail: PassCounters,
    /// Reverse-pass work counters.
    pub reverse: PassCounters,
}

/// High-level candidate failure categories carried by protocol v1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum CandidateErrorCategory {
    /// Record, cutoff, or other request validation failed in the candidate.
    Validation = 1,
    /// A forward, reverse, or tail-continuation solve failed.
    Integration = 2,
    /// The forward constant-continuation tail exceeded its declared work.
    TailWork = 3,
    /// A candidate work counter could not be represented.
    WorkCounterOverflow = 4,
    /// A successful-looking output contained an invalid value.
    InvalidOutput = 5,
    /// A candidate failure does not fit a more specific category.
    Other = 255,
}

impl CandidateErrorCategory {
    fn from_tag(tag: u8) -> Result<Self, CodecError> {
        match tag {
            1 => Ok(Self::Validation),
            2 => Ok(Self::Integration),
            3 => Ok(Self::TailWork),
            4 => Ok(Self::WorkCounterOverflow),
            5 => Ok(Self::InvalidOutput),
            255 => Ok(Self::Other),
            other => Err(CodecError::InvalidCandidateCategory(other)),
        }
    }
}

/// Pass associated with a structured candidate error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Pass {
    /// Forward pass.
    Forward = 1,
    /// Reverse pass.
    Reverse = 2,
    /// Forward-tail continuation.
    Tail = 3,
}

impl Pass {
    fn from_tag(tag: u8) -> Result<Option<Self>, CodecError> {
        match tag {
            0 => Ok(None),
            1 => Ok(Some(Self::Forward)),
            2 => Ok(Some(Self::Reverse)),
            3 => Ok(Some(Self::Tail)),
            other => Err(CodecError::InvalidPassTag(other)),
        }
    }
}

/// Underlying source error category when a candidate error has one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SourceCategory {
    /// An input value was not finite.
    NonFiniteInput = 1,
    /// The candidate rejected an elapsed interval.
    InvalidDeltaTime = 2,
    /// Adaptive integration exhausted its attempt budget.
    StepBudgetExceeded = 3,
    /// Floating-point resolution prevented further integration progress.
    StepSizeUnderflow = 4,
    /// A candidate state became non-finite.
    NonFiniteState = 5,
    /// The source error does not fit a more specific category.
    Other = 255,
}

impl SourceCategory {
    fn from_tag(tag: u8) -> Result<Option<Self>, CodecError> {
        match tag {
            0 => Ok(None),
            1 => Ok(Some(Self::NonFiniteInput)),
            2 => Ok(Some(Self::InvalidDeltaTime)),
            3 => Ok(Some(Self::StepBudgetExceeded)),
            4 => Ok(Some(Self::StepSizeUnderflow)),
            5 => Ok(Some(Self::NonFiniteState)),
            255 => Ok(Some(Self::Other)),
            other => Err(CodecError::InvalidSourceCategory(other)),
        }
    }
}

/// Structured candidate failure for one case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateErrorResponse {
    /// Identity copied from the request.
    pub identity: CaseIdentity,
    /// Stable high-level candidate error category.
    pub category: CandidateErrorCategory,
    /// Pass associated with the error, if known.
    pub pass: Option<Pass>,
    /// Zero-based record interval associated with the error, if known.
    ///
    /// This is always absent for [`Pass::Tail`], whose continuation lies
    /// outside the represented record.
    pub interval: Option<u64>,
    /// Nested source error category, if available.
    pub source_category: Option<SourceCategory>,
}

/// A response to one case request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaseResult {
    /// Candidate completed the case successfully.
    Success(SuccessResponse),
    /// Candidate reported a structured failure for the case.
    CandidateError(CandidateErrorResponse),
}

/// A decoded or encoded protocol frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Frame {
    /// Starts a run and carries its 16-byte nonce.
    Handshake { run_nonce: [u8; 16] },
    /// One complete represented record to process.
    CaseRequest(CaseRequest),
    /// Result for one case request.
    CaseResult(CaseResult),
    /// Marks the end of the run's request stream.
    Done,
}

/// Errors detected while encoding, decoding, or validating a protocol frame.
#[derive(Debug)]
pub enum CodecError {
    /// The underlying reader or writer failed.
    Io(io::Error),
    /// EOF occurred after at least one byte of a frame had been read.
    TruncatedFrame,
    /// A frame length cannot contain its version and tag.
    InvalidFrameLength,
    /// The frame version is not supported.
    InvalidVersion(u8),
    /// The frame tag is not supported.
    UnknownFrameTag(u8),
    /// The scalar tag is not supported.
    UnknownScalarTag(u8),
    /// The requested order is not one of 1, 2, 4, or 8.
    InvalidOrder(u8),
    /// A candidate error category tag is not supported.
    InvalidCandidateCategory(u8),
    /// A pass tag is not supported.
    InvalidPassTag(u8),
    /// A source category tag is not supported.
    InvalidSourceCategory(u8),
    /// An optional-field presence tag is not 0 or 1.
    InvalidPresenceTag(u8),
    /// A tail-evidence tag is not supported.
    InvalidTailEvidenceTag(u8),
    /// A frame payload exceeds the explicit payload limit.
    PayloadTooLarge { actual: usize, limit: usize },
    /// A request or response sample count exceeds the explicit sample limit.
    SampleLimitExceeded { actual: usize, limit: usize },
    /// A length or count calculation overflowed.
    LengthOverflow,
    /// A request interval/sample count pair does not have exactly one more
    /// sample than intervals.
    ShapeMismatch,
    /// A well-framed payload is shorter than the fields it declares.
    PayloadLengthMismatch,
    /// A payload contains bytes after its complete declared fields.
    TrailingPayload,
    /// A bounded payload allocation could not be reserved.
    AllocationFailed,
    /// A response identity does not match the request.
    IdentityMismatch,
    /// A successful response uses a different scalar width than its request.
    ScalarMismatch,
    /// A successful response does not contain one output per request sample.
    OutputShapeMismatch,
    /// Numerical tail evidence uses a different scalar width than the output.
    TailScalarMismatch,
    /// Numerical tail evidence does not contain one analytic output per sample.
    TailOutputShapeMismatch,
    /// Omitted or analytic tail treatment reported numerical tail work.
    TailWorkMismatch,
    /// A candidate error names an interval outside the request record.
    CandidateIntervalOutOfRange,
    /// A tail error incorrectly names a represented-record interval.
    CandidateTailInterval,
    /// A candidate error category, pass, and interval combination is contradictory.
    CandidateErrorContextMismatch,
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "protocol I/O failed: {error}"),
            Self::TruncatedFrame => formatter.write_str("truncated protocol frame"),
            Self::InvalidFrameLength => formatter.write_str("invalid protocol frame length"),
            Self::InvalidVersion(version) => {
                write!(formatter, "unsupported protocol version {version}")
            }
            Self::UnknownFrameTag(tag) => write!(formatter, "unknown frame tag {tag}"),
            Self::UnknownScalarTag(tag) => write!(formatter, "unknown scalar tag {tag}"),
            Self::InvalidOrder(order) => write!(formatter, "unsupported filter order {order}"),
            Self::InvalidCandidateCategory(tag) => {
                write!(formatter, "unknown candidate error category {tag}")
            }
            Self::InvalidPassTag(tag) => write!(formatter, "unknown pass tag {tag}"),
            Self::InvalidSourceCategory(tag) => {
                write!(formatter, "unknown source category {tag}")
            }
            Self::InvalidPresenceTag(tag) => {
                write!(formatter, "invalid optional-field tag {tag}")
            }
            Self::InvalidTailEvidenceTag(tag) => {
                write!(formatter, "unknown tail-evidence tag {tag}")
            }
            Self::PayloadTooLarge { actual, limit } => {
                write!(formatter, "payload size {actual} exceeds limit {limit}")
            }
            Self::SampleLimitExceeded { actual, limit } => {
                write!(formatter, "sample count {actual} exceeds limit {limit}")
            }
            Self::LengthOverflow => formatter.write_str("protocol length calculation overflowed"),
            Self::ShapeMismatch => {
                formatter.write_str("interval count plus one must equal sample count")
            }
            Self::PayloadLengthMismatch => {
                formatter.write_str("payload is shorter than its declared fields")
            }
            Self::TrailingPayload => formatter.write_str("payload contains trailing bytes"),
            Self::AllocationFailed => formatter.write_str("could not reserve bounded payload"),
            Self::IdentityMismatch => formatter.write_str("response identity differs from request"),
            Self::ScalarMismatch => formatter.write_str("response scalar differs from request"),
            Self::OutputShapeMismatch => {
                formatter.write_str("response output count differs from request sample count")
            }
            Self::TailScalarMismatch => {
                formatter.write_str("tail evidence scalar differs from response output")
            }
            Self::TailOutputShapeMismatch => {
                formatter.write_str("analytic-tail output count differs from response output")
            }
            Self::TailWorkMismatch => {
                formatter.write_str("tail treatment cannot have numerical tail work")
            }
            Self::CandidateIntervalOutOfRange => {
                formatter.write_str("candidate error interval is outside the request record")
            }
            Self::CandidateTailInterval => {
                formatter.write_str("tail errors cannot name a request-record interval")
            }
            Self::CandidateErrorContextMismatch => {
                formatter.write_str("candidate error category and location are inconsistent")
            }
        }
    }
}

impl StdError for CodecError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for CodecError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Writes one protocol-v1 frame.
///
/// The frame's shape and total wire length are checked before any bytes are
/// written. A writer failure is returned as [`CodecError::Io`]; as with any
/// `Write`, bytes already accepted by the writer cannot be rolled back.
pub fn write_frame<W: Write>(writer: &mut W, frame: &Frame) -> Result<(), CodecError> {
    let (tag, payload_len) = frame_payload_len(frame)?;
    let frame_len = FRAME_HEADER_BYTES
        .checked_add(payload_len)
        .ok_or(CodecError::LengthOverflow)?;
    let frame_len_u32 = u32::try_from(frame_len).map_err(|_| CodecError::LengthOverflow)?;

    writer.write_all(&frame_len_u32.to_le_bytes())?;
    writer.write_all(&[PROTOCOL_VERSION, tag])?;
    match frame {
        Frame::Handshake { run_nonce } => writer.write_all(run_nonce)?,
        Frame::CaseRequest(request) => write_case_request(writer, request)?,
        Frame::CaseResult(CaseResult::Success(response)) => {
            write_success(writer, response)?;
        }
        Frame::CaseResult(CaseResult::CandidateError(response)) => {
            write_candidate_error(writer, response)?;
        }
        Frame::Done => {}
    }
    Ok(())
}

/// Reads one frame under explicit payload and sample limits.
///
/// Returns `Ok(None)` only for clean EOF between frames. Every partial prefix,
/// header, or payload returns [`CodecError::TruncatedFrame`]. The payload
/// limit excludes the two-byte version/tag header.
pub fn read_frame<R: Read>(
    reader: &mut R,
    limits: DecodeLimits,
) -> Result<Option<Frame>, CodecError> {
    let mut length_bytes = [0; 4];
    let first_read = loop {
        match reader.read(&mut length_bytes) {
            Ok(0) => return Ok(None),
            Ok(count) => break count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(CodecError::Io(error)),
        }
    };
    if first_read < length_bytes.len() {
        read_exact_frame(reader, &mut length_bytes[first_read..])?;
    }

    let frame_len = usize::try_from(u32::from_le_bytes(length_bytes))
        .map_err(|_| CodecError::LengthOverflow)?;
    if frame_len < FRAME_HEADER_BYTES {
        return Err(CodecError::InvalidFrameLength);
    }
    let mut header = [0; FRAME_HEADER_BYTES];
    read_exact_frame(reader, &mut header)?;
    if header[0] != PROTOCOL_VERSION {
        return Err(CodecError::InvalidVersion(header[0]));
    }
    let tag = header[1];
    if !(1..=5).contains(&tag) {
        return Err(CodecError::UnknownFrameTag(tag));
    }

    let payload_len = frame_len - FRAME_HEADER_BYTES;
    if payload_len > limits.max_payload_bytes {
        return Err(CodecError::PayloadTooLarge {
            actual: payload_len,
            limit: limits.max_payload_bytes,
        });
    }

    let mut payload = Vec::new();
    payload
        .try_reserve_exact(payload_len)
        .map_err(|_| CodecError::AllocationFailed)?;
    payload.resize(payload_len, 0);
    read_exact_frame(reader, &mut payload)?;

    decode_frame_payload(tag, &payload, limits).map(Some)
}

/// Checks a decoded case result against the request that produced it.
///
/// This verifies the repeated case identity; success scalar, output, tail
/// evidence, and work shape; and candidate-error location semantics.
pub fn validate_case_result(request: &CaseRequest, result: &CaseResult) -> Result<(), CodecError> {
    case_request_payload_len(request)?;
    let result_identity = match result {
        CaseResult::Success(response) => response.identity,
        CaseResult::CandidateError(response) => response.identity,
    };
    if result_identity != request.identity {
        return Err(CodecError::IdentityMismatch);
    }

    match result {
        CaseResult::Success(response) => {
            success_payload_len(response)?;
            if response.output_bits.scalar_tag() != request.record.scalar_tag() {
                return Err(CodecError::ScalarMismatch);
            }
            let (_, sample_count) = request.record.lengths();
            if response.output_bits.len() != sample_count {
                return Err(CodecError::OutputShapeMismatch);
            }
        }
        CaseResult::CandidateError(response) => {
            validate_candidate_error_context(response)?;
            if let Some(interval) = response.interval {
                let (interval_count, _) = request.record.lengths();
                let interval_count =
                    u64::try_from(interval_count).map_err(|_| CodecError::LengthOverflow)?;
                if interval >= interval_count {
                    return Err(CodecError::CandidateIntervalOutOfRange);
                }
            }
        }
    }
    Ok(())
}

fn validate_candidate_error_context(response: &CandidateErrorResponse) -> Result<(), CodecError> {
    if response.pass == Some(Pass::Tail) && response.interval.is_some() {
        return Err(CodecError::CandidateTailInterval);
    }
    if response.interval.is_some()
        && response.pass.is_none()
        && response.category != CandidateErrorCategory::Validation
    {
        return Err(CodecError::CandidateErrorContextMismatch);
    }

    let consistent = match response.category {
        CandidateErrorCategory::Validation => response.pass.is_none(),
        CandidateErrorCategory::Integration => matches!(
            (response.pass, response.interval),
            (Some(Pass::Forward | Pass::Reverse), Some(_)) | (Some(Pass::Tail), None)
        ),
        CandidateErrorCategory::TailWork => {
            response.pass == Some(Pass::Tail) && response.interval.is_none()
        }
        CandidateErrorCategory::WorkCounterOverflow | CandidateErrorCategory::Other => true,
        CandidateErrorCategory::InvalidOutput => {
            response.pass.is_none() && response.interval.is_none()
        }
    };
    if consistent {
        Ok(())
    } else {
        Err(CodecError::CandidateErrorContextMismatch)
    }
}

fn frame_payload_len(frame: &Frame) -> Result<(u8, usize), CodecError> {
    match frame {
        Frame::Handshake { .. } => Ok((1, 16)),
        Frame::CaseRequest(request) => Ok((2, case_request_payload_len(request)?)),
        Frame::CaseResult(CaseResult::Success(response)) => Ok((3, success_payload_len(response)?)),
        Frame::CaseResult(CaseResult::CandidateError(response)) => {
            Ok((4, candidate_error_payload_len(response)?))
        }
        Frame::Done => Ok((5, 0)),
    }
}

fn case_request_payload_len(request: &CaseRequest) -> Result<usize, CodecError> {
    let (interval_count, sample_count) = request.record.lengths();
    validate_shape(interval_count, sample_count)?;
    u32::try_from(interval_count).map_err(|_| CodecError::LengthOverflow)?;
    u32::try_from(sample_count).map_err(|_| CodecError::LengthOverflow)?;
    let width = request.record.width();
    let (interval_count, sample_count) = request.record.lengths();
    let arrays_len = interval_count
        .checked_add(sample_count)
        .and_then(|count| count.checked_mul(width))
        .ok_or(CodecError::LengthOverflow)?;
    IDENTITY_BYTES
        .checked_add(1 + 1 + width + 4 + 4)
        .and_then(|base| base.checked_add(arrays_len))
        .ok_or(CodecError::LengthOverflow)
}

fn success_payload_len(response: &SuccessResponse) -> Result<usize, CodecError> {
    let output_count = response.output_bits.len();
    if output_count == 0 {
        return Err(CodecError::ShapeMismatch);
    }
    u32::try_from(output_count).map_err(|_| CodecError::LengthOverflow)?;
    let output_bytes = output_count
        .checked_mul(response.output_bits.width())
        .ok_or(CodecError::LengthOverflow)?;
    let tail_evidence_bytes = tail_evidence_payload_len(response)?;
    IDENTITY_BYTES
        .checked_add(1 + 4 + SUCCESS_COUNTER_BYTES)
        .and_then(|base| base.checked_add(output_bytes))
        .and_then(|base| base.checked_add(tail_evidence_bytes))
        .ok_or(CodecError::LengthOverflow)
}

fn tail_evidence_payload_len(response: &SuccessResponse) -> Result<usize, CodecError> {
    match &response.tail_evidence {
        TailEvidence::Omitted | TailEvidence::Analytic => {
            if response.tail != PassCounters::default() {
                return Err(CodecError::TailWorkMismatch);
            }
            Ok(1)
        }
        TailEvidence::NumericalF32 {
            analytic_output_bits,
            ..
        } => {
            if !matches!(&response.output_bits, OutputBits::F32(_)) {
                return Err(CodecError::TailScalarMismatch);
            }
            if analytic_output_bits.len() != response.output_bits.len() {
                return Err(CodecError::TailOutputShapeMismatch);
            }
            analytic_output_bits
                .len()
                .checked_add(2)
                .and_then(|count| count.checked_mul(4))
                .and_then(|bytes| bytes.checked_add(1))
                .ok_or(CodecError::LengthOverflow)
        }
        TailEvidence::NumericalF64 {
            analytic_output_bits,
            ..
        } => {
            if !matches!(&response.output_bits, OutputBits::F64(_)) {
                return Err(CodecError::TailScalarMismatch);
            }
            if analytic_output_bits.len() != response.output_bits.len() {
                return Err(CodecError::TailOutputShapeMismatch);
            }
            analytic_output_bits
                .len()
                .checked_add(2)
                .and_then(|count| count.checked_mul(8))
                .and_then(|bytes| bytes.checked_add(1))
                .ok_or(CodecError::LengthOverflow)
        }
    }
}

fn candidate_error_payload_len(response: &CandidateErrorResponse) -> Result<usize, CodecError> {
    validate_candidate_error_context(response)?;
    IDENTITY_BYTES
        .checked_add(1 + 1 + 1 + 1)
        .and_then(|base| base.checked_add(usize::from(response.interval.is_some()) * 8))
        .ok_or(CodecError::LengthOverflow)
}

fn validate_shape(interval_count: usize, sample_count: usize) -> Result<(), CodecError> {
    match interval_count.checked_add(1) {
        Some(expected) if expected == sample_count => Ok(()),
        Some(_) => Err(CodecError::ShapeMismatch),
        None => Err(CodecError::LengthOverflow),
    }
}

fn validate_wire_shape(interval_count: u32, sample_count: u32) -> Result<(), CodecError> {
    match interval_count.checked_add(1) {
        Some(expected) if expected == sample_count => Ok(()),
        Some(_) => Err(CodecError::ShapeMismatch),
        None => Err(CodecError::LengthOverflow),
    }
}

fn write_case_request<W: Write>(writer: &mut W, request: &CaseRequest) -> io::Result<()> {
    write_identity(writer, request.identity)?;
    writer.write_all(&[request.order.tag(), request.record.scalar_tag()])?;
    match &request.record {
        RecordBits::F32 {
            cutoff_bits,
            interval_bits,
            sample_bits,
        } => {
            writer.write_all(&cutoff_bits.to_le_bytes())?;
            write_count(writer, interval_bits.len())?;
            write_count(writer, sample_bits.len())?;
            for bits in interval_bits {
                writer.write_all(&bits.to_le_bytes())?;
            }
            for bits in sample_bits {
                writer.write_all(&bits.to_le_bytes())?;
            }
        }
        RecordBits::F64 {
            cutoff_bits,
            interval_bits,
            sample_bits,
        } => {
            writer.write_all(&cutoff_bits.to_le_bytes())?;
            write_count(writer, interval_bits.len())?;
            write_count(writer, sample_bits.len())?;
            for bits in interval_bits {
                writer.write_all(&bits.to_le_bytes())?;
            }
            for bits in sample_bits {
                writer.write_all(&bits.to_le_bytes())?;
            }
        }
    }
    Ok(())
}

fn write_success<W: Write>(writer: &mut W, response: &SuccessResponse) -> io::Result<()> {
    write_identity(writer, response.identity)?;
    writer.write_all(&[response.output_bits.scalar_tag()])?;
    write_count(writer, response.output_bits.len())?;
    match &response.output_bits {
        OutputBits::F32(values) => {
            for bits in values {
                writer.write_all(&bits.to_le_bytes())?;
            }
        }
        OutputBits::F64(values) => {
            for bits in values {
                writer.write_all(&bits.to_le_bytes())?;
            }
        }
    }
    match &response.tail_evidence {
        TailEvidence::Omitted => writer.write_all(&[0])?,
        TailEvidence::Analytic => writer.write_all(&[1])?,
        TailEvidence::NumericalF32 {
            final_residual_bits,
            public_normalized_extension_bits,
            analytic_output_bits,
        } => {
            writer.write_all(&[2])?;
            writer.write_all(&final_residual_bits.to_le_bytes())?;
            writer.write_all(&public_normalized_extension_bits.to_le_bytes())?;
            for bits in analytic_output_bits {
                writer.write_all(&bits.to_le_bytes())?;
            }
        }
        TailEvidence::NumericalF64 {
            final_residual_bits,
            public_normalized_extension_bits,
            analytic_output_bits,
        } => {
            writer.write_all(&[2])?;
            writer.write_all(&final_residual_bits.to_le_bytes())?;
            writer.write_all(&public_normalized_extension_bits.to_le_bytes())?;
            for bits in analytic_output_bits {
                writer.write_all(&bits.to_le_bytes())?;
            }
        }
    }
    write_counters(writer, response.forward)?;
    write_counters(writer, response.tail)?;
    write_counters(writer, response.reverse)?;
    Ok(())
}

fn write_candidate_error<W: Write>(
    writer: &mut W,
    response: &CandidateErrorResponse,
) -> io::Result<()> {
    write_identity(writer, response.identity)?;
    writer.write_all(&[
        response.category as u8,
        response.pass.map_or(0, |pass| pass as u8),
        u8::from(response.interval.is_some()),
    ])?;
    if let Some(interval) = response.interval {
        writer.write_all(&interval.to_le_bytes())?;
    }
    writer.write_all(&[response
        .source_category
        .map_or(0, |category| category as u8)])?;
    Ok(())
}

fn write_identity<W: Write>(writer: &mut W, identity: CaseIdentity) -> io::Result<()> {
    writer.write_all(&identity.case_id.to_le_bytes())?;
    writer.write_all(&identity.record_digest)
}

fn write_count<W: Write>(writer: &mut W, count: usize) -> io::Result<()> {
    let count = u32::try_from(count)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "count exceeds u32"))?;
    writer.write_all(&count.to_le_bytes())
}

fn write_counters<W: Write>(writer: &mut W, counters: PassCounters) -> io::Result<()> {
    writer.write_all(&counters.accepted_steps.to_le_bytes())?;
    writer.write_all(&counters.rejected_steps.to_le_bytes())?;
    writer.write_all(&counters.derivative_evaluations.to_le_bytes())
}

fn decode_frame_payload(
    tag: u8,
    payload: &[u8],
    limits: DecodeLimits,
) -> Result<Frame, CodecError> {
    match tag {
        1 => decode_handshake(payload),
        2 => decode_case_request(payload, limits),
        3 => decode_success(payload, limits)
            .map(|response| Frame::CaseResult(CaseResult::Success(response))),
        4 => decode_candidate_error(payload)
            .map(|response| Frame::CaseResult(CaseResult::CandidateError(response))),
        5 => {
            finish_empty(payload)?;
            Ok(Frame::Done)
        }
        other => Err(CodecError::UnknownFrameTag(other)),
    }
}

fn decode_handshake(payload: &[u8]) -> Result<Frame, CodecError> {
    if payload.len() < 16 {
        return Err(CodecError::PayloadLengthMismatch);
    }
    if payload.len() > 16 {
        return Err(CodecError::TrailingPayload);
    }
    let mut run_nonce = [0; 16];
    run_nonce.copy_from_slice(payload);
    Ok(Frame::Handshake { run_nonce })
}

fn decode_case_request(payload: &[u8], limits: DecodeLimits) -> Result<Frame, CodecError> {
    let mut cursor = PayloadCursor::new(payload);
    let identity = cursor.identity()?;
    let order = FilterOrder::from_tag(cursor.u8()?)?;
    let scalar_tag = cursor.u8()?;
    let (width, record_tag) = match scalar_tag {
        1 => (4, 1),
        2 => (8, 2),
        other => return Err(CodecError::UnknownScalarTag(other)),
    };
    let record = match record_tag {
        1 => {
            let cutoff_bits = cursor.u32()?;
            let interval_count_wire = cursor.u32()?;
            let sample_count_wire = cursor.u32()?;
            validate_wire_shape(interval_count_wire, sample_count_wire)?;
            let interval_count =
                usize::try_from(interval_count_wire).map_err(|_| CodecError::LengthOverflow)?;
            let sample_count =
                usize::try_from(sample_count_wire).map_err(|_| CodecError::LengthOverflow)?;
            check_sample_limit(sample_count, limits)?;
            ensure_record_payload_size(cursor.remaining(), interval_count, sample_count, width)?;
            let interval_bits = cursor.u32_values(interval_count)?;
            let sample_bits = cursor.u32_values(sample_count)?;
            RecordBits::F32 {
                cutoff_bits,
                interval_bits,
                sample_bits,
            }
        }
        2 => {
            let cutoff_bits = cursor.u64()?;
            let interval_count_wire = cursor.u32()?;
            let sample_count_wire = cursor.u32()?;
            validate_wire_shape(interval_count_wire, sample_count_wire)?;
            let interval_count =
                usize::try_from(interval_count_wire).map_err(|_| CodecError::LengthOverflow)?;
            let sample_count =
                usize::try_from(sample_count_wire).map_err(|_| CodecError::LengthOverflow)?;
            check_sample_limit(sample_count, limits)?;
            ensure_record_payload_size(cursor.remaining(), interval_count, sample_count, width)?;
            let interval_bits = cursor.u64_values(interval_count)?;
            let sample_bits = cursor.u64_values(sample_count)?;
            RecordBits::F64 {
                cutoff_bits,
                interval_bits,
                sample_bits,
            }
        }
        _ => unreachable!("scalar tag was matched above"),
    };
    cursor.finish()?;
    Ok(Frame::CaseRequest(CaseRequest {
        identity,
        order,
        record,
    }))
}

fn ensure_record_payload_size(
    remaining: usize,
    interval_count: usize,
    sample_count: usize,
    width: usize,
) -> Result<(), CodecError> {
    let value_count = interval_count
        .checked_add(sample_count)
        .ok_or(CodecError::LengthOverflow)?;
    let expected = value_count
        .checked_mul(width)
        .ok_or(CodecError::LengthOverflow)?;
    if remaining < expected {
        return Err(CodecError::PayloadLengthMismatch);
    }
    if remaining > expected {
        return Err(CodecError::TrailingPayload);
    }
    Ok(())
}

fn decode_success(payload: &[u8], limits: DecodeLimits) -> Result<SuccessResponse, CodecError> {
    let mut cursor = PayloadCursor::new(payload);
    let identity = cursor.identity()?;
    let scalar_tag = cursor.u8()?;
    let output_count = usize::try_from(cursor.u32()?).map_err(|_| CodecError::LengthOverflow)?;
    check_sample_limit(output_count, limits)?;
    if output_count == 0 {
        return Err(CodecError::ShapeMismatch);
    }
    let width = match scalar_tag {
        1 => 4,
        2 => 8,
        other => return Err(CodecError::UnknownScalarTag(other)),
    };
    let output_bytes = output_count
        .checked_mul(width)
        .ok_or(CodecError::LengthOverflow)?;
    let expected_remaining = output_bytes
        .checked_add(1)
        .and_then(|bytes| bytes.checked_add(SUCCESS_COUNTER_BYTES))
        .ok_or(CodecError::LengthOverflow)?;
    if cursor.remaining() < expected_remaining {
        return Err(CodecError::PayloadLengthMismatch);
    }

    let output_bits = match scalar_tag {
        1 => OutputBits::F32(cursor.u32_values(output_count)?),
        2 => OutputBits::F64(cursor.u64_values(output_count)?),
        _ => unreachable!("scalar tag was matched above"),
    };
    let tail_evidence = match (cursor.u8()?, scalar_tag) {
        (0, _) => TailEvidence::Omitted,
        (1, _) => TailEvidence::Analytic,
        (2, 1) => TailEvidence::NumericalF32 {
            final_residual_bits: cursor.u32()?,
            public_normalized_extension_bits: cursor.u32()?,
            analytic_output_bits: cursor.u32_values(output_count)?,
        },
        (2, 2) => TailEvidence::NumericalF64 {
            final_residual_bits: cursor.u64()?,
            public_normalized_extension_bits: cursor.u64()?,
            analytic_output_bits: cursor.u64_values(output_count)?,
        },
        (tag, _) => return Err(CodecError::InvalidTailEvidenceTag(tag)),
    };
    let forward = cursor.counters()?;
    let tail = cursor.counters()?;
    let reverse = cursor.counters()?;
    cursor.finish()?;
    let response = SuccessResponse {
        identity,
        output_bits,
        tail_evidence,
        forward,
        tail,
        reverse,
    };
    tail_evidence_payload_len(&response)?;
    Ok(response)
}

fn decode_candidate_error(payload: &[u8]) -> Result<CandidateErrorResponse, CodecError> {
    let mut cursor = PayloadCursor::new(payload);
    let identity = cursor.identity()?;
    let category = CandidateErrorCategory::from_tag(cursor.u8()?)?;
    let pass = Pass::from_tag(cursor.u8()?)?;
    let interval = match cursor.u8()? {
        0 => None,
        1 => Some(cursor.u64()?),
        other => return Err(CodecError::InvalidPresenceTag(other)),
    };
    let source_category = SourceCategory::from_tag(cursor.u8()?)?;
    cursor.finish()?;
    let response = CandidateErrorResponse {
        identity,
        category,
        pass,
        interval,
        source_category,
    };
    validate_candidate_error_context(&response)?;
    Ok(response)
}

fn finish_empty(payload: &[u8]) -> Result<(), CodecError> {
    match payload.len() {
        0 => Ok(()),
        _ => Err(CodecError::TrailingPayload),
    }
}

fn check_sample_limit(sample_count: usize, limits: DecodeLimits) -> Result<(), CodecError> {
    if sample_count > limits.max_samples {
        return Err(CodecError::SampleLimitExceeded {
            actual: sample_count,
            limit: limits.max_samples,
        });
    }
    Ok(())
}

fn read_exact_frame<R: Read>(reader: &mut R, bytes: &mut [u8]) -> Result<(), CodecError> {
    reader.read_exact(bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            CodecError::TruncatedFrame
        } else {
            CodecError::Io(error)
        }
    })
}

struct PayloadCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PayloadCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn identity(&mut self) -> Result<CaseIdentity, CodecError> {
        let case_id = self.u64()?;
        let digest = self.take(32)?;
        let mut record_digest = [0; 32];
        record_digest.copy_from_slice(digest);
        Ok(CaseIdentity {
            case_id,
            record_digest,
        })
    }

    fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, CodecError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| CodecError::PayloadLengthMismatch)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, CodecError> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| CodecError::PayloadLengthMismatch)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn u32_values(&mut self, count: usize) -> Result<Vec<u32>, CodecError> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| CodecError::AllocationFailed)?;
        for _ in 0..count {
            values.push(self.u32()?);
        }
        Ok(values)
    }

    fn u64_values(&mut self, count: usize) -> Result<Vec<u64>, CodecError> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| CodecError::AllocationFailed)?;
        for _ in 0..count {
            values.push(self.u64()?);
        }
        Ok(values)
    }

    fn counters(&mut self) -> Result<PassCounters, CodecError> {
        Ok(PassCounters {
            accepted_steps: self.u64()?,
            rejected_steps: self.u64()?,
            derivative_evaluations: self.u64()?,
        })
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], CodecError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(CodecError::LengthOverflow)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(CodecError::PayloadLengthMismatch)?;
        self.offset = end;
        Ok(bytes)
    }

    fn finish(&self) -> Result<(), CodecError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(CodecError::TrailingPayload)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: DecodeLimits = DecodeLimits::new(4096, 32);

    fn identity() -> CaseIdentity {
        CaseIdentity {
            case_id: 0x0102_0304_0506_0708,
            record_digest: [0xaa; 32],
        }
    }

    fn request_f32() -> CaseRequest {
        CaseRequest {
            identity: identity(),
            order: FilterOrder::Four,
            record: RecordBits::F32 {
                cutoff_bits: 0x3f80_0000,
                interval_bits: vec![0x3f00_0000],
                sample_bits: vec![0x3f80_0000, 0x4000_0000],
            },
        }
    }

    fn request_f64() -> CaseRequest {
        CaseRequest {
            identity: identity(),
            order: FilterOrder::Eight,
            record: RecordBits::F64 {
                cutoff_bits: 0x3ff0_0000_0000_0000,
                interval_bits: vec![0x3fe0_0000_0000_0000],
                sample_bits: vec![0x3ff0_0000_0000_0000, 0x4000_0000_0000_0000],
            },
        }
    }

    fn success_f32() -> SuccessResponse {
        SuccessResponse {
            identity: identity(),
            output_bits: OutputBits::F32(vec![0x3f80_0001, 0x4000_0001]),
            tail_evidence: TailEvidence::NumericalF32 {
                final_residual_bits: 0x3d80_0000,
                public_normalized_extension_bits: 0x4000_0000,
                analytic_output_bits: vec![0x3f80_0002, 0x4000_0002],
            },
            forward: PassCounters {
                accepted_steps: 11,
                rejected_steps: 2,
                derivative_evaluations: 91,
            },
            tail: PassCounters {
                accepted_steps: 5,
                rejected_steps: 1,
                derivative_evaluations: 42,
            },
            reverse: PassCounters {
                accepted_steps: 13,
                rejected_steps: 3,
                derivative_evaluations: 112,
            },
        }
    }

    fn candidate_error() -> CandidateErrorResponse {
        CandidateErrorResponse {
            identity: identity(),
            category: CandidateErrorCategory::Integration,
            pass: Some(Pass::Reverse),
            interval: Some(7),
            source_category: Some(SourceCategory::StepBudgetExceeded),
        }
    }

    fn encode(frame: &Frame) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, frame).expect("frame should encode");
        bytes
    }

    fn decode(bytes: &[u8], limits: DecodeLimits) -> Result<Option<Frame>, CodecError> {
        read_frame(&mut &bytes[..], limits)
    }

    fn raw_frame(version: u8, tag: u8, payload: &[u8]) -> Vec<u8> {
        let frame_len = u32::try_from(2 + payload.len()).expect("test frame length");
        let mut bytes = frame_len.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[version, tag]);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn set_payload_u32(frame: &mut [u8], payload_offset: usize, value: u32) {
        let offset = 6 + payload_offset;
        frame[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn round_trips_handshake_requests_responses_and_done() {
        let frames = [
            Frame::Handshake {
                run_nonce: [0x5a; 16],
            },
            Frame::CaseRequest(request_f32()),
            Frame::CaseRequest(request_f64()),
            Frame::CaseResult(CaseResult::Success(success_f32())),
            Frame::CaseResult(CaseResult::CandidateError(candidate_error())),
            Frame::Done,
        ];
        for frame in frames {
            let bytes = encode(&frame);
            assert_eq!(decode(&bytes, LIMITS).unwrap(), Some(frame));
        }

        let f64_success = SuccessResponse {
            identity: identity(),
            output_bits: OutputBits::F64(vec![0x3ff0_0000_0000_0001, 0x4000_0000_0000_0001]),
            tail_evidence: TailEvidence::NumericalF64 {
                final_residual_bits: 0x3fb0_0000_0000_0000,
                public_normalized_extension_bits: 0x4000_0000_0000_0000,
                analytic_output_bits: vec![0x3ff0_0000_0000_0002, 0x4000_0000_0000_0002],
            },
            forward: PassCounters::default(),
            tail: PassCounters {
                accepted_steps: 1,
                rejected_steps: 0,
                derivative_evaluations: 7,
            },
            reverse: PassCounters::default(),
        };
        let frame = Frame::CaseResult(CaseResult::Success(f64_success));
        assert_eq!(decode(&encode(&frame), LIMITS).unwrap(), Some(frame));

        let unusual_bits = Frame::CaseRequest(CaseRequest {
            identity: identity(),
            order: FilterOrder::One,
            record: RecordBits::F32 {
                cutoff_bits: 0x7fc0_1234,
                interval_bits: vec![0x0000_0001, 0x8000_0000],
                sample_bits: vec![0x8000_0000, 0x7f80_0000, 0xffc0_4321],
            },
        });
        assert_eq!(
            decode(&encode(&unusual_bits), LIMITS).unwrap(),
            Some(unusual_bits)
        );

        let unusual_output = Frame::CaseResult(CaseResult::Success(SuccessResponse {
            identity: identity(),
            output_bits: OutputBits::F32(vec![0x8000_0000, 0x0000_0001, 0x7fc0_1234]),
            tail_evidence: TailEvidence::Omitted,
            forward: PassCounters::default(),
            tail: PassCounters::default(),
            reverse: PassCounters::default(),
        }));
        assert_eq!(
            decode(&encode(&unusual_output), LIMITS).unwrap(),
            Some(unusual_output)
        );
    }

    #[test]
    fn frozen_wire_vectors_pin_frame_and_scalar_tags() {
        let handshake = Frame::Handshake {
            run_nonce: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        };
        assert_eq!(
            encode(&handshake),
            vec![
                18, 0, 0, 0, 1, 1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15
            ]
        );

        let case_f32 = Frame::CaseRequest(request_f32());
        let mut expected_case_f32 = vec![68, 0, 0, 0, 1, 2];
        expected_case_f32.extend_from_slice(&0x0102_0304_0506_0708_u64.to_le_bytes());
        expected_case_f32.extend_from_slice(&[0xaa; 32]);
        expected_case_f32.extend_from_slice(&[
            4, 1, 0, 0, 0x80, 0x3f, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0x3f, 0, 0, 0x80, 0x3f, 0, 0,
            0, 0x40,
        ]);
        assert_eq!(encode(&case_f32), expected_case_f32);

        let case_f64 = Frame::CaseRequest(request_f64());
        assert_eq!(encode(&case_f64)[47], 2);

        let success = Frame::CaseResult(CaseResult::Success(SuccessResponse {
            identity: CaseIdentity {
                case_id: 0,
                record_digest: [0; 32],
            },
            output_bits: OutputBits::F32(vec![0x3f80_0000]),
            tail_evidence: TailEvidence::Analytic,
            forward: PassCounters::default(),
            tail: PassCounters::default(),
            reverse: PassCounters::default(),
        }));
        let mut expected_success = vec![124, 0, 0, 0, 1, 3];
        expected_success.extend_from_slice(&[0; IDENTITY_BYTES]);
        expected_success.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0x80, 0x3f]);
        expected_success.push(1);
        expected_success.extend_from_slice(&[0; SUCCESS_COUNTER_BYTES]);
        assert_eq!(encode(&success), expected_success);

        let error = Frame::CaseResult(CaseResult::CandidateError(candidate_error()));
        let encoded_error = encode(&error);
        assert_eq!(&encoded_error[..6], &[54, 0, 0, 0, 1, 4]);
        assert_eq!(&encoded_error[46..], &[2, 2, 1, 7, 0, 0, 0, 0, 0, 0, 0, 3]);

        let error_without_location =
            Frame::CaseResult(CaseResult::CandidateError(CandidateErrorResponse {
                identity: identity(),
                category: CandidateErrorCategory::Validation,
                pass: None,
                interval: None,
                source_category: None,
            }));
        assert_eq!(
            decode(&encode(&error_without_location), LIMITS).unwrap(),
            Some(error_without_location)
        );

        assert_eq!(encode(&Frame::Done), vec![2, 0, 0, 0, 1, 5]);
    }

    #[test]
    fn validates_response_identity_scalar_and_shape() {
        let request = request_f32();
        let valid = CaseResult::Success(success_f32());
        assert!(validate_case_result(&request, &valid).is_ok());

        let mut mismatched = success_f32();
        mismatched.identity.case_id += 1;
        assert!(matches!(
            validate_case_result(&request, &CaseResult::Success(mismatched)),
            Err(CodecError::IdentityMismatch)
        ));

        let scalar_mismatch = CaseResult::Success(SuccessResponse {
            identity: request.identity,
            output_bits: OutputBits::F64(vec![0, 0]),
            tail_evidence: TailEvidence::Omitted,
            forward: PassCounters::default(),
            tail: PassCounters::default(),
            reverse: PassCounters::default(),
        });
        assert!(matches!(
            validate_case_result(&request, &scalar_mismatch),
            Err(CodecError::ScalarMismatch)
        ));

        let shape_mismatch = CaseResult::Success(SuccessResponse {
            identity: request.identity,
            output_bits: OutputBits::F32(vec![0]),
            tail_evidence: TailEvidence::Omitted,
            forward: PassCounters::default(),
            tail: PassCounters::default(),
            reverse: PassCounters::default(),
        });
        assert!(matches!(
            validate_case_result(&request, &shape_mismatch),
            Err(CodecError::OutputShapeMismatch)
        ));
    }

    #[test]
    fn validates_candidate_error_context() {
        let request = request_f32();
        let mut located_error = candidate_error();
        located_error.interval = Some(0);
        assert!(
            validate_case_result(&request, &CaseResult::CandidateError(located_error.clone()))
                .is_ok()
        );

        located_error.interval = Some(1);
        assert!(matches!(
            validate_case_result(&request, &CaseResult::CandidateError(located_error)),
            Err(CodecError::CandidateIntervalOutOfRange)
        ));

        let tail_error = CaseResult::CandidateError(CandidateErrorResponse {
            identity: request.identity,
            category: CandidateErrorCategory::TailWork,
            pass: Some(Pass::Tail),
            interval: Some(0),
            source_category: Some(SourceCategory::StepBudgetExceeded),
        });
        assert!(matches!(
            validate_case_result(&request, &tail_error),
            Err(CodecError::CandidateTailInterval)
        ));

        let contradictory_error = CaseResult::CandidateError(CandidateErrorResponse {
            identity: request.identity,
            category: CandidateErrorCategory::TailWork,
            pass: Some(Pass::Forward),
            interval: None,
            source_category: None,
        });
        assert!(matches!(
            validate_case_result(&request, &contradictory_error),
            Err(CodecError::CandidateErrorContextMismatch)
        ));

        let unlocated_integration = CaseResult::CandidateError(CandidateErrorResponse {
            identity: request.identity,
            category: CandidateErrorCategory::Integration,
            pass: None,
            interval: None,
            source_category: Some(SourceCategory::NonFiniteState),
        });
        assert!(matches!(
            validate_case_result(&request, &unlocated_integration),
            Err(CodecError::CandidateErrorContextMismatch)
        ));
    }

    #[test]
    fn validates_tail_evidence_shape() {
        let request = request_f32();
        let mut tail_scalar_mismatch = success_f32();
        tail_scalar_mismatch.tail_evidence = TailEvidence::NumericalF64 {
            final_residual_bits: 0,
            public_normalized_extension_bits: 0,
            analytic_output_bits: vec![0, 0],
        };
        assert!(matches!(
            validate_case_result(&request, &CaseResult::Success(tail_scalar_mismatch)),
            Err(CodecError::TailScalarMismatch)
        ));

        let mut tail_shape_mismatch = success_f32();
        if let TailEvidence::NumericalF32 {
            analytic_output_bits,
            ..
        } = &mut tail_shape_mismatch.tail_evidence
        {
            analytic_output_bits.pop();
        }
        assert!(matches!(
            validate_case_result(&request, &CaseResult::Success(tail_shape_mismatch)),
            Err(CodecError::TailOutputShapeMismatch)
        ));

        let mut tail_work_mismatch = success_f32();
        tail_work_mismatch.tail_evidence = TailEvidence::Analytic;
        assert!(matches!(
            validate_case_result(&request, &CaseResult::Success(tail_work_mismatch)),
            Err(CodecError::TailWorkMismatch)
        ));
    }

    #[test]
    fn rejects_unknown_version_and_frame_tag() {
        assert!(matches!(
            decode(&raw_frame(2, 5, &[]), LIMITS),
            Err(CodecError::InvalidVersion(2))
        ));
        assert!(matches!(
            decode(&raw_frame(1, 99, &[]), LIMITS),
            Err(CodecError::UnknownFrameTag(99))
        ));
        assert!(matches!(
            decode(&[0, 0, 0, 0], LIMITS),
            Err(CodecError::InvalidFrameLength)
        ));
    }

    #[test]
    fn rejects_invalid_order_scalar_and_scalar_width_mismatch() {
        let mut invalid_order = encode(&Frame::CaseRequest(request_f32()));
        invalid_order[46] = 3;
        assert!(matches!(
            decode(&invalid_order, LIMITS),
            Err(CodecError::InvalidOrder(3))
        ));

        let mut invalid_scalar = encode(&Frame::CaseRequest(request_f32()));
        invalid_scalar[47] = 9;
        assert!(matches!(
            decode(&invalid_scalar, LIMITS),
            Err(CodecError::UnknownScalarTag(9))
        ));

        let mut mismatched_width = encode(&Frame::CaseRequest(request_f32()));
        mismatched_width[47] = 2;
        assert!(decode(&mismatched_width, LIMITS).is_err());

        let mut invalid_tail_tag =
            encode(&Frame::CaseResult(CaseResult::Success(SuccessResponse {
                identity: identity(),
                output_bits: OutputBits::F32(vec![0]),
                tail_evidence: TailEvidence::Analytic,
                forward: PassCounters::default(),
                tail: PassCounters::default(),
                reverse: PassCounters::default(),
            })));
        invalid_tail_tag[55] = 9;
        assert!(matches!(
            decode(&invalid_tail_tag, LIMITS),
            Err(CodecError::InvalidTailEvidenceTag(9))
        ));
    }

    #[test]
    fn rejects_shape_overflow_and_trailing_payload() {
        let mut bad_shape = encode(&Frame::CaseRequest(request_f32()));
        set_payload_u32(&mut bad_shape, 46, 3);
        assert!(matches!(
            decode(&bad_shape, LIMITS),
            Err(CodecError::ShapeMismatch)
        ));

        let mut overflow_shape = encode(&Frame::CaseRequest(request_f32()));
        set_payload_u32(&mut overflow_shape, 46, u32::MAX);
        set_payload_u32(&mut overflow_shape, 50, 0);
        assert!(matches!(
            decode(&overflow_shape, LIMITS),
            Err(CodecError::LengthOverflow)
        ));

        assert!(matches!(
            decode(&raw_frame(1, 5, &[0]), LIMITS),
            Err(CodecError::TrailingPayload)
        ));

        let mut trailing_case = encode(&Frame::CaseRequest(request_f32()));
        trailing_case[0] += 1;
        trailing_case.push(0xee);
        assert!(matches!(
            decode(&trailing_case, LIMITS),
            Err(CodecError::TrailingPayload)
        ));
    }

    #[test]
    fn rejects_payload_and_sample_limits_before_record_allocation() {
        let handshake = encode(&Frame::Handshake { run_nonce: [0; 16] });
        assert!(matches!(
            decode(&handshake, DecodeLimits::new(15, 32)),
            Err(CodecError::PayloadTooLarge {
                actual: 16,
                limit: 15
            })
        ));

        let request = encode(&Frame::CaseRequest(request_f32()));
        assert!(matches!(
            decode(&request, DecodeLimits::new(4096, 1)),
            Err(CodecError::SampleLimitExceeded {
                actual: 2,
                limit: 1
            })
        ));

        let response = encode(&Frame::CaseResult(CaseResult::Success(success_f32())));
        assert!(matches!(
            decode(&response, DecodeLimits::new(4096, 1)),
            Err(CodecError::SampleLimitExceeded {
                actual: 2,
                limit: 1
            })
        ));

        let mut oversized_prefix = 100_u32.to_le_bytes().to_vec();
        oversized_prefix.extend_from_slice(&[1, 5]);
        assert!(matches!(
            decode(&oversized_prefix, DecodeLimits::new(8, 1)),
            Err(CodecError::PayloadTooLarge {
                actual: 98,
                limit: 8
            })
        ));
    }

    #[test]
    fn rejects_each_candidate_error_enum_tag() {
        let baseline = encode(&Frame::CaseResult(CaseResult::CandidateError(
            candidate_error(),
        )));

        let mut invalid_category = baseline.clone();
        invalid_category[46] = 9;
        assert!(matches!(
            decode(&invalid_category, LIMITS),
            Err(CodecError::InvalidCandidateCategory(9))
        ));

        let mut invalid_pass = baseline.clone();
        invalid_pass[47] = 9;
        assert!(matches!(
            decode(&invalid_pass, LIMITS),
            Err(CodecError::InvalidPassTag(9))
        ));

        let mut invalid_presence = baseline.clone();
        invalid_presence[48] = 2;
        assert!(matches!(
            decode(&invalid_presence, LIMITS),
            Err(CodecError::InvalidPresenceTag(2))
        ));

        let mut invalid_source = baseline;
        invalid_source[57] = 9;
        assert!(matches!(
            decode(&invalid_source, LIMITS),
            Err(CodecError::InvalidSourceCategory(9))
        ));
    }

    #[test]
    fn eof_is_accepted_only_between_frames() {
        assert_eq!(decode(&[], LIMITS).unwrap(), None);

        let frames = [
            Frame::Handshake {
                run_nonce: [0x7b; 16],
            },
            Frame::CaseRequest(request_f32()),
            Frame::CaseResult(CaseResult::Success(success_f32())),
            Frame::CaseResult(CaseResult::CandidateError(candidate_error())),
            Frame::Done,
        ];
        for frame in frames {
            let bytes = encode(&frame);
            for end in 1..bytes.len() {
                assert!(matches!(
                    decode(&bytes[..end], LIMITS),
                    Err(CodecError::TruncatedFrame)
                ));
            }
        }

        let mut stream = encode(&Frame::Handshake { run_nonce: [1; 16] });
        stream.extend_from_slice(&encode(&Frame::Done));
        let mut reader = &stream[..];
        assert!(matches!(
            read_frame(&mut reader, LIMITS).unwrap(),
            Some(Frame::Handshake { .. })
        ));
        assert_eq!(read_frame(&mut reader, LIMITS).unwrap(), Some(Frame::Done));
        assert_eq!(read_frame(&mut reader, LIMITS).unwrap(), None);
    }

    struct FragmentedReader<'a> {
        bytes: &'a [u8],
        position: usize,
        interrupt_next: bool,
    }

    impl Read for FragmentedReader<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.interrupt_next {
                self.interrupt_next = false;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            let Some(byte) = self.bytes.get(self.position) else {
                return Ok(0);
            };
            if output.is_empty() {
                return Ok(0);
            }
            output[0] = *byte;
            self.position += 1;
            self.interrupt_next = true;
            Ok(1)
        }
    }

    #[test]
    fn accepts_fragmented_reads_and_retries_interruptions() {
        let frame = Frame::CaseResult(CaseResult::Success(success_f32()));
        let bytes = encode(&frame);
        let mut reader = FragmentedReader {
            bytes: &bytes,
            position: 0,
            interrupt_next: true,
        };
        assert_eq!(read_frame(&mut reader, LIMITS).unwrap(), Some(frame));
        assert_eq!(read_frame(&mut reader, LIMITS).unwrap(), None);
    }

    struct FailingWriter {
        remaining: usize,
    }

    impl Write for FailingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "sentinel failure",
                ));
            }
            let written = bytes.len().min(self.remaining);
            self.remaining -= written;
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn propagates_write_failures() {
        let mut writer = FailingWriter { remaining: 3 };
        let error = write_frame(&mut writer, &Frame::Done).unwrap_err();
        match error {
            CodecError::Io(error) => assert_eq!(error.kind(), io::ErrorKind::BrokenPipe),
            other => panic!("expected the writer error, got {other:?}"),
        }
    }
}
