#![forbid(unsafe_code)]

//! Private subprocess and loopback transport for the root candidate client.

use std::env;
use std::ffi::OsString;
use std::fmt::{self, Write as FmtWrite};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forward_backward_protocol::{
    CandidateErrorCategory, CandidateErrorResponse, CaseIdentity, CaseRequest, CaseResult,
    DecodeLimits, FilterOrder, Frame, OutputBits, RecordBits, SourceCategory, read_frame,
    validate_case_result, write_frame,
};

use crate::CandidateKind;

const ADDRESS_ENV: &str = "SSFILT_BATCH_MATRIX_ADDR";
const NONCE_ENV: &str = "SSFILT_BATCH_MATRIX_NONCE";
const COLOR_ENV: &str = "CARGO_TERM_COLOR";
const MAX_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
const MAX_SAMPLES: usize = 131_072;
const DECODE_LIMITS: DecodeLimits = DecodeLimits::new(MAX_PAYLOAD_BYTES, MAX_SAMPLES);

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const CHILD_EXIT_TIMEOUT: Duration = Duration::from_secs(30);
const CHILD_CLEANUP_GRACE: Duration = Duration::from_millis(250);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(20);

const OUTPUT_TAIL_BYTES: usize = 64 * 1024;
const OUTPUT_READ_CHUNK_BYTES: usize = 8 * 1024;

static NONCE_COUNTER: AtomicU32 = AtomicU32::new(0);

type SharedTail = Arc<Mutex<TailBuffer>>;

/// Runs a compact end-to-end transport and dispatch check using the same root
/// test client as the full matrix harness.
pub fn run_session_smoke() -> Result<String, String> {
    let mut session =
        CandidateSession::start(CandidateKind::Endpoint).map_err(|error| error.to_string())?;

    let success_request = smoke_success_request();
    let success_result = session
        .round_trip(&success_request)
        .map_err(|error| error.to_string())?;
    let success_samples = match success_result {
        CaseResult::Success(response) => match response.output_bits {
            OutputBits::F32(values) => values.len(),
            OutputBits::F64(values) => values.len(),
        },
        CaseResult::CandidateError(response) => {
            return Err(session
                .fail_and_cleanup(
                    SessionErrorKind::InvalidRequest,
                    format!("valid smoke request returned candidate error {response:?}"),
                )
                .to_string());
        }
    };

    let invalid_request = smoke_non_finite_request();
    let invalid_result = session
        .round_trip(&invalid_request)
        .map_err(|error| error.to_string())?;
    match invalid_result {
        CaseResult::CandidateError(CandidateErrorResponse {
            category: CandidateErrorCategory::Validation,
            pass: None,
            interval: None,
            source_category: Some(SourceCategory::NonFiniteInput),
            ..
        }) => {}
        other => {
            return Err(session
                .fail_and_cleanup(
                    SessionErrorKind::InvalidRequest,
                    format!("non-finite smoke request returned unexpected result {other:?}"),
                )
                .to_string());
        }
    }

    session.finish().map_err(|error| error.to_string())?;
    Ok(format!(
        "session-smoke passed: validated {success_samples}-sample Success, expected Validation/NonFiniteInput response, and clean Done shutdown"
    ))
}

pub(crate) struct CandidateSession {
    child: Option<Child>,
    stream: Option<TcpStream>,
    stdout_tail: SharedTail,
    stderr_tail: SharedTail,
}

impl CandidateSession {
    pub(crate) fn start(candidate: CandidateKind) -> Result<Self, SessionError> {
        let listener =
            TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).map_err(|error| {
                SessionError::bare(
                    SessionErrorKind::Io,
                    format!("could not bind the IPv4 loopback listener: {error}"),
                )
            })?;
        listener.set_nonblocking(true).map_err(|error| {
            SessionError::bare(
                SessionErrorKind::Io,
                format!("could not make the loopback listener nonblocking: {error}"),
            )
        })?;
        let address = match listener.local_addr() {
            Ok(SocketAddr::V4(address)) => address,
            Ok(SocketAddr::V6(_)) => {
                return Err(SessionError::bare(
                    SessionErrorKind::Io,
                    "the session listener did not bind IPv4".to_owned(),
                ));
            }
            Err(error) => {
                return Err(SessionError::bare(
                    SessionErrorKind::Io,
                    format!("could not read the loopback listener address: {error}"),
                ));
            }
        };

