# ssfilt

`ssfilt` is an experimental Rust library for continuous-time filters driven by
irregularly sampled signals. Instead of designing a digital filter for one
fixed sample rate, every update advances a normalized state-space model by the
elapsed time supplied with that sample.

The current milestone intentionally provides one carefully tested filter:

- repeated-pole low-pass response;
- total-filter cutoff normalized to -3 dB;
- adaptive Dormand-Prince 5(4) integration;
- linear, previous-value hold, and current-value hold input models;
- `f32`, `f64`, `no_std`, and allocation-free operation;
- transactional errors and explicit steady-state reset.

Butterworth, Chebyshev, Bessel, high-pass, band-pass, and phase equalization
are planned, but are not yet part of the API.

## Example

```rust
use ssfilt::{InputModel, LowPass};

let mut filter = LowPass::<4>::builder(20.0)
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

## Cutoff convention

For order `N`, the repeated pole is placed at

```text
p = omega_c / sqrt(2^(1/N) - 1)
```

so `cutoff_hz` is the -3 dB frequency of the complete filter. Internally the
model uses normalized time `tau = 2*pi*cutoff_hz*t`, preventing absolute cutoff
frequency from creating huge polynomial coefficients.

## Error behavior

Construction rejects zero order, non-positive or non-finite cutoff, non-finite
initial input, and invalid integration controls. Updates reject non-finite
input and non-positive or non-finite elapsed time.

An update is transactional: if adaptive integration cannot finish, the state,
output, and preceding input remain unchanged. The maximum internal timestep
controls numerical stability; the separate attempt budget bounds worst-case
work. No elapsed time is silently discarded.

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
```

See [DESIGN.md](DESIGN.md) for the numerical model, invariants, and planned
development sequence.
