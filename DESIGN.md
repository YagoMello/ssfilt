# Design notes

## Product boundary

`ssfilt` is a focused continuous-time streaming filter library. It is not meant
to become a general ODE solver, linear-algebra package, or full DSP toolbox.
Users construct one filter object and update it with an input and elapsed time.
Model, interpolation, and integration components remain private until a real
customization requirement justifies stabilizing them.

## Current normalized models

For a cutoff angular frequency `omega_c` and physical time `t`, define
`tau = omega_c * t`. A repeated-pole low-pass of order `N` is realized as a
cascade:

```text
x[0]' = lambda * (u      - x[0])
x[i]' = lambda * (x[i-1] - x[i])
y      = x[N-1]

lambda = 1 / sqrt(expm1(ln(2) / N))
```

The prime denotes differentiation with respect to normalized time. Every state
has the same units and approximate magnitude as the signal. At normalized
frequency one, the complete magnitude is exactly `1/sqrt(2)`.

The implementation deliberately does not assign derivative meaning to state
elements. This leaves room for well-scaled section realizations and numerator
dynamics in future topologies.

### Butterworth

Butterworth uses the same normalized time, with poles on the left half of the
unit circle. Odd orders begin with the real section:

```text
y' = u - y
```

Each complex-conjugate pole pair becomes a real section:

```text
y' = v
v' = u - y - a*v
a  = 2*sin((2*k + 1)*pi/(2*N))
```

The sections all have unity DC gain. They are ordered from greatest damping
(lowest Q) to least damping (highest Q) to reduce the input reaching resonant
sections. This avoids denominator expansion and leaves every normalized pole
with unit magnitude. Its closed-form magnitude is:

```text
|H(j*Omega)| = 1 / sqrt(1 + Omega^(2*N))
```

## Sample timing contract

`update(u_new, dt)` advances from the preceding sample to the new sample.
During the complete interval, the forcing is one of:

```text
Linear:       u(a) = u_old + a * (u_new - u_old), a in [0, 1]
PreviousHold: u(a) = u_old
CurrentHold:  u(a) = u_new
```

The interpolator is evaluated against the original full interval even when the
integrator subdivides it. An endpoint discontinuity in `PreviousHold` is not
fed into endpoint Runge-Kutta stages; `u_new` becomes `u_old` only after a
successful update.

## Integration policy

Dormand-Prince 5(4) supplies a fifth-order candidate and a fourth-order local
error estimate. Error is scaled componentwise:

```text
abs(error[i]) /
    (absolute_tolerance
     + relative_tolerance * max(abs(old[i]), abs(candidate[i])))
```

The maximum component decides acceptance. Two independent limits apply:

- A model-derived maximum normalized step keeps explicit integration within a
  conservative stability region. A user step limit may reduce it further.
- A finite attempt budget bounds CPU work from very large gaps, tight
  tolerances, and rejected steps.

The complete elapsed interval is either integrated or rejected. It is never
clamped or partially committed. Constant input at exact equilibrium is a safe
fast path, allowing arbitrarily large elapsed intervals without needless work.

Forward Euler exists only in tests for convergence and comparison. It is not
an accuracy oracle or public backend.

The most recent successful update exposes an observational diagnostics snapshot
with work counters and its accepted physical step-size range. Diagnostics do
not select an integrator and are not part of the generic filter trait. Failed
updates preserve the preceding snapshot transactionally.

## Invariants

- `cutoff_hz` is finite, positive, and describes the complete -3 dB response.
- `dt_seconds` is finite and positive.
- Publicly supported scalar types are exactly `f32` and `f64`, represented by a
  sealed `Scalar` trait. Precision-specific defaults remain configuration
  concerns rather than scalar operations.
- A failed update changes no observable or internal filter state.
- Steady initialization remains exactly steady for constant input.
- Absolute cutoff scaling changes only the mapping between physical and
  normalized time.
- Runtime storage has fixed size and requires no allocator.

## Planned milestones

1. Repeated-pole low-pass, explicit input timing, RK45, `no_std`, analytic tests. ✓
2. Named case matrices, shrinking numerical properties, and CI quality gates. ✓
3. Butterworth low-pass using normalized real first/second-order sections. ✓
4. Runtime integration diagnostics and benchmarks.
5. Chebyshev I and Bessel responses with explicit normalization conventions.
6. High-pass and band-pass topologies, including direct-feedthrough semantics.
7. Optional delayed group-delay equalization and offline forward-backward
   filtering as separate phase-handling approaches.
8. Advanced custom kernels or alternative integrators only after concrete use
   cases establish the necessary interface.
