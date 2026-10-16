//! Test-only loopback client for the forward-backward endpoint-replay matrix.
//!
//! The oracle owns the listener and supplies one represented record at a time.
//! This module is deliberately not part of the library's runtime surface.

use std::env;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use crate::batch_protocol::{
    CandidateErrorCategory, CandidateErrorResponse, CaseIdentity, CaseRequest, CaseResult,
    DecodeLimits, FilterOrder, Frame, OutputBits, Pass, PassCounters, RecordBits, SourceCategory,
    SuccessResponse, TailEvidence, read_frame, validate_case_result, write_frame,
};

use crate::batch_prototype::{
    PrototypeError, PrototypePass, PrototypePassDiagnostics, endpoint_replay,
};
use crate::{IntegrationConfig, Scalar, UpdateError};

const ADDRESS_ENV: &str = "SSFILT_BATCH_MATRIX_ADDR";
const NONCE_ENV: &str = "SSFILT_BATCH_MATRIX_NONCE";
const MAX_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
const MAX_FROZEN_REFINED_SAMPLES: usize = 119_458;
// Leave explicit headroom above the largest frozen uneven scalar refinement.
const MAX_SAMPLES: usize = 131_072;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
// The isolated multiprecision oracle may pause between records. This only
// detects a dead peer; it is not a numerical or runtime gate.
const STREAM_TIMEOUT: Duration = Duration::from_mins(10);
const SMOKE_TIMEOUT: Duration = Duration::from_secs(5);

const DECODE_LIMITS: DecodeLimits = DecodeLimits::new(MAX_PAYLOAD_BYTES, MAX_SAMPLES);

type ClientResult<T> = Result<T, String>;

/// Run one oracle-owned loopback session. This ignored test is the entry point
/// used by the standalone matrix harness.
#[test]
#[ignore = "requires SSFILT_BATCH_MATRIX_ADDR and SSFILT_BATCH_MATRIX_NONCE from the oracle"]
fn batch_matrix_endpoint_client() {
    if let Err(error) = run_configured_client() {
        panic!("forward-backward matrix client failed: {error}");
    }
}

fn run_configured_client() -> ClientResult<()> {
    let address_text = env::var(ADDRESS_ENV)
        .map_err(|_| format!("{ADDRESS_ENV} must name the oracle's loopback SocketAddr"))?;
    let address = address_text
        .parse::<SocketAddr>()
        .map_err(|error| format!("{ADDRESS_ENV} is not a valid SocketAddr: {error}"))?;
    if !address.ip().is_loopback() {
        return Err(format!("{ADDRESS_ENV} must be a loopback SocketAddr"));
    }

    let nonce_text = env::var(NONCE_ENV)
        .map_err(|_| format!("{NONCE_ENV} must contain exactly 32 hexadecimal characters"))?;
    let nonce = parse_nonce(&nonce_text)?;

    let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
        .map_err(|error| format!("could not connect to {ADDRESS_ENV}: {error}"))?;
    set_stream_timeouts(&stream, STREAM_TIMEOUT)
        .map_err(|error| format!("could not configure matrix-client socket timeouts: {error}"))?;
    run_session(&mut stream, nonce)
}

fn parse_nonce(encoded: &str) -> ClientResult<[u8; 16]> {
    if encoded.len() != 32 {
        return Err(format!(
            "{NONCE_ENV} must contain exactly 32 hexadecimal characters"
        ));
    }

    let mut nonce = [0_u8; 16];
    let (pairs, remainder) = encoded.as_bytes().as_chunks::<2>();
    debug_assert!(remainder.is_empty());
    for (index, pair) in pairs.iter().enumerate() {
        let high = hex_nibble(pair[0]).ok_or_else(|| {
            format!(
                "{NONCE_ENV} contains a non-hexadecimal character at byte {}",
                index * 2
            )
        })?;
        let low = hex_nibble(pair[1]).ok_or_else(|| {
            format!(
                "{NONCE_ENV} contains a non-hexadecimal character at byte {}",
                index * 2 + 1
            )
        })?;
        nonce[index] = (high << 4) | low;
    }
    Ok(nonce)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn set_stream_timeouts(stream: &TcpStream, timeout: Duration) -> std::io::Result<()> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))
}

