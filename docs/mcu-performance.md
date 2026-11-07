# MCU performance and integrator findings

`ssfilt` is designed for continuous-time filters driven by measured, irregular
sample intervals. That flexibility has a real CPU cost on processors that
emulate floating-point arithmetic in software. This note records the first
target measurement and the bounded alternatives that were evaluated, so users
can decide whether the library fits their workload.

## Measured target and workload

The current measurements used a 160 MHz ESP32-C3 (RV32IMC, no floating-point
unit), release optimization for size, `f32`, `libm`, and `InputModel::Linear`.
The deterministic stream contained 4,096 updates with intervals from 800 to
1,311 microseconds and a square-wave-plus-noise input. Reported time includes
input generation and a checksum; the common baseline was 5.3 us/update and was
not subtracted. The public-policy run was captured from commit `be7fa00`.

| Workload | Policy and RK4 cap | Time/update | CPU at 1 kHz | Maximum difference from default |
| --- | --- | ---: | ---: | ---: |
| Butterworth O2, 20 Hz | default DOPRI | 148 us | 14.8% | reference |
| Butterworth O2, 20 Hz | RK4, 2 ms | 53 us | 5.3% | `8.3e-6` |
| Butterworth O4, 20 Hz | default DOPRI | 258 us | 25.8% | reference |
| Butterworth O4, 20 Hz | RK4, 2 ms | 81 us | 8.1% | `6.6e-6` |
| Butterworth O8, 20 Hz | default DOPRI | 486 us | 48.6% | reference |
| Butterworth O8, 20 Hz | RK4, 2 ms | 138 us | 13.8% | `4.9e-6` |
| Butterworth O4, 100 Hz | default DOPRI | 1,085 us | 108.5% | reference |
| Butterworth O4, 100 Hz | RK4, 2 ms | 82 us | 8.2% | `8.5e-3` |
| Butterworth O4, 100 Hz | RK4, 250 us | 381 us | 38.1% | `1.1e-5` |
| 64-sample rolling `f32` average | fixed-rate reference | 6.6 us | 0.7% | not equivalent |

The 20 Hz RK4 cases used one step per update and `max_steps = 1`. The 100 Hz
cases used budgets of eight and six steps respectively; the coarse 2 ms cap
still completed in one step, while the 250 us cap averaged 4.7 steps. The
differences compare endpoint output with the default Dormand--Prince policy on
this one stream. They are useful application evidence, not general error
bounds or proof that the adaptive output is exact. The corresponding RMS
differences were `1.3e-6` to `1.6e-6` at 20 Hz, `8.9e-4` for coarse 100 Hz RK4,
and `1.0e-6` for the 250 us case.

The rows do not all have identical behavior. A moving average does not use the
measured interval or reconstruct the signal between samples. It is a useful
reference for the price of those contracts, not a drop-in numerical
equivalent. Earlier research also measured a conventional fixed-rate
Butterworth biquad at about 11 us/update on this target.

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
- The public fixed-step RK4 policy measured 53 us/update for the nominal
  order-two workload, close to the earlier 49--50 us research prototype. It
  does not honor an error tolerance and requires a validated step cap for
  demanding responses and long gaps. The 100 Hz pair above demonstrates both
  sides of that choice: the coarse cap is inexpensive but visibly less
  accurate, while 250 us recovers close agreement at substantially higher
  cost.
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
  about 5%. The optimization is retained, and its persistent buffer and cache
  validity are now isolated inside the DOPRI-specific runtime rather than the
  common streaming core.
- A five-stage adaptive Merson method cut many ordinary order-one and order-two
  cases by 34--43% while preserving adaptive checks. Its estimator was much
  more conservative for some dynamics: order-two Bessel gained essentially
  nothing, and a narrow 55--65 Hz band-pass became about 43% slower. A broad or
  hidden topology-specific rollout was therefore rejected.

The sealed public policy interface statically selects adaptive Dormand--Prince
5(4) or fixed-step classical RK4. Dormand--Prince remains the general default;
RK4 is the explicit lower-work choice when the application can validate a
physical step cap against its signals, gaps, topology, and response.

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

## Choosing an integration policy

The measurements do not justify publishing `Low`, `Medium`, and `High` as if
they were universal output-accuracy guarantees. A tolerance changes local
state-error control, while a backend change also changes stage count,
stability, long-gap behavior, and failure risk.

Before adding a public profile, compare a small representative matrix of
low-, high-, and band-pass filters, ordinary and long intervals, and `f32` and
`f64`. A useful interface must state whether it offers fixed effort or adaptive
error control, retain complete-interval and transactional-failure semantics,
and give diagnostics enough context to explain the work performed. The
`DormandPrince54` names its local-error tolerances and work limits directly.
`RungeKutta4` instead requires `max_step_seconds` and `max_steps`, making the
lack of automatic error estimation explicit.

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
