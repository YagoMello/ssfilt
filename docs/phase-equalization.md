# Phase equalization design record

## Status

Initial independent proposal, recorded before inspecting the discarded
equalizer implementation, followed by an analytic feasibility implementation.
The bounded one-section search now passes the mandatory Butterworth order 4
and 8 core-band gates and rejects cutoff-wide claims for both orders. The
coupled-integrator realization, steady-tone checks, and complete three-signal
streaming waveform matrix now pass for both orders and scalar types. A narrow
public API has been implemented for these two Butterworth cases and the core
band; its independent numerical and API review is complete.

The analytic tool also emits one inspectable SVG per case. These plots use the
stored holdout records and display their actual points as rings; they do not
claim that a streaming realization has reproduced the analytic curves.

The private solver now accepts a fixed-size state abstraction, with ordinary
filters still using arrays. An exact parity test covers an equivalent split
state, so the coupled source/equalizer model can be added without unstable
generic array expressions or a public solver interface.

A test-only coupled Butterworth prototype exercises the accepted order 4 and
8 core-band candidates. It passes exact equilibrium and steady-tone
magnitude/phase checks for both `f32` and `f64`; stage coupling, step bounds,
linear-ramp partition, physical scaling, and transactional failure currently
have structural `f64` coverage. Together with the independent waveform tiers
below, this completes the private order-4/8 core-band numerical gate. It does
not by itself define or review a public phase API.

The first waveform tier now passes a frozen unequal-amplitude, unequal-phase
five-tone signal. The test uses an independent closed-form Butterworth
magnitude plus predeclared source and corrected constant-delay targets; it does
not fit delay or phase from streamed output. Each stream is scored against its
own target and reference energy, so the relative improvement cannot be earned
merely by adding latency. All 24 order/scalar/schedule/cutoff cells pass.
The current observed worst results are shown below; automated tests enforce the
predeclared gates rather than snapshotting these exact informational values.

| Order | Worst `f64` shape error | Worst `f32` shape error | Worst corrected/source error ratio |
| --- | ---: | ---: | ---: |
| 4 | 0.0863% | 0.0863% | 1.06% |
| 8 | 0.438% | 0.438% | 3.35% |

The cells cover uniform, deterministic-jitter, and bounded-gap schedules at
1 Hz and 37 Hz cutoff.

The Gaussian-modulated-carrier tier now also passes all 24 cells. It uses a
20-cutoff-cycle periodized fixture, verifies the raw spectrum before projecting
it to the scored band, warms for one complete frame, and scores the complete
second frame against the independent magnitude-plus-delay oracle. The frame
provides 24.5 decay constants for the slowest order-8 Butterworth pole. These
observed worst results are informational; the tests enforce the same
predeclared absolute and relative gates as the multi-tone tier.

| Order | Worst `f64` shape error | Worst `f32` shape error | Worst corrected/source error ratio |
| --- | ---: | ---: | ---: |
| 4 | 0.0799% | 0.0799% | 0.552% |
| 8 | 0.330% | 0.330% | 1.44% |

The raw Gaussian fixture has 99.999897% of its two-sided discrete-spectrum
energy inside the scored band. The remaining 0.000103% is reported and
excluded by driving both the streamers and reference from the same in-band
projection.

The tapered linear-chirp tier now passes the same 24-cell matrix. Its
instantaneous frequency sweeps from `0.05` to `0.65 * cutoff`, and a squared
sine taper closes the periodic fixture without a hard seam. The raw fixture
has 99.994350% of its two-sided discrete-spectrum energy inside the scored
band; the remaining 0.005650% is reported and excluded through the same
input/reference projection policy.

| Order | Worst `f64` shape error | Worst `f32` shape error | Worst corrected/source error ratio |
| --- | ---: | ---: | ---: |
| 4 | 0.0769% | 0.0769% | 0.558% |
| 8 | 0.352% | 0.352% | 1.61% |

The same fixtures also provide the promised matched-order Bessel comparison.
Every Bessel stream is scored against its own analytic magnitude and minimax
constant-delay oracle over the core band. All 72
order/signal/schedule/scalar/cutoff cells produce finite errors with
nontrivial reference energy and cutoff-invariant normalized results:

