# Forward-backward batch filtering design record

## Status

This document defines the investigation contract for offline forward-backward
filtering. No public batch API is implemented yet. Names and method signatures
shown below are provisional until the numerical operator, boundary treatment,
and workspace requirements pass the declared experiments.

The feature is intentionally separate from delayed streaming phase
equalization. It must solve an offline waveform-preservation use case without
weakening the streaming API, its allocation-free state, or its causal timing
contract.

## Physical goal

For a real continuous-time filter with frequency response `H(j*w)`, applying
the filter forward, reversing time, applying it again, and reversing the result
gives the ideal interior response:

```text
H_fb(j*w) = H(j*w) * H(-j*w) = |H(j*w)|^2
```

The phase is zero and the magnitude is squared. This operation is noncausal:
an output associated with time `t` depends on input before and after `t`. It
therefore requires a complete finite record or an explicitly bounded block
with a declared continuation outside its endpoints.

The initial use case is offline smoothing of irregularly sampled data when
preserving waveform alignment is more important than streaming latency. It is
not a replacement for the causal phase equalizer and is not available through
`StreamingFilter`.

## Product boundary

The public concepts remain distinct:

- `LowPass`, `HighPass`, and `BandPass` are causal streaming filters.
- `PhaseEqualizedLowPass` is a causal streaming filter that preserves the
  source magnitude and adds disclosed latency.
- forward-backward filtering is a stateless, reusable batch plan operating on
  an entire record.

Forward-backward processing is not a response family or an additive phase
option. In particular, applying it to `PhaseEqualizedLowPass` would cancel the
all-pass phase correction while retaining its state and integration cost.
There will be no supported composition of those two features without a future
independent use case and validation record.

The intended public home is a dedicated `ssfilt::batch` module. The first
possible product is provisionally named `ForwardBackwardLowPass<N, T>`. It
will not be re-exported at the crate root initially and will not implement
`StreamingFilter`.

The first constructor must be Butterworth-specific rather than advertise a
general response selector that mostly fails. The exact constructor, processing
signature, workspace representation, and whether diagnostics are public remain
open until the private prototype establishes their requirements. Rustdoc and
the README must make the batch module discoverable without adding batch-only
settings to the ordinary streaming builders.

## Rejected public shapes

The first design does not use:

- builder flags such as `phase_equalized(true)` and `zero_phase(true)`, because
  the combinations are not additive and have incompatible result types;
- a `PhaseMode` enum, because causal magnitude-preserving equalization and
  noncausal magnitude-squaring batch processing are different products;
- `Response::ForwardBackward`, because `Response` selects an analog pole
  family rather than an execution algorithm;
- `LowPassBuilder::build_forward_backward`, because the existing builder
  carries streaming-only `initial_input` and input reconstruction settings;
- an adaptor or extension trait over a built streaming filter, because its
  mutable state is irrelevant and its ordinary -3 dB normalization is wrong
  for the complete two-pass response;
- a public generic batch trait or filter factory before multiple validated
  batch algorithms demonstrate a stable common contract.

Topology-specific public plans may share a private batch engine and the
existing private continuous models. Public separation must not duplicate the
coefficient tables, section realizations, or integrator.

## Signal and timing definition

The initial investigation reconstructs one continuous input from the samples
using straight lines over each supplied positive elapsed interval. For `n`
sample values, there are exactly `n - 1` intervals. Absolute timestamps are not
part of the first core contract; a later convenience may validate and convert
them.

Linear reconstruction is invariant under time reversal. Hold reconstruction
is deferred. If it is later supported, preserving the same physical signal
requires:

```text
Linear        <-> Linear
PreviousHold  <-> CurrentHold
CurrentHold   <-> PreviousHold
```

This reversal is especially important for direct-feedthrough topologies. The
first low-pass experiment exposes no reconstruction selector rather than
advertising unvalidated combinations.

### Continuous operator versus endpoint operator

The intended physical reference applies the continuous source model in both
directions. A simple implementation that stores only forward endpoint outputs
and linearly reconstructs them for the reverse pass instead applies a sampled
endpoint operator twice. The two are not generally identical: the true
forward output curves between sample endpoints, and the reverse Runge-Kutta
stages may query those missing values.

The endpoint implementation is useful as a private baseline, not an assumed
public solution. It may become the implementation only if it meets the
predeclared refinement and waveform gates over the supported interval range.
Otherwise the implementation must retain, replay, or densely evaluate the
forward trajectory. The public workspace shape must not be frozen before this
decision.

The continuous contract should be partition-invariant within integration
error: inserting a sample that lies exactly on an existing linear input
segment changes where output is observed, not the reconstructed input or the
output at the original timestamps.

## Magnitude and cutoff contract

