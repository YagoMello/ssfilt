import assert from "node:assert/strict";

import { DOPRI_DEFAULTS, dopriSettingsAreDefaults } from "../src/scalar.ts";

assert.deepEqual(DOPRI_DEFAULTS.f32, {
  absoluteTolerance: 1e-6,
  relativeTolerance: 1e-4,
  maxStepMilliseconds: null,
  maxStepAttempts: 4_096,
});
assert.deepEqual(DOPRI_DEFAULTS.f64, {
  absoluteTolerance: 1e-10,
  relativeTolerance: 1e-7,
  maxStepMilliseconds: null,
  maxStepAttempts: 4_096,
});
assert.equal(dopriSettingsAreDefaults({ ...DOPRI_DEFAULTS.f32 }, "f32"), true);
assert.equal(dopriSettingsAreDefaults({ ...DOPRI_DEFAULTS.f32 }, "f64"), false);
assert.equal(dopriSettingsAreDefaults({ ...DOPRI_DEFAULTS.f64 }, "f64"), true);
assert.equal(dopriSettingsAreDefaults({ ...DOPRI_DEFAULTS.f64, relativeTolerance: 2e-7 }, "f64"), false);
assert.equal(dopriSettingsAreDefaults({ ...DOPRI_DEFAULTS.f32, maxStepMilliseconds: 1 }, "f32"), false);

console.log("Scalar default tests passed.");