        let nonce = new_nonce();
        let encoded_nonce = encode_nonce(&nonce);
        let repository_root = repository_root().map_err(|error| {
            SessionError::bare(
                SessionErrorKind::Startup,
                format!("could not locate the ssfilt repository root: {error}"),
            )
        })?;
        let command_spec = candidate_command_spec(
            &repository_root,
            address,
            &encoded_nonce,
            env::var_os("CARGO"),
            candidate,
        );
        let mut command = command_spec.to_command();
        let child = command.spawn().map_err(|error| {
            SessionError::bare(
                SessionErrorKind::Startup,
                format!(
                    "could not start candidate command {}: {error}",
                    command_spec.program.to_string_lossy()
                ),
            )
        })?;

        let mut session = Self {
            child: Some(child),
            stream: None,
            stdout_tail: Arc::new(Mutex::new(TailBuffer::default())),
            stderr_tail: Arc::new(Mutex::new(TailBuffer::default())),
        };
        if let Err(message) = session.start_output_drainers() {
            return Err(session.fail_and_cleanup(SessionErrorKind::Startup, message));
        }

        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let accepted = accept_correlated_connection(&listener, nonce, deadline, || {
            match session.child.as_mut() {
                Some(child) => child.try_wait(),
                None => Err(io::Error::other(
                    "candidate child ownership was lost during startup",
                )),
            }
        });
        let stream = match accepted {
            Ok(stream) => stream,
            Err(error) => {
                return Err(session.fail_and_cleanup(error.kind, error.message));
            }
        };
        if let Err(error) = set_stream_timeouts(&stream, SOCKET_IO_TIMEOUT) {
            drop(stream);
            return Err(session.fail_and_cleanup(
                SessionErrorKind::Io,
                format!("could not configure candidate socket timeouts: {error}"),
            ));
        }
        session.stream = Some(stream);
        Ok(session)
    }

    fn start_output_drainers(&mut self) -> Result<(), String> {
        let (stdout, stderr) = {
            let child = self.child.as_mut().ok_or_else(|| {
                "candidate child ownership was lost before output draining started".to_owned()
            })?;
            (child.stdout.take(), child.stderr.take())
        };
        let Some(stdout) = stdout else {
            return Err("candidate stdout was not piped".to_owned());
        };
        let Some(stderr) = stderr else {
            return Err("candidate stderr was not piped".to_owned());
        };

        spawn_output_drainer(stdout, Arc::clone(&self.stdout_tail), "candidate-stdout")
            .map_err(|error| format!("could not drain candidate stdout: {error}"))?;
        spawn_output_drainer(stderr, Arc::clone(&self.stderr_tail), "candidate-stderr")
            .map_err(|error| format!("could not drain candidate stderr: {error}"))?;
        Ok(())
    }

    pub(crate) fn round_trip(&mut self, request: &CaseRequest) -> Result<CaseResult, SessionError> {
        let exchange = match self.stream.as_mut() {
            Some(stream) => exchange_one_case(stream, request),
            None => {
                return Err(self.fail_and_cleanup(
                    SessionErrorKind::Io,
                    "candidate socket is already closed".to_owned(),
                ));
            }
        };
        match exchange {
            Ok(result) => Ok(result),
            Err(error) => Err(self.fail_and_cleanup(
                error.session_kind(),
                format!("candidate case round trip failed: {error}"),
            )),
        }
    }

    pub(crate) fn finish(mut self) -> Result<(), SessionError> {
        let write_result = match self.stream.as_mut() {
            Some(stream) => write_frame(stream, &Frame::Done),
            None => {
                return Err(self.fail_and_cleanup(
                    SessionErrorKind::Io,
                    "candidate socket is already closed before Done".to_owned(),
                ));
            }
        };
        if let Err(error) = write_result {
            let kind = if codec_error_is_timeout(&error) {
                SessionErrorKind::Timeout
            } else {
                SessionErrorKind::Io
            };
            return Err(
                self.fail_and_cleanup(kind, format!("could not send Done to candidate: {error}"))
            );
        }
        if let Some(stream) = self.stream.take()
            && let Err(error) = stream.shutdown(Shutdown::Both)
        {
            return Err(self.fail_and_cleanup(
                SessionErrorKind::Io,
                format!("could not close candidate socket after Done: {error}"),
            ));
        }

        let deadline = Instant::now() + CHILD_EXIT_TIMEOUT;
        loop {
            let status_result = match self.child.as_mut() {
                Some(child) => child.try_wait(),
                None => {
                    return Err(self.fail_and_cleanup(
                        SessionErrorKind::Io,
                        "candidate child ownership was lost before shutdown completed".to_owned(),
                    ));
                }
            };
            let status = match status_result {
                Ok(status) => status,
                Err(error) => {
                    return Err(self.fail_and_cleanup(
                        SessionErrorKind::Io,
                        format!("could not inspect candidate exit status: {error}"),
                    ));
                }
            };
            if status.is_some() {
                break;
            }
            if Instant::now() >= deadline {
                return Err(self.fail_and_cleanup(
                    SessionErrorKind::Timeout,
                    "candidate did not exit within 30 seconds after Done".to_owned(),
                ));
            }
            thread::sleep(CHILD_POLL_INTERVAL);
        }

        let wait_result = match self.child.as_mut() {
            Some(child) => child.wait(),
            None => {
                return Err(self.fail_and_cleanup(
                    SessionErrorKind::Io,
                    "candidate child ownership was lost before it could be reaped".to_owned(),
                ));
            }
        };
        let status = match wait_result {
            Ok(status) => status,
            Err(error) => {
                return Err(self.fail_and_cleanup(
                    SessionErrorKind::Io,
                    format!("could not wait for candidate exit: {error}"),
                ));
            }
        };
        self.child.take();
        if status.success() {
            Ok(())
        } else {
            Err(self.failure_after_cleanup(
                SessionErrorKind::ChildExit,
                format!("candidate exited unsuccessfully after Done ({status})"),
            ))
        }
    }

    fn fail_and_cleanup(&mut self, kind: SessionErrorKind, message: String) -> SessionError {
        let cleanup_failures = self.terminate();
        let mut error = SessionError::bare(kind, message);
        error.cleanup_failures = cleanup_failures;
        error.stdout = snapshot_tail(&self.stdout_tail);
        error.stderr = snapshot_tail(&self.stderr_tail);
        error
    }

    fn failure_after_cleanup(&self, kind: SessionErrorKind, message: String) -> SessionError {
        SessionError::with_tails(kind, message, &self.stdout_tail, &self.stderr_tail)
    }

    fn terminate(&mut self) -> Vec<String> {
        let mut failures = Vec::new();
        if let Some(stream) = self.stream.take()
            && let Err(error) = stream.shutdown(Shutdown::Both)
        {
            failures.push(format!("could not close candidate socket: {error}"));
        }
        if let Some(child) = self.child.as_mut() {
            let outcome = cleanup_child(child);
            failures.extend(outcome.failures);
            if outcome.reaped {
                self.child.take();
            }
        }
        failures
    }
}

