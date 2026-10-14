# UX evaluation and documentation plan

## Purpose

`ssfilt` should be straightforward to discover, configure, and use correctly
without reading its implementation. Numerical correctness and runtime cost are
necessary, but they do not compensate for an API whose timing, cutoff, startup,
or failure semantics are hard to understand.

This document defines a repeatable evaluation for the public user experience.
It is deliberately separate from numerical acceptance testing: a UX trial can
find unclear documentation, excessive ceremony, missing examples, misleading
names, or absent quality-of-life tools, but it cannot establish filter
accuracy or expand a validated support boundary.

The initial protocol version is `UX-1`. Material changes to scenario meanings,
allowed information, or scoring create a new version so results from different
rounds are not silently compared as if they used the same experiment.

## Principles

1. **Start from user goals.** Scenario briefs describe a physical problem and
   constraints, not the library types or methods expected in the solution.
2. **Evaluate the supported product boundary.** Include useful applications
   for the library as well as control cases that should be declined or
   redirected.
3. **Keep the first attempt cold.** Trial agents use only public user-facing
   material before reporting whether they could build a solution.
4. **Separate discovery from diagnosis.** Source and internal design records
   may be inspected only after the first attempt, to explain a rough edge.
5. **Prefer repeated evidence.** One agent's preference does not justify an API
   change. Repeated friction, correctness traps, and severe discoverability
   failures receive priority.
6. **Do not teach the test.** Existing examples may help users, but holdout
   scenarios must remain meaningfully different from those examples.
7. **Preserve scope.** A successful result can be a clear explanation that
   `ssfilt` is the wrong tool for the requested job.

Agent trials are inexpensive friction detectors, not substitutes for feedback
from human users. Their findings guide documentation and API review; they do
not prove that an interface is intuitive.

## Roles and independence

- The primary agent owns the scenario set, declared support boundary, scoring
  rubric, aggregation, and decisions about public API changes.
- Independent implementation agents perform cold trials. GPT-5.6 Terra Medium
  is the preferred model when available because the task represents ordinary
  application development rather than library architecture.
- A trial agent must not have authored the API or documentation under review.
- Trial agents do not share attempts or findings with one another before their
  reports are complete.
- A separate reviewer may group findings and challenge proposed remedies, but
  the primary agent performs final architectural review.

If the preferred model is unavailable, record the actual model in the review
report. Do not present a different model's result as a Terra baseline.

## Context isolation and contamination

A comparable cold trial requires a fresh agent with no inherited conversation
or project history. Create it with an empty-history mechanism such as
`fork_turns: "none"`, and provide only:

- the frozen scenario prompt;
- the exact public-material allowlist;
- the scratch-crate location and build instructions;
- the frozen action budget and reporting template.

The trial environment should start outside the repository and should not load
repository-specific agent instructions. Merely telling an agent that already
saw source, `AGENTS.md`, internal roadmaps, earlier findings, or this scenario
bank to ignore that knowledge does not make the trial cold.

Record the isolation mode in every result:

- **strict**: no inherited project context, repository instructions, or
  filesystem access beyond the allowlisted snapshot and local crate dependency;
- **advisory**: the platform cannot enforce a clean context or filesystem, but
  the agent is instructed to respect the allowlist.

Only strict trials participate in longitudinal baseline comparisons. Advisory
trials are still useful for piloting scenarios and finding obvious friction,
but their scores are labeled exploratory.

A trial is **contaminated** when the agent receives prior project-specific
context or sees forbidden material before freezing its cold report. Exclude it
from aggregate frequency and before/after comparisons. If a forbidden internal
link is reached naturally from an allowlisted user document, record that
discoverability failure and stop the cold attempt at that point; the event is
evidence, but subsequent work belongs to the diagnostic phase.

## Information boundary

### Cold attempt

The agent may use only the surfaces intended for a library user:

- `README.md`;
- generated crate and item documentation;
- user-guide documents linked from the README or crate documentation;
- explicitly linked, maintained examples;
- compiler and runtime diagnostics from its own scratch application;
- `Cargo.toml` package metadata needed to depend on the crate.

The agent must not inspect `src/`, tests, benchmarks, `DESIGN.md`, numerical
research records, agent instructions, Git history, or another trial's report.
The scratch application lives outside the repository source tree and must not
modify the library.

### Diagnostic follow-up

After freezing the cold-attempt report, the same agent may inspect source and
internal design documents to identify why it struggled and suggest a remedy.
Cold and diagnostic observations remain visibly separated in the report.

## Scenario format

Every scenario freezes these fields before an agent sees it:

```text
id and protocol version
user persona and environment
physical goal
input timing and data characteristics
accuracy, latency, memory, and allocation constraints
configuration that must remain runtime-selectable, if any
observable success criteria
known unsupported or intentionally unspecified behavior
allowed user-facing materials
network and tool access
maximum implementation and compile-correction cycles
stopping conditions
```