| Signal | Order | Worst `f64` Bessel error | Worst `f32` Bessel error |
| --- | ---: | ---: | ---: |
| Multi-tone | 4 | 0.0345% | 0.0346% |
| Multi-tone | 8 | 0.00686% | 0.00686% |
| Gaussian carrier | 4 | 0.0614% | 0.0614% |
| Gaussian carrier | 8 | 0.0124% | 0.0124% |
| Tapered chirp | 4 | 0.0577% | 0.0577% |
| Tapered chirp | 8 | 0.0114% | 0.0114% |

This is a reporting comparison, not a superiority gate. Bessel and
Butterworth have different magnitude responses: at `0.7 * cutoff`, Bessel is
about 1.17 dB below order-4 Butterworth and 1.44 dB below order-8 Butterworth;
both share the same −3 dB magnitude at cutoff. The all-pass equalizer instead
retains the Butterworth magnitude curve while paying added causal delay.

All three waveform tiers now pass their predeclared absolute and relative
limits across both precisions, all schedules, and both physical cutoffs. These
results justified the smallest order-4/8 Butterworth core-band API; they do
not imply cutoff-wide, higher-order, or other-response support.

## Decision summary

The initial public streaming feature is deliberately narrow:

- Butterworth low-pass orders 4 and 8 only;
- equalization validated from DC through `0.7 * cutoff_hz`;
- Bessel remains the no-equalizer waveform-fidelity baseline;
- one real second-order continuous-time all-pass section;
- coefficients selected offline and stored as validated normalized data;
- source and equalizer integrated as one coupled continuous-time model.

Chebyshev Type I remains in the investigation matrix, but is not promised for
the first release. High-pass and band-pass equalization are deferred because
their useful-band and latency semantics need separate designs. A Bessel
equalizer is out of scope: if Bessel already meets the requirement, adding
states and delay is the wrong tradeoff.

The numerical and waveform gates for these two cases passed before the narrow
builder API and coefficient table were added. No other response/order/band
combination is implied by those results.

## Product value and pre-1.0 retention gate

Passing the numerical gates proves that the equalizer implements its declared
response; it does not by itself prove that the extra feature is preferable to
a simpler response family. The currently defensible use case is deliberately
narrow: retain Butterworth's nearly flat passband and sharper magnitude
selectivity for signals confined to the validated core band, while accepting
additional causal delay to preserve their waveform shape.

This differs from the simpler alternatives. At `0.7 * cutoff_hz`, the
order-4/order-8 Butterworth magnitude is approximately `-0.243`/`-0.0144` dB.
The matched Bessel responses are about `1.17`/`1.44` dB lower than those
values, while the repeated-pole responses are approximately `-1.54`/`-1.51`
dB. Bessel and repeated-pole therefore remain attractive when that passband
droop and their gentler transition are acceptable. They avoid the equalizer's
two extra states and added latency. The current equalizer's mean added
delay is approximately `0.597 / cutoff_hz` seconds for order 4 and
`0.686 / cutoff_hz` seconds for order 8; at a 20 Hz cutoff that is about
29.8 ms and 34.3 ms respectively, in addition to the source-filter delay.

Before the 1.0 API is frozen, retain or remove the streaming equalizer using a
predeclared application matrix that compares matched-order plain Butterworth,
phase-equalized Butterworth, Bessel, and repeated-pole filters. It must include:

- desired-band multitone, pulse, and swept-frequency signals plus declared
  stopband interference;
- waveform error against each response's independently predeclared analytic
  constant-delay target, without fitting delay or phase from measured output;
  passband magnitude error and stopband rejection;
- step undershoot, overshoot, and settling behavior;
- total and added latency, state size, adaptive work, and measured throughput;
- uniform and irregular schedules in the existing frozen 1 Hz and 37 Hz
  cross-scaling cases, plus the named scenario's cutoff when different.

Thresholds and the named user scenario must be frozen before running this
comparison. Retain the equalizer only if at least one frozen representative
workload needs its magnitude selectivity, accepts its disclosed delay, and
meets every predeclared hard application threshold only with the equalized
filter while plain Butterworth, Bessel, and repeated-pole each fail at least
one. The winning margins must exceed measurement uncertainty and a frozen
minimum practical effect. Remove it before 1.0 if a simpler response satisfies
every named workload, every target scenario requires correction beyond the
validated band, or measured throughput exceeds the workload's frozen cost
ceiling. This is a product retention gate, not permission to weaken any
existing numerical test.