impl Drop for CandidateSession {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

struct ChildCleanupOutcome {
    failures: Vec<String>,
    reaped: bool,
}

fn cleanup_child(child: &mut Child) -> ChildCleanupOutcome {
    // Child::kill targets this direct cargo process only. Process-tree handling
    // is deliberately left platform-independent and is not attempted here.
    let deadline = Instant::now() + CHILD_CLEANUP_GRACE;
    let mut failures = Vec::new();
    let mut observed_exit = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                observed_exit = true;
                break;
            }
            Ok(None) => {}
            Err(error) => {
                if failures.is_empty() {
                    failures.push(format!(
                        "could not inspect candidate during cleanup: {error}"
                    ));
                }
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        thread::sleep(CHILD_POLL_INTERVAL.min(remaining));
    }
    if !observed_exit && let Err(error) = child.kill() {
        failures.push(format!("could not kill candidate during cleanup: {error}"));
    }
    match child.wait() {
        Ok(_) => ChildCleanupOutcome {
            failures,
            reaped: true,
        },
        Err(error) => {
            failures.push(format!(
                "could not wait for candidate during cleanup: {error}"
            ));
            ChildCleanupOutcome {
                failures,
                reaped: false,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SessionErrorKind {
    Io,
    Startup,
    Timeout,
    UnexpectedFrame,
    UnexpectedEof,
    MalformedData,
    IdentityMismatch,
    InvalidRequest,
    ChildExit,
}

impl SessionErrorKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Io => "session I/O error",
            Self::Startup => "session startup error",
            Self::Timeout => "session timeout",
            Self::UnexpectedFrame => "unexpected protocol frame",
            Self::UnexpectedEof => "unexpected session EOF",
            Self::MalformedData => "malformed protocol data",
            Self::IdentityMismatch => "candidate identity mismatch",
            Self::InvalidRequest => "invalid smoke request result",
            Self::ChildExit => "candidate process error",
        }
    }
}

#[derive(Debug)]
pub(crate) struct SessionError {
    kind: SessionErrorKind,
    message: String,
    stdout: TailSnapshot,
    stderr: TailSnapshot,
    cleanup_failures: Vec<String>,
}

impl SessionError {
    pub(crate) const fn kind(&self) -> SessionErrorKind {
        self.kind
    }

