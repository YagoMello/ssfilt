export type ToleranceDirection = "tighter" | "looser";

const POSITIVE_DECIMAL = /^\+?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/;
const E_SERIES = [1, 2, 5] as const;

export function parsePositiveDecimal(text: string): number | undefined {
  const trimmed = text.trim();
  if (!POSITIVE_DECIMAL.test(trimmed)) return undefined;
  const value = Number(trimmed);
  return Number.isFinite(value) && value > 0 ? value : undefined;
}

export function stepTolerance(value: number, direction: ToleranceDirection): number | undefined {
  if (!Number.isFinite(value) || value <= 0) return undefined;
  const exponent = Math.floor(Math.log10(value));
  const candidates: number[] = [];
  for (let candidateExponent = exponent - 2; candidateExponent <= exponent + 2; candidateExponent += 1) {
    for (const mantissa of E_SERIES) {
      const candidate = Number(`${mantissa}e${candidateExponent}`);
      if (Number.isFinite(candidate) && candidate > 0) candidates.push(candidate);
    }
  }
  candidates.sort((left, right) => left - right);
  if (direction === "looser") return candidates.find((candidate) => candidate > value);
  for (let index = candidates.length - 1; index >= 0; index -= 1) {
    if (candidates[index] < value) return candidates[index];
  }
  return undefined;
}

export function formatTolerance(value: number): string {
  const [mantissa, exponent] = value.toExponential(0).split("e");
  return `${mantissa}e${Number(exponent)}`;
}