fn run_session<S: Read + Write>(stream: &mut S, nonce: [u8; 16]) -> ClientResult<()> {
    write_frame(stream, &Frame::Handshake { run_nonce: nonce })
        .map_err(|error| format!("could not write matrix handshake: {error}"))?;

    loop {
        let frame = read_frame(stream, DECODE_LIMITS)
            .map_err(|error| format!("could not read matrix request: {error}"))?;
        match frame {
            Some(Frame::CaseRequest(request)) => {
                let result = execute_request(&request)?;
                validate_case_result(&request, &result).map_err(|error| {
                    format!("candidate produced a protocol-invalid result: {error}")
                })?;
                write_frame(stream, &Frame::CaseResult(result))
                    .map_err(|error| format!("could not write candidate result: {error}"))?;
            }
            Some(Frame::Done) => return Ok(()),
            Some(other) => {
                return Err(format!(
                    "unexpected {other:?} frame in matrix request stream; expected CaseRequest or Done"
                ));
            }
            None => return Err("oracle closed the session before sending Done".to_owned()),
        }
    }
}

fn execute_request(request: &CaseRequest) -> ClientResult<CaseResult> {
    match &request.record {
        RecordBits::F32 {
            cutoff_bits,
            interval_bits,
            sample_bits,
        } => dispatch_order::<f32, u32>(
            request,
            *cutoff_bits,
            interval_bits,
            sample_bits,
            f32::from_bits,
            f32::to_bits,
        ),
        RecordBits::F64 {
            cutoff_bits,
            interval_bits,
            sample_bits,
        } => dispatch_order::<f64, u64>(
            request,
            *cutoff_bits,
            interval_bits,
            sample_bits,
            f64::from_bits,
            f64::to_bits,
        ),
    }
}

fn dispatch_order<T, Bits>(
    request: &CaseRequest,
    cutoff_bits: Bits,
    interval_bits: &[Bits],
    sample_bits: &[Bits],
    from_bits: fn(Bits) -> T,
    to_bits: fn(T) -> Bits,
) -> ClientResult<CaseResult>
where
    T: Scalar,
    Bits: Copy + IntoOutputBits,
    IntegrationConfig<T>: Default,
{
    match request.order {
        FilterOrder::One => execute_order::<1, T, Bits>(
            request.identity,
            cutoff_bits,
            interval_bits,
            sample_bits,
            from_bits,
            to_bits,
        ),
        FilterOrder::Two => execute_order::<2, T, Bits>(
            request.identity,
            cutoff_bits,
            interval_bits,
            sample_bits,
            from_bits,
            to_bits,
        ),
        FilterOrder::Four => execute_order::<4, T, Bits>(
            request.identity,
            cutoff_bits,
            interval_bits,
            sample_bits,
            from_bits,
            to_bits,
        ),
        FilterOrder::Eight => execute_order::<8, T, Bits>(
            request.identity,
            cutoff_bits,
            interval_bits,
            sample_bits,
            from_bits,
            to_bits,
        ),
    }
}

fn execute_order<const N: usize, T, Bits>(
    identity: CaseIdentity,
    cutoff_bits: Bits,
    interval_bits: &[Bits],
    sample_bits: &[Bits],
    from_bits: fn(Bits) -> T,
    to_bits: fn(T) -> Bits,
) -> ClientResult<CaseResult>
where
    T: Scalar,
    Bits: Copy + IntoOutputBits,
    IntegrationConfig<T>: Default,
{
    let cutoff_hz = from_bits(cutoff_bits);
    let intervals_seconds = decode_values(interval_bits, from_bits, "interval")?;
    let samples = decode_values(sample_bits, from_bits, "sample")?;
    let Some(workspace_length) = samples.len().checked_mul(2) else {
        return Ok(prototype_error_result(
            identity,
            PrototypeError::WorkspaceSizeOverflow,
        ));
    };

    let mut output = zeroed_values(samples.len(), "output")?;
    let mut workspace = zeroed_values(workspace_length, "workspace")?;
    let diagnostics = match endpoint_replay::<N, T>(
        cutoff_hz,
        &intervals_seconds,
        &samples,
        &mut output,
        &mut workspace,
    ) {
        Ok(diagnostics) => diagnostics,
        Err(error) => return Ok(prototype_error_result(identity, error)),
    };

    let Some(forward) = pass_counters(diagnostics.forward) else {
        return Ok(work_counter_overflow(identity));
    };
    let Some(reverse) = pass_counters(diagnostics.reverse) else {
        return Ok(work_counter_overflow(identity));
    };
    let mut output_bits = reserved_vec(output.len(), "output bit patterns")?;
    output_bits.extend(output.into_iter().map(to_bits));

    Ok(CaseResult::Success(SuccessResponse {
        identity,
        output_bits: output_bits_for_scalar(output_bits),
        tail_evidence: TailEvidence::Omitted,
        forward,
        tail: PassCounters::default(),
        reverse,
    }))
}

