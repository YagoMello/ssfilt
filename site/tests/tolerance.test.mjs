import assert from "node:assert/strict";

import { formatTolerance, parsePositiveDecimal, stepTolerance } from "../src/tolerance.ts";

assert.equal(stepTolerance(1e-7, "tighter"), 5e-8);
assert.equal(stepTolerance(1e-7, "looser"), 2e-7);
assert.equal(stepTolerance(5e-8, "looser"), 1e-7);
assert.equal(formatTolerance(stepTolerance(1e-7, "tighter")), "5e-8");
assert.equal(stepTolerance(5e-11, "tighter"), 2e-11);
assert.equal(stepTolerance(5e-11, "looser"), 1e-10);
assert.equal(stepTolerance(5e-6, "tighter"), 2e-6);
assert.equal(stepTolerance(5e-15, "looser"), 1e-14);

assert.equal(stepTolerance(3.7e-7, "tighter"), 2e-7);
assert.equal(stepTolerance(3.7e-7, "looser"), 5e-7);

assert.equal(parsePositiveDecimal(" 1e-7 "), 1e-7);
assert.equal(parsePositiveDecimal(".0005"), 5e-4);
for (const invalid of ["", "0", "-1e-7", "1e-", "Infinity", "NaN", "0x10"]) {
  assert.equal(parsePositiveDecimal(invalid), undefined);
}

assert.equal(stepTolerance(Number.MIN_VALUE, "tighter"), undefined);
assert.equal(stepTolerance(Number.MAX_VALUE, "looser"), undefined);

console.log("Tolerance control tests passed.");
