# ssfilt

`ssfilt` is an experimental Rust library for continuous-time filters driven by
irregularly sampled signals. Instead of designing a digital filter for one
fixed sample rate, every update advances a normalized state-space model by the
elapsed time supplied with that sample.

The current milestone provides carefully tested low-pass, high-pass, and
band-pass filters with four response families:

- repeated-pole, Butterworth, Bessel, and Chebyshev Type I responses;
- total-filter cutoff normalized to -3 dB;
- adaptive Dormand-Prince 5(4) integration;
- linear, previous-value hold, and current-value hold input models;
- `f32`, `f64`, `no_std`, and allocation-free operation;
- transactional errors and explicit steady-state reset;
- per-update integration diagnostics for observing adaptive work.

Delayed streaming phase equalization is planned, but is not yet part of the API.

Offline forward-backward filtering is available for signals that can be held
as a complete batch. It removes phase delay in the interior of the record at
the cost of applying the magnitude response twice. The endpoints depend on
the chosen boundary condition and can retain transients.

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

### High-pass

`HighPass` uses the same builder and streaming trait as `LowPass`:

```rust
use ssfilt::{HighPass, InputModel, Response};

let mut filter = HighPass::<4>::builder(20.0)
    .response(Response::Bessel)
    .input_model(InputModel::CurrentHold)
    .initial_input(0.0)
    .build()?;

let value = filter.update(1.0, 0.012)?;
# assert!(value.is_finite());
# Ok::<(), Box<dyn std::error::Error>>(())
```

High-pass filters have direct feedthrough: their endpoint output depends on the
new sample as well as the integrated state. With `PreviousHold`, the preceding
sample drives the elapsed interval, then the newly arriving sample is used to
evaluate the returned endpoint output. A constant-input equilibrium always has
zero high-pass output, including after `initial_input` or `reset_to_steady`.

### Band-pass

`BandPass` takes the complete response's lower and upper -3 dB edges:

```rust
use ssfilt::{BandPass, Response};

let mut filter = BandPass::<4>::builder(10.0, 40.0)
    .response(Response::Butterworth)
    .initial_input(0.0)
    .build()?;

let value = filter.update(1.0, 0.012)?;
# assert!(value.is_finite());
# Ok::<(), Box<dyn std::error::Error>>(())
```

The const generic is the final band-pass order. It must be positive and even;
the analog transformation doubles the prototype order, so `BandPass::<4>`
uses a second-order low-pass prototype. The geometric center is available from
`center_hz()`. Constant-input equilibrium has zero output. Bessel band-pass
orders are supported through `MAX_BESSEL_BAND_PASS_ORDER` (currently 50).

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

### Offline forward-backward filtering

`forward_backward_into` accepts one elapsed interval for each gap between
samples, so irregularly timed data can be processed without resampling:

```rust
use ssfilt::{LowPass, Response, forward_backward_into};

let filter = LowPass::<4>::builder(20.0)
    .response(Response::Butterworth)
    .build()?;
let samples = [0.0, 1.0, 0.5, 0.0];
let intervals_seconds = [0.010, 0.012, 0.009];
let mut output = [0.0; 4];
forward_backward_into(&filter, &samples, &intervals_seconds, &mut output)?;
# assert!(output.iter().all(|sample| sample.is_finite()));
# Ok::<(), Box<dyn std::error::Error>>(())
```

For equal spacing, use `forward_backward_uniform_into(&filter, &samples,
dt_seconds, &mut output)`. Both functions work without allocation and leave
the supplied filter and input unchanged. Each pass begins at the steady state
of its first sample. The method needs the whole record, and the magnitude
response is squared; a single-pass −3 dB edge becomes approximately −6 dB
away from boundaries.

## Response families

`Response::RepeatedPole` is the default and preserves the library's original
design. Every pole is real and identical. The complete filter is normalized to
the requested -3 dB cutoff.

`Response::Butterworth` provides a maximally flat passband. It is realized as
a cascade of normalized real first-/second-order continuous sections rather
than an expanded denominator polynomial. Low-Q sections precede high-Q
sections to reduce internal peaking.