`cutoff_hz` describes the complete forward-backward response's -3 dB point,
consistent with every other public filter in the crate. Reusing an ordinary
streaming filter whose magnitude is `1/sqrt(2)` at that frequency would square
the magnitude to `1/2`, or approximately -6.02 dB. Each internal pass must
instead satisfy:

```text
|H_pass(j*w_c)|    = 2^(-1/4)
|H_fb(j*w_c)|      = |H_pass(j*w_c)|^2 = 1/sqrt(2)
```

For an order-`N` Butterworth low-pass, with `N` denoting the order of each
causal pass:

```text
Omega_* = (sqrt(2) - 1)^(1 / (2*N))
f_pass  = f_public / Omega_*
```

The complete noncausal response has the squared Butterworth magnitude and a
rolloff equivalent to twice the per-pass order. Documentation must not call
`N` the order of a causal realization of the complete response.

Compensation is response- and topology-specific. Repeated-pole and Bessel
low-pass responses require their own derived crossings. Chebyshev Type I also
doubles passband ripple in decibels. For odd per-pass orders of at least three,
ripple at or above `10*log10(sqrt(2))`, approximately 1.50515 dB, places the
per-pass target inside the ripple range and can produce multiple crossings.
Under ssfilt's unity-DC normalization this ambiguity does not occur for even
orders, whose ripple extends above DC gain. Chebyshev compensation remains
response-, parity-, order-, and ripple-specific. High-pass and both band-pass
edges require separate transformations. None of those cases is implied by the
first Butterworth low-pass experiment.

## Finite-record boundaries

Zero phase and exact magnitude squaring describe an infinite-time operator.
A finite record requires a continuation or an equivalent two-sided state
condition. That choice controls edge transients and is part of the filter
definition, not a cosmetic padding option.

Candidate policies include constant continuation of the original input,
reflected continuation, and optimized two-sided initial conditions. They must
not be exposed as a `BoundaryMode` enum before at least two policies have
independent irregular-time validation.

The private first experiment uses constant continuation of the original input
as its physical reference:

```text
u(t < t_first) = u_first
u(t > t_last)  = u_last
```

This gives an exact forward equilibrium at the left boundary. It does not mean
that the reverse pass may simply start in equilibrium with the last stored
forward output. After the last sample, the forward filter generally continues
moving toward the final input equilibrium. A correct implementation must
account for that tail analytically or integrate it to a declared residual and
work limit.

Constant records must remain exact everywhere. For nonconstant records, edge
error and the physical duration over which it remains significant must be
reported separately from interior phase and magnitude measurements.

## Storage, allocation, and failure behavior

The batch operation necessarily consumes record-sized storage, but the core
library need not allocate it. The foundational API should accept caller-owned
input/output and workspace slices and remain available with `no_std` plus
`libm`.

The configured plan is immutable and reusable. Every operation creates fresh
forward and reverse states privately. A failed operation leaves the caller's
input/output record unchanged; scratch workspace contents may be unspecified.
All lengths, finite samples, positive finite intervals, workspace size, and
representability constraints are validated before numerical work when
possible.

A dedicated batch error type is expected because failures include record
shape, indexed data, workspace capacity, boundary work, and pass-qualified
integration failures that do not belong in `UpdateError`. Exact public error
variants wait for the prototype, but failures must identify the pass and
record interval when that information is available.

An allocating convenience returning `Vec<T>` may later wrap the caller-owned
workspace API behind an explicit `alloc` capability. It is not part of the
first contract and must not make `std` or an allocator necessary for the core
batch algorithm.

## Initial investigation scope

The first private prototype is deliberately limited to:

- low-pass topology;
- Butterworth response selected explicitly;
- per-pass orders 1, 2, 4, and 8;
- linear input reconstruction;
- `f32` and `f64`;
- the scalar's default integration controls, with public customization deferred;
- complete-response -3 dB cutoff compensation;
- constant continuation as the reference boundary policy;
- caller-owned storage and transactional record output;
- uniform, deterministically jittered, and deliberately uneven positive
  intervals whose public normalized duration
  `q = 2*pi*cutoff_hz*dt` is at most `pi/32`.

The prototype must not add public types, traits, builders, errors, or feature
flags. It first compares endpoint reconstruction with a continuous-trajectory
reference. High-pass, band-pass, other response families, hold reconstruction,
alternate boundaries, allocating conveniences, and runtime-selected order are
separate later decisions.

## Independent continuous reference

The finite-record oracle must not call `StreamingCore`, the runtime
`Butterworth` coefficient generator, Dormand-Prince, or the endpoint-replay
baseline. In public-cutoff normalized time, define the compensated pole radius:

```text
tau   = 2*pi*cutoff_hz*t
alpha = (sqrt(2) - 1)^(-1/(2*N))
```

Using at least 192-bit complex arithmetic, construct the poles and residues
directly:

```text
p_k  = alpha * exp(i*pi*(2*k + N + 1)/(2*N)),  k = 0..N-1
r_k  = alpha^N / product(m != k, p_k - p_m)
h(t) = sum(k, r_k*exp(p_k*t)),                  t >= 0
```

The ideal two-pass kernel is:

```text
K(d) = integral(0..infinity, h(a)*h(a + abs(d)) da)
     = -sum(j, sum(k, r_j*r_k*exp(p_k*abs(d))/(p_j + p_k)))
```

For each requested timestamp, the oracle evaluates `K` against the exact
piecewise-linear record with constant half-lines. Each linear segment and both
half-lines are integrated analytically with exponential primitives, using
`expm1` or a series near zero. This is independent of both candidate state
integration and sample-endpoint replay.

The oracle must satisfy all of these before it scores a candidate:

- 192-bit and 256-bit evaluations agree within `2e-13*Q` time-weighted RMS and
  `1e-12*Q` peak;
- the residual imaginary component is at most `1e-30*Q`;
- symmetry follows algebraically from `abs(d)`, while independent evaluations
  at `d` and `-d` agree within `1e-30*Q`;
- DC gain is one within `2e-14`, and the transform agrees within `2e-13` at
  `W = 0, 1/16, 1/4, 0.7, 1, 2, 4` with
  `1 / (1 + (W/alpha)^(2*N))`;
- gain at public normalized frequency one agrees with `1/sqrt(2)` within
  `2e-14`;
- inserting exact collinear samples changes oracle values at original
  timestamps by at most `1e-12*Q`.

The observable scale is:

```text
Q = max(max(abs(input)), default_absolute_tolerance/default_relative_tolerance)
```

Thus the floor is `1e-3` for `f64` and `1e-2` for `f32`. This scale respects
the current absolute-tolerance floor without pretending that local integration
tolerances are a global output-error budget.

## Frozen matrix axes and fixture classes

Use `q_i = 2*pi*cutoff_hz*dt_i` and cross:

```text
per-pass orders: 1, 2, 4, 8
scalars:         f32, f64
cutoffs:         1 Hz, 37 Hz
maximum q_i:     pi/128, pi/64, pi/32
schedules:       uniform, deterministic jitter, deliberately uneven
```

Every schedule has the stated maximum as a hard bound. The deterministic
multipliers are:

```text
jitter: [1/2, 1, 3/4, 1/4, 7/8, 5/8]
uneven: [1, 1/32, 1/2, 1/16, 3/4, 1/8]
```

The final interval may be shortened to end at the exact record boundary.
Corresponding 1 Hz and 37 Hz cases reuse identical normalized samples and
intervals.

Tone frequencies are `1/16`, `1/4`, `0.7`, `1`, and `2` times public cutoff,
with phases 0.17 and 1.19 radians. Time-domain fixtures are a predeclared
unequal multitone, Gaussian pulse, tapered chirp, interior step,
boundary-adjacent step, and triangular impulse-like pulse. Each fixture is
defined analytically before sampling, has bounded nonzero scale, and has
nontrivial oracle energy. The oracle commit freezes every formula, duration,
center, and taper before either runtime candidate is implemented. An
unconstrained one-sample impulse whose slope changes with the partition is
forbidden.

Uniform periodic cases are additionally compared with an FFT applying the
identical smooth response. A brick-wall bin mask is not an oracle.

## Observable numerical gates

For irregular records, use trapezoidal interval weights and define:

```text
time_weighted_rms(e) = sqrt(
    sum(i, dt_i*(e_i^2 + e_(i+1)^2)/2) / sum(i, dt_i)
)
E_rms  = time_weighted_rms(candidate - finite_record_oracle) / Q
E_peak = max(abs(candidate - finite_record_oracle)) / Q
```

Every candidate cell, including boundary samples, must meet:

| Scalar | `E_rms` | `E_peak` |
| --- | ---: | ---: |
| `f64` | `5e-5` | `2e-4` |
| `f32` | `2e-3` | `8e-3` |

These are observable two-pass budgets, intentionally distinct from and looser
than the local embedded-integration tolerances.

On the finest tone schedule, fit constant, sine, and cosine terms at the actual
timestamps after excluding `24/gamma_N` normalized time from each edge, where:

```text
gamma_N = alpha*sin(pi/(2*N))
```

Against the analytic infinite-record response, require:

| Scalar | Absolute gain error | Phase error |
| --- | ---: | ---: |
| `f64` | `5e-4` | `5e-4` rad |
| `f32` | `3e-3` | `3e-3` rad |

Phase is scored only where analytic output gain is at least `1e-3`; below that
level the fit residual and absolute output error are reported instead. Coarser
sampled-sine cases compare to the exact piecewise-linear finite-record oracle,
not directly to an unsampled sine whose chords differ from the reconstruction.