Expansion follows an even stronger rule. A higher Butterworth order must pass
the complete analytic and streaming matrix independently. Chebyshev is worth
investigating only for a use case that actually values its sharper transition;
an all-pass cannot remove its magnitude ripple, and validation must extend to
the response-specific ripple edge rather than reusing the Butterworth core
band. Bessel equalization remains out of scope unless a new requirement shows
that Bessel itself is insufficient. High-pass and band-pass require their own
useful-band and latency contracts. No expansion is justified merely because a
bounded analytic search finds coefficients.

## Problem statement

A filter can give frequencies in its useful band different delays. A
multi-frequency waveform can therefore change shape even where its frequency
components retain acceptable magnitude. A phase equalizer is useful only if
it measurably reduces that shape change while retaining a source response whose
magnitude selectivity the user actually wants.

An all-pass equalizer cannot repair passband droop or ripple. Comparisons must
therefore use a reference with the source filter's exact magnitude and a
constant delay, not the original input waveform.

A causal stable equalizer adds delay so earlier frequency components wait for
later ones. It cannot remove all latency or make a general streaming filter
zero phase. Offline forward-backward filtering is a separate noncausal batch
operation and squares the source magnitude response.

## Candidate topology

For normalized angular frequency, use a stable real second-order all-pass
section with poles `-sigma +/- j*eta`:

```text
A(s) = (s^2 - 2*sigma*s + sigma^2 + eta^2)
       -------------------------------------
       (s^2 + 2*sigma*s + sigma^2 + eta^2)
```

Its magnitude is one and its group delay is:

```text
tau_A(w) = 2*sigma / ((w - eta)^2 + sigma^2)
         + 2*sigma / ((w + eta)^2 + sigma^2)
```

A section realization that avoids polynomial expansion is:

```text
x' = v
v' = u - 2*sigma*v - (sigma^2 + eta^2)*x
y  = u - 4*sigma*v
```

It has unity DC gain and an exact constant-input equilibrium. Stability is
guaranteed by construction when `sigma > 0`.

The streaming prototype stores the position state in the scaled coordinate
`q = (sigma^2 + eta^2) * x`:

```text
q' = (sigma^2 + eta^2)*v
v' = u - 2*sigma*v - q
y  = u - 4*sigma*v
```

This is the same transfer function, while its equilibrium is exactly
`[q, v] = [u, 0]`. It avoids equilibrium division and keeps the all-pass state
on roughly the same scale as the source signal.

All-pass delay is nonnegative, and cascaded delays add. If the desired total
delay is `D`, the equalizer must approximate `D - tau_source(w)`; consequently
an exact zero-residual target cannot use a `D` smaller than the source's
greatest delay in the useful band. With the allowed minimax residual
`R_peak`, the selected midpoint target is bounded below by the source's
greatest delay minus `R_peak`. The added delay must be reported as a cost, not
hidden inside a relative percentage.

The initial experiment is limited to one section. A second section may be
investigated later, but only after the first section's failures are classified
and the extra section demonstrates material absolute improvement. We will not
force inactive or negligible sections merely to make an order configurable.

## Design and coefficient policy

The design tool may use `std`, allocation, dense numerical evaluation, and a
bounded optimizer. Runtime construction may not. Accepted `sigma` and `eta`
values are normalized by the low-pass cutoff angular frequency. After the API
decision, accepted values can be stored as validated data for each exposed
response/order/band combination, following the same general reliability policy
as the Bessel prototypes.

This policy gives:

- deterministic and inexpensive construction;
- no runtime root finding or nonlinear convergence failure;
- fixed two-scalar equalizer state;
- identical normalized behavior at every physical cutoff;
- an explicit unsupported result for combinations that fail validation.

Fit and validation grids must be different. The holdout grid must include both
band endpoints, section pole frequencies that lie nearby, and enough points to
detect narrow peaks. Candidate sections must also satisfy all of:

```text
sigma >= 0.2
section Q <= 2
sqrt(sigma^2 + eta^2) <= 4
```

These initial bounds prevent a fit from buying an attractive sampled curve
with a lightly damped, long-ringing, or needlessly fast section. They may be
changed only with transient and integration-cost evidence recorded here.

## Band policy

The original `0.7 * cutoff_hz` proposal is retained as a **core-band** test,
not as a universal definition of the passband. Every candidate is evaluated
over these predeclared ranges:

