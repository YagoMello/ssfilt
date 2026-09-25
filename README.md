# ssfilt

`ssfilt` is an experimental Rust library for continuous-time filters driven by
irregularly sampled signals. Instead of designing a digital filter for one
fixed sample rate, every update advances a normalized state-space model by the
elapsed time supplied with that sample.

The current milestone intentionally provides one carefully tested low-pass
filter with three response families:

- repeated-pole, Butterworth, and Chebyshev Type I responses;
- total-filter cutoff normalized to -3 dB;
- adaptive Dormand-Prince 5(4) integration;
- linear, previous-value hold, and current-value hold input models;
- `f32`, `f64`, `no_std`, and allocation-free operation;
- transactional errors and explicit steady-state reset;
- per-update integration diagnostics for observing adaptive work.

Bessel, high-pass, band-pass, and phase equalization are planned, but are not
yet part of the API.

## Example

```rust
use ssfilt::{InputModel, LowPass, Response};

let mut filter = LowPass::<4>::builder(20.0)
    .response(Response::Butterworth)
    .input_model(InputModel::Linear)
    .initial_input(0.0)
    .build()?;

let value = filter.update(1.0, 0.012)?;
# assert!(value.is_finite());
# Ok::<(), Box<dyn std::error::Error>>(())
```

`update(input, dt_seconds)` treats `input` as the sample at the end of the
elapsed interval. The selected [`InputModel`](https://docs.rs/ssfilt/latest/ssfilt/enum.InputModel.html)
defines what is assumed between that sample and the preceding one:

- `Linear` interpolates between the two endpoints and is the default.
- `PreviousHold` applies the preceding input throughout the interval.
- `CurrentHold` applies the new input throughout the interval.

The first interval begins at `initial_input`, which defaults to zero.

### Runtime-selected order

The order is a const generic because that gives fixed-size, allocation-free
state. Applications with a finite set of runtime-selectable orders can erase
the concrete order behind the object-safe `StreamingFilter` trait:

```rust
# use ssfilt::{LowPass, StreamingFilter};
# let order = 4;
# let cutoff_hz = 20.0;
let mut filter: Box<dyn StreamingFilter<Scalar = f64>> = match order {
    2 => Box::new(LowPass::<2>::builder(cutoff_hz).build()?),
    4 => Box::new(LowPass::<4>::builder(cutoff_hz).build()?),
    8 => Box::new(LowPass::<8>::builder(cutoff_hz).build()?),
    _ => return Err("unsupported filter order".into()),
};

let value = filter.update(1.0, 0.01)?;
# assert!(value.is_finite());
# Ok::<(), Box<dyn std::error::Error>>(())
```

This approach requires an allocator and exposes the common streaming methods;
order-specific inherent methods remain available only on concrete `LowPass`
values.

## Response families

`Response::RepeatedPole` is the default and preserves the library's original
design. Every pole is real and identical. The complete filter is normalized to
the requested -3 dB cutoff.

`Response::Butterworth` provides a maximally flat passband. It is realized as
a cascade of normalized real first-/second-order continuous sections rather
than an expanded denominator polynomial. Low-Q sections precede high-Q
sections to reduce internal peaking.

`Response::Chebyshev1 { ripple_db }` provides a steeper transition at the cost
of equiripple passband gain and greater ringing. `ripple_db` is the peak-to-peak
passband variation and must be greater than zero and less than 3.0103 dB. Every
section has unity DC gain. Consequently, odd-order responses ripple downward
from unity while even-order responses ripple upward from unity; both retain
the same peak-to-peak variation and exact steady-state behavior.

## Cutoff convention

For `Response::RepeatedPole` of order `N`, the repeated pole is placed at

```text
p = omega_c / sqrt(2^(1/N) - 1)
```

Butterworth is naturally -3 dB at its normalized cutoff. Chebyshev prototypes
normally use the passband-ripple edge as their reference frequency, so ssfilt
rescales their poles to place the unity-DC-gain response at -3 dB instead.
Therefore, `cutoff_hz` has the same whole-filter meaning for every family.
Internally the models use normalized time `tau = 2*pi*cutoff_hz*t`, preventing
absolute cutoff frequency from creating huge polynomial coefficients.

## Scalar types

The public `Scalar` trait is sealed and implemented for `f32` and `f64`. The
algorithms use `num-traits`, but a scalar is exposed as supported only after its
error behavior, precision, defaults, and `no_std` characteristics have been
validated. Precision-specific integration defaults live in
`IntegrationConfig`, not in the scalar trait.

## Error behavior

Construction rejects zero order, non-positive or non-finite cutoff, invalid
Chebyshev ripple, non-finite initial input, and invalid integration controls.
Updates reject non-finite input and non-positive or non-finite elapsed time.

An update is transactional: if adaptive integration cannot finish, the state,
output, and preceding input remain unchanged. The maximum internal timestep
controls numerical stability; the separate attempt budget bounds worst-case
work. No elapsed time is silently discarded.

## Integration diagnostics

`LowPass::last_diagnostics()` reports the work performed by the most recent
successful update: accepted and rejected steps, derivative evaluations, the
smallest and largest accepted step in seconds, and whether the exact-equilibrium
shortcut avoided integration. This is intentionally observational rather than
another configuration interface. Failed updates preserve the previous snapshot;
construction and resets clear it.

## `no_std`

The runtime is allocation-free. For a `no_std` target, disable defaults and use
the pure-Rust math backend:

```toml
[dependencies]
ssfilt = { version = "0.1", default-features = false, features = ["libm"] }
```

## Development

```text
cargo test --all-features
cargo check --no-default-features --features libm
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --all-features --no-deps
cargo +1.85.0 check --all-targets --all-features
cargo bench --bench streaming
```

The suite uses named, table-driven cases for boundary and response matrices,
and shrinking property tests for numerical invariants such as partition and
frequency-scaling independence. Explicit non-finite and transactional failures
remain ordinary regression tests so their contracts stay easy to read.

The Criterion benchmarks cover order and response scaling, input reconstruction
policies, and increasingly large normalized sample intervals. Benchmark
dependencies are development-only and do not affect library users.

See [DESIGN.md](DESIGN.md) for the numerical model, invariants, and planned
development sequence.
