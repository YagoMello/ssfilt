# UX-1 process-pilot review

## Scope and status

This review records the `UX-1-PILOT-1` and `UX-1-PILOT-2` procedure trials.
It is not the streaming API baseline. The available subagent environment could
not prevent access to the repository and automatically supplied project agent
instructions, so no score in this review is comparable longitudinal evidence.

Repository state for the completed round was commit `b802e92`, initially
clean. GPT-5.6 Terra Medium was unavailable; all attempts used GPT-6 Luna Max
and recorded that substitution. Network access was disabled. The shared public
snapshot manifest digest was
`d93af12f82d503557fb8e94146613e1b591d074408ea7dfe2c72a0826067f6cb`.

## Procedure findings

`UX-1-PILOT-1` stopped after P01 and P02. Direct access to the repository
`Cargo.toml` exposed excluded sections, and the report packet omitted the
direction of the ordinal scoring scale. The scores from that round are invalid.

Before `UX-1-PILOT-2`, the packet was changed to provide a public-only snapshot
with sanitized Cargo metadata and all four score meanings. The four scenarios
then exercised snapshot verification, a common action budget, scratch-crate
isolation, compiled demonstrations, and the frozen report format.

The C01 agent explicitly reported that repository-level agent instructions had
been injected despite empty conversation history. C01 is therefore marked
contaminated and excluded. Because the same platform launched P01 through P03,
their results remain advisory and potentially context-contaminated even though
those agents did not report reading forbidden files. They are useful only for
improving the procedure and identifying low-risk documentation hypotheses.

| Scenario | Outcome | First build | Corrections | Evidence status | Main observation |
| --- | --- | --- | ---: | --- | --- |
| P01 irregular temperature | Succeeded | Succeeded with one unused-import warning | 0 | Advisory | Normal dependency setup was not on the README path. |
| P02 embedded band-pass | Succeeded | Succeeded | 0 | Advisory | The `no_std` band-pass path was discoverable and concise. |
| P03 live waveform timing | Succeeded | Succeeded | 0 | Advisory | Equalizer band, delay, and non-zero-phase limits were discoverable; “strongest” did not define how to choose order 4 versus 8. |
| C01 impossible live zero phase | Succeeded by rejecting the premise | Succeeded for the closest streaming demonstration | 0 | Contaminated; excluded | It did not fabricate a public batch API, but prior project context could have supplied that distinction. |

## Finding decisions

| ID | Scenario(s) | Category | Severity | Frequency | Evidence | Decision | Follow-up |
| --- | --- | --- | ---: | ---: | --- | --- | --- |
| UX-P-01 | P01, P03 | Discovery | 1 | 2 advisory | Both agents used supplied metadata because the README lacked normal pre-release dependency setup. | Accept a documentation-only fix. | Add accurate path-based pre-release installation before the quick start and align the `no_std` snippet. |
| UX-P-02 | P01, P03 | Documentation | 1 | 2 advisory | Both suggested a complete irregular-update example with explicit initialization. | Defer. | Recheck in a strict or human trial before promoting another maintained example. |
| UX-P-03 | P03 | Scenario design | 1 | 1 advisory | “Strongest supported” implied an order ranking the public contract does not make. | Fix the future baseline card, not the API. | Ask for the supported waveform-preservation option and let the user choose a validated order unless the scenario supplies an objective. |
| UX-P-04 | C01 | Documentation | 2 | Excluded | The agent could not establish public recorded-data support from the allowed API docs. | Defer product action. | Re-evaluate when the batch API is ready; keep the README explicit that the linked batch record is a design, not a shipped API. |
| UX-P-05 | Curator verification | Protocol | 1 | N/A | Inspection confirmed that the packet advertised a digest without defining that it was the hash of the manifest bytes; no contaminated-trial frequency is counted. | Accept. | Freeze the digest command in the pilot manifest and future run packets. |

The accepted README changes clarify existing status and do not expand public
behavior. No API, numerical, or tooling change is justified by this pilot.

## Scratch-source evidence

The complete scratch crates and transcripts are not committed. Relevant source
digests are retained for traceability:

```text
P01 Cargo.toml  fe97d50021cd38b452b1be14ddba9aea55a70e718ed890a3a12986e9a2b071ba
P01 src/main.rs 90a2b38475f6c03dca233528537e5056850f62745266fb17b4c4d21af7793839
P02 Cargo.toml  acef7694306aa5f6fe778cda67e1be543dac2116e955ad3d9f56da29b0fb4194
P02 src/lib.rs  16612865baabe0c53beb82101ab21ad83fa8b353755e8ea8a1c0f9161551efdc
P03 Cargo.toml  df9814f094aeb95f6a392eafd3279f5416edd44874b3ba3d739a25f7d4d1b82d
P03 src/main.rs 6e45ed8bc974b32e20bdc7b1efa4a716a026b87fc8e52076d6e59a08ca46c5de
C01 Cargo.toml  a7d4cef335dca6056c38812135f23e697c3267b641c579b613ceaa1bdd536c12
C01 src/main.rs 9f06f45047d482114e307f8afaf52b58a535acb1e10c4c34a3319b8f870e017f
```

## Baseline preconditions

A comparable baseline requires a projectless or otherwise sandboxed trial
environment that does not inject this repository's instructions and cannot
browse the local dependency. It should use the preferred Terra Medium model
when available, the same verified public snapshot for every card, and a new
baseline manifest whose prompts do not imply an unsupported ranking. Until
those conditions exist, repeat runs from repository-scoped subagents remain
exploratory and are not presented as usability measurements.