    fn bare(kind: SessionErrorKind, message: String) -> Self {
        Self {
            kind,
            message,
            stdout: TailSnapshot::default(),
            stderr: TailSnapshot::default(),
            cleanup_failures: Vec::new(),
        }
    }

    fn with_tails(
        kind: SessionErrorKind,
        message: String,
        stdout: &SharedTail,
        stderr: &SharedTail,
    ) -> Self {
        Self {
            kind,
            message,
            stdout: snapshot_tail(stdout),
            stderr: snapshot_tail(stderr),
            cleanup_failures: Vec::new(),
        }
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind().label(), self.message)?;
        for failure in &self.cleanup_failures {
            write!(formatter, "\ncleanup issue: {failure}")?;
        }
        write_tail(formatter, "stdout", &self.stdout)?;
        write_tail(formatter, "stderr", &self.stderr)?;
        Ok(())
    }
}

impl std::error::Error for SessionError {}

fn write_tail(formatter: &mut fmt::Formatter<'_>, label: &str, tail: &TailSnapshot) -> fmt::Result {
    if tail.bytes.is_empty() && tail.omitted_bytes == 0 {
        return Ok(());
    }
    write!(formatter, "\n{label} tail")?;
    if tail.omitted_bytes > 0 {
        write!(formatter, " ({} earlier bytes omitted)", tail.omitted_bytes)?;
    }
    formatter.write_str(":\n")?;
    formatter.write_str(&String::from_utf8_lossy(&tail.bytes))
}

#[derive(Clone, Debug, Default)]
struct TailBuffer {
    bytes: Vec<u8>,
    omitted_bytes: u64,
}

impl TailBuffer {
    fn append(&mut self, incoming: &[u8]) {
        let keep_new = incoming.len().min(OUTPUT_TAIL_BYTES);
        let discard_new = incoming.len() - keep_new;
        let keep_old = OUTPUT_TAIL_BYTES - keep_new;
        let discard_old = self.bytes.len().saturating_sub(keep_old);
        if discard_old > 0 {
            self.bytes.drain(..discard_old);
        }
        self.bytes.extend_from_slice(&incoming[discard_new..]);
        let omitted = discard_new.saturating_add(discard_old);
        self.omitted_bytes = self
            .omitted_bytes
            .saturating_add(u64::try_from(omitted).unwrap_or(u64::MAX));
    }
}

#[derive(Clone, Debug, Default)]
struct TailSnapshot {
    bytes: Vec<u8>,
    omitted_bytes: u64,
}

fn snapshot_tail(tail: &SharedTail) -> TailSnapshot {
    let tail = lock_tail(tail);
    TailSnapshot {
        bytes: tail.bytes.clone(),
        omitted_bytes: tail.omitted_bytes,
    }
}

fn lock_tail(tail: &Mutex<TailBuffer>) -> MutexGuard<'_, TailBuffer> {
    tail.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn spawn_output_drainer<R>(reader: R, tail: SharedTail, thread_name: &str) -> io::Result<()>
where
    R: Read + Send + 'static,
{
    thread::Builder::new()
        .name(thread_name.to_owned())
        .spawn(move || {
            let mut reader = reader;
            let mut buffer = [0_u8; OUTPUT_READ_CHUNK_BYTES];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => lock_tail(&tail).append(&buffer[..read]),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        })
        .map(|_detached_handle| ())
}

#[derive(Debug)]
struct AcceptFailure {
    kind: SessionErrorKind,
    message: String,
}

