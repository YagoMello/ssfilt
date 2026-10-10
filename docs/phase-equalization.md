# Phase equalization design record

## Status

Initial independent proposal, recorded before inspecting the discarded
equalizer implementation, followed by an analytic feasibility implementation.
The bounded one-section search now passes the mandatory Butterworth order 4
and 8 core-band gates and rejects cutoff-wide claims for both orders. Streaming
waveform and coupled-integrator gates remain unmet, so there is still no public
phase-equalizer feature.

## Decision summary

The first streaming candidate is deliberately narrow:

- low-pass filters only;
- predeclared study bands rather than an implicit whole-passband claim;
- repeated-pole and Butterworth responses as release candidates;
- Bessel as the no-equalizer waveform-fidelity baseline;
- one real second-order continuous-time all-pass section;
- coefficients selected offline and stored as validated normalized data;
- source and equalizer integrated as one coupled continuous-time model.

Chebyshev Type I remains in the investigation matrix, but is not promised for
the first release. High-pass and band-pass equalization are deferred because
their useful-band and latency semantics need separate designs. A Bessel
equalizer is out of scope: if Bessel already meets the requirement, adding
states and delay is the wrong tradeoff.

No public API or coefficient table should be added until the candidate passes
the numerical and waveform gates in this record.

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
first public API decision is deferred until this evidence shows whether one
fixed band, named bands, or a single upper-band argument offers the clearest
contract.

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

The offline study covers:

| Response | Source orders | Role |
| --- | --- | --- |
| Repeated pole | 2, 4, 8, 16 | release candidate |
| Butterworth | 2, 4, 8, 16 | primary release candidate |
| Chebyshev I, 0.5 dB ripple | 4, 8, 16 | investigation only |
| Bessel | 2, 4, 8, 16, 25 | unequalized baseline |

Only individual response/order pairs that pass may be exposed. The first
feature is worthwhile only if at least Butterworth orders 4 and 8 pass; this
ensures it addresses a response with a real selectivity-versus-phase tradeoff
rather than only a hand-crafted easy case. That minimum refers to the core
study. Every candidate report records its validated band edge; how the eventual
API communicates that edge remains a later decision.

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

The oracle retains the source's analytic magnitude and replaces its phase with
the best constant delay. After fitting only that constant time shift, the
streaming result must satisfy:

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
The returned type should expose the promised band and nominal/added delay in
physical units, with wording that does not claim exact latency at every
frequency. Unsupported responses and orders fail explicitly during build.

The exact public names and error variant are intentionally deferred until the
prototype meets the gates. Arbitrary poles, target delays, equalizer orders,
optimizer tolerances, and fitting weights remain private.

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
2. Decide supported response/order pairs from the predeclared gates. Stop the
   streaming feature if Butterworth orders 4 and 8 do not pass.
3. Refactor the private solver to support fixed composite state, with no public
   behavior change.
4. Implement the coupled all-pass model and analytic/model tests privately.
5. Add public streaming waveform tests, then the smallest self-contained API.
6. Run an independent numerical and API review before declaring the feature.
7. Treat offline forward-backward filtering as a later, separate batch
   milestone; it does not share the streaming equalizer's contract.

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
