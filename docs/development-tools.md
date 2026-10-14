# Development measurement and visualization roadmap

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

Suggested eventual development-only module split:

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
depends on Plotters' emitted markup and literal stroke colors. Isolate it behind
one renderer module with focused tests, or replace it with direct path drawing;
do not mix SVG rewriting with filter analysis.

Reach that structure incrementally. First extract analytic calculation,
measurement records, and rendering from the existing response example without
changing its output. Then add one low-pass measured overlay and CSV export.
Introduce the broader `filter_lab` subcommands only when phase and matrix views
need them; do not begin with a wholesale CLI rewrite.

## Command-line UX

Use explicit named arguments and useful defaults rather than overloaded
positional forms. A representative interface is:

```text
cargo run --release --example filter_lab -- response \
  --topology low-pass --response chebyshev --order 8 --ripple-db 0.5

cargo run --release --example filter_lab -- phase \
  --response butterworth --order 8 --band cutoff --schedule jittered

cargo run --release --example filter_lab -- matrix --output target/filter-lab
```

`--help` should list valid responses, orders, band profiles, schedules, and
default output paths. Each run prints its complete configuration and writes to
a reproducible case directory under `target/filter-lab/`. A lightweight
development-only argument parser is acceptable; it must not add a runtime
dependency to `ssfilt`.

## Analytic lines and real implementation points

Frequency-domain plots draw the independent analytic response as a continuous
line. Over it, draw conspicuous markers obtained through the public filter API:

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

Run both uniform timestamps and deterministic bounded jitter. Labels and CSV
call these **piecewise-linear sampled-sine measurements** and state input model,
samples per cycle, maximum interval, jitter pattern, warm-up, precision, and
fit residual. Under jitter the sine/cosine least-squares result is a fundamental
projection at the actual timestamps, not a periodic-sample shortcut. These
points demonstrate the implementation's achievable result; they do not replace
the ideal continuous-time oracle.

The fitted phase is intrinsically wrapped. The CSV preserves that wrapped
value; for the phase panel and analytic error column, the tool selects the
equivalent `2π` branch nearest the independent analytic phase. This branch
selection makes the comparison readable but does not alter the least-squares
fit or participate in settling detection.

Group-delay markers may be derived from symmetric nearby measured phase points,
but analytic group delay remains the primary reference because differentiating
measured phase amplifies error. Never differentiate wrapped phase directly.

Frequency selection includes:

- logarithmic coverage outside the passband for magnitude context;
- DC-near and interior passband points;
- dense points from `0.7` through `1.1` times each cutoff edge;
- the exact -3 dB cutoff or both band-pass edges;
- calculated Chebyshev ripple extrema and ripple edge;
- any equalizer pole frequency near the displayed range.

The incremental response tool currently covers low-pass `f64` measurements at
`0.05`, `0.1`, `0.25`, `0.5`, `0.7`, `0.8`, `0.9`, `0.95`, `1.0`, `1.05`, and
`1.1` times cutoff. Filled dots denote uniform timestamps and open rings denote
deterministic bounded jitter. Its positional interface remains compatible with
the original development example while accepting an explicit CSV path:

```text
cargo run --release --example plot_responses -- 8 target/order-8.svg \
  --csv target/order-8.csv
```

If `--csv` is omitted, the report is written beside the SVG with a `.csv`
extension. The analytic curves and both marker schedules use the same stored
measurement records as that report. Markers below the magnitude plot's visible
floor are retained in CSV but omitted from the chart; unresolved phase markers
below −80 dB are likewise CSV-only.

The nominal 256 samples per cycle is a minimum density derived from the maximum
allowed interval, not the jitter schedule's mean density. The deterministic
jitter multipliers are `0.625|0.875|1.0|0.75|0.9375|0.6875`, whose mean is
`0.8125`; the report exports both the exact pattern and its mean alongside the
`f64` precision and linear input model.

## Current phase-design tool

The first analytic feasibility increment is available separately from the
response dashboard:

```text
cargo run --release --example phase_design -- \
  --output-dir target/phase-design
```

It evaluates the mandatory Butterworth order 4 and 8 cases over both the core
and cutoff bands. `summary.csv` contains coefficients, metrics, search and grid
metadata, and every gate; `sweep.csv` contains the exact stored holdout points;
`summary.md` is a readable view of the same case records. One SVG per case
plots source and source-plus-candidate group delay, absolute delay residual and
its gate, phase residual and its gate, and the candidate equalizer's added
delay. Open rings are a sparse view of the exact analytic holdout records
behind each curve; they must not be confused with public-API streaming
measurements. Every SVG repeats that limitation in its title. Named `--order`
and `--band` options narrow the default four-case matrix.

The deterministic search passes both core-band cases and explicitly reports no
useful cutoff-wide design. Those analytic results originally authorized the
coupled-streaming experiment. The subsequent three-signal waveform matrix now
passes for both precisions, uniform and irregular schedules, and 1 Hz/37 Hz
physical scaling for Butterworth orders 4 and 8 over DC through
`0.7 * cutoff`. Separate structural `f64` tests cover stage coupling, step
bounds, ramp partitioning, physical scaling, and transactional failure. The
narrow `build_phase_equalized` API is public, remains fixed-size and
allocation-free, and exposes integration diagnostics for observing its work;
this record does not claim a separate runtime-cost gate. Cutoff-wide claims,
other orders, and other response families remain unsupported. This tool is
still an analytic design record: its rings are not public-API streaming
measurements, and the separate response dashboard must provide those
implementation overlays.

## Views

### Response dashboard

- magnitude and unwrapped phase: analytic lines plus measured markers;
- group delay: analytic line, optional measured markers, and cutoff guides;
- step response: actual `update` samples shown as markers rather than an
  unexplained smooth curve;
- explicit topology, response, order, precision, and timestamp schedule.

### Streaming phase-equalization dashboard

This broader view follows the current analytic per-case dashboards. The public
coupled streaming candidate now exists, but its dashboard is not implemented
yet. Build it incrementally rather than making the response plot a second
phase-design program.

The first slice stays low-pass-only and adds named topology/response/order
selection plus an optional Butterworth order-4/order-8 phase-equalized overlay
to `plot_responses`. Source and equalized measurements must use the same
timestamps, inputs, fit windows, and convergence checks. Magnitude shows their
shared analytic curve with distinct public-API markers; phase and group delay
show source and combined responses, the constant-delay target, and the
`0.7 * cutoff` validation boundary. Frequencies above that boundary are
diagnostic context, not a supported equalization claim. Sparse measured phase
must not be differentiated into group-delay markers in this first slice.

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
code to the runtime fitting algorithm. The current response example had grown
to combine CLI parsing, analytic math, public simulations, rendering, SVG text
rewriting, and tests in one large file; the module boundaries above are a
prerequisite for adding more views safely.

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