Briefs must not name expected Rust types, builder methods, response families,
or implementation techniques unless a real user would already have made that
choice. Success criteria describe observable behavior rather than requiring a
copy of an existing example.

## Initial scenario bank

The first baseline should select six to eight supported scenarios and at least
two boundary controls. Rotate holdouts between rounds so documentation cannot
be optimized only for one fixed exercise.

### Supported scenarios

1. **Irregular environmental sensor smoothing.** Smooth noisy ADC readings
   with real elapsed intervals while preserving a slow trend and handling one
   delayed sample.
2. **IMU low-pass selection.** Reduce high-frequency acceleration noise while
   prioritizing waveform shape, then explain the chosen response and startup
   behavior.
3. **Sensor-bias rejection.** Remove a slowly varying offset from an irregular
   stream and explain the endpoint behavior of the selected input model.
4. **Vibration-band isolation.** Preserve a declared frequency band from an
   irregularly sampled signal using two complete-response edge frequencies.
5. **Runtime-selected configuration.** Choose among a small finite set of
   filter orders at runtime without pretending arbitrary order is free.
6. **Embedded `no_std` use.** Build an allocation-free `f32` filter using the
   supported math backend and fixed-size state.
7. **Reconnect and recovery.** Initialize from a known steady sensor value,
   reject an invalid update transactionally, and restore a meaningful steady
   state after reconnection.
8. **Causal waveform preservation.** Use the supported streaming phase
   equalizer, report its validated band and added delay, and avoid claiming
   zero phase.
9. **Recorded zero-phase processing.** Once the batch API is public, filter a
   complete irregular record noncausally and explain its different magnitude
   and boundary semantics. This scenario is inactive until that milestone.

### Boundary controls

1. Apply a two-dimensional image filter.
2. Implement an arbitrary brick-wall or user-drawn spectral mask.
3. Run on fixed-point-only hardware without floating-point support.
4. Obtain zero-phase, zero-latency streaming output.
5. Build a broad fixed-rate audio equalizer where a conventional discrete-time
   DSP library is the more natural choice.

A boundary response passes when it identifies the mismatch, explains the
relevant limitation accurately, and suggests the general class of alternative
without distorting `ssfilt` into a different product.

The bank above is a curator's coverage map, not an executable trial. Complete,
versioned cards freeze the actual prompt, evaluator-only notes, allowlist,
budget, and success criteria. The first process pilot is defined in
[`ux-scenarios/UX-1-pilot.md`](ux-scenarios/UX-1-pilot.md). Pilot results tune
the protocol and are not a longitudinal baseline. Freeze a separate baseline
manifest only after the pilot demonstrates that isolation, reporting, and
stopping rules work.

## Trial procedure

1. Record the repository commit, dirty state, documentation snapshot digest,
   Rust version, platform, model, protocol version, scenario ID, isolation
   mode, and exact allowed documentation set.
2. Start a no-history agent outside the repository. Create a clean scratch
   crate depending on the frozen local `ssfilt` checkout without granting the
   agent permission to browse that checkout.
3. Send only the trial-prompt portion of the frozen card, allowlist, action
   budget, and report template before opening user documentation.
4. Attempt a compiling solution using only the cold information boundary.
5. Record compiler attempts and documentation consulted. Do not silently erase
   wrong assumptions from the report after fixing them.
6. Run a small deterministic demonstration where practical. This verifies the
   example is executable; it is not a numerical oracle.
7. Freeze the cold report and source code.
8. Optionally perform the diagnostic follow-up with internal materials.
9. Delete or retain the scratch directory outside Git as requested by the
   primary agent. Never commit build output.

Trials may run in parallel only when they use separate scratch directories and
do not edit repository files.

Stop the cold attempt when it succeeds, exhausts its frozen action budget,
requires unavailable user information, or becomes contaminated. Do not grant
extra iterations selectively to a struggling scenario. A round may choose a
different budget, but every trial compared within that round uses the same
definition of an implementation/compile-correction cycle.

## Evidence and scoring

Each trial records facts before recommendations:

- whether a compiling solution was reached;
- whether source inspection was needed;
- user documents and examples consulted;
- number and category of failed compile attempts;
- incorrect timing, cutoff, phase, initialization, or error assumptions;
- boilerplate unrelated to the scenario's physical choices;
- diagnostics that were actionable or opaque;
- missing helper, example, guide, or measurement tool;
- whether the final solution stayed inside the declared support boundary.

Use this ordinal score for each dimension:

| Score | Meaning |
| ---: | --- |
| 0 | No material friction; the intended path was discoverable and correct. |
| 1 | Minor friction; a user can recover directly from documentation or diagnostics. |
| 2 | Significant friction; repeated guessing, unnecessary ceremony, or a source lookup was needed. |
| 3 | Blocking or dangerous; the task could not be completed, or the likely interpretation is materially wrong. |

