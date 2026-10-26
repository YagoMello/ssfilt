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
| `ssfilt` Chebyshev I O2, 20 Hz | 162 us | 16.2% |
| fixed-rate Butterworth biquad O2 | 11 us | 1.1% |
| 64-sample rolling `f32` average | 7 us | 0.7% |

These rows do not have identical behavior. The fixed-rate filters do not use
the measured interval, reconstruct the signal between samples, or adapt work
to a local-error tolerance. They are useful references for the price of those
contracts, not drop-in numerical equivalents.

## Alternatives evaluated

- Relaxing the default tolerance did not help the common order-two case. It
  already completed each update in one Dormand--Prince step, so its minimum
  seven derivative evaluations dominated the cost.
- Forward Euler was fast (about 11 us/update in a specialized order-two
  comparison) but its maximum output error was about `7.9e-2`; it is not an
  acceptable replacement.
- Fixed-step classical RK4 established a useful ceiling of roughly 49--50
  us/update with small error on the nominal order-two workload. It does not
  honor the configured tolerance and requires a conservative subdivision
  policy for demanding responses and long gaps, so it remains a research
  reference rather than a production backend.
- An adaptive RK23 implementation was slower than Dormand--Prince because its
  lower order required many more attempts.
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