`Response::Bessel` provides maximally flat group delay at DC, prioritizing
waveform shape and low transient ringing over transition sharpness. It uses
prevalidated magnitude-normalized prototypes, also realized as real sections
with unity DC gain. Bessel orders 1 through `MAX_BESSEL_ORDER` (currently 25)
are supported; construction returns `BuildError::UnsupportedBesselOrder`
beyond that limit. The bounded table avoids allocation and unreliable
high-order polynomial root finding in user code.

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

Butterworth is naturally -3 dB at its normalized cutoff. Bessel prototypes use
magnitude normalization. Chebyshev prototypes normally use the passband-ripple
edge as their reference frequency, so ssfilt rescales their poles to place the
unity-DC-gain response at -3 dB instead. Therefore, `cutoff_hz` has the same
whole-filter meaning for every family.
Internally the models use normalized time `tau = 2*pi*cutoff_hz*t`, preventing
absolute cutoff frequency from creating huge polynomial coefficients.

High-pass responses are obtained with the analog low-pass-to-high-pass
frequency transformation. This preserves the response family and the same
whole-filter -3 dB cutoff meaning while introducing zeros at DC and unity gain
at infinite frequency.

Band-pass responses use the analog low-pass-to-band-pass transformation. The
two public cutoffs are both complete-filter -3 dB points, their geometric mean
is the center frequency, and their difference determines the bandwidth. The
implementation factors the transformed model into real second-order sections
without expanding a high-order polynomial or subtracting nearly equal complex
roots.

## Scalar types

The public `Scalar` trait is sealed and implemented for `f32` and `f64`. The
algorithms use `num-traits`, but a scalar is exposed as supported only after its
error behavior, precision, defaults, and `no_std` characteristics have been
validated. Precision-specific integration defaults live in
`IntegrationConfig`, not in the scalar trait.

## Error behavior

Construction rejects zero order, non-positive or non-finite cutoff, odd
band-pass order, invalid or reversed band-pass edges, invalid Chebyshev ripple,
unsupported Bessel order, non-finite initial input, and invalid integration
controls. Updates reject non-finite input and non-positive or non-finite
elapsed time.

An update is transactional: if adaptive integration cannot finish, the state,
output, and preceding input remain unchanged. The maximum internal timestep
controls numerical stability; the separate attempt budget bounds worst-case
work. No elapsed time is silently discarded.

## Integration diagnostics

Each concrete topology's `last_diagnostics()` reports the work performed by
the most recent successful update: accepted and rejected steps, derivative
evaluations, the smallest and largest accepted step in seconds, and whether
the exact-equilibrium shortcut avoided integration. This is intentionally
observational rather than another configuration interface. Failed updates
preserve the previous snapshot; construction and resets clear it.

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

The Criterion benchmarks cover order, response and topology scaling, input
reconstruction policies, and increasingly large normalized sample intervals.
Benchmark dependencies are development-only and do not affect library users.

To generate an SVG dashboard with magnitude, unwrapped phase, normalized group
delay, and unit-step responses, run:

```text
cargo run --release --example plot_responses -- 4 target/filter-responses.svg
```

Orders 1 through 50 are accepted through a compile-time dispatch macro. The
output path and order are optional and default to
`target/filter-responses.svg` and order 4. Frequency curves use 1,601
logarithmically spaced points; high orders can therefore take noticeably
longer to render. Bessel is included through its supported maximum order and
is omitted from higher-order dashboards. Frequency-domain curves are evaluated
directly from the normalized continuous transfer functions, avoiding settling
artifacts in deep high-order stopbands. The unit-step panel still drives the
public streaming API end to end. Curves are written as shape-preserving cubic
SVG paths from an oversampled internal canvas, so subpixel detail remains
smooth when zoomed without introducing spline overshoot.

The Bessel prototype table can be regenerated separately from the Rust build
with `scripts/generate_bessel_table.py`; that development helper requires
SciPy, while the crate itself does not.

See [DESIGN.md](DESIGN.md) for the numerical model, invariants, and planned
development sequence.
