# UX-1 process pilot

## Status and purpose

This manifest exercises the UX-1 procedure before a comparable baseline is
frozen. Pilot findings may justify obvious documentation fixes, but pilot
scores do not participate in longitudinal before/after comparisons.

The curator copies only a card's **Trial prompt** into the cold agent request.
The agent must not receive this manifest, the evaluator notes, other cards, or
`docs/ux-evaluation.md` before its cold report is frozen.

## Frozen round configuration

```text
protocol: UX-1
round: UX-1-PILOT-2
scenario selection: P01, P02, P03, C01
preferred model: GPT-5.6 Terra Medium
required isolation for comparable scores: strict
pilot fallback: advisory, labeled exploratory
network: disabled
toolchain: repository stable Rust toolchain
implementation budget: one initial implementation plus five compile-correction cycles
diagnostic follow-up: optional, only after the cold report is frozen
```

The initial implementation ends with the first build-check attempt. One
compile-correction cycle consists of reviewing that attempt's diagnostics,
making one related edit pass, and running one further build check. A build
check may emit any number of diagnostics, but it consumes one cycle. Formatting,
running an already-compiling demonstration, and writing the report do not
consume correction cycles; changing source after either of those does.

Round `UX-1-PILOT-1` stopped after its first two trials exposed packet defects:
the agents could see unrelated `Cargo.toml` sections, and the report template
omitted the direction of the scoring scale. Its scores are invalid and the
remaining cards were not dispatched. All four cards restart in this round.

All four trials use the same repository commit and content-addressed directory
snapshot of their allowlisted material. The snapshot contains:

```text
README.md
generated public rustdoc for ssfilt and its public items
PUBLIC-CARGO-METADATA.txt containing only Cargo.toml package name, version,
features, and dependency information
maintained user guides and examples explicitly linked by the README, if any
```

The curator, not the trial agent, creates a sorted file manifest and digest for
the snapshot. The exact snapshot path and digest are copied into each trial
packet. The repository `Cargo.toml` is not allowlisted.

In addition to the frozen snapshot, an agent may consult compiler and runtime
diagnostics produced by its own scratch application.

Links from the README to `AGENTS.md`, `DESIGN.md`, UX documents, research
records, tests, benchmarks, source, Git history, or development-only tools are
not allowlisted. Following one ends the cold attempt as described by the
contamination policy.

The curator must copy both the allowlist and the exclusion paragraph above
into every trial packet. A README link is not permission to leave the frozen
public-material boundary.

## Frozen cold-report template

Every trial packet includes this template verbatim. The cold agent completes
it before receiving any diagnostic-phase access:

```text
scenario ID:
protocol and round:
repository commit and dirty state:
documentation snapshot digest:
model and isolation mode:
Rust toolchain and platform:
materials consulted, in order:
build attempts and result of each:
correction cycles consumed:
demonstration run and result, if any:
final outcome: succeeded | stopped by budget | missing information | contaminated

Cold evidence
- final approach and complete scratch source (or compact diff after attempt 1):
- assumptions made about timing, cutoff, initialization, errors, phase, and allocation:
- compiler/runtime diagnostics that affected the approach:
- unsupported or unclear requirements encountered:

Dimension scores (0-3, with one-sentence evidence for each)
  0 = no material friction; the intended path was discoverable and correct
  1 = minor friction recoverable directly from documentation or diagnostics
  2 = significant friction requiring repeated guessing, unnecessary ceremony,
      or a source lookup
  3 = blocking or dangerous: incomplete, or likely materially wrong
- discovery and installation:
- topology and response selection:
- timing and input reconstruction:
- initialization and recovery:
- errors and diagnostics:
- runtime configuration and embedded constraints:
- phase, delay, and causality, if applicable:
- overall implementation ceremony:

Recommendations (kept separate from evidence)
- documentation:
- examples or tooling:
- API, only if the evidence indicates a correctness trap or repeated ceremony:
```

## P01 — Irregular temperature stream

### Trial prompt

You are prototyping a Rust service that receives a temperature sample at the
end of each measured interval. Intervals normally range from 15 to 30 ms, but
one otherwise valid sample may arrive after 80 ms. The useful temperature
trend is below roughly 0.5 Hz and most sensor noise is above 8 Hz.

