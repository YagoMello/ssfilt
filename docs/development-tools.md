# Development measurement and visualization tools

## Purpose

The development tools must make numerical claims inspectable without becoming
part of the runtime library. They should answer three different questions:

1. What does the continuous transfer-function model predict?
2. What does the public streaming implementation actually produce for sampled
   inputs and timestamps?
3. Does the difference remain inside the declared accuracy, latency, and work
   limits?

Plots are supporting evidence. Machine-readable measurements and automated
assertions remain the acceptance oracle.

User-facing friction and requests for new inspection utilities are evaluated
under [`ux-evaluation.md`](ux-evaluation.md). A repeated UX need can justify a
small tool extension, but does not bypass the numerical evidence requirements
or turn these development programs into a general signal-processing suite.

## Data pipeline

Keep one structured pipeline rather than computing values inside chart code:

```text
Case specification
    -> analytic reference
    -> streaming measurements
    -> metrics and pass/fail decisions
    -> CSV/Markdown report
    -> SVG views
```

The CSV rows and plotted points must come from the same measurement records.
This makes a suspicious marker reproducible and prevents a chart from silently
using different math than the report.

The current examples separate analytic calculation, public-API measurement,
report generation, and rendering into development-only modules. A future shared
tool could consolidate them along these boundaries:

```text
examples/filter_lab.rs              small CLI and order dispatch
examples/filter_lab/config.rs       cases, named bands, schedules
examples/filter_lab/analytic.rs     independent transfer-function oracle
examples/filter_lab/measure.rs      public-API simulations and lock-in fits
examples/filter_lab/metrics.rs      delay, phase, waveform, and cost scores
examples/filter_lab/signals.rs      tones, pulse, chirp, timestamp schedules
examples/filter_lab/report.rs       CSV and Markdown output
examples/filter_lab/plot/           shared style and individual SVG views
```

The current shape-preserving SVG smoothing is visually useful, but its parser
depends on Plotters' emitted markup and literal stroke colors. Keep it isolated
behind rendering code with focused tests, or replace it with direct path
drawing; do not mix SVG rewriting with filter analysis. The current focused
examples remain the user-facing tools; add a broader shared CLI only when its
phase and matrix workflows are ready.

## Current command-line tools

Both examples are development-only and leave the runtime dependency graph of
`ssfilt` unchanged. Run `--help` to see the options accepted by the checked-out
version:

```text
cargo run --release --example plot_responses -- --help
cargo run --release --example phase_design -- --help
```

Generate the response dashboard and a sibling CSV report:

```text
cargo run --release --example plot_responses -- \
  --topology low-pass --response all --order 4 \
  --output target/filter-responses.svg \
  --csv target/filter-responses.csv
```

| `plot_responses` option | Values and default |
| --- | --- |
| `--topology <TOPOLOGY>` | `low-pass` (default); the only current topology |
| `--response <RESPONSE>` | `all` (default), `repeated-pole`, `butterworth`, `bessel`, or `chebyshev1` |
| `--order <ORDER>` | Integer 1–50; defaults to 4. Bessel is available only through its supported maximum order. |
| `--output <PATH>` | SVG path; defaults to `target/filter-responses.svg` |
| `--csv <PATH>` | CSV path; defaults to a `.csv` file beside the SVG |
| `--phase-equalized` | Add the public Butterworth phase-equalized measurements; requires order 4 or 8 and response `all` or `butterworth` |

For example, compare the public order-4 Butterworth source and
phase-equalized responses:

```text
cargo run --release --example plot_responses -- \
  --topology low-pass --response butterworth --order 4 \
  --phase-equalized --output target/order-4-phase-equalized.svg \
  --csv target/order-4-phase-equalized.csv
```

The legacy positional form `[ORDER] [OUTPUT.SVG]` remains available. For
example, `cargo run --release --example plot_responses -- 4 target/order-4.svg`
selects order 4 and that SVG path.

The phase-equalized overlay is a public streaming filter measurement. It keeps
the source Butterworth magnitude response and complete-filter −3 dB cutoff,
while adding causal delay to reduce group-delay variation over DC through
`0.7 * cutoff_hz`. Frequencies above that boundary are diagnostic context, not
a supported equalization claim.

Run the analytic candidate-design study and write its reports:

```text
cargo run --release --example phase_design -- \
  --output-dir target/phase-design
```

