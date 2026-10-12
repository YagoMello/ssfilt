# Forward-backward stage-2 fixtures

This file freezes the analytic inputs and record construction for stage 2 of
[`forward-backward.md`](forward-backward.md). It is a private investigation
fixture specification. It adds no public API and does not change the frozen
operator, boundary, accuracy, refinement, or work gates in the design record.

## Time and sampling conventions

Use public-cutoff normalized time

```text
tau = 2*pi*cutoff_hz*t
q_i = tau_(i+1) - tau_i = 2*pi*cutoff_hz*dt_i
```

Thus nominal `tau` and `q_i` are dimensionless radians at the public cutoff.
Construct the nominal normalized schedule with the exact-tick and frozen
high-precision rules below, then evaluate each analytic formula from its
independently converted `f64` timestamps. Reuse those normalized sample values
at 1 Hz and 37 Hz. The nominal physical interval is
`dt_i = q_i/(2*pi*cutoff_hz)`. Convert the cutoff, interval durations, and
input samples separately to the candidate scalar before passing them to the
candidate. The finite-record input outside its two endpoints is always the
constant endpoint value required by `forward-backward.md`, even when the
analytic formula would continue to oscillate or taper.

Each analytic formula is a function of `tau` only. For any base schedule,
sample `u_i = f(tau_i)` directly; never derive phase or amplitude from a sample
index or the requested step size. This gives the same analytic value whenever
two nominal schedules share a timestamp. The runtime's signal is then the
straight line through the stored scalar samples, with constant half-lines.
These are distinct claims: sampling the analytic function on a different
schedule generally changes its piecewise-linear approximation, while
inserting collinear points into one already-defined piecewise-linear record
must leave that reconstructed input unchanged.

## Shared time-domain record

The six time-domain fixture classes comprise seven records, since the
boundary-step class includes left and right orientations. They use the exact
interval

```text
0 <= tau <= T,       T = 128*pi       # 64 cutoff cycles
```

The endpoints are included. This duration makes the multitone record exactly
periodic on its analytic formula and leaves a long interior for pulse, chirp,
and step cases. The finite-record oracle still applies constant continuation,
not periodic continuation, at `tau < 0` and `tau > T`.

### Unequal five-tone record

Use the same frozen unequal-amplitude and unequal-phase five-tone shape as the
streaming waveform fixture:

```text
ratios    = [5/64, 13/64, 25/64, 37/64, 43/64]
amplitudes= [1, 0.63, 0.41, 0.29, 0.19]
phases    = [0.17, -0.73, 1.19, -1.41, 0.48] radians

u_multi(tau) = sum(k=0..4,
    amplitudes[k] * cos(ratios[k]*tau + phases[k]))
```

The distinct integer cycle counts over `T` are `[5, 13, 25, 37, 43]`, so its
analytic period closes at both record endpoints. Its absolute value is bounded
by `2.52`; its exact period-average is zero and its period-mean square is
`sum(amplitude[k]^2)/2 > 0`. This fixes a nonzero, reproducible scale without
depending on the sampling partition.

### Gaussian-modulated pulse

Use one Gaussian-modulated carrier centered at the record midpoint. This uses
the carrier, width, and phase parameters of the existing phase waveform test,
without that test's periodic image sum:

```text
center = T/2
sigma  = 1.5 cutoff cycles = 3*pi normalized-time units
carrier_W = 0.375
carrier_phase = 0.37 radians

u_gaussian(tau) = exp(-0.5*((tau-center)/sigma)^2)
                  * cos(carrier_W*(tau-center) + carrier_phase)
```

It is bounded by one, is nonzero at its center, and at either endpoint its
envelope is exactly `exp(-2048/9)`, approximately `1.44e-99`; the carrier can
only reduce the magnitude. It is one finite pulse, not a periodic train; the
oracle still continues the exact nonzero endpoint sample as a constant.

### Tapered linear-frequency chirp

Use the frozen phase-test sweep limits and initial phase, scaled to the
64-cycle record. Let `W0 = 0.05`, `W1 = 0.65`, and define

