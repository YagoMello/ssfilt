export type CodeScalar = "f32" | "f64";

export type DopriSettings = {
  absoluteTolerance: number;
  relativeTolerance: number;
  maxStepMilliseconds: number | null;
  maxStepAttempts: number;
};

export const DOPRI_DEFAULTS: Record<CodeScalar, Readonly<DopriSettings>> = {
  f32: {
    absoluteTolerance: 1e-6,
    relativeTolerance: 1e-4,
    maxStepMilliseconds: null,
    maxStepAttempts: 4_096,
  },
  f64: {
    absoluteTolerance: 1e-10,
    relativeTolerance: 1e-7,
    maxStepMilliseconds: null,
    maxStepAttempts: 4_096,
  },
};

export function dopriSettingsAreDefaults(settings: DopriSettings, scalar: CodeScalar): boolean {
  const defaults = DOPRI_DEFAULTS[scalar];
  return settings.absoluteTolerance === defaults.absoluteTolerance
    && settings.relativeTolerance === defaults.relativeTolerance
    && settings.maxStepMilliseconds === defaults.maxStepMilliseconds
    && settings.maxStepAttempts === defaults.maxStepAttempts;
}