| `phase_design` option | Values and default |
| --- | --- |
| `--output-dir <PATH>` | Report directory; defaults to `target/phase-design` |
| `--order <ORDER>` | One order from 1–16; by default evaluates orders 4 and 8 |
| `--band <BAND>` | `core` or `cutoff`; by default evaluates both |

Use this command to examine one exploratory order-12 core-band candidate:

```text
cargo run --release --example phase_design -- \
  --order 12 --band core --output-dir target/phase-design-order-12
```

Public streaming equalization remains supported only for Butterworth orders 4
and 8. Design results for every other order are labeled exploratory and do not
imply public support.

## Tracked tooling roadmap

The development tools intentionally expose evidence in stages rather than
offering switches for unimplemented products:

| Area | Current state | Next evidence boundary |
| --- | --- | --- |
| Ordinary low-pass responses | Analytic curves and public streamed points for every response family and supported order | Keep as the shared numerical/style baseline |
| Ordinary high-pass and band-pass responses | Library implementations exist; dashboard views do not | Add topology-specific analytic references, cutoff guides, streamed points, and reports without changing the public API |
| Butterworth phase equalization, orders 4 and 8 | Analytic and public streamed overlays exist | Add the pre-1.0 application/cost comparison against Bessel and repeated-pole |
| Other Butterworth equalizer orders | Analytic candidates can be inspected for orders 1 through 16 | Add private coupled streaming validation one order at a time; do not display public measurement markers before promotion |
| Other response families and topologies | No supported equalizer design | Define a response-specific useful band and user value first; Chebyshev must include its ripple edge |
| Forward-backward batch filtering | Oracle and private negative-control components exist; no validated batch filter or public API exists | Implement deterministic filtered reports, then the full matrix and candidate selection before adding batch plots |

Batch plots will live in the isolated forward-backward oracle workflow, not in
`plot_responses`. They must be generated from the same scored records as the
typed report and identify the noncausal operator, boundary continuation,
per-pass order, complete-response cutoff, candidate status, and whether the
run is filtered or complete. The first view should show the input, finite-record
oracle, candidate output, signed or absolute error, edge influence, and work
metadata. A favorable plot cannot substitute for the full acceptance matrix.

## Tool dependency policy

Development programs have a different dependency budget from the runtime
library: they may use `std`, allocate, and depend on application-oriented
crates. The current examples use `clap` for CLI parsing, `anyhow` at executable
boundaries, and `csv` for reports; keep these in root `dev-dependencies` or a
standalone tool manifest so disabling default library features leaves the
`ssfilt` runtime graph unchanged.

Adopt dependencies only where they remove a maintained subsystem or provide a
well-tested boundary:

