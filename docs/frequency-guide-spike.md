# Frequency-guide spike

## User problem

Someone filtering their own irregularly timestamped capture can see input and
output in the time domain, but cannot easily tell when an unwanted frequency
is present or how the selected cutoff relates to it. A global Fourier summary
would lose that timing information and would be particularly misleading for
steps, bursts, and changing-frequency signals.

This spike asks whether a small time-frequency guide improves cutoff and band
selection enough to justify becoming a maintained part of the project site.
It is not part of the `ssfilt` public API and is not a new runtime filter
feature.

## Implemented experiment

The optional guide appears below the uploaded-signal plot after filtering. It:

- reconstructs the input with the active `Linear`, `PreviousHold`, or
  `CurrentHold` contract;
- resamples that reconstruction to a fixed 640-point display grid;
- evaluates 30 logarithmically spaced complex Morlet wavelets;
- displays magnitude relative to the strongest time-frequency cell over a
  fixed 48 dB color window;
- overlays the active whole-filter -3 dB cutoff or band edges;
- fades capture edges where wavelet support is incomplete; and
- shades intervals larger than four times the median sample spacing.

The computation lives only in the unpublished `site/wasm` adapter. It adds no
dependency or API to the allocation-free, `no_std` library. The ordinary
filtering flow performs no wavelet work until the visitor opens the guide.
Changing only filter order or cutoff reuses the existing guide and redraws the
reference line; changing the samples or reconstruction model recomputes it.

## Numerical scope

The frequency range is capture-derived. The lower edge requires two cycles in
the capture duration. The upper edge is bounded conservatively by both the
median observed interval and the uniform analysis grid. A cutoff outside that
range is reported instead of expanding the analysis into unsupported
frequencies.

The spike deliberately does not claim calibrated spectral density,
statistical significance, or information inside a sampling gap. Color means
relative wavelet magnitude within one capture. A held or interpolated gap is
still the selected input reconstruction, while the pale gap marker tells the
visitor that the original observations were sparse there.

## Initial evidence

Automated fixtures establish that:

- the three reconstruction policies produce distinct, contract-matching
  regular grids;
- an 8 Hz tone remains identifiable under deterministic irregular timing;
- clustered timestamps do not move the reconstructed ridge materially away
  from a quadrature-weighted direct irregular-time reference in representative
  tone cases;
- severe deterministic interval jitter keeps a stationary tone within one
  displayed frequency bin of both its true frequency and the direct irregular
  reference at interior times;
- under the same severe jitter, an interior 2–18 Hz chirp ridge stays within
  two displayed bins of its instantaneous frequency and the direct reference;
- high-frequency step energy remains localized near the transition rather
  than appearing as a persistent oscillation;
- unusually long intervals are marked; and
- work and output size remain fixed at 640 by 30 cells for the 50,000-row
  upload limit.

On the development machine, the optimized native test containing one complete
analysis reported about 0.06 seconds for both a 420-row tone and a 50,000-row
tone. This is development evidence, not a browser or device performance
promise. In the browser, changing the reconstruction model through response
recalculation, refiltering, guide calculation, and rendering became visible in
about 455 ms during one local trial. The opt-in panel was also checked at a
390-pixel viewport without horizontal overflow.

## Direct irregular-time reference

The shipped experiment does not include a direct irregular-time transform.
Tests retain a small quadrature-weighted Morlet calculation only as an
independent comparison oracle. Promoting that approach would require policy
for local conditioning, clustered samples, gaps, time-varying alias limits,
and confidence reporting. Those costs remain unjustified by the initial tone
comparison.

## What the spike still needs to prove

Before removing the experimental label, evaluate representative real captures
and record whether the guide changes a real filter choice. The synthetic pass
now covers severe timing jitter, clustered timestamps, a chirp, a step, and a
long gap. Remaining useful trials include:

- impulses and short non-periodic bursts;
- motion-like pulses; and
- all three reconstruction contracts.

Judge the feature by cutoff or band-selection agreement, ridge error, burst
timing error, false energy around gaps, perceived explanation cost, and
browser latency. Keep it only if visitors can choose a defensible filter more
reliably than from the time trace and response plot alone. If disagreement
with the direct reference is confined to visibly faded edges or shaded gaps,
prefer the smaller reconstructed-CWT implementation.

## Known limitations

- Edge fading is an explanatory reliability cue, not a confidence interval.
- The median-interval upper-frequency bound is a heuristic for the guide, not
  a general irregular-sampling Nyquist theorem.
- Global mean removal does not replace a selectable detrending model.
- Relative color cannot compare absolute energy between different captures.
- The calculation currently runs on the main browser thread. The bounded grid
  keeps the trial usable, but a worker and cancellation protocol should be
  considered if representative devices show disruptive pauses.
