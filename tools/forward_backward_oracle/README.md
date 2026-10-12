# Forward-backward oracle development tool

This is an isolated development tool for the independent, high-precision
forward-backward reference and fixture generation described in
[`docs/forward-backward.md`](../../docs/forward-backward.md) and
[`docs/forward-backward-fixtures.md`](../../docs/forward-backward-fixtures.md).
The Rust sources implement the independent compensated Butterworth modes,
stable affine-segment primitives, and the linear-time constant-continued
modal convolution. The seven shared-duration time-domain fixtures and 360
nominal tone records have frozen schedules and samples. Both case families use
the same represented-scalar conversion and one-third/two-thirds refinement
paths, while tone cases retain their copied fit metadata. The full 2,448-cell
acceptance scoring matrix and its reports are not yet implemented.

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
cargo run --release --manifest-path tools/forward_backward_oracle/Cargo.toml -- session-smoke
```

The `verify` command prints a compact order-1/2/4/8 report with compensated
pole radius, complete gain at the public cutoff, fresh 192/256-bit convolution
agreement, and the 256-bit imaginary residual. It exits unsuccessfully if any
reported value exceeds its compact self-check threshold. The test suite
additionally checks independent pole/residue forms, signed kernel symmetry and
reality, exact constants, record reversal, exact collinear refinement, and
scalar-record promotion. It also exhaustively checks represented conversion
and scalar refinement across the current time-domain fixture axes, including
native-width rounding and original-knot preservation.

The `session-smoke` command starts the exact ignored root-library endpoint
client used by the future matrix harness. It sends a small nonconstant valid
record, checks the client's structured non-finite-input validation response,
then sends `Done` and requires a successful client exit. This exercises
process startup, loopback handshake, framing, request dispatch, and shutdown;
it is transport evidence, not numerical acceptance.

The oracle binds an ephemeral IPv4 loopback address before starting the child.
The per-run 16-byte nonce combines system-clock nanoseconds, the process ID,
and a process-local atomic counter. Its lowercase 32-character hexadecimal
encoding correlates this run's handshake; it is not an authentication secret.
Child stdout and stderr are drained continuously and retained only as bounded
64 KiB diagnostic tails. Error cleanup gives the direct Cargo child a brief
grace period, then kills and reaps it if needed; `Child::kill` does not promise
to terminate any process tree Cargo may have started.

The future matrix command will build on this oracle and stream full represented
records and outputs on demand; those large arrays are not committed as the
reference. Small frozen sentinels or constants, when useful, must be emitted as
Rust `f64::from_bits(...)` values. Compact coordinate manifests, metrics, and
recorded tool/source identities preserve provenance for complete and filtered
runs; checksums verify artifact integrity. The protocol, sharding, and artifact
contract is frozen in
[`docs/forward-backward-fixtures.md`](../../docs/forward-backward-fixtures.md#acceptance-harness-and-artifacts).
