# Streaming equalizer retention check

## Purpose

This is a small product-value screen for the existing order-4/order-8
Butterworth phase equalizer. Its correctness and supported band are already
covered elsewhere. This check asks whether the added states, work, and latency
solve a believable user problem better than the ordinary responses.

The workload, metrics, and thresholds below are frozen before any result is
calculated. Do not tune the signal or limits after seeing which filter wins.

## Workload

The scenario is pump/manifold pressure-signature monitoring. The output is
used for condition monitoring and event classification, not feedback or a
protection loop, so bounded streaming latency is acceptable. The filter should
preserve the pressure-cycle shape while rejecting pump commutation and nearby
structural vibration.

The deterministic clean signal is:

```text
0.70 cos(2*pi* 2*t + 0.10)
+ 0.28 cos(2*pi* 4*t - 0.70)
+ 0.15 cos(2*pi* 8*t + 1.10)
+ 0.08 cos(2*pi*12*t - 1.40)
```

The fixed interference is:

```text
0.25 cos(2*pi*32*t + 0.40)
+ 0.12 cos(2*pi*47*t - 0.90)
```

The cutoff is 20 Hz. Desired content therefore ends at `0.6 * cutoff`, inside
the equalizer's validated `0..0.7 * cutoff` band, while the interference starts
at `1.6 * cutoff`. The input is sampled near 1 kHz with the repeating interval
pattern `[0.875, 1.0, 1.125, 0.95, 1.05]` ms, whose mean is exactly 1 ms.
Every filter uses `InputModel::Linear`, the same endpoint values and intervals,
and its input value at time zero as `initial_input`.

Warm up for 3 seconds, then score the following 4 seconds. Run repeated-pole,
Bessel, plain Butterworth, and phase-equalized Butterworth at orders 4 and 8.
This is intentionally one workload, not a response matrix.

## Alignment and metrics

The desired target is the original clean pressure waveform delayed by a
predeclared constant. Passband droop therefore counts as application error.
Alignment is never fitted from streamed output:

- ordinary responses use the midpoint of their analytic minimum and maximum
  group delay over `0..0.7 * cutoff`, evaluated at 4,097 uniformly spaced
  frequency ratios including both endpoints;
- the equalizer uses `target_constant_delay_seconds()`.

The application metrics are scored with `f64`. Object size is also reported
for `f32`; the existing automated suite remains responsible for the supported
`f32` numerical contract. RMS values use time-weighted trapezoidal integration
over the four-second score window. Peaks are the largest absolute value at a
streamed endpoint. Clean and total error are normalized by the delayed clean
target's RMS and peak over that window. Residual-interference error is
normalized by the same desired-signal values; attenuation uses the input
interference's RMS or peak as its denominator.

Report these values for each filter:

- RMS and peak error of the clean-input output against the delayed clean
  target, normalized by the target RMS and peak;
- residual interference from `F(clean + interference) - F(clean)`, normalized
  by desired RMS and peak, plus attenuation relative to input interference;
- the same total error for the contaminated input;
- the predeclared alignment delay and, for the equalizer, mean added delay;
- mean and p99 derivative evaluations, rejected-step share, and update
  failures from `last_diagnostics()`;
- concrete `f32` and `f64` object sizes;
- host-local update throughput over the frozen schedule.

Integration diagnostics cover the complete seven-second contaminated-input
replay, which represents the actual application workload. The separate clean
replay exists only to isolate desired-waveform and residual-interference
metrics. Rejected-step share is `sum(rejected) / sum(accepted + rejected)`.
Mean derivative work is per update and p99 is the nearest-rank percentile over
per-update derivative evaluation counts. Any failed update makes the candidate
fail.

Throughput uses the contaminated input, a release build, and a precomputed
seven-second schedule. Each timed trial constructs a fresh filter outside the
timer, repeatedly replays the periodic schedule for at least 250 ms, and
reports the median of five trials. Record the processor, operating system,
Rust version, and build profile. Relative time and size comparisons always use
the matched-order plain Butterworth filter.

Plots show the raw input/output timeline and the delay-aligned target/output
over one representative pressure cycle. They support the decision but are not
an additional acceptance mechanism.

## Product gates

A candidate is usable for this workload only when all of these hold:

```text
clean waveform normalized RMS error <= 3%
clean waveform normalized peak error <= 5%
residual interference RMS <= 5% of desired RMS
residual interference peak <= 10% of desired peak
predeclared alignment delay <= 100 ms
no update failure
rejected steps <= 5% of attempted steps
host throughput >= 50,000 updates/second
```

