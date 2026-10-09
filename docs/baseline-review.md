# Baseline implementation review

## Status

Independent read-only review of commit `d13e51f`, performed before phase
equalizer implementation. The full quality gates passed, but the review found
two correctness issues and two documentation/boundary issues to address before
the solver refactor.

## Findings

### 1. Linear interpolation can overflow between finite endpoints

`InputSegment::value_at` evaluates linear input as:

```text
start + fraction * (end - start)
```

The subtraction can overflow for opposite, finite endpoints such as
`-f64::MAX` and `f64::MAX`. Even the start of the segment can then become NaN
through `0 * infinity`. Replace it with a stable convex interpolation and add
`f32` and `f64` regression tests at both endpoints and interior fractions.

### 2. Band-pass equilibrium can contain infinity for a finite input

Band-pass equilibrium divides the constant input by the first section's
normalized frequency squared. For sufficiently large finite inputs and a
section frequency below one, this can overflow. Construction and
`reset_to_steady` currently commit that state without validating every state
component.

Equilibrium creation must be validated transactionally. Construction should
return a precise build error, and a failed steady reset must preserve state,
output, previous input, diagnostics, and equilibrium tracking. Cover both
scalar types and affected response families.

### 3. Tiny positive Chebyshev ripple can be unrepresentable

The public contract currently reads as though every finite ripple strictly
between zero and 3.0103 dB is supported. For very small positive values,
floating-point evaluation of the ripple factor can round to zero and the
constructor returns `InvalidPassbandRipple`.

Either reformulate the calculation to extend the representable range or state
that numerically unrepresentable positive values are rejected. Add explicit
boundary tests so the choice is stable.

### 4. Cutoff has a finite representable upper bound

Low-pass and high-pass builders correctly reject a cutoff whose multiplication
by `2*pi` overflows, but their documentation can be read as promising every
finite positive cutoff. Document that physical scaling must remain
representable and test values around the scalar-specific boundary.

## Additional test gaps

- high-pass and band-pass transactional behavior on work-budget failure;
- high-pass and band-pass steady-reset failure preservation;
- non-smoke `f32` response and irregular-timing cases;
- high-order streaming checks;
- band-pass narrow/wide reference cases outside `f64` model-only tests.

These gaps do not all block phase research, but fixes 1 and 2 and their
transactional regressions do block the solver refactor. Keep baseline fixes in
separate commits from phase equalization so regressions remain attributable.
