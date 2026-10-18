# Beta 1 advisory UX round

## Round definition

- Protocol: `UX-1`
- Candidate: `ssfilt 0.1.0-beta.1`
- Isolation: advisory; agents have no inherited conversation context and work
  outside the repository, but filesystem isolation cannot be enforced by the
  execution platform.
- Model: prefer GPT-5.6 Terra Medium; record the actual available model.
- Allowed material: frozen `README.md`, generated public API documentation,
  the three maintained introductory examples linked by the README, and the
  curator-generated package metadata summary.
- Forbidden during the cold attempt: repository source, tests, benchmarks,
  `DESIGN.md`, `AGENTS.md`, Git history, development tools, this scenario file,
  and other trial reports.
- Dependency: the frozen local packaged crate, used only by Cargo.
- Network: disabled.
- Budget: at most three implementation/compile-correction cycles.
- Report: freeze attempted source, consulted material, compiler failures,
  assumptions, outcome, friction scores, and suggestions before any diagnostic
  source inspection.

Each trial stops after success, the third compile-correction cycle, a need for
missing user information, or contamination.

## S1 — Irregular environmental sensor

- Persona: application developer familiar with Rust but not control theory.
- Goal: smooth a temperature sensor sampled nominally every 100 ms, with
  intervals varying from 70 to 140 ms and one delayed 800 ms sample.
- Known start: the sensor has been steady at 22 °C before the first sample.
- Desired response: preserve changes slower than roughly 0.5 Hz without adding
  a startup transient.
- Constraints: ordinary `std`, `f64`, no runtime-selectable configuration.
- Success: a compiling, deterministic demonstration uses actual timestamp
  differences, chooses and explains the between-sample assumption, handles the
  delayed interval without silently changing elapsed time, and explains the
  chosen response family.
- Intentionally unspecified: exact application accuracy and acceptable delay.

## S2 — Vibration-band isolation

- Persona: mechanical test engineer with basic Rust experience.
- Goal: keep vibration between complete-response −3 dB edges of 8 Hz and
  20 Hz from irregular acceleration samples.
- Start: the accelerometer is stationary at zero.
- Constraints: ordinary `std`, `f64`, a modest order chosen by the developer.
- Success: a compiling deterministic demonstration constructs the appropriate
  topology, advances it with real elapsed intervals, and correctly explains
  the two edge frequencies, order meaning, startup state, and between-sample
  assumption.
- Intentionally unspecified: spectral selectivity outside the two edges.

## S3 — Reconnect and recovery

- Persona: telemetry application developer.
- Goal: smooth a sensor whose established baseline is 100 units, survive one
  non-finite sample without corrupting the filter, then reconnect at a known
  new steady baseline of 120 units.
- Constraints: ordinary `std`, `f64`; errors must be handled rather than
  unwrapped in the update path.
- Success: a compiling deterministic demonstration starts at the known steady
  value, shows the invalid update is rejected and leaves observable state
  unchanged, restores a meaningful steady state after reconnect, and resumes
  filtering.
- Intentionally unspecified: application-level logging and retry policy.

## S4 — Embedded selectable order

- Persona: firmware developer targeting a 32-bit MCU.
- Goal: build an allocation-free `no_std` `f32` low-pass filter whose order is
  selected at startup from the finite set 2 or 4.
- Constraints: pure-Rust math backend, no heap, cutoff fixed after construction,
  and compilation on the host is sufficient for the trial.
- Success: a compiling library crate disables default features, selects the
  supported math feature, uses fixed-size storage for both orders, and updates
  the selected filter without allocation.
- Intentionally unspecified: target-specific floating-point performance.

## B1 — Impossible streaming phase request

- Persona: developer processing live sensor samples.
- Goal: obtain exactly zero phase distortion and zero added latency while
  continuing to emit each output sample immediately.
- Success: no implementation is attempted. The response clearly explains why
  the requirements conflict for a causal filter, states that the current crate
  offers neither streaming phase correction nor offline zero-phase filtering,
  and identifies offline forward-backward or frequency-domain processing as a
  different class of solution when recorded data and latency are acceptable.
- Failure: inventing an API, claiming a delayed causal equalizer is zero-phase,
  or implying the current crate provides the requested behavior.