fn accept_correlated_connection<F>(
    listener: &TcpListener,
    expected_nonce: [u8; 16],
    deadline: Instant,
    mut child_status: F,
) -> Result<TcpStream, AcceptFailure>
where
    F: FnMut() -> io::Result<Option<ExitStatus>>,
{
    let mut rejected_connections = 0_u64;
    let mut last_rejection = None;
    loop {
        match child_status() {
            Ok(Some(status)) => {
                return Err(AcceptFailure {
                    kind: SessionErrorKind::ChildExit,
                    message: format!(
                        "candidate exited before a valid handshake ({status}); rejected {rejected_connections} connection(s)"
                    ),
                });
            }
            Ok(None) => {}
            Err(error) => {
                return Err(AcceptFailure {
                    kind: SessionErrorKind::Io,
                    message: format!("could not inspect candidate during startup: {error}"),
                });
            }
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let last = last_rejection
                .map(|reason| format!("; last rejection: {reason}"))
                .unwrap_or_default();
            return Err(AcceptFailure {
                kind: SessionErrorKind::Timeout,
                message: format!(
                    "no matching handshake arrived within 10 minutes; rejected {rejected_connections} connection(s){last}"
                ),
            });
        }

        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    rejected_connections = rejected_connections.saturating_add(1);
                    last_rejection = Some("peer address was not loopback".to_owned());
                    continue;
                }

                let handshake_timeout = remaining.min(HANDSHAKE_TIMEOUT);
                if let Err(error) = stream.set_read_timeout(Some(handshake_timeout)) {
                    rejected_connections = rejected_connections.saturating_add(1);
                    last_rejection = Some(format!("could not set handshake timeout: {error}"));
                    continue;
                }
                let handshake = read_frame(&mut stream, DECODE_LIMITS);
                match check_candidate_handshake(stream, handshake, expected_nonce) {
                    Ok(stream) => return Ok(stream),
                    Err((stream, reason)) => {
                        last_rejection = Some(reason);
                        drop(stream);
                    }
                }
                rejected_connections = rejected_connections.saturating_add(1);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL.min(remaining));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(AcceptFailure {
                    kind: SessionErrorKind::Io,
                    message: format!("loopback accept failed: {error}"),
                });
            }
        }
    }
}

fn check_candidate_handshake<C>(
    connection: C,
    handshake: Result<Option<Frame>, forward_backward_protocol::CodecError>,
    expected_nonce: [u8; 16],
) -> Result<C, (C, String)> {
    match handshake {
        Ok(Some(Frame::Handshake { run_nonce })) if run_nonce == expected_nonce => Ok(connection),
        Ok(Some(Frame::Handshake { .. })) => Err((
            connection,
            "handshake nonce did not match this run".to_owned(),
        )),
        Ok(Some(frame)) => Err((
            connection,
            format!("first frame was {}, not a handshake", frame_name(&frame)),
        )),
        Ok(None) => Err((connection, "peer closed before its handshake".to_owned())),
        Err(error) => Err((connection, format!("malformed handshake: {error}"))),
    }
}

fn exchange_one_case<S>(stream: &mut S, request: &CaseRequest) -> Result<CaseResult, RoundTripError>
where
    S: Read + Write,
{
    validate_request_limits(request)?;
    write_frame(stream, &Frame::CaseRequest(request.clone())).map_err(RoundTripError::Write)?;

    let frame = read_frame(stream, DECODE_LIMITS).map_err(RoundTripError::Read)?;
    match frame {
        Some(Frame::CaseResult(result)) => {
            validate_case_result(request, &result).map_err(RoundTripError::Validation)?;
            Ok(result)
        }
        Some(frame) => Err(RoundTripError::UnexpectedFrame(frame_name(&frame))),
        None => Err(RoundTripError::Eof),
    }
}

fn validate_request_limits(request: &CaseRequest) -> Result<(), RoundTripError> {
    let (interval_count, sample_count, width) = match &request.record {
        RecordBits::F32 {
            interval_bits,
            sample_bits,
            ..
        } => (interval_bits.len(), sample_bits.len(), 4_usize),
        RecordBits::F64 {
            interval_bits,
            sample_bits,
            ..
        } => (interval_bits.len(), sample_bits.len(), 8_usize),
    };
    if interval_count.checked_add(1) != Some(sample_count) {
        return Err(RoundTripError::InvalidRequest(
            "request interval and sample counts do not match".to_owned(),
        ));
    }
    if sample_count > MAX_SAMPLES {
        return Err(RoundTripError::InvalidRequest(format!(
            "request has {sample_count} samples, exceeding the {MAX_SAMPLES}-sample v1 limit"
        )));
    }
    let payload_bytes = 50_usize
        .checked_add(width)
        .and_then(|base| {
            interval_count
                .checked_add(sample_count)?
                .checked_mul(width)?
                .checked_add(base)
        })
        .ok_or_else(|| {
            RoundTripError::InvalidRequest("request payload length overflowed".to_owned())
        })?;
    if payload_bytes > MAX_PAYLOAD_BYTES {
        return Err(RoundTripError::InvalidRequest(format!(
            "request payload has {payload_bytes} bytes, exceeding the {MAX_PAYLOAD_BYTES}-byte v1 limit"
        )));
    }
    Ok(())
}