The equalizer additionally requires:

```text
clean waveform RMS error at least 30% lower than plain Butterworth
p99 derivative evaluations/update <= 1.5 * plain Butterworth
host time/update <= 2 * plain Butterworth
object-size increment <= 32 bytes for f32 and 64 bytes for f64
```

The unfiltered interference must exceed at least one residual-interference
budget, otherwise selectivity is not important in this workload.

## Decision rule

Retain the narrow equalizer only if an equalized order passes every gate while
its clean RMS error is no more than `0.70` times its matched-order plain
Butterworth error, and all six ordinary candidates fail at least one
signal-quality gate. A simpler candidate's failure counts as material only
when it exceeds a clean or residual RMS/peak limit by at least 20%:
respectively `3.6%`, `6%`, `6%`, or `12%`. The winning margin must not depend
on fitted alignment.

Remove it before 1.0 if any simpler response passes all gates or misses a
signal-quality gate by less than that material margin, if equalization fails
latency or cost, or if the advantage disappears when the frozen workload is
replayed unchanged. A positive synthetic result justifies retaining the narrow
feature for early user feedback; it does not justify more orders, responses,
topologies, or validation infrastructure.

## Recorded run

The frozen workload was replayed in release mode on commit `d398f53` with
Rust 1.98.1 on an AMD Ryzen 5 2600X. The temporary evaluator used public
filtering and diagnostics APIs; it was not retained as product tooling.

The useful signal results were:

| Candidate | Clean RMS / peak error | Residual RMS / peak | Alignment delay | Throughput |
| --- | ---: | ---: | ---: | ---: |
| repeated-pole 4 | 2.058% / 2.750% | 15.088% / 14.140% | 13.26 ms | 2.89 M updates/s |
| Bessel 4 | 1.611% / 1.787% | 12.223% / 10.858% | 16.81 ms | 2.48 M updates/s |
| Butterworth 4 | 6.147% / 8.007% | 4.889% / 4.143% | 24.36 ms | 3.40 M updates/s |
| equalized Butterworth 4 | 0.135% / 0.175% | 4.889% / 4.146% | 52.65 ms | 2.56 M updates/s |
| repeated-pole 8 | 1.889% / 2.447% | 14.334% / 13.171% | 18.75 ms | 1.51 M updates/s |
| Bessel 8 | 1.679% / 1.851% | 12.687% / 10.910% | 25.30 ms | 1.38 M updates/s |
| Butterworth 8 | 9.770% / 12.755% | 0.751% / 0.593% | 46.45 ms | 1.97 M updates/s |
| equalized Butterworth 8 | 0.501% / 0.533% | 0.751% / 0.593% | 78.15 ms | 1.63 M updates/s |

The interference was material at 35.88% of desired RMS. Both equalized orders
passed the signal, latency, failure, throughput, and relative-cost gates. Their
p99 derivative work was identical to matched plain Butterworth, host time per
update was approximately `1.33x` and `1.21x`, and their concrete objects were
smaller because the dedicated concrete model does not store the general
response-model enum. All six ordinary candidates materially failed either
waveform fidelity or interference rejection.

The frozen conjunction nevertheless returned `REMOVE`: every candidate failed
the absolute rejected-step-share gate, at roughly 27% to 31% rather than 5%.
An independent audit reproduced the counts and found that each streaming
update restarts adaptive step selection from the complete interval. The common
rejections were successful controller startup work, not instability: there
were no failed updates or non-finite outputs, and every candidate exceeded the
absolute throughput requirement by more than 27 times. A preselected 0.5 ms
public maximum step reduced the order-4 Butterworth shares to 1.64%, improved
throughput by about 13%, and changed outputs by less than `6e-9`.

This run is therefore recorded as **inconclusive under its frozen mechanical
rule**, not retroactively changed to a pass. The rejected-step percentage was
not a valid user-value gate and the binary evaluator incorrectly collapsed a
library-wide common-gate failure into feature removal. No threshold was
loosened and no cached-step runtime feature was added merely to satisfy the
experiment.

The product action is to retain the existing narrow order-4/order-8 equalizer
provisionally for early user feedback. The workload demonstrates a plausible
case where it preserves waveform shape while keeping Butterworth selectivity,
at acceptable disclosed delay and cost. It does not establish a final 1.0
commitment: do not add orders, responses, or topologies until an authentic
user workload confirms that value.
