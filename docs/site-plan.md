# Project site plan

The project site is a demonstration and inspection surface for the shipped
streaming filters. It is not a second filter library, a validation laboratory,
or a promise that browser performance predicts embedded performance.

## User goals

The first public version should let a visitor:

1. understand why an irregular-time continuous filter is useful;
2. compare magnitude, causal phase, group delay, and a bounded canonical
   current-hold step trace;
3. configure every shipped topology and response over the documented order
   limits;
4. copy a small Rust builder example matching the visible configuration;
5. upload timestamped samples and inspect the output produced by the public
   streaming API; and
6. download the filtered samples for further analysis.

The site must call phase plots what they are. It does not provide phase
correction, zero-phase streaming, or offline forward-backward filtering.

## Architecture boundary

The site lives under `site/` and is excluded from the published crate by the
package's explicit include list.

- `site/src/` is a small, framework-free TypeScript application.
- `site/wasm/` is an unpublished browser adapter depending on `ssfilt` through
  its existing public builders and update methods.
- uPlot renders browser charts. The native Plotters examples remain the source
  for publication-quality SVG and CSV inspection reports.
- highlight.js colors the generated Rust snippet; the panel remains read-only
  and does not execute source code.
- GitHub Actions builds static assets and deploys them to GitHub Pages. The
  browser requires no server and uploaded data never leaves the page.

The adapter may share the existing development-only analytic transfer oracle
for ideal curves. The canonical current-hold step trace and uploaded-data
results must be produced through public `ssfilt` objects. The site must not
expose private runtime models, add a feature to the runtime crate, or change
the `no_std` dependency graph.

An opt-in, experimental frequency guide is being evaluated as a private site
adapter feature. It reconstructs uploaded data with the selected public input
model before a bounded wavelet analysis; it is not a public library contract
or a calibrated spectrum. Its scope, evidence, and exit criteria are recorded
in [`frequency-guide-spike.md`](frequency-guide-spike.md).

## Configuration contract

The playground supports:

- low-pass, high-pass, and band-pass topologies;
- repeated-pole, Butterworth, Bessel, and Chebyshev Type I responses;
- final orders 1 through 50, subject to even band-pass order and documented
  Bessel limits;
- whole-filter −3 dB cutoff values or band edges;
- Chebyshev passband ripple; and
- linear, previous-value hold, and current-value hold reconstruction.

The URL records the current configuration so a view can be shared. The Rust
panel is generated text, not an editor or remote compiler, and it uses only
public API available in the current prerelease.

Magnitude data is retained without an artificial floor. The interactive chart
uses a documented -140 dB display window and clips curves below it rather than
showing clipped values as computed response data.

## Uploaded data contract

The initial format is deliberately narrow: a comma-separated file containing
`time,value`, optionally preceded by that header. Times are seconds, all
values must be finite, timestamps must increase strictly, and the browser
adapter accepts at most 50,000 rows.

Visitors without a capture can generate one of a few deterministic example
signals. These use irregular timestamps, are created locally, and pass through
the same CSV parser and public filtering path as pasted or uploaded data.

Two initial-state modes make the first interval unambiguous:

- **steady at sample 0** initializes the filter at the first sample and starts
  integration with the second row;
- **steady at a custom value** initializes before the first row and requires a
  positive elapsed time from that initial point to the first CSV timestamp.

The latter intentionally asks for the missing first interval instead of
inventing it from later samples. Filtering reports aggregate accepted steps,
rejected steps, derivative evaluations, and equilibrium shortcuts. Those are
observations for the uploaded workload, not timing estimates.

Files are parsed and filtered locally. The page should state that plainly and
must not add telemetry or upload storage without a separate privacy decision.

## Performance scope

The first site release does not estimate performance from the presence or
absence of an FPU. It may show integration work observed for an uploaded
signal. Host and device timing dashboards remain a later milestone requiring
a versioned record containing the library commit, compiler, target, clock,
scalar, filter configuration, workload, and timing distribution.

Browser/Wasm timing must never be presented as an MCU estimate.

## Deployment

Before the first deployment, select **GitHub Actions** under the repository's
Settings → Pages → Build and deployment source. The Pages workflow builds and
tests pull requests without deploying them, and deploys only successful builds
from `main` (or an explicitly dispatched non-PR run).

## Quality checks

The site release gate is:

```text
cargo test --manifest-path site/wasm/Cargo.toml --locked
cargo clippy --manifest-path site/wasm/Cargo.toml --all-targets --locked -- -D warnings
npm --prefix site ci
npm --prefix site run typecheck
npm --prefix site run build
```

The browser adapter has native tests for cutoff mapping, initialization
semantics, order validation, and timestamp rejection. The production build
must also compile the adapter for `wasm32-unknown-unknown`. Before publicizing
the page, perform a keyboard and narrow-screen pass, inspect the four response
views, and filter one known CSV whose expected output was generated natively.

## Deferred work

- measured host and device performance records;
- direct SVG export of arbitrary browser configurations;
- multiple overlaid configurations;
- additional CSV dialects, time units, or column mapping;
- in-browser source editing or compilation;
- phase equalization and offline filtering.

Each deferred item needs a recurring user problem before it expands the site.
