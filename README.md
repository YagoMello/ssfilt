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

Explore the filters in the
[interactive browser playground](https://yagomello.github.io/ssfilt/), or use
the native plotting example to generate publication-quality SVG and CSV
reports locally.

<a href="https://yagomello.github.io/ssfilt/">
  <img src="https://raw.githubusercontent.com/YagoMello/ssfilt/main/docs/assets/low-pass-response.svg" alt="Magnitude, phase, group-delay, and unit-step responses for ssfilt order-four low-pass filters" width="900">
</a>

Phase correction and offline batch filtering are deferred and are not part of
the API. Exact zero-phase streaming would require future samples or added
latency; for recorded data, forward-backward or frequency-domain processing is
a different offline solution. These features will be reconsidered only for a
demonstrated use case.

## Installation

Add the crate to your application:

```toml
[dependencies]
ssfilt = "0.1.0-beta.1"
```

For embedded use without `std`, see the [`no_std`](#no_std) configuration
below.

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

### Runnable examples

| Goal | Command |
| --- | --- |
| Smooth timestamped sensor readings | `cargo run --example irregular_sensor` |
| Inspect high-pass endpoint timing | `cargo run --example high_pass_endpoint` |
| Select order 2 or 4 without allocating | `cargo run --example runtime_order -- 2` |

The corresponding sources are
[`irregular_sensor.rs`](examples/irregular_sensor.rs),
[`high_pass_endpoint.rs`](examples/high_pass_endpoint.rs), and
[`runtime_order.rs`](examples/runtime_order.rs).

### Choosing a starting point

- To smooth a noisy sensor value, start with a low-pass Butterworth response.
  Try Bessel when preserving pulse or motion shape matters more than a sharp
  cutoff; use repeated-pole for a simple monotonic response. Chebyshev I
  trades passband ripple and more ringing for a sharper transition.
- To remove slow baseline drift, use `HighPass`. To keep a frequency range
  between two edges, use `BandPass` (with an even final order).
- Choose the input model based on what happened *between* timestamps, not on
  the sensor name. `PreviousHold` fits a value held until the next sample;
  `Linear` fits a smoothly changing estimate; `CurrentHold` treats the newly
  received value as having applied throughout the preceding interval.
- If the signal starts near a known constant value, set `initial_input` to
  that value to avoid a startup transient. Use `reset_to_steady` after a known
  change of baseline, rather than to mask invalid samples.

All cutoff frequencies refer to the whole filter's −3 dB point. Begin with a
low order and inspect the transient response before increasing it; a sharper
cutoff usually costs more work and delay. The [response plot tool](#development)
can help compare those tradeoffs.

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

For embedded applications with a small set of configurable orders, an enum
keeps the same choice allocation-free and preserves the concrete variants:

```rust
use ssfilt::{BuildError, LowPass, UpdateError};

enum Order { Two, Four }
enum SensorFilter {
    Two(LowPass<2, f32>),
    Four(LowPass<4, f32>),
}

impl SensorFilter {
    fn new(order: Order, cutoff_hz: f32) -> Result<Self, BuildError> {
        match order {
            Order::Two => Ok(Self::Two(LowPass::<2, f32>::builder(cutoff_hz).build()?)),
            Order::Four => Ok(Self::Four(LowPass::<4, f32>::builder(cutoff_hz).build()?)),
        }
    }

    fn update(&mut self, input: f32, dt_seconds: f32) -> Result<f32, UpdateError> {
        match self {
            Self::Two(filter) => filter.update(input, dt_seconds),
            Self::Four(filter) => filter.update(input, dt_seconds),
        }
    }
}

let mut filter = SensorFilter::new(Order::Four, 20.0)?;
let output = filter.update(1.0, 0.012)?;
# assert!(output.is_finite());
# let _ = SensorFilter::new(Order::Two, 20.0)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The final `Ok` line is only an example harness; `SensorFilter` itself never
allocates. On a `no_std` target, use a concrete error type in the surrounding
application instead.

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
controls. It also rejects a finite initial input when its equilibrium state or
output cannot be represented. Updates reject non-finite input and non-positive
or non-finite elapsed time.

An update is transactional: if adaptive integration cannot finish, the state,
output, and preceding input remain unchanged. The maximum internal timestep
controls numerical stability; the separate attempt budget bounds worst-case
work. No elapsed time is silently discarded.

`reset_to_steady` validates its finite equilibrium state and output before
committing them. An unrepresentable equilibrium fails transactionally, leaving
the existing state, output, preceding input, diagnostics, and equilibrium
tracking unchanged.

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
ssfilt = { version = "0.1.0-beta.1", default-features = false, features = ["libm"] }
```

Use `LowPass::<N, f32>` (or `HighPass`/`BandPass`) when `f32` is appropriate for
the target's precision and range. The `f32` choice does not guarantee speed on
an MCU without a floating-point unit: benchmark the actual target, especially
for high orders and adaptive integration. The fixed-size enum above also works
without `std` or an allocator.

On a measured 160 MHz ESP32-C3, an order-two 20 Hz Butterworth update took
about 155 us for the representative irregular 1 kHz workload. A fixed-rate
biquad took about 11 us and a 64-sample moving average about 7 us. Those are not
equivalent filters: `ssfilt` is paying for measured elapsed-time integration,
explicit input reconstruction, adaptive error control, and transactional
failure. If the signal is truly fixed-rate and those contracts are unnecessary,
a conventional digital IIR or moving average is usually the better embedded
choice. See [MCU performance and integrator findings](docs/mcu-performance.md)
for the workload, tradeoffs, and alternatives that were evaluated.

## Development

```text
cargo test --all-features
cargo check --no-default-features --features libm
cargo test --no-default-features --features libm --lib
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --all-features --no-deps
cargo bench --bench streaming
```

During pre-1.0 development the crate follows stable Rust and does not declare
an MSRV. A supported minimum version will be selected and tested before the
1.0 compatibility promise is made.

The suite uses named, table-driven cases for boundary and response matrices,
and shrinking property tests for numerical invariants such as partition and
frequency-scaling independence. Explicit non-finite and transactional failures
remain ordinary regression tests so their contracts stay easy to read.

The Criterion benchmarks cover order, response and topology scaling, input
reconstruction policies, and increasingly large normalized sample intervals.
Benchmark dependencies are development-only and do not affect library users.

To generate an SVG dashboard and a sibling CSV report with magnitude,
unwrapped phase, normalized group delay, unit-step response, and measurements
made through the public streaming API, run:

```text
cargo run --release --example plot_responses -- 4 target/filter-responses.svg
```

Select a topology, response family, or order with named options:

```text
cargo run --release --example plot_responses -- --topology high-pass --order 4 --output target/high-pass-responses.svg
cargo run --release --example plot_responses -- --topology band-pass --response bessel --order 8 --output target/band-pass-bessel.svg
cargo run --release --example plot_responses -- --response chebyshev1 --order 12 --output target/chebyshev-12.svg --csv target/chebyshev-12.csv
cargo run --release --example plot_performance -- target/filter-performance.svg
```

`cargo run --release --example plot_responses -- --help` lists every option.
The commonly used flags are:

| Flag | Values and behavior |
| --- | --- |
| `--topology` | `low-pass` (default), `high-pass`, or `band-pass` |
| `--response` | `all` (default), `repeated-pole`, `butterworth`, `bessel`, or `chebyshev1` |
| `--order` | Final order 1–50; band-pass orders must be even |
| `--output` | SVG path; parent directories are created |
| `--csv` | Measurement report path; defaults beside the SVG |

The legacy low-pass form `[ORDER] [OUTPUT.svg]` remains available. Band-pass
uses −3 dB edges at 0.5 and 2 times the geometric center. Explicit Bessel
orders above the supported limit fail clearly; `--response all` omits Bessel
when the selected order exceeds its limit.

Analytic lines represent ideal continuous-time sine responses. Circular
markers are inspection measurements using the public filter,
`InputModel::Linear`, and 256 samples per cycle. The SVG title and CSV record
that reconstruction policy. In a very deep stopband, piecewise-linear
reconstruction error can dominate the ideal sine response, so a marker may
legitimately sit above the analytic line; this shows what that sampling setup
actually achieves. A two-million-update limit prevents one expensive
high-order marker from blocking the plot. Such points remain in CSV with an
explicit work-limit status and no fabricated measurement.

The performance SVG is intentionally separate: it shows indicative median
host timings for order-four response families/topologies and RK45 tolerance,
step-cap, and input-reconstruction settings. There is no second production
integrator to compare yet. Timings depend on the host and load and should not
be treated as portable benchmarks.

The GitHub Pages playground is built from `site/`. To run the same WebAssembly
adapter and uPlot interface locally:

```text
cd site
npm ci
npm run dev
```

Uploaded CSV data is parsed and filtered entirely in the browser. See
the [site plan](https://github.com/YagoMello/ssfilt/blob/main/docs/site-plan.md)
for its deliberately narrow input and performance contracts.

Orders 1 through 50 are accepted through a compile-time dispatch macro. The
output path and order are optional and default to
`target/filter-responses.svg` and order 4. Frequency curves use 1,601
logarithmically spaced points; high orders can therefore take noticeably
longer to render. Bessel is included through its supported maximum order and
is omitted from higher-order `all` dashboards. Frequency-domain curves are
evaluated directly from normalized continuous transfer functions. Sparse
gain and phase markers and the unit-step panel drive the public streaming API
end to end. Curves are written as shape-preserving cubic SVG paths from an
oversampled internal canvas, so subpixel detail remains smooth when zoomed
without introducing spline overshoot.

The Bessel prototype table can be regenerated separately from the Rust build
with `scripts/generate_bessel_table.py`; that development helper requires
SciPy, while the crate itself does not.

See [DESIGN.md](DESIGN.md) for the numerical model, invariants, and planned
development sequence.

Licensed under either [Apache License 2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT), at your option.