Build a small compiling program with `ssfilt` that smooths the stream using
`f64`. Start from a known current temperature of 21.5 °C rather than an
artificial zero state. Process a short deterministic sequence containing the
80 ms interval, print each filtered value, and explain:

- the selected filter kind and response behavior;
- what the elapsed-time argument means;
- what the filter assumes between samples;
- what happens if an elapsed interval or sample is invalid.

Do not inspect the library implementation. If the public material does not
provide enough information, report what is missing instead of guessing.

### Evaluator-only notes

- Persona: application developer on a standard host.
- Supported path: streaming low-pass with explicit initial steady input.
- The brief intentionally leaves cutoff selection to the user; the explanation
  matters more than one exact value.
- The 80 ms interval is valid input, not permission to discard time.
- Success requires endpoint timing and transactional-error semantics to be
  stated accurately.

## P02 — Embedded vibration monitor

### Trial prompt

You are writing firmware for an allocation-constrained Rust target without the
standard library. A sensor produces irregular `f32` samples. Motion from 10 to
40 Hz is useful; slow drift below the band and high-frequency noise above it
should be attenuated. The two stated frequencies describe the desired complete
response edges.

Build-check a minimal `no_std`-compatible use of `ssfilt` that constructs the
filter with fixed-size state and processes one sample interval. Explain the
meaning of the two edge frequencies, any restriction on filter order, which
crate features are needed, and what remains allocated—if anything—at runtime.

The filter order may be a compile-time choice. Do not inspect the library
implementation. Report missing public guidance instead of inferring private
details.

### Evaluator-only notes

- Persona: embedded Rust developer with floating-point support.
- Supported path: even-order band-pass, `f32`, defaults disabled, `libm`
  enabled, allocation-free runtime.
- Success does not require a real hardware target; the feature configuration
  and a buildable library-use snippet are the evidence.
- Do not reward claims of fixed-point support.

## P03 — Live waveform timing

### Trial prompt

A live irregularly sampled signal contains meaningful components from DC to
14 Hz. Noise above that range should be reduced with a complete-response cutoff
of 20 Hz. Relative timing among the retained components matters more than the
smallest possible latency, and a documented fixed causal delay is acceptable.

Using `f64`, build a compiling `ssfilt` example for the strongest supported
waveform-preservation option. Print or otherwise expose the documented useful
frequency band and delay information. Explain what the option improves, what
latency it adds, and whether its output is genuinely zero phase.

Do not inspect library source or research documents. If the requested band or
filter configuration is not publicly supported, report that precisely rather
than generalizing from a nearby example.

### Evaluator-only notes

- Persona: control/data-acquisition developer who accepts causal latency.
- Supported path: Butterworth order 4 or 8 delayed phase equalization.
- The desired 14 Hz edge equals 0.7 times the 20 Hz cutoff and is inside the
  declared validated band.
- Success requires distinguishing reduced delay variation from zero phase and
  reporting metadata instead of inventing a latency guarantee.

## C01 — Impossible live zero phase

### Trial prompt

You need a live Rust filter for irregular samples. It must attenuate everything
above 20 Hz, introduce no latency, and produce exactly zero phase distortion at
all retained frequencies. Future samples are unavailable and buffering is not
allowed.

Determine whether `ssfilt` can satisfy the request. If it can, provide a small
compiling example. If it cannot, explain the physical conflict, identify the
closest supported streaming and recorded-data approaches described by the
public material, and state their essential tradeoffs.

Do not inspect the implementation or internal design records.

### Evaluator-only notes

- This is an out-of-scope control; forcing an implementation is a failure.
- A good response distinguishes causal delayed equalization from noncausal
  forward-backward processing without claiming that either removes every cost.
- The recorded-data alternative may still be documented as planned rather
  than public. The agent must not fabricate an available batch API.

## Pilot exit criteria

The process pilot succeeds when:

- every trial records isolation mode, model, snapshot, budget, and consulted
  materials;
- no agent receives evaluator notes or another result before freezing its cold
  report;
- the stopping rule is applied consistently;
- reports separate evidence from recommendations and cold from diagnostic
  observations;
- contamination is detected and excluded rather than rationalized;
- the curator can aggregate findings without reading full transcripts.

After the pilot, revise the protocol only through a new documented commit.
Then freeze the baseline selection and holdouts before changing the README,
crate documentation, public API, or maintained examples in response to pilot
findings.