- use [`clap`](https://docs.rs/clap/) with its derive API for explicit options,
  useful help, invalid-value diagnostics, and tool-local value enums. Preserve
  the legacy `plot_responses [order] [output.svg]` form through the existing
  compatibility normalization and test help, invalid combinations, defaults,
  and that form with `try_parse_from`;
- keep [`anyhow`](https://docs.rs/anyhow/) at executable and orchestration
  boundaries, adding context for the case or output path while retaining typed
  filter, numerical, and scoring errors below that boundary. Do not expose
  `anyhow::Error` from the library;
- use [`csv`](https://docs.rs/csv/) without Serde for report writing and
  reading. Configure line endings explicitly, validate consistent record
  widths, and compare complete report bytes when changing the report format.
  Add Serde only if a stable typed replay or manifest schema later justifies it;
- defer [`rayon`](https://docs.rs/rayon/) until release-mode timing shows that
  independent cases dominate a tool's runtime. Parallelism must operate on
  complete keyed cases, never on stateful updates or floating-point reductions
  within a case. Provide a bounded job count, collect into deterministic order,
  and require byte-identical reports plus unchanged metrics between one and
  multiple jobs. The matrix oracle should use a dedicated pool because each
  shard owns a candidate session and high-precision records can be large;
- reserve `thiserror` for new structured tool errors when it materially removes
  display/source boilerplate. Do not churn the existing contextual error
  families merely to replace their hand-written implementations;
- defer progress-bar, logging, snapshot, and general serialization frameworks
  until a demonstrated workflow needs them. Plain status lines and focused
  assertions are sufficient today; snapshots must never become numerical
  acceptance oracles.

Before landing a new crate, inspect its current primary documentation,
maintenance activity, license, MSRV, feature set, transitive graph, and known
advisories. Commit lockfile changes and note why the dependency is a net
positive. Popularity is not a security argument. Add a modest automated
license, source, and RustSec advisory policy as a separate CI change rather
than mixing supply-chain configuration into a numerical feature commit.

## Analytic lines and real implementation points

Frequency-domain plots draw the independent continuous-time transfer-function
response as a continuous line. Markers are separate measurements made through
the public streaming API, not samples taken from or resampled out of that line:

1. construct a fresh filter for each measurement frequency;
2. drive it with a settled sine wave through `update`;
3. fit sine and cosine coefficients using the actual timestamps;
4. derive measured gain and phase from that fit;
5. record integration diagnostics and the sampling configuration.

The validation preset uses `InputModel::Linear` and at least 256 samples per
cycle, including under jitter where every interval is bounded by the same
maximum. Repeat selected points at 512 samples per cycle and require the fitted
gain and phase to converge before comparing them with the analytic line. Initial
convergence limits are `1e-4` relative gain and `1e-4` radian phase for `f64`,
and `1e-3` for both quantities for `f32`; tighter limits may be recorded after
measurement. The analytic-comparison tolerance additionally includes the
documented reconstruction and integration budgets rather than silently
treating sampled chords as an exact sine.

Before fitting, the current low-pass response plot warms a fresh filter for at
least 60 input cycles and for 24 time constants of its slowest pole, whichever
is longer. The pole-derived floor matters for high-order Chebyshev filters:
their lightly damped edge sections can retain startup energy for thousands of
cutoff periods even though a low-order response appears settled. The CSV
records both elapsed warm-up seconds and input cycles. This duration is chosen
from the pole locations, not by waiting until a measurement agrees with the
analytic gain or phase.

The pole duration is a conservative floor rather than a residual proof. After
it, the tool fits consecutive eight-cycle windows and accepts a measurement
only after two successive gain/phase comparisons agree within `1e-4`; eight
windows is the hard work limit. Failure is reported explicitly. Below −80 dB,
phase is considered unresolved under the default `f64` integration floor: it
is retained with its fit residual in CSV but omitted from the phase plot, and
gain convergence uses a `1e-8` full-scale absolute floor.

Run both uniform timestamps and deterministic bounded jitter. Source
measurements use circles and phase-equalized measurements use triangles;
filled markers denote uniform timestamps and open markers denote bounded
jitter. Labels and CSV call these **piecewise-linear sampled-sine
measurements** and state input model, samples per cycle, maximum interval,
jitter pattern, warm-up, precision, and fit residual. Under jitter the
sine/cosine least-squares result is a fundamental projection at the actual
timestamps, not a periodic-sample shortcut. These points demonstrate the
implementation's achievable result; they do not replace the ideal
continuous-time oracle.

When `--phase-equalized` is selected, the dashboard adds the public
Butterworth-plus-equalizer response measured under the same schedules and
sampling conditions as the source response. The magnitude retains the shared
analytic response; phase shows the source and combined response measurements.
Group delay is evaluated from the analytic pole contributions and includes the
constant-delay target and validation-edge guide. Sparse measured phase is not
differentiated to manufacture group-delay markers. The CSV contains the same
measurement records represented by the plotted markers. Its `variant` and
`measurement_pair_id` columns join source/equalized records made from the same
timestamp stream; `equalization_scope` distinguishes validated (`f/fc <= 0.7`)
from diagnostic points. Target delay, mean added delay, analytic group delay,
delay residual, and analytic magnitude difference remain explicit columns
rather than values inferred from the drawing.

The fitted phase is intrinsically wrapped. The CSV preserves that wrapped
value; for the phase panel and analytic error column, the tool selects the
equivalent `2π` branch nearest the independent analytic phase. This branch
selection makes the comparison readable but does not alter the least-squares
fit or participate in settling detection.

The response dashboard evaluates analytic group delay directly from the
independent pole contributions used by its transfer-function oracle. Future
measured group-delay markers may be derived from symmetric nearby measured
phase points, but they remain secondary because differentiation amplifies
measurement error. Never differentiate wrapped phase directly.

Frequency selection includes:

- logarithmic coverage outside the passband for magnitude context;
- DC-near and interior passband points;
- dense points from `0.7` through `1.1` times each cutoff edge;
- the exact -3 dB cutoff or both band-pass edges;
- calculated Chebyshev ripple extrema and ripple edge;
- any equalizer pole frequency near the displayed range.

The response tool currently covers low-pass `f64` measurements at `0.05`,
`0.1`, `0.25`, `0.5`, `0.7`, `0.8`, `0.9`, `0.95`, `1.0`, `1.05`, `1.1`,
`1.25`, `1.5`, `2`, `3`, `5`, `10`, and `20` times cutoff. The logarithmically
spaced stopband points extend real public-API evidence to the analytic plot's
right edge. A single frequency/schedule measurement has a 24-million-update
work guard; an exceptionally slow high-order response fails with a clear
diagnostic instead of silently truncating the interval. Named options are
preferred, while the positional order and output path remain compatible with
the original development example:

```text
cargo run --release --example plot_responses -- \
  --topology low-pass --response butterworth --order 8 \
  --output target/order-8.svg \
  --csv target/order-8.csv
```

`--response` accepts `all`, `repeated-pole`, `butterworth`, `bessel`, and
`chebyshev1`. The current `--topology` value is `low-pass`; keeping it explicit
lets later topology-specific views fail clearly rather than overloading a
response name. The default remains the complete low-pass response dashboard.
An explicitly requested Bessel order above its validated limit fails instead
of silently omitting the requested trace.

If `--csv` is omitted, the report is written beside the SVG with a `.csv`
extension. Both streamed marker schedules come directly from the measurement
records in that report, which also stores their independent analytic reference
columns. The dense analytic curves are rendered separately and are not
exported as CSV rows. Markers below the magnitude plot's visible floor are
retained in CSV but omitted from the chart; unresolved phase markers below
−80 dB are likewise CSV-only.

The nominal 256 samples per cycle is a minimum density derived from the maximum
allowed interval, not the jitter schedule's mean density. The deterministic
jitter multipliers are `0.625|0.875|1.0|0.75|0.9375|0.6875`, whose mean is
`0.8125`; the report exports both the exact pattern and its mean alongside the
`f64` precision and linear input model.

## Phase-design reports

By default, `phase_design` evaluates Butterworth orders 4 and 8 over both the
core `0..0.7 * cutoff` band and the cutoff-wide `0..1.0 * cutoff` band.
`--order <ORDER>` selects one order from 1 through 16, and `--band <BAND>`
selects `core` or `cutoff`. Every case other than the released order-4/order-8
core-band designs is exploratory: its generated reports and SVG identify it as
`streaming-not-validated / public-support-no`, and it does not extend the
public equalizer's support.

The tool searches the bounded one-section all-pass candidate and evaluates it
against analytic response and holdout points. `summary.csv` contains
coefficients, metrics, search and grid metadata, and each gate; `sweep.csv`
contains the stored holdout points; `summary.md` presents the same case records.
One SVG per case plots source and source-plus-candidate group delay, absolute
delay residual and its gate, phase residual and its gate, and the candidate's
added delay. Open rings are a sparse view of analytic holdout records, not
measurements from `update`; the reports and every SVG label this distinction.

For supported orders 4 and 8, the deterministic search passes the core band and
does not establish a useful cutoff-wide design. The public
`build_phase_equalized` API remains limited to Butterworth orders 4 and 8 from
DC through `0.7 * cutoff_hz`. Its coupled streaming behavior has separately
passed waveform checks across both precisions, uniform and irregular schedules,
and 1 Hz/37 Hz physical scaling. The `phase_design` holdout markers remain
analytic; use the `plot_responses --phase-equalized` overlay to inspect public
streaming measurements.

## Views

### Response dashboard

- magnitude and unwrapped phase: analytic lines plus measured markers;
- group delay: analytic curves, with the equalization target and validation
  edge shown when the phase-equalized overlay is selected;
- step response: a connected line through actual public `update` values; it
  uses no point markers, and its shape-preserving SVG path does not imply an
  analytic continuous-time step response;
- explicit topology, response, order, precision, and timestamp schedule.

### Streaming phase-equalization dashboard

The `plot_responses --phase-equalized` option adds this low-pass-only view for
Butterworth order 4 or 8. Source and equalized measurements use the same
timestamps, inputs, fit windows, and convergence checks. Magnitude shows their
shared analytic curve with separate public-API markers; phase shows both
measured responses. Group delay comes from the analytic model and includes the
constant-delay target and the `0.7 * cutoff` validation boundary. Frequencies
above that boundary provide context only. Sparse measured phase is not
differentiated into group-delay markers.

Later waveform views may add:

- source delay, equalized delay, and best constant-delay target;
- absolute delay residual with the acceptance threshold drawn;
- magnitude difference between source and equalized models;
- raw waveform view where added causal latency remains visible;
- display-aligned shape view with the analytic same-magnitude oracle;
- actual streamed samples as markers over every waveform line;
- a band-edge frontier showing residual error and added delay as the requested
  upper frequency moves from `0.5` to `1.0 * cutoff`.

Show `0..0.7`, `0.7..1.0`, and `0..1.0` metric summaries together. This makes a
core-band success and a near-cutoff failure equally visible.

### Matrix report

For every response/order/band/precision combination, export:

- fitted section parameters and pole-derived damping/Q;
- peak and RMS delay residual before and after equalization;
- maximum residual phase and magnitude change;
- mean and maximum added delay in normalized and physical units;
- waveform RMS and peak error before and after;
- accepted/rejected steps, derivative evaluations, and observed step sizes;
- each limit and its explicit pass/fail result.

A compact heat map may summarize the matrix, but CSV and Markdown are the
reviewable source data.

### Performance and work

Retain a separate host-local performance chart with clear non-portability
wording. Add object size and integration diagnostics before adding elaborate
microbenchmarks; they directly reveal the equalizer's memory and adaptive-work
cost. Criterion remains the repeatable benchmark oracle.

## Small quality-of-life extensions

After the measurement overlays work:

- replay timestamp/value CSV files through a selected public filter and export
  timestamp/input/output/diagnostics rows;
- provide a deterministic irregular-timestamp preset representative of sensor
  acquisition jitter;
- generate all standard dashboards and reports with one `matrix` command;
- retain topology-specific cutoff/center guides and automatic output-directory
  creation;
- give an explicit `unsupported` or `no useful design` result instead of
  omitting a requested trace silently.

CSV replay visualizes real data but is not an accuracy oracle unless a trusted
reference output is also supplied.

## Automated checks

- formatting passes with `cargo fmt --all -- --check`;
- analytic responses hit the public -3 dB edge convention;
- measured low-order gain and phase match known analytic cases at `0.7`, `0.9`,
  and `1.0 * cutoff` within declared sampling/integration error;
- normalized measurements are invariant under physical cutoff scaling;
- uniform and jittered timestamp fits use their actual timestamps;
- plot points exactly match exported measurement rows;
- waveform markers exactly equal logged `update` returns;
- CLI parsing, defaults, invalid values, and output naming are tested;
- SVG smoothing/rendering has focused smoke tests, never numerical assertions;
- no plot is the sole evidence for a library behavior claim.

## Prior implementation inventory

Useful ideas from the discarded branch that may be reimplemented cleanly:

- low-pass, high-pass, band-pass, phase, and waveform views;
- response-family selection with clear Bessel order handling;
- raw-latency and display-aligned waveform panels;
- clear `no improving design` output;
- topology-specific cutoff and center-frequency guide lines;
- a separate host-local median performance chart;
- correctly scaled SVG legend strokes.

Do not restore its permissive equalizer success criterion or couple plotting
code to the runtime fitting algorithm. The current examples already separate
analytic calculation, streaming measurements, reports, and rendering into
modules. Keep future view work within those boundaries and use shared records
for plots and exported reports.

## Other audited quality-of-life work

Several changes on the discarded branch are independent of its equalizer and
remain useful candidates for separate commits:

- the stable linear-interpolation overflow fix already called out in
  `baseline-review.md`;
- exercising the `libm` configuration in CI, not merely checking it;
- adding the MIT and Apache-2.0 license texts already named by package metadata;
- practical sensor/embedded guidance covering response choice, input timing,
  startup equilibrium, and fixed-size runtime order selection;
- a small public-API compile test for the documented runtime-order enum.

Offline forward-backward filtering is not a QoL patch. It is a separate public
batch feature with boundary-condition, squared-magnitude, allocation, and
partial-output error semantics, so it stays behind its own design review.