```text
window(tau) = sin(pi*tau/T)^2
phase(tau)  = 0.23 + W0*tau + (W1-W0)*tau^2/(2*T)
u_chirp(tau)= window(tau) * cos(phase(tau))
```

Its instantaneous normalized angular frequency is
`W0 + (W1-W0)*tau/T`, so it sweeps linearly from `0.05` to `0.65` times
cutoff. It is bounded by one, nonzero in the open interval, and exactly zero
at each endpoint. The squared-sine taper has zero endpoint slope, avoiding a
hard seam if a periodic FFT cross-check is made.

### Interior and boundary-adjacent finite-slope steps

For a center `c`, define a unit-height-change ramp with total width `pi`:

```text
step_c(tau) = -1/2                         tau <= c - pi/2
              (tau - c)/pi                 c - pi/2 < tau < c + pi/2
              +1/2                         tau >= c + pi/2
```

The slope is exactly `1/pi`, so no one-sample impulse or partition-dependent
slope is involved. Include these three records on `[0,T]`:

```text
interior:          c = T/2
left-adjacent:     c = 4*pi
right-adjacent:    u_right(tau) = step_(4*pi)(T-tau)
```

The two edge-adjacent records are exact time reversals: define the right record
as `u_right(tau) = step_(4*pi)(T-tau)`, so it descends from `+1/2` to `-1/2`.
Their ramps remain strictly inside the record, and their plateaus and finite
slopes are nontrivial under every permitted schedule. The interior record has
equal plateau durations; the edge records intentionally put the transient
within two cutoff cycles of one boundary. There are seven time-domain records
in total because the boundary-adjacent class contains both edge orientations.

### Triangular pulse

Use a resolved, compact, symmetric pulse rather than a one-sample impulse:

```text
center = T/2
half_width = 4*pi                    # 2 cutoff cycles on each side
u_triangle(tau) = max(1 - abs(tau-center)/half_width, 0)
```

It is bounded in `[0,1]`, has positive area and a four-cycle total base, and
has finite slopes `+/-1/half_width`. Even the largest allowed interval
(`pi/32`) places at least 256 intervals across its support.

## Tone records and steady-state fit windows

Use five separate unit-amplitude analytic tone families and both frozen phase
values:

```text
W       = [1/16, 1/4, 0.7, 1, 2]
phase   = [0.17, 1.19] radians
u_(W,p)(tau) = cos(W*tau + p)
```

For each per-pass order `N`, use the compensated pole scale and slowest decay
rate already defined in `forward-backward.md`:

```text
alpha_N = (sqrt(2)-1)^(-1/(2*N))
gamma_N = alpha_N*sin(pi/(2*N))
G_N     = 24/gamma_N
P_W     = 2*pi/W
```

The tone record boundaries and analytic steady-state fit window are

```text
tau_first = 0
tau_last  = 2*G_N + P_W
fit window = [G_N, G_N + P_W] = [G_N, tau_last - G_N]
```

Nominally, this gives one complete tone period between the two frozen
`24/gamma` edge guards. Scalar conversion can move the represented endpoint
slightly. For each candidate case, construct high-precision normalized
timestamps from its represented cutoff and intervals as specified below, then
use the fit window `[G_N, tauhat_last-G_N]`. Fit a constant, cosine, and sine
term separately to input and output at actual represented timestamps in that
closed window. Include only timestamps inside the window. For
reproducibility, form Voronoi cells from the included timestamps: place each
interior boundary halfway between neighboring included samples, and extend
the first and last cells to the fit-window edges. The weight of a sample is
the length of that clipped cell. If `X=b_x-i*c_x` and `Y=b_y-i*c_y` are the
fitted input and output phasors, use `Y/X` for gain and phase. Compare its
magnitude and phase with

```text
gain_fb(W) = 1 / (1 + (W/alpha_N)^(2*N))
phase_fb(W) = 0
```

under the unchanged scalar-specific gain and phase tolerances in
`forward-backward.md`. Where `gain_fb < 1e-3`, report phase fit and residual
but do not score phase. Include the fitted constants and residuals in
diagnostics. At coarser `q_max`, compare tone outputs with the exact
finite-record oracle for the stored piecewise-linear input; do not compare its
chords directly with the unsampled infinite sine.