// This scalar wrapper preserves the exact bit width selected by the request.
fn output_bits_for_scalar<Bits>(bits: Vec<Bits>) -> OutputBits
where
    Bits: IntoOutputBits,
{
    Bits::wrap(bits)
}

trait IntoOutputBits: Sized {
    fn wrap(values: Vec<Self>) -> OutputBits;
}

impl IntoOutputBits for u32 {
    fn wrap(values: Vec<Self>) -> OutputBits {
        OutputBits::F32(values)
    }
}

impl IntoOutputBits for u64 {
    fn wrap(values: Vec<Self>) -> OutputBits {
        OutputBits::F64(values)
    }
}

fn decode_values<T, Bits>(
    bits: &[Bits],
    from_bits: fn(Bits) -> T,
    description: &str,
) -> ClientResult<Vec<T>>
where
    Bits: Copy,
{
    let mut values = reserved_vec(bits.len(), description)?;
    values.extend(bits.iter().copied().map(from_bits));
    Ok(values)
}

fn zeroed_values<T: Scalar>(length: usize, description: &str) -> ClientResult<Vec<T>> {
    let mut values = reserved_vec(length, description)?;
    values.resize(length, T::zero());
    Ok(values)
}

fn reserved_vec<T>(length: usize, description: &str) -> ClientResult<Vec<T>> {
    length
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(|| format!("{description} allocation size overflowed"))?;
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(|error| {
        format!("could not allocate {description} for {length} values: {error}")
    })?;
    Ok(values)
}

fn pass_counters(diagnostics: PrototypePassDiagnostics) -> Option<PassCounters> {
    Some(PassCounters {
        accepted_steps: u64::try_from(diagnostics.accepted_steps).ok()?,
        rejected_steps: u64::try_from(diagnostics.rejected_steps).ok()?,
        derivative_evaluations: u64::try_from(diagnostics.derivative_evaluations).ok()?,
    })
}

fn prototype_error_result(identity: CaseIdentity, error: PrototypeError) -> CaseResult {
    match error {
        PrototypeError::InvalidInterval { index }
        | PrototypeError::IntervalOutOfScope { index } => match u64::try_from(index) {
            Ok(interval) => candidate_error(
                identity,
                CandidateErrorCategory::Validation,
                None,
                Some(interval),
                None,
            ),
            Err(_) => work_counter_overflow(identity),
        },
        PrototypeError::NonFiniteSample { .. } => candidate_error(
            identity,
            CandidateErrorCategory::Validation,
            None,
            None,
            Some(SourceCategory::NonFiniteInput),
        ),
        PrototypeError::Integration {
            pass,
            interval,
            source,
        } => {
            let Ok(interval) = u64::try_from(interval) else {
                return work_counter_overflow(identity);
            };
            candidate_error(
                identity,
                CandidateErrorCategory::Integration,
                Some(match pass {
                    PrototypePass::Forward => Pass::Forward,
                    PrototypePass::Reverse => Pass::Reverse,
                }),
                Some(interval),
                Some(update_source_category(source)),
            )
        }
        PrototypeError::TailInitialization => candidate_error(
            identity,
            CandidateErrorCategory::Integration,
            Some(Pass::Tail),
            None,
            Some(SourceCategory::NonFiniteState),
        ),
        PrototypeError::WorkCounterOverflow => work_counter_overflow(identity),
        PrototypeError::ZeroOrder
        | PrototypeError::UnsupportedOrder
        | PrototypeError::InvalidCutoff
        | PrototypeError::InvalidIntegrationConfig
        | PrototypeError::EmptyRecord
        | PrototypeError::ShapeMismatch
        | PrototypeError::OutputLength
        | PrototypeError::WorkspaceSizeOverflow
        | PrototypeError::WorkspaceTooSmall { .. } => candidate_error(
            identity,
            CandidateErrorCategory::Validation,
            None,
            None,
            None,
        ),
    }
}