```text
Core:       0 <= W <= 0.7
Cutoff:     0 <= W <= 1.0
Diagnostic band-edge sweep: 0.5 through 1.0 in steps of 0.05
```

Plots extend beyond the cutoff so a curve cannot disappear at the difficult
edge. Reports contain separate `0..0.7`, `0.7..1.0`, and `0..1.0` summaries.
The cutoff-wide result is always visible even when only the core study passes.

Chebyshev needs a response-specific interpretation. Its ripple edge is below
the public -3 dB cutoff by an order- and ripple-dependent amount. For 0.5 dB
ripple it is approximately `0.904 * cutoff` at order 4 and `0.974 * cutoff` at
order 8. Passing only the core test is therefore insufficient for Chebyshev:
either the cutoff study or an explicit ripple-edge study must pass the same
absolute gates before Chebyshev can be called supported.

The diagnostic sweep maps the error/latency frontier and helps choose a small
set of useful designs. Sweep points are report columns, not supported library
values, and do not authorize interpolation between coefficient tables. The
initial API selects the fixed core band and reports its upper edge through
`validated_band_edge_hz()`; it does not expose a caller-selected band.

## Metrics

Let normalized frequency be `W = f / cutoff_hz` and normalized group delay be
`t(W) = 2*pi*cutoff_hz*tau(W)`. Apply the following scores independently to
each claimed band.

For the primary minimax score, choose the best constant delay:

```text
D = (max(t) + min(t)) / 2
R_peak = max(abs(t(W) - D))
```

Also report uniform-frequency RMS delay residual about its best constant,
mean and maximum equalizer delay, and the corresponding physical delay in
seconds. RMS delay uses the uniform-frequency mean as its independently best
constant. Phase uses the same `D` as the primary minimax delay score, is
anchored at zero phase at DC, and reports:

```text
phase_residual(W) = phase_source(W) + phase_equalizer(W) + D*W
```

The waveform oracle uses this same `D`; it does not fit a separate phase slope
or constant phase rotation. These are absolute normalized quantities;
dividing residual error by the newly enlarged total delay is forbidden as a
success metric.

The analytic holdout tier must meet all of these gates:

```text
R_peak <= 0.10
R_peak <= 0.25 * uncorrected_R_peak
maximum residual phase <= 0.05 radian
mean added normalized delay <= 5.0
maximum relative magnitude change <= 1e-12 for f64
maximum relative magnitude change <= 1e-5  for f32
```

The initial deterministic search uses 4,096 uniformly spaced cell midpoints
as its fit grid. The holdout uses 8,193 uniform intervals (8,194 points), so
the two rational grids are disjoint while exceeding the required validation
density. It searches a 65-by-65 bounded grid in section `Q` and
logarithmic `sigma`, resolves the best coarse cell on a 33-by-33 local grid,
then refines eight finalists for 18 fixed levels. Candidate refinement permits
at most 16 improving moves per level. Candidate selection never reads the
holdout scores. The holdout has 8,194 uniform points including both endpoints
plus in-band section-neighborhood frequencies; selected cases are
cross-checked on 16,387 uniform points. Search constants and the resulting
evaluation count are exported with the report.

The core band remains the minimum release gate for Butterworth orders 4 and 8.
A whole-passband claim additionally requires the cutoff study to pass. Results
beyond cutoff are diagnostic rather than scored because magnitude approaches
zero and phase ceases to describe a useful transmitted waveform.

## Validation matrix

The exploratory offline study matrix is a roadmap, not a declaration of public
support. Entries above order 8 remain deferred until the initial streaming
gate passes:

| Response | Source orders | Role |
| --- | --- | --- |
| Repeated pole | 2, 4, 8, 16 | later investigation; not in the initial API |
| Butterworth | 2, 4, 8, 16 | orders 4 and 8 are initially supported |
| Chebyshev I, 0.5 dB ripple | 4, 8, 16 | investigation only |
| Bessel | 2, 4, 8, 16, 25 | unequalized baseline |

Only response/order/band pairs that pass their own gates may be exposed. The
initial API covers Butterworth orders 4 and 8 over the core band only. Passing
an exploratory matrix entry does not automatically expand that support. Every
candidate report records its validated band edge, while the public type
currently reports the fixed supported edge in hertz.