For the supplementary uniform-grid FFT check, make a separate exact-period
grid for each tone with `n_W = ceil(P_W/(pi/32))` samples. Construct its
periodic points directly as `tau_i = i*P_W/n_W` for `i = 0..n_W-1`; do not
obtain them by repeated addition and do not duplicate the endpoint. Apply the
smooth two-pass gain `gain_fb(W_m)` at every DFT frequency `W_m`; do not use a
brick-wall mask. Construct the multi-tone FFT grid independently in the same
indexed way with 4,096 samples `tau_i = i*T/4096`. It corresponds to the
uniform `q_max = pi/32` spacing but is not borrowed from the finite-record
schedule. These FFT checks are supporting comparisons only and never replace
the constant-continued finite-record oracle, including at the boundaries.

## Deterministic interval schedules

Cross every frozen maximum normalized interval

```text
q_max in [pi/128, pi/64, pi/32]
```

with these schedules, resetting the sequence index to zero for each record:

```text
uniform:      [1]
jitter:       [1/2, 1, 3/4, 1/4, 7/8, 5/8]
deliberately uneven: [1, 1/32, 1/2, 1/16, 3/4, 1/8]
```

Represent every multiplier as an integer number of `q_max/32` ticks:

```text
uniform ticks:             [32]
jitter ticks:              [16, 32, 24, 8, 28, 20]
deliberately uneven ticks: [32, 1, 16, 2, 24, 4]
```

Accumulate only the integer tick count. Derive each nominal timestamp directly
as `tau_i = tick_count*q_max/32`; never obtain it by repeatedly adding a
floating-point interval. The shared `T = 128*pi` records have exact total tick
counts of 524,288, 262,144, and 131,072 for `q_max = pi/128`, `pi/64`, and
`pi/32`, respectively. Stop those records by comparing integer tick counts, so
an exactly divisible schedule cannot acquire a rounding sliver.

Tone and other non-tick-aligned endpoints use one frozen 256-bit,
round-to-nearest fixture calculation. Append full rational-tick intervals
while their directly calculated timestamp is below `T_record`, then append
the positive high-precision remainder once. The 192-bit and 256-bit oracle
evaluations consume this same frozen record; they do not regenerate its
schedule at their working precision. Every interval obeys `q_i <= q_max`, and
no zero-length sentinel interval is appended.

Only after the complete high-precision schedule exists, convert each nominal
timestamp independently to `f64` for analytic waveform evaluation. Never form
the next timestamp by adding to the preceding `f64` timestamp. This prevents
an exactly divisible record from acquiring a tiny terminal interval through
rounding drift. Fail construction if converted timestamps are not strictly
increasing. This rule also applies when `T_record` is the order- and
tone-specific endpoint above.

After constructing the nominal schedule as above, convert each `tau_i` to
`f64` and evaluate its analytic sample once. For scalar type `T` and nominal
cutoff `fc`, form exactly the values passed to or derived inside the candidate:

```text
fc_T                  = round_T(fc)
omega_candidate_T     = round_T(TAU_T * fc_T)
dt_i_T                = round_down_T(q_i / (2*pi*promote(fc_T)))
u_i_T                 = round_T(f(round_f64(tau_i)))
```

Here `TAU_T` is the scalar's represented `TAU` constant, and `round_T`
operations for candidate values are done in `T` in the same order as
candidate configuration. `omega_candidate_T` records the time scale that the
candidate derives internally from `fc_T`; it is not an oracle input. The
oracle deliberately does not mirror that `TAU_T` multiplication, whose
rounding remains part of candidate error. `round_down_T` chooses the greatest
positive representable duration no larger than the high-precision quotient,
preserving the nominal `q_i` hard bound after cutoff conversion. The
independent oracle promotes the exact represented `fc_T`, `dt_i_T`, and
`u_i_T` values to its high-precision type. It does not use nominal `tau_i`,
`q_i`, or unrounded `f64` input samples as the candidate record's geometry or
values. Reconstruct its actual normalized timestamps with the mathematical
`2*pi` constant at high precision:

```text
tauhat_0 = 0
tauhat_(i+1) = tauhat_i + 2*pi*promote(fc_T)*promote(dt_i_T)
```

The oracle knots are `(tauhat_i, promote(u_i_T))`, with constant half-lines.
Thus it scores the exact finite record represented by candidate inputs;
scalar conversion effects remain in the candidate cell instead of being
hidden by nominal timestamps. This applies to both scalars, both physical
cutoffs, tone fits, and refinement comparisons.

## Collinear refinement and scale

Two related refinements serve different purposes and must not share rounded
data accidentally.

For the oracle self-check, start from its high-precision knots and insert the
exact one-third and two-thirds points of every segment in high precision:

```text
tau_1 = tau_i + (tau_(i+1)-tau_i)/3
tau_2 = tau_i + 2*(tau_(i+1)-tau_i)/3
u_1   = u_i + (u_(i+1)-u_i)/3
u_2   = u_i + 2*(u_(i+1)-u_i)/3
```

These knots define exactly the same continuous piecewise-linear signal. The
original and refined oracle evaluations at original timestamps must meet the
`1e-12*Q` peak self-check in `forward-backward.md`. No candidate-scalar
conversion participates in this check.

Separately, for every represented candidate interval of every base fixture and
schedule, create a scalar refinement with two additional elapsed positions
near one-third and two-thirds. Compute them from its represented base duration
`d_T = dt_i_T`, not from nominal `q_i`:

```text
a_T = round_T(d_T/3)
b_T = 2*a_T                    # exact power-of-two scaling in T
d_1_T = a_T
d_2_T = a_T
d_3_T = d_T - b_T
theta_1 = promote(a_T)/promote(d_T)
theta_2 = promote(b_T)/promote(d_T)
u_1_T = round_T(promote(u_i_T)
                 + theta_1*(promote(u_(i+1)_T)-promote(u_i_T)))
u_2_T = round_T(promote(u_i_T)
                 + theta_2*(promote(u_(i+1)_T)-promote(u_i_T)))
```

Compute the elapsed offsets in `T`; after promoting each represented duration,
`d_1_T + d_2_T + d_3_T` must equal `d_T` exactly. Reject a construction if a
split duration is nonpositive or that identity fails. `theta_1` and
`theta_2` are the actual represented elapsed fractions, near one-third and
two-thirds. The normalized oracle
intervals are `2*pi*promote(fc_T)*promote(d_j_T)`, so their high-precision sum
is exactly the base interval's represented normalized duration. Inserted
values come from the represented base segment at its represented elapsed
fractions, not by re-evaluating the analytic fixture. They are collinear in
high precision before their one conversion back to `T`, but the stored scalar
values need not be exactly collinear. The scalar-refined record is therefore
scored against its own exact finite-record oracle. Compare candidate outputs
only at original timestamps. Both scalar partitions must separately pass their
oracle gates, then meet the unchanged mutual-refinement RMS and peak limits
from `forward-backward.md`; those mutual limits include this unavoidable input
representation difference.

## Boundary and reversal probes

The seven time-domain records and ten tone records are the response matrix,
not replacements for the separate boundary invariants in
`forward-backward.md`. Freeze these additional probes:

- constant records with every sample equal to `0`, `3/4`, and `-5/4`, using
  the shared `T = 128*pi` schedules; every completed output must be bit-exact;
- an asymmetric two-sample ramp with values `[-3/8, 7/8]` and one represented
  interval obtained from each `q_max` by the conversion above;
- a long endpoint-slope ramp on `[0, 8*pi]`,
  `u(tau) = -3/8 + (5/4)*tau/(8*pi)`, under all three schedules;
- the reverse-start sentinel: the order-8, `f64`, 1 Hz, `q_max = pi/32`
  deliberately uneven right-adjacent step, whose transition remains close to
  the final boundary. Initializing the reverse pass from only its last stored
  forward endpoint must fail at least one whole-record oracle gate before the
  sentinel is accepted into the frozen test data.