#[derive(Debug)]
enum RoundTripError {
    Write(forward_backward_protocol::CodecError),
    Read(forward_backward_protocol::CodecError),
    Validation(forward_backward_protocol::CodecError),
    UnexpectedFrame(&'static str),
    Eof,
    InvalidRequest(String),
}

impl RoundTripError {
    fn session_kind(&self) -> SessionErrorKind {
        match self {
            Self::Write(error) | Self::Read(error) if codec_error_is_timeout(error) => {
                SessionErrorKind::Timeout
            }
            Self::Write(forward_backward_protocol::CodecError::Io(_))
            | Self::Read(forward_backward_protocol::CodecError::Io(_)) => SessionErrorKind::Io,
            Self::Validation(forward_backward_protocol::CodecError::IdentityMismatch) => {
                SessionErrorKind::IdentityMismatch
            }
            Self::Validation(_) | Self::Read(_) | Self::Write(_) => SessionErrorKind::MalformedData,
            Self::UnexpectedFrame(_) => SessionErrorKind::UnexpectedFrame,
            Self::Eof => SessionErrorKind::UnexpectedEof,
            Self::InvalidRequest(_) => SessionErrorKind::InvalidRequest,
        }
    }
}

impl fmt::Display for RoundTripError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write(error) => write!(formatter, "request write failed: {error}"),
            Self::Read(error) => write!(formatter, "response read failed: {error}"),
            Self::Validation(error) => write!(formatter, "response validation failed: {error}"),
            Self::UnexpectedFrame(frame) => {
                write!(formatter, "received {frame} where CaseResult was required")
            }
            Self::Eof => formatter.write_str("peer closed before sending CaseResult"),
            Self::InvalidRequest(message) => formatter.write_str(message),
        }
    }
}

fn codec_error_is_timeout(error: &forward_backward_protocol::CodecError) -> bool {
    matches!(
        error,
        forward_backward_protocol::CodecError::Io(io_error)
            if matches!(io_error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)
    )
}

fn set_stream_timeouts(stream: &TcpStream, timeout: Duration) -> io::Result<()> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))
}

fn frame_name(frame: &Frame) -> &'static str {
    match frame {
        Frame::Handshake { .. } => "Handshake",
        Frame::CaseRequest(_) => "CaseRequest",
        Frame::CaseResult(_) => "CaseResult",
        Frame::Done => "Done",
    }
}

fn new_nonce() -> [u8; 16] {
    let timestamp_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_le_bytes();
    let process_id = std::process::id().to_le_bytes();
    let counter = NONCE_COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes();

    let mut nonce = [0_u8; 16];
    nonce[..8].copy_from_slice(&timestamp_nanos[..8]);
    nonce[8..12].copy_from_slice(&process_id);
    nonce[12..].copy_from_slice(&counter);
    nonce
}

fn encode_nonce(nonce: &[u8; 16]) -> String {
    let mut encoded = String::with_capacity(32);
    for byte in nonce {
        write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn repository_root() -> io::Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
}

struct ClientCommandSpec {
    program: OsString,
    arguments: Vec<OsString>,
    current_dir: PathBuf,
    environment_additions: Vec<(OsString, OsString)>,
}

impl ClientCommandSpec {
    fn to_command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.arguments)
            .current_dir(&self.current_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &self.environment_additions {
            command.env(key, value);
        }
        command
    }
}

fn candidate_command_spec(
    repository_root: &Path,
    address: SocketAddrV4,
    encoded_nonce: &str,
    cargo_program: Option<OsString>,
    candidate: CandidateKind,
) -> ClientCommandSpec {
    let manifest = repository_root.join("Cargo.toml");
    let arguments = [
        OsString::from("test"),
        OsString::from("--release"),
        OsString::from("--locked"),
        OsString::from("--manifest-path"),
        manifest.into_os_string(),
        OsString::from("--lib"),
        OsString::from(candidate.client_test_filter()),
        OsString::from("--"),
        OsString::from("--exact"),
        OsString::from("--ignored"),
        OsString::from("--nocapture"),
        OsString::from("--test-threads=1"),
    ]
    .into();
    ClientCommandSpec {
        program: cargo_program.unwrap_or_else(|| OsString::from("cargo")),
        arguments,
        current_dir: repository_root.to_path_buf(),
        environment_additions: vec![
            (
                OsString::from(ADDRESS_ENV),
                OsString::from(address.to_string()),
            ),
            (OsString::from(NONCE_ENV), OsString::from(encoded_nonce)),
            (OsString::from(COLOR_ENV), OsString::from("never")),
        ],
    }
}