Orders 4 and 8 are the streaming-design gate, not the intended permanent
matrix limit. After both pass the coupled streaming and waveform criteria,
investigate the order sweep incrementally up to order 16 as evidence warrants.
Treat each response/order pair as a separate result: a higher order may
require more all-pass sections or may be rejected rather than inheriting
support from a neighboring order. At that stage, response dashboards should
overlay the source, analytic equalized response, and measured equalized
streaming points for magnitude, phase, and group delay. Do not broaden the
public builder from analytic curves alone.

Analytic group delay comes from pole contributions or the complex logarithmic
derivative, not finite differences of wrapped phase. Validate on a dense
holdout grid of at least 8,193 points and cross-check selected cases at twice
that density. Round accepted coefficients to `f32`, then repeat the stability,
delay, phase, and latency checks. Evaluate the rounded coefficients with the
same `f64` analytic oracle so this check isolates coefficient quantization; an
eventual streaming implementation must separately validate its actual `f32`
state realization.

Use three time-domain signals whose energy lies predominantly in the band being
evaluated:

- an unequal-amplitude, unequal-phase multi-tone signal;
- a Gaussian pulse or Gaussian-modulated carrier;
- a tapered chirp across the selected band.

At least 99% of each pulse or chirp's discrete reference-spectrum energy must
lie inside the scored band after windowing. Report the remaining leakage and do
not count it toward the in-band waveform score. Multi-tone frequencies are all
placed strictly inside the scored band.

Each oracle retains its response's analytic magnitude and replaces its phase
with that response's independently determined best constant delay. The
uncorrected Butterworth source and corrected response are scored against their
own targets and reference energies. No delay or phase is fitted from streamed
output. The streaming result must satisfy:

```text
waveform-shape RMS error <= 2% for f64 and <= 4% for f32
shape error <= 0.25 * the uncorrected source's shape error
steady-tone magnitude change <= 0.1% for f64 and <= 0.5% for f32
```

Run uniform, deterministic-jitter, and bounded gap schedules. The fine-grid
reference tier limits every interval to at most `1/80` of the shortest tone
period. A separate linear-ramp partition test verifies that splitting the same
piecewise-linear input interval does not alter the continuous cascade beyond
the configured integration tolerance. Repeat at a second physical cutoff with
the same normalized time trace.

The equalized Butterworth result is also compared with a matched-order Bessel
filter. The comparison reports both waveform error and their different
magnitude responses; it must not imply that the all-pass makes Butterworth and
Bessel equivalent.

## Integration architecture

Feeding a second streaming filter only the source filter's endpoint outputs is
not the intended continuous-time cascade. It makes the second filter linearly
interpolate those endpoints and misses the source's Runge-Kutta stage outputs,
so the answer can depend on how an interval is partitioned.

For source state `x`, equalizer state `z`, and source output `y_s`, each solver
stage must instead evaluate:

```text
x' = source.derivative(x, u)
z' = equalizer.derivative(z, y_s(x, u))
y  = equalizer.output(z, y_s(x, u))
```

The likely enabling refactor makes the private solver operate on a private
fixed-size state abstraction. Ordinary filters continue to use `[T; N]`; the
composite model uses a structure containing `[T; N]` and `[T; 2]`. This avoids
unstable generic expressions such as `[T; N + 2]`, preserves allocation-free
storage, and does not expose a general ODE framework.

The refactor must land separately with exact behavioral-parity tests before an
equalizer model is introduced.

## Public UX direction

If the evidence passes, the preferred entry point is a terminal operation on
the existing builder:

```rust,ignore
let filter = LowPass::<4>::builder(20.0)
    .response(Response::Butterworth)
    .build_phase_equalized()?;
```

It returns one dedicated streaming-filter value containing the coupled model.
The user does not construct an all-pass section, solver, or composite object.
The builder preserves its input model, initial input, and integration settings.
The default repeated-pole response is unsupported here, so callers must select
Butterworth explicitly. Other responses and orders fail with
`BuildError::UnsupportedPhaseEqualization`.

The returned type exposes these metadata in physical units:

- `validated_band_edge_hz()`: the upper edge of the supported band, exactly
  `0.7 * cutoff_hz`; the claim covers DC through this edge.
- `target_constant_delay_seconds()`: the minimax constant-delay target for the
  combined source-plus-equalizer response. Actual group delay varies slightly
  with frequency.
- `mean_added_delay_seconds()`: the equalizer's mean group delay, averaged
  uniformly in frequency over the validated band; this describes added
  latency, not total delay at every frequency.