fn update_source_category(error: UpdateError) -> SourceCategory {
    match error {
        UpdateError::NonFiniteInput => SourceCategory::NonFiniteInput,
        UpdateError::InvalidDeltaTime => SourceCategory::InvalidDeltaTime,
        UpdateError::StepBudgetExceeded => SourceCategory::StepBudgetExceeded,
        UpdateError::StepSizeUnderflow => SourceCategory::StepSizeUnderflow,
        UpdateError::NonFiniteState => SourceCategory::NonFiniteState,
    }
}

fn work_counter_overflow(identity: CaseIdentity) -> CaseResult {
    candidate_error(
        identity,
        CandidateErrorCategory::WorkCounterOverflow,
        None,
        None,
        None,
    )
}

fn candidate_error(
    identity: CaseIdentity,
    category: CandidateErrorCategory,
    pass: Option<Pass>,
    interval: Option<u64>,
    source_category: Option<SourceCategory>,
) -> CaseResult {
    CaseResult::CandidateError(CandidateErrorResponse {
        identity,
        category,
        pass,
        interval,
        source_category,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn identity() -> CaseIdentity {
        CaseIdentity {
            case_id: 17,
            record_digest: [0x5a; 32],
        }
    }

    fn request_f32(identity: CaseIdentity) -> CaseRequest {
        CaseRequest {
            identity,
            order: FilterOrder::One,
            record: RecordBits::F32 {
                cutoff_bits: 1.0_f32.to_bits(),
                interval_bits: vec![0.01_f32.to_bits()],
                sample_bits: vec![0.25_f32.to_bits(), 0.75_f32.to_bits()],
            },
        }
    }

    fn request_nonfinite_f64(identity: CaseIdentity) -> CaseRequest {
        CaseRequest {
            identity,
            order: FilterOrder::Two,
            record: RecordBits::F64 {
                cutoff_bits: 1.0_f64.to_bits(),
                interval_bits: vec![0.01_f64.to_bits()],
                sample_bits: vec![0.25_f64.to_bits(), f64::INFINITY.to_bits()],
            },
        }
    }

    fn write_test_frame<W: Write>(writer: &mut W, frame: &Frame) -> ClientResult<()> {
        write_frame(writer, frame).map_err(|error| format!("test peer write failed: {error}"))
    }

    fn read_test_frame<R: Read>(reader: &mut R) -> ClientResult<Frame> {
        read_frame(reader, DECODE_LIMITS)
            .map_err(|error| format!("test peer read failed: {error}"))?
            .ok_or_else(|| "matrix client closed before the peer finished".to_owned())
    }

    fn receive_case_result<R: Read>(
        reader: &mut R,
        request: &CaseRequest,
    ) -> ClientResult<CaseResult> {
        let result = match read_test_frame(reader)? {
            Frame::CaseResult(result) => result,
            other => return Err(format!("expected CaseResult, received {other:?}")),
        };
        validate_case_result(request, &result)
            .map_err(|error| format!("client result failed protocol validation: {error}"))?;
        Ok(result)
    }

    fn test_peer(mut stream: TcpStream, nonce: [u8; 16]) -> ClientResult<()> {
        set_stream_timeouts(&stream, SMOKE_TIMEOUT)
            .map_err(|error| format!("could not set smoke-peer timeouts: {error}"))?;
        match read_test_frame(&mut stream)? {
            Frame::Handshake { run_nonce } if run_nonce == nonce => {}
            other => return Err(format!("unexpected client handshake: {other:?}")),
        }

        let valid = request_f32(CaseIdentity {
            case_id: 41,
            record_digest: [0x41; 32],
        });
        write_test_frame(&mut stream, &Frame::CaseRequest(valid.clone()))?;
        match receive_case_result(&mut stream, &valid)? {
            CaseResult::Success(response) => {
                assert!(matches!(response.output_bits, OutputBits::F32(_)));
                assert_eq!(response.tail_evidence, TailEvidence::Omitted);
                assert_eq!(response.tail, PassCounters::default());
            }
            CaseResult::CandidateError(error) => {
                return Err(format!("valid smoke case failed: {error:?}"));
            }
        }

        let invalid = request_nonfinite_f64(CaseIdentity {
            case_id: 42,
            record_digest: [0x42; 32],
        });
        write_test_frame(&mut stream, &Frame::CaseRequest(invalid.clone()))?;
        match receive_case_result(&mut stream, &invalid)? {
            CaseResult::CandidateError(error) => {
                assert_eq!(error.category, CandidateErrorCategory::Validation);
                assert_eq!(error.pass, None);
                assert_eq!(error.interval, None);
                assert_eq!(error.source_category, Some(SourceCategory::NonFiniteInput));
            }
            CaseResult::Success(_) => {
                return Err("non-finite smoke case unexpectedly succeeded".to_owned());
            }
        }

        write_test_frame(&mut stream, &Frame::Done)
    }

    #[test]
    fn parses_exact_nonce_hex() {
        assert_eq!(
            parse_nonce("000102030405060708090a0b0c0d0e0F").unwrap(),
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        );
        assert!(parse_nonce("0011").unwrap_err().contains("32 hexadecimal"));
        assert!(parse_nonce("000102030405060708090a0b0c0d0e0g").is_err());
    }

    #[test]
    fn maps_prototype_errors_to_protocol_context() {
        let invalid_interval =
            prototype_error_result(identity(), PrototypeError::InvalidInterval { index: 3 });
        assert_eq!(
            invalid_interval,
            candidate_error(
                identity(),
                CandidateErrorCategory::Validation,
                None,
                Some(3),
                None,
            )
        );

        let out_of_scope =
            prototype_error_result(identity(), PrototypeError::IntervalOutOfScope { index: 2 });
        assert_eq!(
            out_of_scope,
            candidate_error(
                identity(),
                CandidateErrorCategory::Validation,
                None,
                Some(2),
                None,
            )
        );

        let nonfinite_sample =
            prototype_error_result(identity(), PrototypeError::NonFiniteSample { index: 7 });
        assert_eq!(
            nonfinite_sample,
            candidate_error(
                identity(),
                CandidateErrorCategory::Validation,
                None,
                None,
                Some(SourceCategory::NonFiniteInput),
            )
        );

        let integration = prototype_error_result(
            identity(),
            PrototypeError::Integration {
                pass: PrototypePass::Reverse,
                interval: 4,
                source: UpdateError::InvalidDeltaTime,
            },
        );
        assert_eq!(
            integration,
            candidate_error(
                identity(),
                CandidateErrorCategory::Integration,
                Some(Pass::Reverse),
                Some(4),
                Some(SourceCategory::InvalidDeltaTime),
            )
        );

        assert_eq!(
            prototype_error_result(identity(), PrototypeError::TailInitialization),
            candidate_error(
                identity(),
                CandidateErrorCategory::Integration,
                Some(Pass::Tail),
                None,
                Some(SourceCategory::NonFiniteState),
            )
        );

        assert_eq!(
            prototype_error_result(identity(), PrototypeError::WorkCounterOverflow),
            work_counter_overflow(identity())
        );
    }

    fn assert_nonconstant_f32_dispatch<const N: usize>(order: FilterOrder) {
        let cutoff_hz = 2.0_f32;
        let intervals = [0.005_f32, 0.003_f32];
        let samples = [-0.25_f32, 0.75_f32, 0.125_f32];
        let request = CaseRequest {
            identity: identity(),
            order,
            record: RecordBits::F32 {
                cutoff_bits: cutoff_hz.to_bits(),
                interval_bits: intervals.iter().map(|value| value.to_bits()).collect(),
                sample_bits: samples.iter().map(|value| value.to_bits()).collect(),
            },
        };

        let mut expected = [0.0; 3];
        let mut workspace = [0.0; 6];
        let diagnostics = endpoint_replay::<N, f32>(
            cutoff_hz,
            &intervals,
            &samples,
            &mut expected,
            &mut workspace,
        )
        .expect("direct f32 prototype should run");

        let result = execute_request(&request).expect("f32 candidate should run");
        validate_case_result(&request, &result).expect("f32 result should validate");
        let CaseResult::Success(response) = result else {
            panic!("valid f32 request returned a candidate error");
        };
        assert_eq!(
            response.output_bits,
            OutputBits::F32(expected.map(f32::to_bits).to_vec())
        );
        assert_eq!(
            response.forward,
            pass_counters(diagnostics.forward).unwrap()
        );
        assert_eq!(
            response.reverse,
            pass_counters(diagnostics.reverse).unwrap()
        );
        assert_eq!(response.tail_evidence, TailEvidence::Omitted);
        assert_eq!(response.tail, PassCounters::default());
    }

    fn assert_nonconstant_f64_dispatch<const N: usize>(order: FilterOrder) {
        let cutoff_hz = 2.0_f64;
        let intervals = [0.005_f64, 0.003_f64];
        let samples = [-0.25_f64, 0.75_f64, 0.125_f64];
        let request = CaseRequest {
            identity: identity(),
            order,
            record: RecordBits::F64 {
                cutoff_bits: cutoff_hz.to_bits(),
                interval_bits: intervals.iter().map(|value| value.to_bits()).collect(),
                sample_bits: samples.iter().map(|value| value.to_bits()).collect(),
            },
        };

        let mut expected = [0.0; 3];
        let mut workspace = [0.0; 6];
        let diagnostics = endpoint_replay::<N, f64>(
            cutoff_hz,
            &intervals,
            &samples,
            &mut expected,
            &mut workspace,
        )
        .expect("direct f64 prototype should run");

        let result = execute_request(&request).expect("f64 candidate should run");
        validate_case_result(&request, &result).expect("f64 result should validate");
        let CaseResult::Success(response) = result else {
            panic!("valid f64 request returned a candidate error");
        };
        assert_eq!(
            response.output_bits,
            OutputBits::F64(expected.map(f64::to_bits).to_vec())
        );
        assert_eq!(
            response.forward,
            pass_counters(diagnostics.forward).unwrap()
        );
        assert_eq!(
            response.reverse,
            pass_counters(diagnostics.reverse).unwrap()
        );
        assert_eq!(response.tail_evidence, TailEvidence::Omitted);
        assert_eq!(response.tail, PassCounters::default());
    }

    #[test]
    fn dispatches_all_orders_and_scalars_with_exact_outputs_and_work() {
        assert_nonconstant_f32_dispatch::<1>(FilterOrder::One);
        assert_nonconstant_f32_dispatch::<2>(FilterOrder::Two);
        assert_nonconstant_f32_dispatch::<4>(FilterOrder::Four);
        assert_nonconstant_f32_dispatch::<8>(FilterOrder::Eight);
        assert_nonconstant_f64_dispatch::<1>(FilterOrder::One);
        assert_nonconstant_f64_dispatch::<2>(FilterOrder::Two);
        assert_nonconstant_f64_dispatch::<4>(FilterOrder::Four);
        assert_nonconstant_f64_dispatch::<8>(FilterOrder::Eight);
    }

    #[test]
    fn decode_limits_admit_the_largest_frozen_scalar_refinement() {
        let interval_count = MAX_FROZEN_REFINED_SAMPLES - 1;
        let request = CaseRequest {
            identity: identity(),
            order: FilterOrder::Eight,
            record: RecordBits::F64 {
                cutoff_bits: 37.0_f64.to_bits(),
                interval_bits: vec![0.000_01_f64.to_bits(); interval_count],
                sample_bits: vec![0.0_f64.to_bits(); MAX_FROZEN_REFINED_SAMPLES],
            },
        };
        let frame = Frame::CaseRequest(request.clone());
        let mut encoded = Vec::new();
        write_frame(&mut encoded, &frame).expect("largest frozen request should encode");
        assert!(encoded.len() - 6 < MAX_PAYLOAD_BYTES);
        assert_eq!(
            read_frame(&mut encoded.as_slice(), DECODE_LIMITS)
                .expect("largest frozen request should decode"),
            Some(Frame::CaseRequest(request))
        );
    }

    #[test]
    fn localhost_session_handshakes_processes_success_and_candidate_error_then_finishes() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener should bind");
        listener
            .set_nonblocking(true)
            .expect("smoke listener should accept with a bounded poll loop");
        let address = listener
            .local_addr()
            .expect("loopback listener should have an address");
        let nonce = [0x6d; 16];

        let client = thread::spawn(move || -> ClientResult<()> {
            let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))
                .map_err(|error| format!("smoke client could not connect: {error}"))?;
            set_stream_timeouts(&stream, SMOKE_TIMEOUT)
                .map_err(|error| format!("could not set smoke-client timeouts: {error}"))?;
            run_session(&mut stream, nonce)
        });

        let deadline = Instant::now() + SMOKE_TIMEOUT;
        let (stream, _) = loop {
            match listener.accept() {
                Ok(accepted) => break accepted,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "smoke client did not connect within the 5s deadline"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("smoke listener accept failed: {error}"),
            }
        };
        let peer_result = test_peer(stream, nonce);
        peer_result.unwrap_or_else(|error| panic!("smoke peer failed: {error}"));
        client
            .join()
            .expect("smoke client thread should not panic")
            .unwrap_or_else(|error| panic!("smoke client failed: {error}"));
    }
}
