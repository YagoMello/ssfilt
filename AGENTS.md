# Agent collaboration guide

## Mission

Build a small, reliable Rust library for continuous-time filters driven by
irregularly sampled signals. Preserve the public timing, cutoff, numerical,
and error contracts while keeping the common API simple, allocation-free, and
usable in `no_std` environments.

This project favors a narrow, well-tested feature over a broad interface with
unclear numerical behavior. Do not turn an experiment into public API before
its use case and limits are demonstrated.

## Product and complexity budget

The product is the irregular-time streaming filter, not its research harness.
Every feature must begin with a named user problem and a simple public path.
Prefer a documented limit over machinery for implausible combinations, and do
not exhaust a Cartesian product when representative cases establish the same
contract.

Treat implementation, tests, tools, documentation, and review surface as one
maintenance budget. If private evidence infrastructure becomes larger or more
complicated than the feature it supports, pause and reassess the feature's
user value before adding more. Git history can preserve a useful experiment
without keeping it on the active product roadmap or in the release package.

Use three levels of evidence:

- automate public correctness contracts such as timing, cutoff, equilibrium,
  finite-value handling, transactional failure, supported scalar types, and
  `no_std` operation;
- test representative numerical behavior across ordinary and demanding but
  supported configurations;
- use plots and application trials for qualitative tradeoffs such as ringing,
  phase shape, and whether a response is suitable for a user's signal.

Do not build support for undeclared scalar types, extreme orders, or every
possible topology/response combination merely because they can be enumerated.

## Required reading

Before changing code or tests, read the files relevant to the assignment:

1. `README.md`
2. `DESIGN.md`
3. the relevant document under `docs/`
4. the public module and its private model
5. nearby tests that define the same contract

When documents disagree, report the conflict to the primary agent. Do not
silently choose the interpretation that makes implementation easiest.

## Roles and model guidance

The primary agent owns architecture, numerical methods, public API, work
package boundaries, final integration review, and Git history.

Preferred delegation:

- GPT-6 Luna Max: bounded, locally specified work such as test matrices,
  reference cases, documentation consistency, repetitive implementation,
  plotting support, and test-failure triage. Do not give Luna architectural
  ownership or decisions whose long-term effects span public contracts or
  several modules.
- GPT-5.6 Terra Med/High: well-specified functions and focused module
  implementation. Terra High is also appropriate for independent reviews of
  medium-complexity changes.
- GPT-5.6 Sol Light: bounded research, option surveys, and localized planning
  whose surrounding contracts are already clear.
- GPT-5.6 Sol Medium: wide-ranging architecture, public API alternatives,
  cross-cutting design, roadmap decomposition, and reviews that must account
  for long-term interactions between subsystems.
- GPT-5.6 Sol High: difficult numerical design, stability analysis, ambiguous
  cross-cutting problems, and independent review of high-risk changes. Use it
  selectively because of its higher resource cost.
- Primary frontier model: final architectural and API decisions, numerical
  synthesis, work-package boundaries, difficult integration bugs, and final
  review.

GPT-6 Astra is intentionally outside the normal delegation ladder because one
task can consume a substantial share of the available usage budget. Consider
it only for a tightly bounded frontier-level impasse, with explicit approval
from the primary agent after less expensive models have proved insufficient.
Do not use it for routine implementation, planning, or review.

Choose the least expensive model that can reliably own the assigned scope.
Exploration from a smaller model is evidence for the primary agent, not an
architectural decision by itself. Cross-cutting conclusions must be synthesized
by the primary agent or independently checked by an appropriately capable Sol
reviewer.

These are defaults. Every delegated task must state its owned files, relevant
contracts, expected verification, and completion criteria.

## Work-package rules

Each agent receives a bounded task with:

- one concrete outcome;
- explicit files or modules it may edit;
- contracts that must remain true;
- commands or tests to run;
- conditions requiring escalation.

Agents share one worktree. Do not assign the same file to multiple agents at
the same time. Read-only reviews may overlap implementation when the reviewer
does not edit files.

Sub-agents normally leave changes uncommitted and report:

- files changed;
- tests run and their results;
- assumptions made;
- remaining risks or questions.

The primary agent reviews, integrates, and commits. A sub-agent may commit only
when its task explicitly grants ownership of a separate branch or commit.

## Escalate instead of guessing

Contact the primary agent when:

- the whole-filter -3 dB cutoff convention would change;
- input reconstruction or endpoint timing semantics would change;
- a phase feature's delay, useful band, or objective is unspecified;
- numerical stability requires a new order, bandwidth, tolerance, or work
  limit;
- a change weakens transactional failure behavior or finite-value checks;
- `no_std`, allocation-free operation, supported scalar types, or the MSRV
  would change;