Score these dimensions separately:

- discovery and installation;
- topology and response selection;
- timing and input reconstruction;
- initialization and recovery;
- errors and diagnostics;
- runtime configuration and embedded constraints;
- phase, delay, and causality claims where applicable;
- overall implementation ceremony.

Do not combine them into a deceptively precise single usability number. Agent
wall-clock time is observational only and is not compared across models or
hosts.

## Finding policy

Aggregate findings by underlying user problem, not by each agent's proposed
method name. Classify each finding as:

- **correctness risk**: encourages a physically or numerically wrong result;
- **discoverability**: the supported path exists but is difficult to find;
- **diagnostic**: failure information does not lead toward a correction;
- **ceremony**: repeated application code lacks a useful, narrow abstraction;
- **documentation**: the behavior is insufficiently explained or demonstrated;
- **tooling**: users cannot readily inspect or replay a supported behavior;
- **out of scope**: the requested capability should not be added.

An API change normally requires the same underlying problem in at least two
independent supported scenarios. A single finding may still be promoted when
it is a severe correctness trap. Documentation fixes and narrowly scoped
examples may be accepted with less repetition when they clarify an existing
contract without expanding it.

Every accepted remedy states:

- the finding IDs it addresses;
- why documentation, an example, a diagnostic, or an API change is the right
  layer;
- the behavior that remains intentionally unsupported;
- how the next UX round will verify the improvement without deleting its
  holdout coverage.

## Stored artifacts

Commit only compact review evidence:

```text
docs/ux-reviews/YYYY-MM-DD-<surface>.md
```

Each review contains protocol and scenario versions, environment identities,
an aggregate finding table, accepted actions, deferred suggestions, and links
to any new maintained examples or guides. Include the final relevant source
or a compact diff, the exact diagnostic excerpts supporting each finding, and
a digest of the complete scratch source. Full agent transcripts, complete
scratch crates, target directories, timing noise, and generated binaries are
not committed.

Use this finding table:

| ID | Scenario(s) | Category | Severity | Frequency | Evidence | Decision | Follow-up |
| --- | --- | --- | ---: | ---: | --- | --- | --- |

## Documentation architecture

The intended user path is:

```text
README landing page
    -> crate-level quick start and API reference
    -> task-oriented user guides
    -> focused, compiled examples
```

Contributor and research records remain linked but do not interrupt that path.

### README landing page

Keep the README concise and task-oriented:

1. what problem `ssfilt` solves and its experimental status;
2. standard and `no_std` installation;
3. the smallest useful irregular-sample example;
4. a compact topology/response selection table;
5. the endpoint timing and whole-response cutoff conventions;
6. short phase and offline-batch distinctions;
7. links to user guides, API documentation, examples, development tools, and
   contributor design records.

### Crate documentation

The crate-level documentation should compile its quick start and explain the
common vocabulary before listing modules and types. Public builders, errors,
diagnostics, and phase metadata should link back to the relevant guide rather
than duplicating long explanations inconsistently.

### User guides

Add guides incrementally when their examples and behavior are ready:

```text
docs/guide/choosing-a-filter.md
docs/guide/timing-and-input-models.md
docs/guide/initialization-and-errors.md
docs/guide/embedded-no-std.md
docs/guide/runtime-configuration.md
docs/guide/phase-and-batch.md
```

Each guide answers a user decision, includes at least one compiled example,
states important non-goals, and links to the precise API reference. Avoid
creating placeholder guide files whose promised examples do not exist.

### Examples

Promote a UX trial into `examples/` only after removing scenario-specific
noise and verifying that it teaches one stable workflow. Examples are compiled
in CI and remain small enough to understand without reading helper frameworks.
Development measurement programs remain explicitly separate from introductory
user examples.

## Cadence

Run UX reviews at these points:

1. baseline against the current streaming API and current documentation;
2. after addressing repeated baseline friction;
3. before exposing the offline batch API;
4. after material public API or timing/error-contract changes;
5. during the release-readiness pass before 1.0.

Reuse part of the scenario bank to measure regressions and rotate part as
holdout coverage. Record documentation-only improvements separately from API
changes so their effects remain visible.

## Relationship to tooling

UX findings may request inspection or replay utilities, but development tools
remain evidence-driven and outside the runtime library. Prefer one structured
measurement pipeline whose records feed CSV, Markdown, and SVG views. Add a
tool view only when it answers a recurring user or validation question; do not
grow a generic signal-processing application around the library.

The current tooling priorities remain topology coverage, measured streaming
equalizer overlays, incremental named CLI options, and later CSV replay. The
private forward-backward matrix harness is numerical acceptance infrastructure,
not a user-facing filtering tool.