fn smoke_success_request() -> CaseRequest {
    CaseRequest {
        identity: CaseIdentity {
            case_id: 1,
            record_digest: [0x21; 32],
        },
        order: FilterOrder::One,
        record: RecordBits::F64 {
            cutoff_bits: 1.0_f64.to_bits(),
            interval_bits: [0.01_f64, 0.0125, 0.015]
                .into_iter()
                .map(f64::to_bits)
                .collect(),
            sample_bits: [0.0_f64, 1.0, -0.25, 0.5]
                .into_iter()
                .map(f64::to_bits)
                .collect(),
        },
    }
}

fn smoke_non_finite_request() -> CaseRequest {
    CaseRequest {
        identity: CaseIdentity {
            case_id: 2,
            record_digest: [0x42; 32],
        },
        order: FilterOrder::One,
        record: RecordBits::F64 {
            cutoff_bits: 1.0_f64.to_bits(),
            interval_bits: vec![0.01_f64.to_bits()],
            sample_bits: vec![0.0_f64.to_bits(), f64::NAN.to_bits()],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Cursor;

    use forward_backward_protocol::{SuccessResponse, TailEvidence};

    struct ScriptedStream {
        incoming: Cursor<Vec<u8>>,
        outgoing: Vec<u8>,
    }

    impl ScriptedStream {
        fn with_incoming(incoming: Vec<u8>) -> Self {
            Self {
                incoming: Cursor::new(incoming),
                outgoing: Vec::new(),
            }
        }
    }

    impl Read for ScriptedStream {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.incoming.read(buffer)
        }
    }

    impl Write for ScriptedStream {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.outgoing.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn test_request() -> CaseRequest {
        CaseRequest {
            identity: CaseIdentity {
                case_id: 17,
                record_digest: [0x91; 32],
            },
            order: FilterOrder::One,
            record: RecordBits::F64 {
                cutoff_bits: 2.0_f64.to_bits(),
                interval_bits: vec![0.125_f64.to_bits()],
                sample_bits: vec![0.0_f64.to_bits(), 1.0_f64.to_bits()],
            },
        }
    }

    fn encoded(frame: &Frame) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, frame).expect("test frame should encode");
        bytes
    }

    fn success_response(identity: CaseIdentity) -> CaseResult {
        CaseResult::Success(SuccessResponse {
            identity,
            output_bits: OutputBits::F64(vec![0.0_f64.to_bits(), 0.5_f64.to_bits()]),
            tail_evidence: TailEvidence::Omitted,
            forward: Default::default(),
            tail: Default::default(),
            reverse: Default::default(),
        })
    }

    #[test]
    fn nonce_encoding_is_32_lowercase_hex_characters_and_varies() {
        let first = encode_nonce(&new_nonce());
        let second = encode_nonce(&new_nonce());

        assert_eq!(first.len(), 32);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert_ne!(first, second);
    }

    #[test]
    fn smoke_success_intervals_obey_the_pi_over_32_normalized_bound() {
        let request = smoke_success_request();
        let RecordBits::F64 {
            cutoff_bits,
            interval_bits,
            ..
        } = request.record
        else {
            panic!("smoke success request should use f64 intervals");
        };
        let cutoff_hz = f64::from_bits(cutoff_bits);
        let maximum_normalized_interval = interval_bits
            .into_iter()
            .map(|bits| std::f64::consts::TAU * cutoff_hz * f64::from_bits(bits))
            .fold(0.0_f64, f64::max);

        assert!(
            maximum_normalized_interval <= std::f64::consts::PI / 32.0,
            "smoke q_max {maximum_normalized_interval} exceeds pi/32"
        );
    }

    #[test]
    fn wrong_handshake_is_rejected_before_a_matching_handshake_is_accepted() {
        let expected_nonce = [0x35; 16];
        let mut rejected = Vec::new();
        let candidates = [
            (
                "wrong connection",
                Frame::Handshake {
                    run_nonce: [0x53; 16],
                },
            ),
            (
                "correct connection",
                Frame::Handshake {
                    run_nonce: expected_nonce,
                },
            ),
        ];

        let accepted =
            candidates
                .into_iter()
                .find_map(|(connection, frame)| {
                    match check_candidate_handshake(connection, Ok(Some(frame)), expected_nonce) {
                        Ok(connection) => Some(connection),
                        Err((connection, _reason)) => {
                            rejected.push(connection);
                            None
                        }
                    }
                });

        assert_eq!(rejected, ["wrong connection"]);
        assert_eq!(accepted, Some("correct connection"));
    }

    #[test]
    fn round_trip_rejects_unexpected_frame() {
        let request = test_request();
        let mut stream = ScriptedStream::with_incoming(encoded(&Frame::Done));

        let error = exchange_one_case(&mut stream, &request).unwrap_err();

        assert!(matches!(error, RoundTripError::UnexpectedFrame("Done")));
        let sent = read_frame(&mut stream.outgoing.as_slice(), DECODE_LIMITS)
            .expect("request should decode")
            .expect("request frame should exist");
        assert_eq!(sent, Frame::CaseRequest(request));
    }

    #[test]
    fn round_trip_rejects_eof_before_result() {
        let request = test_request();
        let mut stream = ScriptedStream::with_incoming(Vec::new());

        let error = exchange_one_case(&mut stream, &request).unwrap_err();

        assert!(matches!(error, RoundTripError::Eof));
    }

    #[test]
    fn round_trip_rejects_mismatched_result_identity() {
        let request = test_request();
        let mismatched_identity = CaseIdentity {
            case_id: request.identity.case_id + 1,
            record_digest: request.identity.record_digest,
        };
        let response = success_response(mismatched_identity);
        let mut stream = ScriptedStream::with_incoming(encoded(&Frame::CaseResult(response)));

        let error = exchange_one_case(&mut stream, &request).unwrap_err();

        assert!(matches!(
            error,
            RoundTripError::Validation(forward_backward_protocol::CodecError::IdentityMismatch)
        ));
    }

    #[test]
    fn output_tail_is_bounded_and_counts_omitted_bytes() {
        let mut tail = TailBuffer::default();
        let bytes = (0..(OUTPUT_TAIL_BYTES + 37))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();

        tail.append(&bytes);

        assert_eq!(tail.bytes.len(), OUTPUT_TAIL_BYTES);
        assert_eq!(tail.omitted_bytes, 37);
        assert_eq!(tail.bytes, bytes[37..]);
    }

    #[test]
    fn session_failure_keeps_its_fatal_cell_category_without_transport() {
        for kind in [SessionErrorKind::Io, SessionErrorKind::IdentityMismatch] {
            let error = SessionError::bare(
                kind,
                "synthetic session failure for category mapping".to_owned(),
            );
            let cell_error = crate::matrix::CellExecutionError::Session(error);

            match &cell_error {
                crate::matrix::CellExecutionError::Session(error) => {
                    assert_eq!(error.kind(), kind);
                }
                other => panic!("session failure changed fatal category: {other:?}"),
            }
            assert!(cell_error.to_string().contains("candidate session failed"));
            assert!(std::error::Error::source(&cell_error).is_some());
        }
    }

    #[test]
    fn candidate_command_uses_exact_arguments_and_only_the_three_session_environment_additions() {
        let root = PathBuf::from("ssfilt-command-test");
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42_317);
        let nonce = "00112233445566778899aabbccddeeff";
        for candidate in [CandidateKind::Endpoint, CandidateKind::Trajectory] {
            let spec = candidate_command_spec(
                &root,
                address,
                nonce,
                Some(OsString::from("custom-cargo")),
                candidate,
            );

            assert_eq!(spec.program, OsString::from("custom-cargo"));
            assert_eq!(spec.current_dir, root);
            assert_eq!(
                spec.arguments,
                vec![
                    OsString::from("test"),
                    OsString::from("--release"),
                    OsString::from("--locked"),
                    OsString::from("--manifest-path"),
                    root.join("Cargo.toml").into_os_string(),
                    OsString::from("--lib"),
                    OsString::from(candidate.client_test_filter()),
                    OsString::from("--"),
                    OsString::from("--exact"),
                    OsString::from("--ignored"),
                    OsString::from("--nocapture"),
                    OsString::from("--test-threads=1"),
                ]
            );
            let additions = spec
                .environment_additions
                .into_iter()
                .collect::<BTreeMap<_, _>>();
            assert_eq!(additions.len(), 3);
            assert_eq!(
                additions.get(OsString::from(ADDRESS_ENV).as_os_str()),
                Some(&OsString::from(address.to_string()))
            );
            assert_eq!(
                additions.get(OsString::from(NONCE_ENV).as_os_str()),
                Some(&OsString::from(nonce))
            );
            assert_eq!(
                additions.get(OsString::from(COLOR_ENV).as_os_str()),
                Some(&OsString::from("never"))
            );
        }
    }
}
