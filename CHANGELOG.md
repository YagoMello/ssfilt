# Changelog

This project follows [Semantic Versioning](https://semver.org/). While the
crate remains below 1.0, minor releases may include API changes. Prereleases
are intended for field testing and should not be treated as production-ready.

## [0.1.0-beta.1] - Unreleased

The first public field-test release.

### Added

- Allocation-free continuous-time low-pass, high-pass, and band-pass filters.
- Repeated-pole, Butterworth, Bessel, and Chebyshev Type I responses with a
  consistent whole-filter −3 dB cutoff convention.
- Explicit linear, previous-hold, and current-hold input reconstruction for
  irregular sample intervals.
- Adaptive Dormand–Prince integration with transactional failures, bounded
  work, steady-state initialization and reset, and per-update diagnostics.
- Validated `f32` and `f64` operation, including `no_std` builds through the
  `libm` feature.
- Runnable examples and development tools for inspecting frequency, phase,
  group-delay, transient, and performance behavior.

### Known limitations

- The public filter order is a const generic; runtime selection requires an
  enum or an allocator-backed trait object over a finite set of orders.
- Bessel filters are limited to validated order 25 prototypes (final order 50
  for band-pass filters).
- Fixed-point arithmetic, streaming phase correction, and offline zero-phase
  filtering are not part of this release.
- No minimum supported Rust version is promised before 1.0; current stable
  Rust is used during prerelease development.

[0.1.0-beta.1]: https://github.com/yagomello/ssfilt/releases/tag/v0.1.0-beta.1
