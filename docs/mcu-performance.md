# MCU performance and integrator findings

`ssfilt` is designed for continuous-time filters driven by measured, irregular
sample intervals. That flexibility has a real CPU cost on processors that
emulate floating-point arithmetic in software. This note records the first
target measurement and the bounded alternatives that were evaluated, so users
can decide whether the library fits their workload.

## Measured target and workload

The measurements below used a 160 MHz ESP32-C3 (RV32IMC, no floating-point
unit), release optimization for size, `f32`, `libm`, and `InputModel::Linear`.
The deterministic stream contained 4,096 updates with intervals from 800 to
1,311 microseconds and a square-wave-plus-noise input. Reported time includes
input generation and a checksum; the common baseline was about 5.3 us/update
and was not subtracted.

| Workload | Time/update | Approximate CPU at 1 kHz |
| --- | ---: | ---: |
| `ssfilt` Butterworth O2, 20 Hz | 155 us | 15.5% |
| relaxed RK23, 1% local tolerance preset | 90 us | 9.0% |
| relaxed RK23, 5% local tolerance preset | 83 us | 8.3% |
| fixed-step RK4 research reference | 50 us | 5.0% |
| `ssfilt` Chebyshev I O2, 20 Hz | 162 us | 16.2% |
| fixed-rate Butterworth biquad O2 | 11 us | 1.1% |
| 64-sample rolling `f32` average | 7 us | 0.7% |

These rows do not have identical behavior. The RK23 implementation and RK4 row
are private research prototypes, not selectable production backends. The
fixed-rate filters do not use the measured interval, reconstruct the signal
between samples, or adapt work to a local-error tolerance. They are useful
references for the price of those contracts, not drop-in numerical
equivalents.

The two relaxed presets were `(absolute, relative) = (1e-4, 1e-2)` and
`(5e-4, 5e-2)`. These are componentwise *local state-error* controls. Calling
them 1% and 5% is convenient experimental shorthand, not a promise that the
filter output is within that percentage for every response, order, input, or
sample interval.

## Alternatives evaluated

- Relaxing the default tolerance did not help the common order-two
  Dormand--Prince case. It already completed each update in one step, so its
  minimum seven derivative evaluations dominated the cost. With intervals ten
  times longer, however, the relaxed 1% and 5% presets reduced
  Dormand--Prince from about 832 us/update to 376 and 374 us/update. Their
  maximum absolute output difference from default Dormand--Prince was about
  `4.5e-4` on the roughly unit-scale test signal.
- Forward Euler was fast (about 11 us/update in a specialized order-two
  comparison) but its maximum output error was about `7.9e-2`; it is not an
  acceptable replacement.
- Fixed-step classical RK4 established a useful ceiling of roughly 49--50
  us/update with small error on the nominal order-two workload. It does not
  honor the configured tolerance and requires a conservative subdivision
  policy for demanding responses and long gaps, so it remains a research
  reference rather than a production backend.
- Adaptive RK23 at the production tolerance was much slower than
  Dormand--Prince because its lower order required many more attempts. At the
  relaxed 1% and 5% presets it took about 90 and 83 us/update on ordinary
  intervals, with maximum absolute output differences from production
  Dormand--Prince of about `3.2e-4` and `3.7e-4`. On ten-times-longer intervals
  it took about 468 and 392 us/update, with differences of `5.0e-3` and
  `4.9e-3`: slower and less accurate than relaxed Dormand--Prince. RK23 is
  therefore a plausible fast profile, not a universal replacement.
- An adaptive Euler--Heun 1(2) prototype was not competitive. Even at the 1%
  and 5% presets it took about 749 and 367 us/update, and production
  tolerances exhausted the existing attempt budget in 24 library tests. It
  was discarded.
- Reusing Dormand--Prince's endpoint derivative reduced the common case by
  only about 5%, which did not justify the extra persistent buffer and cache
  validity state.
- A five-stage adaptive Merson method cut many ordinary order-one and order-two
  cases by 34--43% while preserving adaptive checks. Its estimator was much
  more conservative for some dynamics: order-two Bessel gained essentially
  nothing, and a narrow 55--65 Hz band-pass became about 43% slower. A broad or
  hidden topology-specific rollout was therefore rejected.

There is currently one private production integrator: Dormand--Prince 5(4).
The public API does not expose an integrator selector.

## Hardware-FPU host reference

The same nominal order-two stream was also measured on a Ryzen 5 2600X with
`rustc 1.99.1`. Median release-mode results were approximately 64 ns/update
for production Dormand--Prince, 56--60 ns/update for relaxed RK23, 27 ns/update
for the fixed RK4 reference, and 3--4 ns/update for specialized Euler, moving
average, and fixed-rate biquad loops. Absolute host cost is tiny, but the
relative ranking remains useful: hardware floating point does not make a
seven-stage method free, and it does make the entire choice irrelevant for
many desktop workloads.

These host figures include construction amortized across each 4,096-update
stream and are indicative rather than a portable guarantee. The fixed-rate
filters implement different responses and timing semantics.

## Accuracy profiles under consideration

The measurements justify investigating an ergonomic performance choice, but
not publishing `Low`, `Medium`, and `High` as if they were universal output
accuracy guarantees. A preset changes local state-error tolerance, while a
backend change also changes stage count, stability, long-gap behavior, and
failure risk.

Before adding a public profile, compare a small representative matrix of
low-, high-, and band-pass filters, ordinary and long intervals, and `f32` and
`f64`. A useful interface must state whether it offers fixed effort or adaptive
error control, retain complete-interval and transactional-failure semantics,
and give diagnostics enough context to explain the work performed. The
existing `IntegrationConfig` remains the honest custom path in the meantime.

## Equilibrium shortcut

The exact-equilibrium branch is intentionally retained. On the same target an
order-four steady update took about 6.5 us with the shortcut and about 211 us
without it. On changing data, removing the inactive branch improved the
measured order-two workload by less than 1%. Real ADC or IMU noise normally
leaves equilibrium on the first changed sample, but initialization,
`reset_to_steady`, held constants, and disconnected sensors can still benefit
substantially.

## Choosing a filter for an embedded application

Use `ssfilt` when measured irregular timing, explicit reconstruction semantics,
continuous cutoff definitions, or graceful handling of occasional longer
intervals matters more than minimum CPU cost. Inspect `last_diagnostics()` on
representative data and benchmark the exact target, order, cutoff, topology,
and response.

For a genuinely fixed-rate stream on a soft-float MCU, a conventional digital
IIR is usually the better default. A moving average is cheaper still when its
frequency response and delay are acceptable. Higher clock rates, lower update
rates, hardware floating point, or an application-specific tolerance/step cap
can change the tradeoff; desktop timings are not a substitute for a target
measurement.