For every response-matrix record, reverse both the scalar samples and the
scalar interval sequence, run the batch operation, and reverse its output.
Score that result against the correspondingly reversed oracle under the same
whole-record gates. The two edge-step orientations provide an explicit pair
whose analytic records are reversals; the generic reversal check still
applies to every record.

For each scalar-specific input record, use exactly the frozen observable scale

```text
Q_f64 = max(max_i(abs(u_i)), 1e-3)
Q_f32 = max(max_i(abs(u_i)), 1e-2)
```

These are the default absolute-tolerance divided by relative-tolerance floors
from the design record. Use one `Q` for every output, oracle, and refinement
metric belonging to that input record. It is computed from the actual stored
input samples, not from a candidate result or a schedule-dependent analytic
maximum. Trapezoidal RMS uses the promoted represented physical `dt_i_T`
weights, or equivalently the actual normalized intervals derived from
`2*pi*fc_T*dt_i_T`; do not weight candidate metrics with nominal `q_i`.

## Formula and construction invariants

Before scoring filters, fixture tests should establish the following cheap,
deterministic properties:

- every formula returns the same value at the same `tau`, independent of
  schedule, cutoff, scalar, or interval index;
- all seven time-domain records are finite and bounded by their stated
  amplitude bounds over `[0,T]`, and every tone is bounded by one;
- every time-domain fixture has nonzero sampled scale and nonzero finite-record
  oracle energy; tones have unit analytic amplitude and nonzero gain reference;
- the multitone closes after `T`; the Gaussian pulse and triangular pulse are
  centered; the chirp is zero with zero slope at its endpoints; each step has
  the stated finite slope and plateaus; both edge-step records are reversals;
- all schedule endpoints are strictly increasing, close exactly at their
  record endpoint, and obey their `q_max` bound;
- each high-precision oracle refinement preserves the base segment exactly;
- each scalar one-third/two-thirds refinement preserves total represented
  duration exactly and differs from the base segment only by the disclosed
  sample conversion rounding.

This analytic-function check and the collinear-refinement check are separate:
the first samples `f(tau)` directly at each schedule's timestamps, while the
second preserves the particular piecewise-linear record generated by one
base schedule.

## Test tiers without matrix reduction

Use tiers to shorten feedback during development, not to remove acceptance
cells:

1. **Fixture tier:** formula bounds, nontriviality, schedule closure, endpoint
   constants, analytic-value reproducibility, and collinear construction.
2. **Oracle tier:** run all frozen multiprecision, symmetry, transfer,
   constant-gain, and oracle-refinement checks before using any candidate
   score. Cache/reuse oracle values only when every key (order, exact input
   samples, timestamp, and scalar rounding) matches.
3. **Focused development tier:** a small representative slice may be run while
   editing a candidate, but its result is diagnostic only.
4. **Full acceptance tier:** retain every frozen per-pass order, scalar,
   physical cutoff, `q_max`, schedule, fixture, tone phase, and refinement
   combination required above. Split this tier into deterministic test shards
   by fixture or matrix coordinates if needed; the normal full acceptance run
   must execute every shard. Do not mark omitted cells ignored, infer their
   result from another cell, or substitute a finer interval schedule for a
   failing `q_max = pi/32` cell. Run the separately frozen boundary, reversal,
   and reverse-start probes in addition to this response matrix.

The response matrix contains 2,448 base cells before scalar refinements. The
oracle implementation must use a linear-time prefix/suffix modal accumulation
or an equivalently bounded analytic recurrence for each record; evaluating
every input segment afresh for every output timestamp is not an acceptable
implementation strategy. This computational requirement does not reduce the
matrix or weaken the independent modal-kernel definition.

## Acceptance harness and artifacts

The complete matrix is a dedicated development acceptance run, not part of an
ordinary `cargo test`. Its canonical entry point is:

```text
cargo run --release --manifest-path tools/forward_backward_oracle/Cargo.toml -- matrix --output target/forward-backward-matrix
```

