# Phase equalization design record

## Status

Planning. No topology or public API has been selected.

This record must define the problem and acceptance criteria before the
alternative equalizer implementation is inspected. Findings from that later
comparison will be added in a separate section with their provenance stated.

## Problem statement

A causal low-pass or band-pass filter can give frequencies in its useful band
different delays. A multi-frequency waveform can therefore change shape even
when each component's magnitude remains acceptable. A streaming phase feature
is useful only if it measurably reduces that delay variation over a declared
band for representative filters and signals.

A causal stable equalizer may add delay so earlier frequency components wait
for later ones. It cannot remove all latency or make a general streaming filter
zero phase. Offline forward-backward filtering is a separate noncausal batch
operation and has a different magnitude response.

## Questions to resolve

- Which metric should drive the design: group-delay ripple, waveform error,
  or a bounded combination of both?
- How should users specify the useful band?
- Which source responses have enough phase distortion for equalization to be
  worthwhile compared with choosing a Bessel response?
- Which continuous-time all-pass topology remains well conditioned across the
  supported orders and scalar types?
- How are equalizer order, target delay, useful band, and achievable error
  related?
- Can construction be deterministic and allocation-free without runtime root
  finding or nonlinear optimization?
- Should the equalizer wrap a filter, be independently composable, or remain a
  batch/offline helper?
- What failure must construction report when a requested target is infeasible?

## Evidence required before implementation

The design proposal must include:

1. an independently derived or published transfer-function reference;
2. a metric evaluated over an explicit normalized passband;
3. comparison with the uncorrected response and a Bessel response;
4. a matrix covering multiple source orders and response families;
5. at least one multi-tone or pulse-like waveform test;
6. the added delay, equalizer order, runtime state, and derivative cost;
7. conditioning checks for `f32` and `f64`;
8. declared cases where no useful equalizer can be built.

Plots should make failures visible, but automated numerical assertions decide
whether the feature succeeds.

## Candidate approaches to research

- continuous-time first- and second-order all-pass cascades fitted to a target
  group delay over a bounded passband;
- closed-form or table-driven delay equalizers associated with classical
  low-pass prototypes;
- a direct choice of Bessel response when its magnitude tradeoff already meets
  the waveform requirement;
- offline forward-backward filtering for stored samples where noncausality and
  squared magnitude are acceptable.

Discrete-time equalizers that assume a fixed sampling interval need separate
justification because the library's primary timing model accepts variable
elapsed time.

## Decision gates

The streaming equalizer proceeds only if one bounded design:

- improves the predeclared metric across the chosen test matrix;
- has stable, finite coefficients and states at supported boundaries;
- offers a simpler user decision than manually selecting arbitrary poles;
- retains fixed-size, allocation-free runtime storage;
- states its latency and limitations precisely.

If those gates are not met, the library should document Bessel selection and
provide only the offline batch operation rather than ship a weak equalizer.

## Independent comparison

Pending. Do not inspect or summarize the alternative implementation until the
initial design proposal and acceptance thresholds are recorded above.