- a public trait, enum, builder method, or error variant would be added;
- a test appears to require weaker accuracy or a broader tolerance;
- alternative implementations disagree in a way that affects architecture;
- unrelated work or destructive Git operations might be affected.

State uncertainty plainly. Do not hide it behind a plausible coefficient set
or a test tuned to one hand-crafted signal.

## Contract-first development

For numerical behavior:

1. define the physical goal and useful operating range;
2. derive or cite an independent reference result;
3. define metrics and acceptance thresholds before implementation;
4. add tests that fail for the missing behavior;
5. implement the smallest complete design;
6. run focused tests, then the full quality gates;
7. inspect plots as supporting evidence, never as the only oracle;
8. record limitations and computational cost.

Important invariants include:

- public cutoff values describe the complete response's -3 dB points;
- constant-input equilibria are exact for the topology;
- physical frequency scaling changes only the time mapping;
- failed updates leave all observable and internal state unchanged;
- the complete elapsed interval is integrated or rejected;
- section ordering and state scaling avoid avoidable internal peaking;
- streaming phase correction states its added delay and useful band;
- a claimed improvement holds in a named representative application;
- runtime storage remains fixed-size and allocation-free.

Never delete, skip, loosen, or mark a failing test expected without explicit
review by the primary agent.

## Review independence

Numerical methods and public API changes require review by an agent that did
not author them. Reviews prioritize:

1. whether the feature solves the stated physical problem;
2. derivation and reference-model correctness;
3. stability, conditioning, and boundary behavior;
4. misleading phase, latency, or cutoff claims;
5. test coverage across orders, responses, bands, and time steps;
6. `no_std`, allocation, memory, and runtime costs;
7. API clarity and maintainability.

An alternative implementation branch is comparison material only after the
current approach and acceptance criteria have been written down independently.

## UX trials

Public usability reviews follow `docs/ux-evaluation.md`. The default pre-1.0
review is a lightweight goal-based pass using a fresh agent, public material,
and a separate scratch crate. Use the document's frozen snapshots, scoring,
and stricter isolation only when a comparable longitudinal baseline would
change a product decision; protocol machinery is not itself a release goal.
In either mode, the first attempt must not inspect `src/`, tests, benchmarks,
internal design records, Git history, or another trial's findings.

Prefer GPT-5.6 Terra Medium for ordinary application trials when it is
available. Record the actual model rather than presenting another model as a
Terra baseline. Trial agents do not edit the library. They report the program
attempted, public documentation consulted, compiler failures, incorrect
assumptions, source lookups, ceremony, missing guidance or tools, and whether
the request was appropriately rejected as out of scope.

Do not add public API from one agent preference. Promote repeated friction,
clear correctness hazards, or narrowly justified documentation and diagnostic
improvements. The primary agent owns aggregation, remedies, and final API
decisions.

## Quality gates

Run focused tests while iterating. Before a functional commit, run:

```text
cargo fmt --all -- --check
cargo test --all-features
cargo check --no-default-features --features libm
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --all-features --no-deps
```

The pre-1.0 crate follows stable Rust and intentionally has no declared MSRV.
Do not add a fixed-version gate or `rust-version` field without an explicit
compatibility decision; establishing the 1.0 MSRV is a public-policy change.

Development plots and benchmarks are evidence for numerical and performance
claims. They do not replace analytic, reference, property, or public API tests.

## Git discipline

Use small, coherent commits. A functional commit includes its tests and
documentation whenever practical.

- One numerical decision, public behavior, tool, or refactor per commit.
- Each commit should pass the gates appropriate to that phase.
- Commit messages describe intent, for example:
  `feat: add delayed passband phase equalizer`.
- Generated `target/` outputs are never committed.
- Do not rewrite, squash, rebase, or force-update shared history without an
  explicit request.
- Never use destructive Git commands to remove another agent's work.
- The primary agent checks the worktree and stages only the intended files.

## Current boundaries

The implemented public topologies are low-pass, high-pass, and band-pass with
repeated-pole, Butterworth, Bessel, and Chebyshev Type I responses. Runtime
scalars are sealed to validated `f32` and `f64` implementations. The adaptive
Dormand-Prince integrator and continuous models remain private.

Phase handling is limited to the existing streaming equalizer retention
decision. It must demonstrate useful passband waveform preservation and
disclose added delay; do not expand it before that decision. Offline
forward-backward filtering is deferred beyond 1.0 and its research harness is
preserved only in Git history. Fixed point, custom public kernels, alternative
public integrators, and a possible separate batch crate remain deferred until
a measured use case justifies their interfaces.
