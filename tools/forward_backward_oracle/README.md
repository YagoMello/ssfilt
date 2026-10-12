# Forward-backward oracle development tool

This is an isolated development tool for the independent, high-precision
forward-backward reference and fixture generation described in
[`docs/forward-backward.md`](../../docs/forward-backward.md) and
[`docs/forward-backward-fixtures.md`](../../docs/forward-backward-fixtures.md).
The Rust sources implement the independent compensated Butterworth modes,
stable affine-segment primitives, and the linear-time constant-continued
modal convolution. They do not yet generate the full frozen fixture matrix.

The tool has its own empty Cargo workspace so its multiprecision dependency is
not part of the root `ssfilt` build, its `no_std` configuration, or downstream
users' dependency graph. `rug` is pinned to `1.30.0` and enabled only with its
`complex` and `std` features. Its native GMP, MPFR, and MPC components can make
the first build substantially slower because they may be compiled locally;
building them requires a native C toolchain and `make`.

Run commands from the repository root:

```sh
cargo fmt --manifest-path tools/forward_backward_oracle/Cargo.toml -- --check
cargo check --manifest-path tools/forward_backward_oracle/Cargo.toml
cargo test --manifest-path tools/forward_backward_oracle/Cargo.toml
cargo run --manifest-path tools/forward_backward_oracle/Cargo.toml -- verify
```

The `verify` command prints a compact order-1/2/4/8 report with compensated
pole radius, complete gain at the public cutoff, fresh 192/256-bit convolution
agreement, and the 256-bit imaginary residual. It exits unsuccessfully if any
reported value exceeds its compact self-check threshold. The test suite
additionally checks independent pole/residue forms, signed kernel symmetry and
reality, exact constants, record reversal, exact collinear refinement, and
scalar-record promotion.

The future fixture generator will build on this oracle. Any generated fixture
values must be emitted as Rust `f64::from_bits(...)` constants and accompanied
by a manifest and checksum so the committed binary values and their provenance
can be verified.
