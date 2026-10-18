# `0.1.0-beta.1` advisory UX review

## Round identity

- Protocol: `UX-1`
- Scenario set: `2026-10-18-beta1-advisory`
- Candidate commit: `a4ca3eec1c3181d63e9fb5ab38facbcbc8c11b79`
- Candidate version: `0.1.0-beta.1`
- Model: GPT-6 Luna Max (GPT-5.6 Terra Medium was unavailable)
- Rust: `rustc 1.98.1 (48a229cea 2026-09-01)`
- Host: `x86_64-unknown-linux-gnu`
- Isolation: advisory; no inherited conversation context or reported boundary
  violations, but the execution platform could not enforce filesystem access
- Action budget: three implementation/compile-correction cycles per scenario

The public snapshot contained 129 files. Its sorted-path manifest SHA-256 was
`753d558f93cf0e00dc9be794a253f98dffa5da391dbdf08e6ed301c5d97a9d5c`
and its reproducible public-tree SHA-256 was
`3d09bea5af074f891b2d97147390898348e3e624f5705f63545ff26d365b5e18`.
Allowed material was the README, generated API documentation without generated
source pages, package metadata, and the three introductory examples linked by
the README. Each trial used the same packaged local dependency only through
Cargo.

## Results

All four supported scenarios compiled on their first attempt. No agent needed
a correction cycle, source inspection, internal design material, or diagnostic
follow-up. The boundary control correctly declined the impossible request and
did not invent an API.

| Scenario | Outcome | Attempts | Complete scratch-source SHA-256 |
| --- | --- | ---: | --- |
| S1 irregular sensor | Passed and ran | 1 | `581bc5edff30cae0907aa12098d6e04d70c8966255e4140a017aa453ba406161` |
| S2 vibration band | Passed and ran | 1 | `73c6da79f32a14a5cfd834dbab0fd1d4f268995c1157f12aec837dc762782c12` |
| S3 reconnect recovery | Passed and ran | 1 | `109f348b67137bfc856372c073f0083268e67871806527a95bfd91374387ae1e` |
| S4 embedded selectable order | Passed `cargo check` | 1 | `79569e086c5a2011f4e191455695b5e384c8fa7138d11ab4ea5bd56b59d4bfc6` |
| B1 zero-phase/zero-latency | Correctly rejected | 0 | `f4c2e22a5ecacd94e2d3e00b86b2466ba0a94414af9bfc73128f4651e36e30f9` |

The first three prompts named the dimensions but accidentally omitted the
numeric rubric. Those agents initially treated 3 as best. After their cold
attempts were frozen, they were given only the rubric and corrected the scores;
no implementation or documentation work was reopened. Later prompts included
the rubric. Future scenario cards must include it directly.

| Scenario | Discovery | Topology | Timing | Init/recovery | Errors | Runtime | Phase | Ceremony |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| S1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| S2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| S3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| S4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| B1 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 0 |

Scores use 0 for no material friction and 3 for blocking or dangerous
friction. Phase information was not applicable to S3's requested behavior. B1
found the product boundary immediately but supplied the causality explanation
and offline alternatives from general knowledge rather than the README.

## Representative source evidence

The supported trials independently converged on the intended small interfaces:

```rust
let mut filter = LowPass::<2, f64>::builder(0.5)
    .response(Response::Butterworth)
    .input_model(InputModel::Linear)
    .initial_input(22.0)
    .build()?;
let output = filter.update(reading, elapsed.as_secs_f64())?;
```

```rust
let mut filter = BandPass::<4, f64>::builder(8.0, 20.0)
    .response(Response::Butterworth)
    .input_model(InputModel::Linear)
    .initial_input(0.0)
    .build()?;
```

The recovery trial matched `UpdateError::NonFiniteInput`, verified that
`output()` was unchanged, called `reset_to_steady(120.0)`, and resumed normal
updates. The embedded trial used a two-variant enum holding `LowPass<2, f32>`
or `LowPass<4, f32>` and compiled with:

```toml
ssfilt = { version = "0.1.0-beta.1", default-features = false, features = ["libm"] }
```

The boundary answer correctly stated that exact zero phase requires future
samples or waiting, distinguished Bessel's flat group delay from zero phase,
and directed recorded-data users toward offline forward-backward or
frequency-domain processing.

## Findings and decisions

| ID | Scenario(s) | Category | Severity | Frequency | Evidence | Decision | Follow-up |
| --- | --- | --- | ---: | ---: | --- | --- | --- |
| UX-B1-01 | B1 | Documentation | 1 | 1/1 boundary controls | The README states that phase and batch APIs are deferred but does not explain the causality conflict or name appropriate offline alternatives. | Accept a concise README/crate-doc clarification; do not add API. | Repeat a different phase/batch boundary control before 1.0. |
| UX-R1-01 | S1–S4 | API/UX | 0 | 4/4 supported scenarios | Every supported task compiled first try from public material without source inspection. | Preserve the current builder and update surface. | Re-run after a material public-API or timing-contract change. |
| UX-P1-01 | S1–S3 | Protocol | 1 | 3/5 prompts | The numeric scoring direction was absent from early trial prompts. | Include the complete rubric in every future frozen card. | Verify before the next round begins. |

No public API change is justified by this round. The intentionally unsupported
features remain fixed point, streaming phase correction, and offline zero-phase
filtering. Agent trials are only friction detectors; field feedback from the
beta remains necessary before any production-readiness claim.
