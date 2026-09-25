# Design notes

## Product boundary

`ssfilt` is a focused continuous-time streaming filter library. It is not meant
to become a general ODE solver, linear-algebra package, or full DSP toolbox.
Users construct one filter object and update it with an input and elapsed time.
Model, interpolation, integration, and the shared streaming runtime remain
private until a real customization requirement justifies stabilizing them.

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

### Chebyshev Type I

For passband ripple `r_p` in decibels, define:

```text
epsilon^2 = expm1(ln(10) * r_p / 10)
mu        = asinh(1 / epsilon) / N
```

The standard prototype poles lie on an ellipse:

```text
p_k = -sinh(mu)*sin(theta_k) + j*cosh(mu)*cos(theta_k)
theta_k = (2*k - 1)*pi/(2*N)
```

Textbook prototypes define normalized frequency one as the ripple edge, not
the -3 dB point. ssfilt gives every real and conjugate-pair section unity DC
gain, then divides all poles by the prototype frequency whose gain is 1/sqrt(2)
relative to DC. If `C0` is zero for odd orders and one for even orders:

```text
T_target^2 = (1 + 2*epsilon^2*C0) / epsilon^2
Omega_3dB  = cosh(acosh(T_target) / N)
```

This retains a consistent public cutoff and exact constant-input equilibrium.
It also means even-order ripple extends above unity after DC normalization;
`ripple_db` consistently describes peak-to-peak variation. Ripple is restricted
to `(0, 10*log10(2))` dB so the -3 dB normalization remains unambiguous for all
orders.

### Bessel

Bessel uses roots of reverse Bessel polynomials for maximally flat group delay
at DC. The poles are magnitude-normalized so normalized frequency one is the
complete response's -3 dB point, then factored into the same unity-DC-gain real
sections used by the other classical responses.

The roots have no simple closed form and direct high-order polynomial root
finding is ill-conditioned. The runtime therefore performs no root solving:
validated prototypes for orders 1 through 25 are stored as real-section
coefficients. This keeps construction deterministic, allocation-free, and
`no_std`; larger Bessel orders fail explicitly rather than silently using
unreliable coefficients. Low-Q sections are again placed first.

### High-pass transformation

High-pass models apply the normalized analog transformation `s -> 1/s` to the
selected low-pass prototype. A real low-pass section

```text
r / (s + r)
```

becomes `s / (s + 1/r)`. A second-order section with denominator
`s^2 + a*s + b` becomes:

```text
s^2 / (s^2 + (a/b)*s + 1/b)
```

Each section uses a well-scaled low-pass state internally and computes its
high-pass output as the complementary direct-feedthrough term. This preserves
the prototype's magnitude at normalized frequency one, so the public cutoff
remains the complete filter's -3 dB point. Constant-input equilibrium has zero
output.

The direct term also makes endpoint semantics observable: integration uses the
chosen input reconstruction over the interval, then `output` is evaluated with
the newly supplied endpoint sample. In particular, `PreviousHold` does not
smear the new sample backward into the interval, but the returned endpoint can
still jump when that sample arrives.

### Band-pass transformation

For lower and upper -3 dB edges `f_l` and `f_h`, band-pass models normalize
time by their geometric center and define the fractional bandwidth:

```text
f_0  = sqrt(f_l * f_h)
beta = (f_h - f_l) / f_0
tau  = 2*pi*f_0*t
```

The analog substitution is `z = (s^2 + 1)/(beta*s)`. Every real low-pass pole
therefore becomes one second-order band-pass section, while every complex pole
pair becomes two real second-order sections. The final band-pass order is twice
the prototype order and must consequently be even.

The direct complex-root mapping is poorly conditioned for narrow bands. For a
low-pass factor `z^2 + a*z + b`, ssfilt instead factors the transformed
denominator using reciprocal natural frequencies `W` and `1/W`:

```text
s^4 + a*beta*s^3 + (2 + b*beta^2)*s^2 + a*beta*s + 1

y = W + 1/W
c*y = a*beta
c^2 + y^2 = 4 + b*beta^2
```

The two section damping coefficients are `c*W` and `c/W`. The small quantity
`y^2 - 4` is evaluated directly rather than by subtracting two nearly equal
numbers. This preserves the -3 dB edge normalization for narrow, high-order
filters without polynomial expansion or complex arithmetic. Low-Q sections
are placed first, and every constant input has exactly zero output at
equilibrium.

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

## Offline forward-backward filtering

The batch functions clone a configured streaming filter, initialize it to the
first input sample's equilibrium, run the forward pass, then initialize a
second pass to the final forward value's equilibrium and traverse the result
backward. A gap between samples `i` and `i + 1` is used in both directions.
The caller supplies an output slice, so the library performs no allocation.

The interior response of a uniformly sampled linear filter has transfer
function `H(z) H(z^-1)`, with zero phase and magnitude `|H(z)|^2`. Unequal
sample gaps have no single global frequency response, but the reverse pass
still uses the same physical intervals. Finite-record endpoint transients
depend on the steady-state boundary condition. Validation of lengths, input,
and intervals finishes before the output is written; an integration failure
may leave a partial output.

## Streaming phase equalization design boundary

A future delayed all-pass equalizer should be designed against a selected
frequency band and report the additional group delay it introduces. The
continuous-time all-pass model can preserve magnitude while adding delay;
however, merely feeding it successive endpoint outputs of an existing filter
would force an interpolation of the unobserved output between samples. For
irregular timing, that shortcut would change the magnitude and delay response
in ways the analytic all-pass design cannot predict. A reliable implementation
should advance the filter and equalizer states together at the integrator's
internal stages, so the equalizer sees the filter's actual continuous output.
This needs a coupled-state solver while retaining fixed-size storage and
transactional updates. The public equalizer API will follow that numerical
foundation.

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
4. Runtime integration diagnostics and benchmarks. ✓
5. Chebyshev I and Bessel responses with explicit normalization conventions. ✓
6. High-pass and band-pass topologies, including direct-feedthrough semantics. ✓
7. Optional delayed group-delay equalization and offline forward-backward
   filtering as separate phase-handling approaches. Batch filtering is complete;
   delayed streaming equalization is pending.
8. Advanced custom kernels or alternative integrators only after concrete use
   cases establish the necessary interface.