## Partition and physical-scaling gates

For every base record, insert samples at one-third and two-thirds of every
interval with exactly collinear values, then compare only original timestamps.
Both partitions must independently pass the oracle gates. Their mutual
difference must meet:

| Scalar | Time-weighted RMS | Peak |
| --- | ---: | ---: |
| `f64` | `2.5e-5*Q` | `1e-4*Q` |
| `f32` | `1e-3*Q` | `4e-3*Q` |

The endpoint-replay baseline is selectable only if it passes every cell through
`q_i = pi/32`. Passing only after reducing the maximum interval does not support
the declared range. Otherwise use a trajectory-aware implementation without
loosening these gates.

Corresponding normalized records at 1 Hz and 37 Hz may differ by at most
`2e-9*Q` peak for `f64` and `2e-4*Q` peak for `f32`.

## Tail and boundary gates

The oracle evaluates the infinite constant-continuation tail analytically. If
a candidate obtains its reverse initial condition by numerical continuation,
define:

```text
R_tail = max_i(
    abs(x_i - equilibrium_i)
    / (absolute_tolerance
       + relative_tolerance*max(Q, abs(x_i), abs(equilibrium_i)))
)
```

The tail may stop only when `R_tail <= 0.25`. Replacing its reverse initial
state with the analytic modal-tail value may change the completed record by no
more than `5e-5*Q` peak for `f64` or `2e-3*Q` peak for `f32`. Tail work is also
bounded by:

```text
maximum public-normalized extension = 32/gamma_N
maximum attempted adaptive steps    = 512
maximum derivative evaluations      = 3,584
```

Failure to meet the residual before either limit is a tail-work failure; a
candidate may not silently extend them after seeing results.

Constant records are bit-exact throughout. Asymmetric two-sample ramps,
nonzero endpoint slopes, and transients next to both edges must pass the
whole-record oracle gates. Reversing samples and intervals, filtering, and
reversing the result must meet the same gates. A sentinel must show that
initializing the reverse pass from the last stored forward output is
insufficient for a nonconstant record.

Validation, forward, reverse, and tail failures leave caller-visible
input/output unchanged. Empty-record and single-sample behavior must be chosen
before public exposure rather than inherited accidentally.

Separately report the difference between the constant-continued finite-record
oracle and a known infinite-record signal as peak magnitude and duration above
`1e-3*Q`, `1e-4*Q`, and `1e-5*Q`. These are report levels, not acceptance
thresholds: acceptable physical edge loss requires a named user scenario and
must not be inferred from favorable measurements.

## Work and workspace gates

For `q_i <= pi/32` and default tolerances, cap each adaptive interval solve at
64 attempts and 448 derivative evaluations. Investigation-wide ceilings are:

```text
endpoint baseline:
  attempts               <= 128*(n - 1) + 512
  derivative evaluations <= 896*(n - 1) + 3,584
  workspace               <= 2*n + 4*N scalars

trajectory-aware candidate:
  attempts               <= 192*(n - 1) + 512
  derivative evaluations <= 1,344*(n - 1) + 3,584
  workspace               <= (N + 2)*n + 8*N scalars
```

The third interval solve allowance covers one forward-trajectory replay. A
candidate needing unbounded dense-output storage, more than one replay, or
adaptive workspace growth is outside this prototype. All size arithmetic is
overflow-checked. These are investigation ceilings, not a public workspace
formula.

The prototype records actual work for every pass and tail. It has no wall-clock
speed or maximum-record-length success threshold because neither belongs to a
named product requirement yet. Likewise, support above order 8, above
`q_i = pi/32`, or for unconstrained input slope is a non-claim rather than a
permissive untested behavior.

## Staged work

1. Commit this physical and architectural contract. ✓
2. Commit the exact fixture formulas, durations, centers, tapers, and record
   boundaries, then implement and independently validate the multiprecision
   modal-kernel oracle and analytic cutoff compensation without adding runtime
   API.
3. Implement a private endpoint-only negative-control baseline with validation,
   transactional failure tests, and no assumption that it will be selected.
4. Evaluate its oracle error and partition dependence over the frozen matrix;
   implement one bounded trajectory-aware replay candidate if it fails.
5. Select endpoint replay or a trajectory-aware engine from the predeclared
   gates, then independently review that numerical decision.
6. Finalize workspace, boundary, error, and diagnostics representations.
7. Expose only the validated `ssfilt::batch` low-pass surface and run an
   independent numerical and public-API review.
8. Consider allocating convenience, further responses, and further topologies
   as separate milestones.

## Non-goals

The first milestone does not provide a general FFT filter, arbitrary spectral
masking, a public ODE trajectory API, caller-defined state-space models,
streaming look-ahead, automatic resampling, or a menu of padding policies. It
does not change any existing streaming filter or equalizer behavior.
