# Integration-policy advisory UX review

## Round identity

- Protocol: lightweight `UX-1` advisory pass
- Surface: public integration-policy selection documentation
- Candidate commit: `be7fa00383abd6a8b7dd5a1618f9e3ea636ac66a`
- Model: GPT-6.1 Terra, medium reasoning
- Rust: `rustc 1.99.1 (58cf8f9c3 2026-10-13)`
- Host: `x86_64-unknown-linux-gnu`
- Isolation: fresh scratch crate and no source inspection; advisory rather than
  a frozen longitudinal baseline because project-level instructions and the
  repository manifest were available

The trial built an order-four `f32` Butterworth low-pass, selected adaptive or
fixed RK4 integration at runtime without heap allocation, and exercised an
ordinary 1 ms interval plus a 10 ms gap. It used the local crate with default
features disabled and `libm` enabled.

## Result

The final program compiled and ran without inspecting library source. Its
adaptive and fixed variants produced finite output, and the configured RK4
budget covered the 10 ms interval. The complete scratch source SHA-256 was
`c510a46ff9e76327e01cb65353dad86875b678f4ebe8e16e977ef5a4736072bc`.

The documentation made these contracts clear:

- Dormand--Prince is the safest general default;
- RK4's cap is an application accuracy choice, not an accuracy guarantee;
- `max_steps` must cover the longest interval and a private model bound may
  require more steps;
- an exhausted budget rejects the update transactionally.

## Findings and decisions

| ID | Scenario(s) | Category | Severity | Frequency | Evidence | Decision | Follow-up |
| --- | --- | ---: | ---: | ---: | --- | --- | --- |
| UX-IP-01 | runtime policy selection | Documentation | 1 | 1/1 | The trial had to infer the concrete enum variants `LowPass<4, f32, DormandPrince54<f32>>` and `LowPass<4, f32, RungeKutta4<f32>>`; the existing runtime example covered order rather than policy. | Add one allocation-free runnable policy-selection example; do not add dynamic-dispatch API. | Reuse the example in a later embedded trial. |
| UX-IP-02 | `no_std + libm` | Documentation | 1 | 1/1 | The first harness returned `Box<dyn std::error::Error>` and could not convert `BuildError` or `UpdateError` when ssfilt's `std` feature was disabled. Handling the concrete errors compiled immediately. | State the feature boundary and keep the new example free of boxed-error conversion. | Verify from the README in the next embedded trial. |

The accepted remedies clarify existing contracts. Runtime integrator dispatch
remains an application enum, and neither custom integrators nor a new public
type-erasure layer is introduced.