Arbitrary poles, caller-selected target delays, equalizer orders, optimizer
tolerances, and fitting weights remain private. The public type stays fixed
size and allocation-free, with two extra scalar states for the all-pass
section.

## References

- A. Stojanovic and G. Schiro, ["Design of continuous-time all-pass filters
  with maximally flat group delay"](https://doi.org/10.1049/el.2018.5104),
  derives continuous-time first- and second-order all-pass delay sections.
- M. Lang and T. Laakso, ["A method for design of phase
  equalizers"](https://doi.org/10.1109/ICASSP.1979.1170643), describes fitting
  all-pass networks to prescribed group delay.
- W. E. Thomson, ["Delay networks having maximally flat frequency
  characteristics"](https://doi.org/10.1049/jiee-2.1949.0141), establishes
  the Bessel/Thomson delay-flat baseline used for comparison.

## Staged implementation

0. Fix the independent baseline-review findings in `baseline-review.md`.
1. Refactor and extend the development tools described in
   `development-tools.md`: build an analytic evaluator, measured streaming
   overlays, bounded coefficient search, and machine-readable reports for the
   complete band sweep and response matrix.
2. Decide supported response/order pairs from the predeclared gates. The
   initial supported pair is Butterworth orders 4 and 8 over the core band. ✓
3. Refactor the private solver to support fixed composite state, with no public
   behavior change. ✓
4. Implement the coupled all-pass model and analytic/model tests privately. ✓
5. Complete the private streaming waveform matrix. Multi-tone, Gaussian,
   tapered-chirp, and matched-order Bessel comparison tiers pass. ✓
6. Add the smallest self-contained public API for only the validated cases,
   then run an independent numerical and API review before declaring the
   feature. The API is implemented and independently reviewed.
7. Treat offline forward-backward filtering as a later, separate batch
   milestone; it does not share the streaming equalizer's contract. Its
   investigation contract is recorded in `forward-backward.md`.

## Independent comparison

Inspected after the proposal and thresholds above were committed as `bee1418`.
The discarded implementation is commit `792ab0b` on `main`; none of its code
was copied into this branch.

It independently reached one important correct conclusion: filter and
equalizer states must be integrated together at every Runge-Kutta stage. Its
private `SolverState` abstraction also used separate fixed arrays behind
component indexing, which supports the composite-state direction proposed
above without unstable generic array lengths.

Its equalizer design differed substantially:

- one through four first-order all-pass sections, selected by a public const
  generic;
- an arbitrary user-supplied band extending as far as the cutoff;
- runtime coordinate search over only 33 frequency samples;
- sampled delay variance as the sole design objective;
- success when variance improved by just 5%;
- no absolute delay-ripple, latency, holdout-grid, transient, or waveform
  acceptance threshold.

Additional requested sections could settle at the search's maximum rate and
add nearly constant delay and runtime work with negligible absolute benefit.
Tests asserted only that a curve or four-tone phase score became better, not
that the result became useful.

Re-evaluating its deterministic search on a 16,385-point holdout grid explains
the observed weak behavior. Values below are normalized peak delay residuals
before and after equalization:

| Band | Butterworth case | Old design result | Added mean delay |
| --- | --- | --- | --- |
| DC–0.7 cutoff | order 4, 2 sections | `0.448 -> 0.112` | `2.79` |
| DC–0.7 cutoff | order 4, 4 sections | `0.448 -> 0.096` | `3.79` |
| DC–0.7 cutoff | order 8, 4 sections | `0.711 -> 0.201` | `4.31` |
| DC–cutoff | order 4, 2 sections | `0.648 -> 0.193` | `2.01` |
| DC–cutoff | order 8, 4 sections | `2.089 -> 0.841` | `6.10` |

Thus the old design misses the new absolute `0.10` gate for mandatory order 8
even after four states, and its cutoff-wide order-8 result also exceeds the new
latency cap. The order-4 four-section core-band result only barely passes while
using twice the state planned for the new candidate.

The comparison strengthens rather than changes the independent plan:

- retain coupled stage integration and a private composite-state solver;
- replace arbitrary runtime fitting with offline-validated normalized data;
- test one second-order section, whose complex pole pair provides more delay
  shape control for the same two states;
- keep the useful band and supported source cases explicit;
- do not expose section count or arbitrary fitting bands in the first API;
- require absolute and waveform gates before any public feature lands.