The isolated multiprecision tool owns fixture construction, oracle evaluation,
scoring, and report generation. For each shard it starts one root-library
unit-test client and streams one represented record at a time through a
versioned framed binary protocol over a loopback connection. The client calls
the actual private batch prototype and returns its exact scalar output bits
plus work diagnostics. This keeps `rug` outside the root dependency graph,
adds no public or feature-gated library surface, and bounds live record storage
independently of total matrix size. Routine root tests exercise protocol
framing and a compact smoke case; they do not start the multiprecision matrix.
Those tests use root-local codec code or frozen byte vectors, not an oracle
dev-dependency that would pull `rug` and its native libraries into root tests.

The protocol is versioned and framed, represents candidate values by their
exact `f32` or `f64` bits, bounds every decoded length before allocation, and
identifies every case by both coordinate and represented-record digest. A
successful response repeats that identity and supplies exact output bits plus
per-pass work. A structured candidate-error response instead records the pass,
interval, and source category when available. Unknown versions or tags,
inconsistent lengths, arithmetic overflow, truncation, and identity mismatch
invalidate the run. Candidate errors and non-finite successful outputs are
cell errors: the report records them and continues when protocol and oracle
integrity remain intact. Response arrival order is not significant; reports
sort by the frozen coordinate order. The concrete byte layout, connection
handshake, and process concurrency belong to the implemented protocol version,
not this numerical contract.

The default response-matrix shard key is `(fixture record, per-pass order)`,
with fixture records in this document's declaration order and orders
`[1, 2, 4, 8]`. Boundary probes occupy a separate named shard namespace.
Alternative deterministic partitions may be used after measurement, provided
the manifest proves that every required coordinate is covered exactly once.
Unfiltered `matrix` executes every response and boundary shard. Development
filters may select a fixture record or probe, order, scalar, cutoff, maximum
interval, or schedule, but a filtered report is diagnostic and cannot satisfy
the full-acceptance gate. Every report records the selected coordinate set and
its digest so partial and complete runs cannot be confused.

Full output arrays are generated and consumed during the run; they are not
committed or cached as the reference. The committed evidence is compact:

- a coordinate manifest and digest covering formulas, scalar conversions,
  schedules, refinements, reversals, and boundary probes;
- typed metrics rows whose member case identifiers are explicit: single-cell
  rows contain oracle convergence RMS and peak, imaginary residual, candidate
  RMS and peak, tone fit fields where applicable, per-pass work, and every
  local gate; comparison rows contain the base/refined, 1/37 Hz, reversal, or
  other grouped members and their corresponding gates;
- an aggregate summary with tool/protocol versions, canonical source and
  manifest digests, complete-versus-filtered status, observational elapsed
  time, and the count of passed, failed, and errored cells. Host-local peak
  resident memory may be reported when available only if its platform and
  measurement method are named; time and memory are non-gating observations.

The canonical source digest covers the relevant ssfilt source-file contents,
oracle and client protocol source, fixture document/version, oracle
`Cargo.lock`, and manifest bytes. A Git commit plus an explicit dirty-state
digest may encode the same identity. Absolute paths, timestamps, target
directories, report ordering, and other host-dependent metadata are excluded.

Reports collect all cells in their selected coordinate set instead of stopping
at the first numerical failure. Transport, validation, or oracle self-check
failures still make the run unsuccessful and are distinguished from an
expected candidate gate failure. The full matrix initially remains an explicit
acceptance job until measured runtime and memory justify a normal CI cadence.
A candidate may be selected at stage 5 only after that candidate passes a
complete, reproducible run. The stage-4 endpoint negative-control report remains
complete and useful when it records expected gate failures.

Fresh 192-bit and 256-bit oracle runs score both the time-weighted RMS and peak
convergence gates for every represented base, scalar-refined, and reversed
record. Exact high-precision collinear refinement is exhaustive for every
selected base record; it is not recursively reapplied to the derived
scalar-refined or reversed records, whose own convergence, partition, and
reversal gates remain exhaustive. Compact `verify` remains a fast oracle smoke
test and does not substitute for these per-record checks.

The frozen whole-record, tone, partition, physical-scaling, boundary, tail,
and work ceilings remain those in `forward-backward.md`. This file supplies
only reproducible inputs and execution organization.
